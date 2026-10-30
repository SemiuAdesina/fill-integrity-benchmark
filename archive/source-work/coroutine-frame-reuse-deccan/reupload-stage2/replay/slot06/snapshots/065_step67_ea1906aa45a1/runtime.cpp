
#include "roundel/runtime.hpp"

#include <algorithm>

namespace roundel {
namespace {

// The arena the next coroutine frame should be taken from. A live Conductor
// installs itself here; nested Conductors save and restore the previous one.
FrameArena* g_arena = nullptr;

// Identity of the storage handed out by the most recent frame allocation. A
// promise is built straight after its frame, and there is nothing else the
// promise constructor could read the slot from.
SlotId g_last_slot{};

constexpr std::uint32_t kUnpooledSlot = 0xFFFFFFFFu;

// Frames taken from an arena are remembered here. `operator delete` is handed
// nothing but a pointer, and the block header an arena keeps in front of its
// blocks may only be read for a pointer that came from one, so a frame that
// was not pooled has to be recognisable without touching the memory in front
// of it.
using OwnerEntry = std::pair<void*, FrameArena*>;

std::vector<OwnerEntry>& arena_frames() {
    static std::vector<OwnerEntry>* table = new std::vector<OwnerEntry>();
    return *table;
}

}  // namespace

// -------------------------------------------------------------------------
// Frame storage
// -------------------------------------------------------------------------

void* Duty::promise_type::operator new(std::size_t bytes) {
    if (g_arena == nullptr) {
        g_last_slot = SlotId{kUnpooledSlot, 0};
        return ::operator new(bytes);
    }
    void* p = g_arena->check_out(bytes);
    g_last_slot = FrameArena::slot_of(p);
    arena_frames().push_back(OwnerEntry{p, g_arena});
    return p;
}

void Duty::promise_type::operator delete(void* p, std::size_t bytes) noexcept {
    (void)bytes;
    std::vector<OwnerEntry>& table = arena_frames();
    for (std::size_t i = 0; i < table.size(); ++i) {
        if (table[i].first != p) {
            continue;
        }
        FrameArena* owner = table[i].second;
        table[i] = table.back();
        table.pop_back();
        owner->reclaim(p);
        return;
    }
    ::operator delete(p);
}

Duty::promise_type::promise_type() noexcept : slot(g_last_slot) {}

std::coroutine_handle<> Duty::promise_type::FinalStage::await_suspend(handle_type me) noexcept {
    promise_type& p = me.promise();
    if (p.parent) {
        // A child hands control back to whoever awaited it.
        return p.parent;
    }
    // A root job has nobody to continue into, so it settles here. The frame
    // itself is at its final suspension; the conductor releases it as soon as
    // control is back with it.
    if (p.conductor != nullptr) {
        Outcome o = Outcome::Delivered;
        if (p.conductor->cancel_asked(p.job)) {
            o = Outcome::Cancelled;
        } else if (p.fault) {
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
        dispose();
    }
}

void Duty::ChildAwaiter::dispose() noexcept {
    if (!child_) {
        return;
    }
    // Hand the frame over: after this neither the awaiter nor the parent
    // promise owns it, so it can never be destroyed twice.
    if (parent_promise_ != nullptr && parent_promise_->awaiting_child &&
        parent_promise_->awaiting_child.address() == child_.address()) {
        parent_promise_->awaiting_child = {};
    }
    detached_ = true;
    child_.destroy();
}

std::coroutine_handle<> Duty::ChildAwaiter::await_suspend(handle_type parent) noexcept {
    promise_type& cp = child_.promise();
    promise_type& pp = parent.promise();
    parent_promise_ = &pp;
    cp.parent = parent;
    cp.conductor = pp.conductor;
    cp.job = pp.job;
    // The awaiting frame owns the child while it is suspended on it.
    pp.awaiting_child = child_;
    if (cp.conductor != nullptr && cp.conductor->cancel_asked(cp.job)) {
        // Do not start the child: hand control straight back so the job
        // unwinds instead of running more of itself.
        skipped_ = true;
        return parent;
    }
    return child_;
}

std::int64_t Duty::ChildAwaiter::await_resume() {
    promise_type& cp = child_.promise();
    Conductor* c = cp.conductor;
    const JobId job = cp.job;
    const std::int64_t value = cp.produced;
    std::exception_ptr err = cp.fault;

    // The child's frame and its storage are gone before the awaiting job
    // observes anything at all -- a value or a re-raised failure.
    if (c != nullptr) {
        c->journal().record(Mark::ChildDestroyed, job);
    }
    dispose();

    if (skipped_ || (c != nullptr && c->cancel_asked(job))) {
        throw Abandoned{};
    }
    if (err) {
        std::rethrow_exception(err);
    }
    if (c != nullptr) {
        c->journal().record(Mark::ParentContinued, job,
                            static_cast<std::int64_t>(c->arena().live_slots()));
    }
    return value;
}

// -------------------------------------------------------------------------
// ParkAwaiter
// -------------------------------------------------------------------------

bool ParkAwaiter::await_suspend(Duty::handle_type me) noexcept {
    promise = &me.promise();
    Conductor* c = promise->conductor;
    if (c == nullptr) {
        return true;
    }
    if (c->cancel_asked(promise->job) && !promise->cancel_flagged) {
        // Cancellation was asked for: do not wait for anything. The frame is
        // marked so that it unwinds once, from this spot.
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
    Conductor* c = promise->conductor;
    if (promise->cancel_flagged || (c != nullptr && c->cancel_asked(promise->job))) {
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
    std::vector<JobId> open;
    for (const auto& kv : jobs_) {
        if (!kv.second.settled) {
            open.push_back(kv.first);
        }
    }
    std::sort(open.begin(), open.end());
    for (JobId job : open) {
        cancel(job);
    }
    release_pending();
    for (auto& kv : jobs_) {
        JobState& st = kv.second;
        if (st.root) {
            destroy_stack(st.parked ? st.parked : st.root);
            st.parked = {};
            st.root = {};
            if (!st.released) {
                st.released = true;
                journal_.record_slot(Mark::FrameReleased, kv.first, st.root_slot);
            }
        }
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

bool Conductor::cancel_asked(JobId job) const noexcept {
    const JobState* st = find(job);
    return st != nullptr && st->cancel_asked;
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
    st.root_slot = h.promise().slot;
    jobs_.emplace(job, st);

    journal_.record_slot(Mark::JobStarted, job, st.root_slot);

    resume_frame(h, job);
    release_pending();
    return job;
}

void Conductor::note_parked(JobId job, std::coroutine_handle<> h) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    st->parked = h;
    journal_.record(Mark::JobParked, job);
}

void Conductor::enqueue(JobId job, std::int64_t value, bool faulted, std::uint64_t delay) {
    Pending t;
    t.job = job;
    t.value = value;
    t.faulted = faulted;
    t.ready = tick_ + delay;
    t.seq = next_seq_++;
    // A ticket authorises the wait that is happening now: the frame the job is
    // parked in at this instant, named by its slot and that slot's epoch. A job
    // that is not parked authorises no frame at all.
    JobState* st = find(job);
    if (st != nullptr && !st->settled && st->parked) {
        t.bound = true;
        t.slot = Duty::handle_type::from_address(st->parked.address()).promise().slot;
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
    // Asking is recorded once per call, whatever state the job is in and even
    // when no such job was ever launched.
    journal_.record(Mark::CancelRequested, job);

    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    st->cancel_asked = true;

    if (running_job_ == job) {
        // The job is inside its own frames. Destroying them under its feet is
        // not possible, so it is pushed along: it unwinding at its next
        // parking spot, and settles Cancelled if it gets there first.
        return;
    }

    std::coroutine_handle<> parked = st->parked;
    if (parked) {
        Duty::handle_type typed = Duty::handle_type::from_address(parked.address());
        typed.promise().cancel_flagged = true;
        st->parked = {};
        resume_frame(typed, job);
        release_pending();
        st = find(job);
    }

    // Cancellation is decisive: whatever is left of the job stops here.
    if (st != nullptr && !st->settled) {
        finish_now(job, Outcome::Cancelled);
    }
}

void Conductor::finish_now(JobId job, Outcome outcome) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    settle(job, outcome, 0);
    release_pending();
}

void Conductor::settle(JobId job, Outcome outcome, std::int64_t value) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        // One outcome per job: settling twice changes nothing.
        return;
    }
    st->outcome = outcome;
    st->value = value;
    st->settled = true;
    journal_.record(Mark::JobSettled, job, static_cast<std::int64_t>(outcome));
    if (outcome == Outcome::Faulted) {
        journal_.record(Mark::FaultReported, job);
    }
    pending_release_.push_back(job);
}

void Conductor::release_pending() {
    while (!pending_release_.empty()) {
        const JobId job = pending_release_.front();
        pending_release_.erase(pending_release_.begin());
        JobState* st = find(job);
        if (st == nullptr) {
            continue;
        }
        if (st->root) {
            destroy_stack(st->parked ? st->parked : st->root);
            st->parked = {};
            st->root = {};
        }
        if (!st->released) {
            st->released = true;
            journal_.record_slot(Mark::FrameReleased, job, st->root_slot);
        }
    }
}

void Conductor::destroy_stack(std::coroutine_handle<> innermost) {
    std::coroutine_handle<> h = innermost;
    while (h) {
        Duty::handle_type typed = Duty::handle_type::from_address(h.address());
        Duty::promise_type& p = typed.promise();
        std::coroutine_handle<> up = p.parent;
        if (up) {
            // Inner frames go first, and the awaiting frame is told it no
            // longer owns this child before it is destroyed itself.
            Duty::promise_type& pp = Duty::handle_type::from_address(up.address()).promise();
            if (pp.awaiting_child && pp.awaiting_child.address() == h.address()) {
                pp.awaiting_child = {};
            }
            journal_.record(Mark::ChildDestroyed, p.job);
        }
        typed.destroy();
        h = up;
    }
}

void Conductor::resume_frame(Duty::handle_type h, JobId job) {
    const JobId saved = running_job_;
    running_job_ = job;
    try {
        h.resume();
    } catch (...) {
        running_job_ = saved;
        throw;
    }
    running_job_ = saved;
    // Cancellation is decisive: a job that was asked to stop and has not
    // settled on its own does not get to carry on.
    JobState* st = find(job);
    if (st != nullptr && !st->settled && st->cancel_asked) {
        finish_now(job, Outcome::Cancelled);
    }
}

void Conductor::deliver(const Pending& t) {
    JobState* st = find(t.job);
    Duty::handle_type target{};
    if (st != nullptr && !st->settled && st->parked && t.bound) {
        Duty::handle_type parked = Duty::handle_type::from_address(st->parked.address());
        if (parked.promise().slot == t.slot) {
            target = parked;
        }
    }
    if (!target) {
        // Nobody to give it to: a rejection touches nothing at all.
        journal_.record(Mark::TicketRejected, t.job);
        return;
    }

    journal_.record(Mark::TicketAccepted, t.job, t.value);
    Duty::promise_type& p = target.promise();
    if (t.faulted) {
        p.fault = std::make_exception_ptr(std::runtime_error("posted fault"));
    } else {
        p.produced = t.value;
    }
    st->parked = {};
    resume_frame(target, t.job);
}

bool Conductor::step() {
    if (queue_.empty()) {
        return false;
    }
    // Earliest ready time, then lowest job identifier, then earliest queued.
    auto best = queue_.begin();
    for (auto it = queue_.begin(); it != queue_.end(); ++it) {
        bool better;
        if (it->ready != best->ready) {
            better = it->ready < best->ready;
        } else if (it->job != best->job) {
            better = it->job < best->job;
        } else {
            better = it->seq < best->seq;
        }
        if (better) {
            best = it;
        }
    }
    const Pending t = *best;
    queue_.erase(best);
    if (t.ready > tick_) {
        tick_ = t.ready;
    }
    deliver(t);
    release_pending();
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
