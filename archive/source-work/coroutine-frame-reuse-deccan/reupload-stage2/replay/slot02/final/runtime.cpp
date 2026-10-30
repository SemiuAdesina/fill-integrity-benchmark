
#include "roundel/runtime.hpp"

#include <algorithm>

namespace roundel {
namespace {

// The arena the next coroutine frame should be taken from. A live Conductor
// installs itself here; nested Conductors save and restore the previous one.
FrameArena* g_arena = nullptr;

}  // namespace

// -------------------------------------------------------------------------
// Frame storage
// -------------------------------------------------------------------------

void* Duty::promise_type::operator new(std::size_t bytes) {
    if (g_arena == nullptr) {
        return ::operator new(bytes);
    }
    return g_arena->check_out(bytes);
}

void Duty::promise_type::operator delete(void* p, std::size_t bytes) noexcept {
    (void)bytes;
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
        // A child hands control back to whoever awaited it. The child is not
        // settled here: the job settles when its outermost frame does.
        return p.parent;
    }
    // A root job has nobody to continue into, so it settles here. Whether the
    // job was cancelled is decided by the conductor, which counts the requests
    // rather than trusting the frame alone.
    if (p.conductor != nullptr) {
        Outcome o = Outcome::Delivered;
        if (p.fault) {
            o = Outcome::Faulted;
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
        const JobId job = child_.promise().job;
        Conductor* c = child_.promise().conductor;
        handle_type child = std::exchange(child_, {});
        child.destroy();
        if (c != nullptr && job != kNoJob) {
            c->journal().record(Mark::ChildDestroyed, job);
        }
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
    const JobId job = cp.job;
    Conductor* c = cp.conductor;
    std::coroutine_handle<> parent = cp.parent;

    // Everything the child holds is read before the frame goes: after
    // destroy() the promise is no longer there to ask.
    const std::exception_ptr fault = cp.fault;
    const std::int64_t produced = cp.produced;
    const bool abandoned = cp.cancel_flagged;

    // The child is destroyed here, so the awaiting job can never observe a
    // child that is still alive, and its storage is back in the arena before
    // the parent sees a result. Inner frames go first: destroying this frame
    // destroys any frame it is itself awaiting.
    handle_type child = child_;
    child_ = {};
    detached_ = true;
    child.destroy();

    if (c != nullptr) {
        c->journal().record(Mark::ChildDestroyed, job);
        if (fault && !abandoned) {
            // A real failure, as opposed to a cancellation travelling up.
            c->report_fault(job);
        }
        if (abandoned && parent) {
            // The whole job is being stopped, so the frame above has to know.
            Duty::handle_type::from_address(parent.address()).promise().cancel_flagged = true;
        }
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
        return true;  // nobody is driving this job; it simply waits
    }
    if (c->cancel_requested(promise->job)) {
        // Asked to stop while it was running: do not wait, abandon at once.
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
    // Stop whatever is still open so that no frame outlives the arena. Cancelling
    // unwinds the frames, which runs the cleanup they hold; anything left over is
    // destroyed here so its storage goes back to the pool.
    for (;;) {
        JobId open = kNoJob;
        for (JobId id : all_ids()) {
            const JobState* st = find(id);
            if (st != nullptr && !st->settled) {
                open = id;
                break;
            }
        }
        if (open == kNoJob) {
            break;
        }
        try {
            cancel(open);
        } catch (...) {
            // A job that will not unwind is torn down by the sweep below.
            break;
        }
    }
    release_settled();
    for (JobId id : all_ids()) {
        JobState* st = find(id);
        if (st == nullptr || !st->root) {
            continue;
        }
        std::coroutine_handle<> r = std::exchange(st->root, {});
        st->parked = {};
        st->released = true;
        const SlotId slot = FrameArena::slot_of(r.address());
        r.destroy();
        journal_.record_slot(Mark::FrameReleased, id, slot);
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

std::vector<JobId> Conductor::all_ids() const {
    std::vector<JobId> ids;
    ids.reserve(jobs_.size());
    for (const auto& kv : jobs_) {
        ids.push_back(kv.first);
    }
    std::sort(ids.begin(), ids.end());
    return ids;
}

bool Conductor::cancel_requested(JobId job) const {
    const JobState* st = find(job);
    return st != nullptr && !st->settled && st->cancel_asked;
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
    jobs_.emplace(job, st);

    journal_.record_slot(Mark::JobStarted, job, FrameArena::slot_of(h.address()));

    resume_frame(job, h);
    pump_cancels();
    return job;
}

void Conductor::note_parked(JobId job, std::coroutine_handle<> h) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    st->parked = h;
    // The frame's name is read while the frame is alive and kept, so a later
    // delivery never has to ask storage that may since have been handed out
    // again.
    st->parked_slot = FrameArena::slot_of(h.address());
    journal_.record(Mark::JobParked, job);
}

void Conductor::enqueue(JobId job, std::int64_t value, bool faulted, std::uint64_t delay) {
    Pending t;
    t.job = job;
    t.value = value;
    t.faulted = faulted;
    t.ready = tick_ + delay;
    t.seq = next_seq_++;
    JobState* st = find(job);
    if (st != nullptr && !st->settled && st->parked) {
        // This ticket authorises one wait: the one the job is in right now.
        t.bound = true;
        t.target = st->parked;
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

void Conductor::post_fault_after(JobId job, std::uint64_t delay) { enqueue(job, 0, true, delay); }

void Conductor::cancel(JobId job) {
    // Every request is recorded, whoever it names and whatever the job's state.
    journal_.record(Mark::CancelRequested, job);
    JobState* st = find(job);
    if (st == nullptr) {
        return;
    }
    st->cancel_asked = true;
    if (st->settled) {
        return;
    }
    // A parked job is pushed along now, so it unwinds, settles and hands its
    // storage back before this call returns. A running one is left to reach its
    // next wait, which will not wait.
    if (st->parked && !st->running()) {
        unwind(job);
    }
}

void Conductor::settle(JobId job, Outcome outcome, std::int64_t value) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;  // one outcome per job, however many times it is reached
    }
    if (st->cancel_asked) {
        // Cancellation is decisive: a job asked to stop never delivers.
        outcome = Outcome::Cancelled;
    }
    st->outcome = outcome;
    st->value = value;
    st->settled = true;
    st->parked = {};
    st->parked_slot = SlotId{};
    journal_.record(Mark::JobSettled, job, static_cast<std::int64_t>(outcome));
    if (outcome == Outcome::Faulted) {
        report_fault(job);
    }
}

void Conductor::report_fault(JobId job) {
    JobState* st = find(job);
    if (st == nullptr || st->fault_reported) {
        return;
    }
    st->fault_reported = true;
    journal_.record(Mark::FaultReported, job);
}

void Conductor::resume_frame(JobId job, std::coroutine_handle<> h) {
    JobState* st = find(job);
    if (st == nullptr) {
        return;
    }
    ++st->depth;
    h.resume();
    // The job's own records survive anything the resume did, but a frame that
    // is still running must never be destroyed underneath itself.
    st = find(job);
    if (st != nullptr) {
        --st->depth;
    }
    release_settled();
}

void Conductor::unwind(JobId job) {
    JobState* st = find(job);
    if (st == nullptr || st->settled || !st->parked || st->running()) {
        return;
    }
    std::coroutine_handle<> h = std::exchange(st->parked, {});
    st->parked_slot = SlotId{};
    // The wait it comes back from raises, so the job unwinds: whatever its
    // frames hold is cleaned up on the way out.
    Duty::handle_type::from_address(h.address()).promise().cancel_flagged = true;
    resume_frame(job, h);
}

void Conductor::release_settled() {
    for (;;) {
        JobId chosen = kNoJob;
        for (JobId id : all_ids()) {
            JobState* st = find(id);
            if (st == nullptr || !st->settled || st->released || !st->root || st->running()) {
                continue;
            }
            chosen = id;
            break;
        }
        if (chosen == kNoJob) {
            return;
        }
        JobState* st = find(chosen);
        std::coroutine_handle<> r = std::exchange(st->root, {});
        st->parked = {};
        st->released = true;
        const SlotId slot = FrameArena::slot_of(r.address());
        // Destroying the outermost frame destroys the frames it holds, innermost
        // first, and returns all of their storage.
        r.destroy();
        journal_.record_slot(Mark::FrameReleased, chosen, slot);
    }
}

void Conductor::pump_cancels() {
    for (;;) {
        JobId chosen = kNoJob;
        for (JobId id : all_ids()) {
            JobState* st = find(id);
            if (st == nullptr || st->settled || st->running() || !st->cancel_asked || !st->parked) {
                continue;
            }
            chosen = id;
            break;
        }
        if (chosen == kNoJob) {
            return;
        }
        unwind(chosen);
    }
}

void Conductor::deliver(const Pending& t) {
    JobState* st = find(t.job);
    const bool acceptable = t.bound && st != nullptr && !st->settled && bool(st->parked) &&
                            st->parked_slot == t.slot;
    if (!acceptable) {
        // Gone, settled, not waiting, or waiting in another frame: the ticket
        // authorises nobody and touches nothing.
        journal_.record(Mark::TicketRejected, t.job);
        return;
    }
    journal_.record(Mark::TicketAccepted, t.job, t.value);

    std::coroutine_handle<> h = std::exchange(st->parked, {});
    st->parked_slot = SlotId{};
    auto typed = Duty::handle_type::from_address(h.address());
    if (t.faulted) {
        typed.promise().fault = std::make_exception_ptr(std::runtime_error("posted fault"));
    } else {
        typed.promise().produced = t.value;
    }
    resume_frame(t.job, h);
}

bool Conductor::step() {
    if (queue_.empty()) {
        return false;
    }
    auto chosen = queue_.begin();
    for (auto it = queue_.begin(); it != queue_.end(); ++it) {
        const Pending& a = *it;
        const Pending& b = *chosen;
        if (a.ready != b.ready ? a.ready < b.ready
                               : (a.job != b.job ? a.job < b.job : a.seq < b.seq)) {
            chosen = it;
        }
    }
    Pending t = *chosen;
    queue_.erase(chosen);
    // The clock moves to the ready time of the ticket a step delivers, and at
    // no other time. It never moves backwards.
    if (t.ready > tick_) {
        tick_ = t.ready;
    }
    deliver(t);
    pump_cancels();
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
