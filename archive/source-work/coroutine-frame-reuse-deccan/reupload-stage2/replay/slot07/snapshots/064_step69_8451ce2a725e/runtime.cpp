
#include "roundel/runtime.hpp"

#include <algorithm>
#include <new>
#include <cstdio>

namespace roundel {
namespace {

// The arena the next coroutine frame should be taken from. A live Conductor
// installs itself here; nested Conductors save and restore the previous one.
FrameArena* g_arena = nullptr;

// Frames made while no conductor was installed. They came from the general
// heap, so their storage must go back there; asking the arena about them would
// mean reading a block header that was never written.
std::vector<const void*> g_heap_frames;

std::coroutine_handle<> untyped(const std::coroutine_handle<>& h) noexcept { return h; }

// Name of the storage a frame sits in. A frame made before any conductor was
// installed has no block header behind it, so it has no slot to name.
SlotId slot_of_frame(const void* p) noexcept {
    if (p == nullptr) {
        return SlotId{};
    }
    for (const void* q : g_heap_frames) {
        if (q == p) {
            return SlotId{};
        }
    }
    return FrameArena::slot_of(p);
}

}  // namespace

// -------------------------------------------------------------------------
// Frame storage
// -------------------------------------------------------------------------

void* Duty::promise_type::operator new(std::size_t bytes) {
    std::fprintf(stderr, "FRAME %zu\n", bytes);
    if (g_arena == nullptr) {
        void* p = ::operator new(bytes);
        g_heap_frames.push_back(p);
        return p;
    }
    return g_arena->check_out(bytes);
}

void Duty::promise_type::operator delete(void* p, std::size_t bytes) noexcept {
    (void)bytes;
    if (p == nullptr) {
        return;
    }
    for (std::size_t i = 0; i < g_heap_frames.size(); ++i) {
        if (g_heap_frames[i] == p) {
            g_heap_frames.erase(g_heap_frames.begin() + static_cast<std::ptrdiff_t>(i));
            ::operator delete(p);
            return;
        }
    }
    FrameArena* owner = FrameArena::owner_of(p);
    if (owner == nullptr) {
        ::operator delete(p);
        return;
    }
    owner->reclaim(p);
}

std::coroutine_handle<> Duty::promise_type::FinalStage::await_suspend(handle_type me) noexcept {
    promise_type& p = me.promise();
    if (p.parent) {
        // A child hands control back to whoever awaited it. The awaiter owns
        // the frame from here on and is the one that destroys it.
        return p.parent;
    }
    // A root job has nobody to continue into, so it settles here. The frame is
    // suspended at its final suspension point; the conductor gives its storage
    // back as soon as control is out of this resume.
    if (p.conductor != nullptr) {
        Outcome o = Outcome::Delivered;
        if (p.fault) {
            o = Outcome::Faulted;
            try {
                std::rethrow_exception(p.fault);
            } catch (const Abandoned&) {
                // Stopped on request, not broken.
                o = Outcome::Cancelled;
            } catch (...) {
                o = Outcome::Faulted;
            }
        } else if (p.cancel_flagged) {
            o = Outcome::Cancelled;
        }
        p.conductor->settle(p.job, o, p.produced);
    }
    return std::noop_coroutine();
}

// -------------------------------------------------------------------------
// ChildAwaiter
// -------------------------------------------------------------------------

Duty::ChildAwaiter::~ChildAwaiter() {
    if (child_ && !detached_) {
        // The awaiting frame is being destroyed while its child is still
        // suspended. Destroying the child here keeps the chain intact: a frame
        // never outlives the frame that was waiting for it.
        Conductor* c = child_.promise().conductor;
        if (c != nullptr) {
            c->forget_parked(untyped(child_));
        }
        child_.destroy();
    }
}

std::coroutine_handle<> Duty::ChildAwaiter::await_suspend(handle_type parent) noexcept {
    promise_type& cp = child_.promise();
    promise_type& pp = parent.promise();
    cp.parent = parent;
    cp.conductor = pp.conductor;
    cp.job = pp.job;
    return child_;
}

std::int64_t Duty::ChildAwaiter::await_resume() {
    promise_type& cp = child_.promise();
    Conductor* c = cp.conductor;
    const JobId job = cp.job;
    const std::int64_t produced = cp.produced;
    const std::exception_ptr fault = cp.fault;

    // The child has run to its final suspension. Its frame goes now, so the
    // awaiting job only ever sees a child whose storage is already back in the
    // arena -- whether the child produced a value or failed.
    const SlotId slot = slot_of_frame(child_.address());
    if (c != nullptr) {
        c->forget_parked(untyped(child_));
    }
    detached_ = true;
    child_.destroy();

    if (c != nullptr) {
        c->journal().record_slot(Mark::ChildDestroyed, job, slot);
    }
    if (fault) {
        std::rethrow_exception(fault);
    }
    if (c != nullptr) {
        c->journal().record(Mark::ParentContinued, job,
                            static_cast<std::int64_t>(c->arena().live_slots()));
    }
    return produced;
}

// -------------------------------------------------------------------------
// ParkAwaiter
// -------------------------------------------------------------------------

bool ParkAwaiter::await_suspend(Duty::handle_type me) noexcept {
    promise = &me.promise();
    Conductor* c = promise->conductor;
    if (c == nullptr) {
        return false;
    }
    // A job that was asked to stop while it was running does not wait: the
    // wait expression raises instead, and the job unwinds from here.
    if (c->take_cancel_request(promise->job)) {
        promise->cancel_flagged = true;
        return false;
    }
    c->note_parked(promise->job, me);
    return true;
}

std::int64_t ParkAwaiter::await_resume() {
    if (promise == nullptr) {
        return 0;
    }
    if (promise->cancel_flagged) {
        promise->cancel_flagged = false;
        throw Abandoned{};
    }
    if (promise->fault) {
        // A failing ticket raises where the job was waiting. The job may catch
        // it; if it does not, it escapes and the job faults.
        std::exception_ptr e = std::exchange(promise->fault, std::exception_ptr{});
        std::rethrow_exception(e);
    }
    return promise->produced;
}

// -------------------------------------------------------------------------
// Conductor
// -------------------------------------------------------------------------

Conductor::Conductor(std::size_t capacity) : arena_(capacity, 512) {
    arena_.observe(&journal_);
    previous_arena_ = g_arena;
    g_arena = &arena_;
}

Conductor::~Conductor() {
    // Anything still open is stopped and its frames destroyed, lowest
    // identifier first, so the arena is whole when it goes away.
    std::vector<JobId> ids;
    ids.reserve(jobs_.size());
    for (const auto& kv : jobs_) {
        ids.push_back(kv.first);
    }
    std::sort(ids.begin(), ids.end());
    for (JobId job : ids) {
        const JobState* st = find(job);
        if (st != nullptr && !st->settled) {
            settle(job, Outcome::Cancelled, 0);
        }
    }
    flush_releases();
    for (JobId job : ids) {
        JobState* st = find(job);
        if (st == nullptr || !st->root) {
            continue;
        }
        std::coroutine_handle<> r = std::exchange(st->root, {});
        st->parked = {};
        st->parked_slot = SlotId{};
        const SlotId slot = slot_of_frame(r.address());
        r.destroy();
        journal_.record_slot(Mark::FrameReleased, job, slot);
    }
    g_arena = previous_arena_;
}

FrameArena* Conductor::active_arena() noexcept { return g_arena; }

Conductor::JobState* Conductor::find(JobId job) {
    auto it = jobs_.find(job);
    return (it == jobs_.end()) ? nullptr : &it->second;
}

const Conductor::JobState* Conductor::find(JobId job) const {
    auto it = jobs_.find(job);
    return (it == jobs_.end()) ? nullptr : &it->second;
}

JobId Conductor::launch(Duty duty) {
    Duty::handle_type h = duty.release();
    if (!h) {
        return kNoJob;
    }
    const JobId job = next_job_id();
    h.promise().conductor = this;
    h.promise().job = job;

    JobState st;
    st.root = h;
    JobState* state = &jobs_.emplace(job, st).first->second;

    journal_.record_slot(Mark::JobStarted, job, slot_of_frame(h.address()));

    resume_frame(state, h);
    return job;
}

void Conductor::note_parked(JobId job, std::coroutine_handle<> h) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    st->parked = h;
    st->parked_slot = slot_of_frame(h.address());
    journal_.record_slot(Mark::JobParked, job, st->parked_slot);
}

void Conductor::settle(JobId job, Outcome outcome, std::int64_t value) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        // One outcome per job: whatever else arrives afterwards changes
        // nothing.
        return;
    }
    st->settled = true;
    st->outcome = outcome;
    st->value = value;
    st->parked = {};
    st->parked_slot = SlotId{};
    journal_.record(Mark::JobSettled, job, static_cast<std::int64_t>(outcome));
    if (outcome == Outcome::Faulted) {
        journal_.record(Mark::FaultReported, job);
    }
    // The frames go back now, not at the next step and not at shutdown. When
    // the settle happened inside a resume the release waits only until control
    // is back out of that resume, so no frame is destroyed underneath itself.
    release_queue_.push_back(job);
    if (resume_depth_ == 0) {
        flush_releases();
    }
}

void Conductor::flush_releases() {
    if (flushing_) {
        return;
    }
    flushing_ = true;
    while (!release_queue_.empty()) {
        std::vector<JobId> batch;
        batch.swap(release_queue_);
        for (JobId job : batch) {
            JobState* st = find(job);
            if (st == nullptr || !st->root) {
                continue;
            }
            std::coroutine_handle<> r = std::exchange(st->root, {});
            st->parked = {};
            st->parked_slot = SlotId{};
            // Read the name of the storage before it goes back.
            const SlotId slot = slot_of_frame(r.address());
            r.destroy();
            journal_.record_slot(Mark::FrameReleased, job, slot);
        }
    }
    flushing_ = false;
}

void Conductor::resume_frame(JobState* st, std::coroutine_handle<> h) {
    {
        RunGuard guard{this, st};
        h.resume();
    }
    flush_releases();
}

void Conductor::enqueue(JobId job, std::int64_t value, bool faulted, std::uint64_t delay) {
    Pending t;
    t.job = job;
    t.value = value;
    t.faulted = faulted;
    t.ready = tick_ + delay;
    t.seq = next_seq_++;
    // A ticket authorises the wait that was in progress when it was made. A job
    // that was not parked then gets a ticket bound to no frame at all, and no
    // later change of state can bind it.
    const JobState* st = find(job);
    if (st != nullptr && !st->settled && st->parked) {
        t.bound = true;
        t.slot = st->parked_slot;
    }
    queue_.push_back(t);
    journal_.record(Mark::TicketQueued, job);
}

void Conductor::post(JobId job, std::int64_t value) { enqueue(job, value, false, 0); }
void Conductor::post_after(JobId job, std::int64_t value, std::uint64_t delay) {
    enqueue(job, value, false, delay);
}
void Conductor::post_fault(JobId job) { enqueue(job, 0, true, 0); }
void Conductor::post_fault_after(JobId job, std::uint64_t delay) {
    enqueue(job, 0, true, delay);
}

void Conductor::cancel(JobId job) {
    // Every request is recorded, whether or not there is anything to stop.
    journal_.record(Mark::CancelRequested, job);
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    st->cancel_asked = true;
    if (st->running > 0) {
        // The job is on the stack already; the request is carried out where it
        // next tries to wait.
        return;
    }
    if (!st->parked) {
        // Nothing is waiting, so there is nothing to unwind here.
        return;
    }
    st->cancel_asked = false;
    std::coroutine_handle<> target = std::exchange(st->parked, {});
    st->parked_slot = SlotId{};
    // Push the frame along so the job unwinds now: the wait expression raises,
    // each frame runs its cleanup, and the innermost frame goes first.
    auto typed = Duty::handle_type::from_address(target.address());
    typed.promise().cancel_flagged = true;
    resume_frame(st, target);
}

bool Conductor::take_cancel_request(JobId job) noexcept {
    JobState* st = find(job);
    if (st == nullptr || !st->cancel_asked) {
        return false;
    }
    st->cancel_asked = false;
    return true;
}

void Conductor::forget_parked(std::coroutine_handle<> h) noexcept {
    for (auto& kv : jobs_) {
        JobState& st = kv.second;
        if (st.parked && st.parked.address() == h.address()) {
            st.parked = {};
            st.parked_slot = SlotId{};
        }
    }
}

bool Conductor::earlier(const Pending& a, const Pending& b) noexcept {
    if (a.ready != b.ready) {
        return a.ready < b.ready;
    }
    if (a.job != b.job) {
        return a.job < b.job;
    }
    return a.seq < b.seq;
}

void Conductor::deliver(const Pending& t) {
    JobState* st = find(t.job);
    const bool waiting = st != nullptr && !st->settled && static_cast<bool>(st->parked);
    // The frame is named by slot and epoch together. Storage that came back
    // and went out again carries the same index under a new epoch, and two
    // frames alive at once share an epoch under different indices, so both
    // numbers have to match.
    if (!t.bound || !waiting || st->parked_slot != t.slot) {
        // A rejection touches nothing: no frame is resumed and none destroyed.
        journal_.record(Mark::TicketRejected, t.job);
        return;
    }
    journal_.record(Mark::TicketAccepted, t.job, t.value);

    std::coroutine_handle<> target = std::exchange(st->parked, {});
    st->parked_slot = SlotId{};
    auto typed = Duty::handle_type::from_address(target.address());
    if (t.faulted) {
        typed.promise().fault = std::make_exception_ptr(std::runtime_error("posted fault"));
    } else {
        typed.promise().produced = t.value;
    }
    resume_frame(st, target);
}

bool Conductor::step() {
    if (queue_.empty()) {
        return false;
    }
    // The order is fixed by the tickets themselves, not by where they sit:
    // ready time, then job identifier, then the order they were queued in.
    std::size_t best = 0;
    for (std::size_t i = 1; i < queue_.size(); ++i) {
        if (earlier(queue_[i], queue_[best])) {
            best = i;
        }
    }
    const Pending t = queue_[best];
    queue_.erase(queue_.begin() + static_cast<std::ptrdiff_t>(best));
    // The clock moves to the ready time of the ticket this step delivers, and
    // at no other time.
    if (t.ready > tick_) {
        tick_ = t.ready;
    }
    deliver(t);
    return true;
}

std::size_t Conductor::drain() {
    std::size_t n = 0;
    while (step()) {
        ++n;
    }
    return n;
}

std::optional<Outcome> Conductor::outcome_of(JobId job) const {
    const JobState* st = find(job);
    if (st == nullptr || !st->settled) {
        return std::nullopt;
    }
    return st->outcome;
}

std::optional<std::int64_t> Conductor::value_of(JobId job) const {
    const JobState* st = find(job);
    if (st == nullptr || !st->settled || st->outcome != Outcome::Delivered) {
        return std::nullopt;
    }
    return st->value;
}

bool Conductor::settled(JobId job) const {
    const JobState* st = find(job);
    return st != nullptr && st->settled;
}

std::size_t Conductor::live_jobs() const {
    std::size_t n = 0;
    for (const auto& kv : jobs_) {
        if (!kv.second.settled) {
            ++n;
        }
    }
    return n;
}

}  // namespace roundel
