
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
        // A child hands control back to whoever awaited it.
        return p.parent;
    }
    // A root job has nobody to continue into, so it settles here. The frame is
    // still suspended at its final suspension: the conductor destroys it once
    // control has come back to it.
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
    if (cp.conductor != nullptr) {
        // The parent has handed control to the child: it is not waiting for a
        // ticket, so nothing is parked for this job meanwhile.
        cp.conductor->note_child_started(pp.job);
    }
    return child_;
}

std::int64_t Duty::ChildAwaiter::await_resume() {
    promise_type& cp = child_.promise();
    Conductor* c = cp.conductor;
    const JobId job = cp.job;

    // Read the result out while the frame is still alive.
    const std::exception_ptr fault = cp.fault;
    const std::int64_t value = cp.produced;
    const SlotId slot = FrameArena::slot_of(child_.address());

    // The child is gone before the parent sees anything of it. Destroying the
    // frame runs whatever the frame holds and puts its storage back now.
    detached_ = true;
    std::coroutine_handle<> dead = std::exchange(child_, {});
    dead.destroy();

    if (c != nullptr) {
        c->note_child_finished(job);
        c->journal().record_slot(Mark::ChildDestroyed, job, slot);
        c->journal().record(Mark::ParentContinued, job,
                            static_cast<std::int64_t>(c->arena().live_slots()));
    }

    if (fault) {
        std::rethrow_exception(fault);
    }
    return value;
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
    tearing_down_ = true;

    // Whatever is still running is stopped here, so that no frame outlives the
    // conductor and every slot is back in the pool.
    std::vector<JobId> ids;
    ids.reserve(jobs_.size());
    for (const auto& kv : jobs_) {
        ids.push_back(kv.first);
    }
    std::sort(ids.begin(), ids.end());
    for (JobId job : ids) {
        JobState* st = find(job);
        if (st != nullptr && !st->settled) {
            st->cancel_asked = true;
            unwind(job);
        }
    }
    for (JobId job : ids) {
        JobState* st = find(job);
        if (st == nullptr || !st->root) {
            continue;
        }
        std::coroutine_handle<> r = std::exchange(st->root, {});
        st->parked = {};
        st->has_parked = false;
        const SlotId slot = FrameArena::slot_of(r.address());
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

bool Conductor::is_running(JobId job) const noexcept {
    return std::find(running_.begin(), running_.end(), job) != running_.end();
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
    return job;
}

void Conductor::note_parked(JobId job, std::coroutine_handle<> h) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    st->parked = h;
    st->has_parked = true;
    // Named here, while the frame is alive: the slot's epoch moves on as soon
    // as the storage is handed out again.
    st->parked_slot = FrameArena::slot_of(h.address());
    journal_.record(Mark::JobParked, job);
}

void Conductor::note_child_started(JobId job) {
    JobState* st = find(job);
    if (st == nullptr) {
        return;
    }
    st->parked = {};
    st->has_parked = false;
}

void Conductor::note_child_finished(JobId job) {
    JobState* st = find(job);
    if (st == nullptr) {
        return;
    }
    // The frame the job was waiting in is gone; it is not parked any more.
    st->parked = {};
    st->has_parked = false;
}

void Conductor::enqueue(JobId job, std::int64_t value, bool faulted, std::uint64_t delay) {
    Pending t;
    t.job = job;
    t.value = value;
    t.faulted = faulted;
    t.ready = tick_ + delay;
    t.seq = next_seq_++;
    // A ticket is bound to the frame the job is parked in at this moment, and
    // to no frame at all when it is not parked.
    JobState* st = find(job);
    if (st != nullptr && !st->settled && st->has_parked) {
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
    // Every request is recorded, whatever it names.
    journal_.record(Mark::CancelRequested, job);

    JobState* st = find(job);
    if (st == nullptr || st->settled || tearing_down_) {
        return;
    }
    st->cancel_asked = true;

    if (is_running(job)) {
        // The frame is on the stack. It cannot be destroyed from here, so it is
        // stopped once control has come back to the conductor.
        return;
    }
    unwind(job);
}

void Conductor::unwind(JobId job) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    if (st->has_parked && st->parked) {
        // A cancelled job is never resumed, so make that plain in the frame too.
        auto typed = Duty::handle_type::from_address(st->parked.address());
        typed.promise().cancel_flagged = true;
    }
    settle(job, Outcome::Cancelled, 0);
    release_pending();
}

void Conductor::settle(JobId job, Outcome outcome, std::int64_t value) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        // One outcome per job, reported once.
        return;
    }
    if (outcome == Outcome::Delivered && st->cancel_asked) {
        // Cancellation is decisive: a job that was asked to stop never
        // delivers a value.
        outcome = Outcome::Cancelled;
    }
    st->outcome = outcome;
    st->value = value;
    st->settled = true;
    journal_.record(Mark::JobSettled, job, static_cast<std::int64_t>(outcome));
    if (outcome == Outcome::Faulted) {
        journal_.record(Mark::FaultReported, job);
    }
    pending_release_.push_back(job);
    if (!is_running(job)) {
        // Settled from outside a frame -- a cancellation -- so the frames can
        // go back to the arena here and now.
        release_pending();
    }
}

void Conductor::release_pending() {
    while (!pending_release_.empty()) {
        const JobId job = pending_release_.front();
        pending_release_.erase(pending_release_.begin());
        JobState* st = find(job);
        if (st == nullptr || !st->root) {
            continue;
        }
        // Destroying the outermost frame destroys the frames it holds,
        // innermost first, and returns every slot they occupy.
        std::coroutine_handle<> r = std::exchange(st->root, {});
        st->parked = {};
        st->has_parked = false;
        const SlotId slot = FrameArena::slot_of(r.address());
        r.destroy();
        journal_.record_slot(Mark::FrameReleased, job, slot);
    }
}

void Conductor::deliver(const Pending& t) {
    JobState* st = find(t.job);
    const bool live = st != nullptr && !st->settled && st->has_parked && static_cast<bool>(st->parked);
    // Both numbers have to match: the slot repeats with a new epoch, and two
    // frames alive together can share an epoch under different slots.
    const bool same_frame =
        live && t.bound && st->parked_slot.index == t.slot.index && st->parked_slot.epoch == t.slot.epoch;

    if (!same_frame) {
        // A rejection touches nothing: no frame is resumed, no frame destroyed.
        journal_.record(Mark::TicketRejected, t.job);
        return;
    }

    journal_.record(Mark::TicketAccepted, t.job, t.value);

    std::coroutine_handle<> target = std::exchange(st->parked, {});
    st->has_parked = false;
    auto typed = Duty::handle_type::from_address(target.address());
    if (t.faulted) {
        typed.promise().fault = std::make_exception_ptr(std::runtime_error("posted fault"));
    } else {
        typed.promise().produced = t.value;
    }
    resume_frame(t.job, target);
}

void Conductor::resume_frame(JobId job, std::coroutine_handle<> h) {
    {
        // While the frame is on the stack it cannot be destroyed, so a
        // cancellation for it waits until control has come back here.
        running_.push_back(job);
        struct Pop {
            Conductor* self;
            ~Pop() { self->running_.pop_back(); }
        } pop{this};
        h.resume();
    }
    finish_after_resume();
}

void Conductor::finish_after_resume() {
    // A job that settled while it ran gives its storage back now.
    release_pending();
    // A cancellation that had to wait for the frame to stop running acts now.
    process_cancels();
}

void Conductor::process_cancels() {
    std::vector<JobId> due;
    for (const auto& kv : jobs_) {
        if (kv.second.cancel_asked && !kv.second.settled && !is_running(kv.first)) {
            due.push_back(kv.first);
        }
    }
    if (due.empty()) {
        return;
    }
    std::sort(due.begin(), due.end());
    for (JobId job : due) {
        JobState* st = find(job);
        if (st != nullptr && st->cancel_asked && !st->settled && !is_running(job)) {
            unwind(job);
        }
    }
}

bool Conductor::step() {
    if (queue_.empty()) {
        return false;
    }
    // Lowest ready time, then lowest job identifier, then earliest queued. The
    // order the queue happens to be stored in is not part of this.
    auto it = std::min_element(queue_.begin(), queue_.end(),
                               [](const Pending& a, const Pending& b) {
                                   if (a.ready != b.ready) {
                                       return a.ready < b.ready;
                                   }
                                   if (a.job != b.job) {
                                       return a.job < b.job;
                                   }
                                   return a.seq < b.seq;
                               });
    Pending t = *it;
    queue_.erase(it);

    // The clock moves at the start of a delivery and never backwards.
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
