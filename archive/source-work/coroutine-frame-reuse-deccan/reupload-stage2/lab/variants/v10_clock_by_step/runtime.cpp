
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

// A finished root does not destroy itself here: it is still executing at this
// point, and a coroutine may only be destroyed while suspended. It records its
// result and is reaped once control is back in the conductor.
std::coroutine_handle<> Duty::promise_type::FinalStage::await_suspend(handle_type me) noexcept {
    promise_type& p = me.promise();
    if (p.parent) {
        // A child hands control back to whoever awaited it. The awaiting
        // parent owns this frame and destroys it.
        return p.parent;
    }
    if (p.conductor != nullptr) {
        Outcome o = Outcome::Delivered;
        if (p.fault) {
            o = Outcome::Faulted;
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

std::coroutine_handle<> Duty::ChildAwaiter::await_suspend(handle_type parent) noexcept {
    promise_type& cp = child_.promise();
    promise_type& pp = parent.promise();
    cp.parent = parent;
    cp.conductor = pp.conductor;
    cp.job = pp.job;
    return child_;
}

std::int64_t Duty::ChildAwaiter::await_resume() {
    // The child is suspended at its final suspension, so it is safe to destroy
    // and it is this awaiter's job to do so. Everything the parent needs is
    // copied out first, and the frame goes away before the parent is told
    // anything at all -- value or failure.
    Duty::handle_type child = std::exchange(child_, {});
    promise_type& cp = child.promise();
    Conductor* c = cp.conductor;
    const JobId job = cp.job;
    const std::int64_t produced = cp.produced;
    std::exception_ptr fault = cp.fault;

    const SlotId slot = FrameArena::slot_of(child.address());
    child.destroy();
    if (c != nullptr) {
        c->journal().record_slot(Mark::ChildDestroyed, job, slot);
        // The count of live frames at this exact instant is what tells the
        // parent the child is really gone, not merely reported gone.
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

bool ParkAwaiter::await_suspend(Duty::handle_type me) noexcept {
    promise = &me.promise();
    Conductor* c = promise->conductor;
    if (c == nullptr) {
        return true;
    }
    if (promise->cancel_flagged || c->cancel_asked(promise->job)) {
        // Cancellation reached this job while it was running. It cannot be
        // torn down from under itself, so it is let go at its own next
        // parking point instead.
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
    // Anything still live is torn down here, innermost frame first, so no
    // storage is left checked out.
    std::vector<JobId> open;
    for (const auto& kv : jobs_) {
        if (!kv.second.settled) {
            open.push_back(kv.first);
        }
    }
    std::sort(open.begin(), open.end());
    for (JobId job : open) {
        settle(job, Outcome::Cancelled, 0);
    }
    reap();
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

bool Conductor::cancel_asked(JobId job) const {
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
    jobs_.emplace(job, st);

    journal_.record_slot(Mark::JobStarted, job, FrameArena::slot_of(h.address()));

    JobState* live = find(job);
    live->running = true;
    h.resume();
    live = find(job);
    if (live != nullptr) {
        live->running = false;
    }
    reap();
    return job;
}

void Conductor::note_parked(JobId job, std::coroutine_handle<> h) {
    JobState* st = find(job);
    if (st == nullptr) {
        return;
    }
    st->parked = h;
    journal_.record(Mark::JobParked, job);
}

// One place where a ticket is made, so the ready time, the queueing order and
// the frame binding are decided identically however the ticket was asked for.
//
// The binding is taken here and never recomputed: it names the frame that is
// parked *now*, by slot index and slot generation together. A job that is not
// parked at this moment gives an unbound ticket, which no later state can turn
// into a valid one.
void Conductor::enqueue(JobId job, std::int64_t value, bool faulted, std::uint64_t delay) {
    Pending t;
    t.job = job;
    t.value = value;
    t.faulted = faulted;
    t.ready = tick_ + delay;
    t.seq = next_seq_++;

    const JobState* st = find(job);
    if (st != nullptr && !st->settled && st->parked) {
        t.frame = FrameArena::slot_of(st->parked.address());
        t.bound = true;
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

// The delivery order, in full: earliest ready time, then lowest job
// identifier, then earliest queued. Every comparison is on a value the
// protocol defines, so neither an address nor the queue's own layout can reach
// the decision.
std::size_t Conductor::pick_next() const noexcept {
    std::size_t best = queue_.size();
    for (std::size_t i = 0; i < queue_.size(); ++i) {
        if (best == queue_.size()) {
            best = i;
            continue;
        }
        const Pending& a = queue_[i];
        const Pending& b = queue_[best];
        if (a.ready != b.ready) {
            if (a.ready < b.ready) {
                best = i;
            }
            continue;
        }
        if (a.job != b.job) {
            if (a.job < b.job) {
                best = i;
            }
            continue;
        }
        if (a.seq < b.seq) {
            best = i;
        }
    }
    return best;
}

// Destroy the frames a job still owns. The root's destruction runs the
// destructors of everything living in that frame, and an awaiter living there
// destroys its child, so inner frames go before outer ones on their own.
void Conductor::release_frames(JobId job) {
    JobState* st = find(job);
    if (st == nullptr || !st->root) {
        return;
    }
    std::coroutine_handle<> r = std::exchange(st->root, {});
    st->parked = {};
    const SlotId slot = FrameArena::slot_of(r.address());
    r.destroy();
    journal_.record_slot(Mark::FrameReleased, job, slot);
}

void Conductor::settle(JobId job, Outcome outcome, std::int64_t value) {
    JobState* st = find(job);
    if (st == nullptr) {
        // A job that was never launched, or one whose record is long gone.
        return;
    }
    if (st->settled) {
        // Settling is once per job, whatever asks for it and however often.
        return;
    }
    st->settled = true;
    st->outcome = outcome;
    st->value = value;
    st->parked = {};
    journal_.record(Mark::JobSettled, job, static_cast<std::int64_t>(outcome));
    if (outcome == Outcome::Faulted) {
        // The single place a failure is announced.
        journal_.record(Mark::FaultReported, job);
    }

    if (st->running) {
        // Its own frame is on the stack right now. Destroying it here would be
        // destroying a coroutine that is not suspended; it waits for reap().
        reap_.push_back(job);
        return;
    }
    release_frames(job);
}

void Conductor::reap() {
    while (!reap_.empty()) {
        std::vector<JobId> batch;
        batch.swap(reap_);
        for (JobId job : batch) {
            JobState* st = find(job);
            if (st == nullptr || st->running) {
                continue;
            }
            release_frames(job);
        }
    }
}

void Conductor::cancel(JobId job) {
    JobState* st = find(job);
    if (st == nullptr) {
        // Never launched, or already forgotten. Nothing to do and nothing to
        // complain about.
        journal_.record(Mark::CancelRequested, job);
        return;
    }
    journal_.record(Mark::CancelRequested, job);
    if (st->settled) {
        // Already finished. Cancellation arriving afterwards changes nothing.
        return;
    }
    st->cancel_asked = true;
    settle(job, Outcome::Cancelled, 0);
}

void Conductor::deliver(const Pending& t) {
    JobState* st = find(t.job);
    if (st == nullptr || st->settled || !st->parked) {
        // The job this ticket names is gone, finished, or not waiting for
        // anything. Whatever occupies its old storage now is none of this
        // ticket's business.
        journal_.record(Mark::TicketRejected, t.job);
        return;
    }

    // The job is the right job and it is waiting -- but a job is a stack of
    // frames, and the frame waiting now need not be the frame this ticket was
    // made for. Both halves of the binding have to agree: the slot index,
    // because a different frame of the same job is a different wait; and the
    // slot generation, because the pool hands the same index out again and the
    // frame living at it now is a different frame that happens to share an
    // address. Either half alone lets a stale completion through.
    if (!t.bound || FrameArena::slot_of(st->parked.address()) != t.frame) {
        journal_.record(Mark::TicketRejected, t.job);
        return;
    }

    std::coroutine_handle<> target = std::exchange(st->parked, {});
    journal_.record(Mark::TicketAccepted, t.job, t.value);

    auto typed = Duty::handle_type::from_address(target.address());
    if (t.faulted) {
        typed.promise().fault = std::make_exception_ptr(std::runtime_error("posted fault"));
    } else {
        typed.promise().produced = t.value;
    }

    st->running = true;
    typed.resume();
    st = find(t.job);
    if (st != nullptr) {
        st->running = false;
    }
    reap();
}

bool Conductor::step() {
    const std::size_t i = pick_next();
    if (i == queue_.size()) {
        return false;
    }
    const Pending t = queue_[i];
    queue_.erase(queue_.begin() + static_cast<std::deque<Pending>::difference_type>(i));
    // The clock follows the work: it moves to the ready time of the ticket
    // being delivered, and never backwards, so a ticket queued from inside a
    // resumed job is ready no earlier than the step that resumed it.
    ++tick_;
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
