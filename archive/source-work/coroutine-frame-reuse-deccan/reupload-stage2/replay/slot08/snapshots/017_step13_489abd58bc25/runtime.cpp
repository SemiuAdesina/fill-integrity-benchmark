
#include "roundel/runtime.hpp"

#include <algorithm>

namespace roundel {
namespace {

// The arena the next coroutine frame should be taken from. A live Conductor
// installs itself here; nested Conductors save and restore the previous one.
FrameArena* g_arena = nullptr;

// Keeps track of the job whose frames are on the stack inside a resume. A job
// that asks to cancel itself cannot be torn down from inside its own frame, so
// the request is remembered and acted on once control is back with the
// conductor.
class RunGuard {
  public:
    RunGuard(std::vector<JobId>& stack, JobId job) noexcept : stack_(stack) { stack_.push_back(job); }
    ~RunGuard() { stack_.pop_back(); }

    RunGuard(const RunGuard&) = delete;
    RunGuard& operator=(const RunGuard&) = delete;

  private:
    std::vector<JobId>& stack_;
};

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
    if (p == nullptr) {
        return;
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
        // A child hands control back to whoever awaited it. The frame is not
        // destroyed here: the awaiter does that, so that the child is gone
        // before the parent observes its result.
        return p.parent;
    }
    // A root job has nobody to continue into, so it settles here.
    if (p.conductor != nullptr) {
        Outcome o = Outcome::Delivered;
        if (p.cancel_flagged) {
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

std::coroutine_handle<> Duty::ChildAwaiter::await_suspend(handle_type parent) noexcept {
    promise_type& cp = child_.promise();
    promise_type& pp = parent.promise();
    cp.parent = parent;
    cp.conductor = pp.conductor;
    cp.job = pp.job;
    cp.cancel_flagged = pp.cancel_flagged || cp.cancel_flagged;
    if (cp.conductor != nullptr) {
        // From here on the conductor owns the child's frame, so that it can
        // tear the whole stack down innermost first.
        detached_ = cp.conductor->push_frame(cp.job, child_);
    }
    return child_;
}

std::int64_t Duty::ChildAwaiter::await_resume() {
    handle_type child = child_;
    promise_type& cp = child.promise();
    Conductor* c = cp.conductor;
    const JobId job = cp.job;

    // Read everything out of the child before it goes away.
    const std::exception_ptr fault = cp.fault;
    const std::int64_t produced = cp.produced;

    detached_ = true;
    if (c != nullptr) {
        c->finish_child(job, child);
    } else {
        child.destroy();
    }

    if (c != nullptr) {
        // The child's frame is destroyed and its storage is back in the arena
        // by the time the parent is handed the result.
        c->journal().record(Mark::ParentContinued, job,
                            static_cast<std::int64_t>(c->arena().live_slots()));
    }

    if (fault) {
        std::rethrow_exception(fault);
    }
    return produced;
}

// -------------------------------------------------------------------------
// ParkAwaiter
// -------------------------------------------------------------------------

void ParkAwaiter::await_suspend(Duty::handle_type me) noexcept {
    promise = &me.promise();
    Conductor* c = promise->conductor;
    if (c != nullptr) {
        c->note_parked(promise->job, me);
    }
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
    // Anything still open is stopped now, so that every slot is back in the
    // pool before the arena dies.
    std::vector<JobId> open;
    for (const auto& kv : jobs_) {
        if (!kv.second.settled) {
            open.push_back(kv.first);
        }
    }
    std::sort(open.begin(), open.end());
    for (JobId job : open) {
        JobState* st = find(job);
        if (st == nullptr) {
            continue;
        }
        st->cancel_asked = true;
        for (const auto& f : st->frames) {
            if (f) {
                Duty::handle_type::from_address(f.address()).promise().cancel_flagged = true;
            }
        }
        teardown(job);
    }
    std::vector<JobId> all;
    for (const auto& kv : jobs_) {
        all.push_back(kv.first);
    }
    std::sort(all.begin(), all.end());
    for (JobId job : all) {
        release_frames(job);
    }
    release_pending();
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

bool Conductor::is_running(JobId job) const {
    for (JobId j : running_) {
        if (j == job) {
            return true;
        }
    }
    return false;
}

// -------------------------------------------------------------------------
// Frame ownership
// -------------------------------------------------------------------------

void Conductor::push_frame(JobId job, std::coroutine_handle<> h) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        // Nobody to own it: drop the frame rather than leak it.
        if (h) {
            h.destroy();
        }
        return;
    }
    st->frames.push_back(h);
}

void Conductor::finish_child(JobId job, std::coroutine_handle<> child) {
    JobState* st = find(job);
    if (st != nullptr) {
        for (std::size_t i = st->frames.size(); i-- > 0;) {
            if (st->frames[i].address() == child.address()) {
                st->frames.erase(st->frames.begin() + static_cast<std::ptrdiff_t>(i));
                break;
            }
        }
        if (st->parked && st->parked.address() == child.address()) {
            st->parked = {};
        }
    }
    const SlotId slot = FrameArena::slot_of(child.address());
    journal_.record_slot(Mark::ChildDestroyed, job, slot);
    child.destroy();
}

void Conductor::release_frames(JobId job) {
    JobState* st = find(job);
    if (st == nullptr || st->released) {
        return;
    }
    st->released = true;
    st->parked = {};

    // Take the whole stack out first: destroying a frame can run user code,
    // which may well come back into the conductor.
    std::vector<std::coroutine_handle<>> frames;
    frames.swap(st->frames);
    st->root = {};

    std::vector<SlotId> slots;
    slots.reserve(frames.size());
    for (const auto& f : frames) {
        slots.push_back(f ? FrameArena::slot_of(f.address()) : SlotId{});
    }

    // Innermost first: a child's frame goes before the frame that awaited it.
    for (std::size_t i = frames.size(); i-- > 0;) {
        if (!frames[i]) {
            continue;
        }
        if (i + 1 < frames.size()) {
            journal_.record_slot(Mark::ChildDestroyed, job, slots[i]);
        }
        frames[i].destroy();
    }

    journal_.record_slot(Mark::FrameReleased, job, frames.empty() ? SlotId{} : slots.back());
}

// -------------------------------------------------------------------------
// Driving
// -------------------------------------------------------------------------

void Conductor::run_frame(JobId job, Duty::handle_type h) {
    {
        RunGuard guard{running_, job};
        h.resume();
    }
    checkpoint(job);
}

void Conductor::checkpoint(JobId job) {
    JobState* st = find(job);
    if (st != nullptr && !st->settled && st->cancel_asked) {
        teardown(job);
    }
    release_pending();
}

void Conductor::teardown(JobId job) {
    JobState* st = find(job);
    if (st == nullptr || st->settled || is_running(job)) {
        return;
    }
    st->cancel_asked = true;
    for (const auto& f : st->frames) {
        if (f) {
            Duty::handle_type::from_address(f.address()).promise().cancel_flagged = true;
        }
    }
    settle(job, Outcome::Cancelled, 0);
    release_frames(job);
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
    st.frames.push_back(std::coroutine_handle<>(h));
    jobs_.emplace(job, std::move(st));

    journal_.record_slot(Mark::JobStarted, job, FrameArena::slot_of(h.address()));

    run_frame(job, h);
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
    // A ticket authorises the one frame the job was parked in when the ticket
    // was queued. A job that was not parked authorises nothing at all, and no
    // later change of state can bind the ticket to a frame.
    JobState* st = find(job);
    if (st != nullptr && !st->settled && st->parked) {
        t.bound = true;
        t.target = FrameArena::slot_of(st->parked.address());
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
    // The request is recorded once per call, whatever state the job is in and
    // whether or not it was ever launched.
    journal_.record(Mark::CancelRequested, job);

    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    st->cancel_asked = true;
    for (const auto& f : st->frames) {
        if (f) {
            Duty::handle_type::from_address(f.address()).promise().cancel_flagged = true;
        }
    }
    if (is_running(job)) {
        // Asked for from the inside: the frames are on the stack, so they go
        // as soon as control is back with the conductor.
        return;
    }
    teardown(job);
    release_pending();
}

void Conductor::settle(JobId job, Outcome outcome, std::int64_t value) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        // One outcome per job. Anything that arrives afterwards changes
        // nothing and records nothing.
        return;
    }
    st->outcome = outcome;
    st->value = value;
    st->settled = true;
    st->parked = {};
    journal_.record(Mark::JobSettled, job, static_cast<std::int64_t>(outcome));
    if (outcome == Outcome::Faulted) {
        journal_.record(Mark::FaultReported, job);
    }
    pending_release_.push_back(job);
}

void Conductor::release_pending() {
    // Destroying a frame can settle further jobs, so this runs to a fixpoint.
    while (!pending_release_.empty()) {
        std::vector<JobId> batch;
        batch.swap(pending_release_);
        for (JobId job : batch) {
            JobState* st = find(job);
            if (st != nullptr && st->settled) {
                release_frames(job);
            }
        }
    }
}

void Conductor::deliver(const Pending& t) {
    JobState* st = find(t.job);
    std::coroutine_handle<> target{};
    bool accepted = false;
    if (st != nullptr && !st->settled && t.bound && st->parked) {
        // Both numbers have to match. The slot repeats with a new epoch, and
        // two frames alive at once can share an epoch but not a slot.
        const SlotId here = FrameArena::slot_of(st->parked.address());
        if (here.index == t.target.index && here.epoch == t.target.epoch) {
            accepted = true;
            target = st->parked;
        }
    }

    if (!accepted) {
        // A rejection touches nothing at all.
        journal_.record(Mark::TicketRejected, t.job);
        return;
    }

    journal_.record(Mark::TicketAccepted, t.job, t.value);
    st->parked = {};

    auto typed = Duty::handle_type::from_address(target.address());
    if (t.faulted) {
        typed.promise().fault = std::make_exception_ptr(std::runtime_error("posted fault"));
    } else {
        typed.promise().produced = t.value;
    }
    run_frame(t.job, typed);
}

bool Conductor::step() {
    if (queue_.empty()) {
        return false;
    }
    // Earliest ready time, then lowest job identifier, then earliest queued.
    // The stored order is not a tie-break.
    auto best = queue_.begin();
    for (auto it = queue_.begin(); it != queue_.end(); ++it) {
        if (it->ready != best->ready) {
            if (it->ready < best->ready) {
                best = it;
            }
            continue;
        }
        if (it->job != best->job) {
            if (it->job < best->job) {
                best = it;
            }
            continue;
        }
        if (it->seq < best->seq) {
            best = it;
        }
    }
    const Pending t = *best;
    queue_.erase(best);

    // The clock moves to the ready time of the ticket a step delivers, and to
    // nothing else.
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
