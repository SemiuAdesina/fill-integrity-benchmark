
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
        // A child hands control back to whoever awaited it. The awaiter
        // destroys this frame before it observes the result.
        return p.parent;
    }
    // A root job has nobody to continue into, so it settles here. Its frame is
    // released by whoever resumed it, as soon as control comes back to them.
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
        // From here on the conductor owns the child's frame: it is on the job's
        // frame stack, and the awaiter must not destroy it a second time if the
        // parent's frame is torn down underneath it.
        cp.conductor->push_frame(cp.job, child_);
        detached_ = true;
    }
    return child_;
}

std::int64_t Duty::ChildAwaiter::await_resume() {
    promise_type& cp = child_.promise();
    Conductor* c = cp.conductor;
    const JobId job = cp.job;
    // Read everything out of the child before its frame goes away.
    const std::int64_t v = cp.produced;
    std::exception_ptr e = cp.fault;

    if (c != nullptr) {
        c->finish_child(job, child_);
    } else {
        child_.destroy();
    }
    child_ = {};
    detached_ = true;

    if (c != nullptr) {
        // The child's frame is destroyed and its storage is back in the arena,
        // so the count here is the count at this instant.
        c->journal().record(Mark::ParentContinued, job,
                            static_cast<std::int64_t>(c->arena().live_slots()));
    }
    if (e) {
        std::rethrow_exception(e);
    }
    return v;
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
    // Whatever is still open is stopped and its storage handed back, so the
    // arena goes away empty.
    std::vector<JobId> ids;
    ids.reserve(jobs_.size());
    for (const auto& kv : jobs_) {
        ids.push_back(kv.first);
    }
    std::sort(ids.begin(), ids.end());
    for (JobId id : ids) {
        JobState* st = find(id);
        if (st != nullptr && !st->settled) {
            teardown(id);
        }
    }
    for (JobId id : ids) {
        release_frames(id);
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
    st.stack.push_back(h);
    jobs_.emplace(job, std::move(st));

    journal_.record_slot(Mark::JobStarted, job, FrameArena::slot_of(h.address()));

    run_frame(job, h);
    return job;
}

void Conductor::note_parked(JobId job, std::coroutine_handle<> h) {
    (void)h;
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    if (st->cancel_asked) {
        // Cancellation was asked for while this job was running. It does not
        // wait at all: the conductor tears it down as soon as control comes
        // back, and nothing is resumed.
        st->cancel_pending = true;
        return;
    }
    st->parked = true;
    journal_.record(Mark::JobParked, job);
}

void Conductor::push_frame(JobId job, Duty::handle_type child) {
    JobState* st = find(job);
    if (st == nullptr) {
        return;
    }
    st->stack.push_back(child);
    // The parent is suspended waiting for its child, not for a ticket.
    st->parked = false;
}

void Conductor::finish_child(JobId job, Duty::handle_type child) {
    JobState* st = find(job);
    if (st != nullptr && !st->stack.empty() && st->stack.back() == child) {
        st->stack.pop_back();
    }
    const SlotId slot = FrameArena::slot_of(child.address());
    child.destroy();
    journal_.record_slot(Mark::ChildDestroyed, job, slot);
}

void Conductor::enqueue(JobId job, std::int64_t value, bool faulted, std::uint64_t delay) {
    Pending t;
    t.job = job;
    t.value = value;
    t.faulted = faulted;
    t.ready = tick_ + delay;
    t.seq = next_seq_++;
    const JobState* st = find(job);
    // A ticket authorises the one frame the job is parked in right now. A job
    // that is not parked gets no frame at all, and no later change of state can
    // give it one.
    if (st != nullptr && !st->settled && st->parked && !st->stack.empty()) {
        t.slot = FrameArena::slot_of(st->stack.back().address());
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

void Conductor::cancel(JobId job) {
    // The request itself is recorded once per call, whatever the job's state is
    // and even when there is no such job.
    journal_.record(Mark::CancelRequested, job);

    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    st->cancel_asked = true;
    if (running_ == job) {
        // The job is on the stack right now, so its frames cannot be destroyed
        // from here. It stops at the point where it would next wait, and if it
        // reaches its end first it still ends cancelled rather than delivered.
        if (!st->stack.empty()) {
            st->stack.front().promise().cancel_flagged = true;
        }
        return;
    }
    teardown(job);
}

void Conductor::settle(JobId job, Outcome outcome, std::int64_t value) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        // One outcome per job: whatever arrives later changes nothing.
        return;
    }
    st->outcome = outcome;
    st->value = value;
    st->settled = true;
    st->parked = false;
    journal_.record(Mark::JobSettled, job, static_cast<std::int64_t>(outcome));
    if (outcome == Outcome::Faulted) {
        journal_.record(Mark::FaultReported, job);
    }
    st->release_pending = true;
}

void Conductor::release_frames(JobId job) {
    JobState* st = find(job);
    if (st == nullptr) {
        return;
    }
    st->release_pending = false;
    st->parked = false;
    while (!st->stack.empty()) {
        Duty::handle_type h = st->stack.back();
        st->stack.pop_back();
        const SlotId slot = FrameArena::slot_of(h.address());
        const bool root = st->stack.empty();
        h.destroy();
        if (root) {
            journal_.record_slot(Mark::FrameReleased, job, slot);
        }
    }
}

void Conductor::teardown(JobId job) {
    JobState* st = find(job);
    if (st == nullptr || st->settled || running_ == job) {
        return;
    }
    settle(job, Outcome::Cancelled, 0);
    release_frames(job);
}

bool Conductor::acceptable(const Pending& t) const {
    if (!t.bound) {
        return false;
    }
    const JobState* st = find(t.job);
    if (st == nullptr || st->settled || !st->parked || st->stack.empty()) {
        return false;
    }
    // Both numbers have to match: the index repeats when storage is recycled,
    // and the epoch repeats across slots that are live at the same time.
    return FrameArena::slot_of(st->stack.back().address()) == t.slot;
}

void Conductor::deliver(const Pending& t) {
    if (!acceptable(t)) {
        // A rejection touches nothing: nothing is resumed, nothing destroyed.
        journal_.record(Mark::TicketRejected, t.job);
        return;
    }
    journal_.record(Mark::TicketAccepted, t.job, t.value);

    JobState* st = find(t.job);
    Duty::handle_type h = st->stack.back();
    st->parked = false;
    if (t.faulted) {
        h.promise().fault = std::make_exception_ptr(std::runtime_error("posted fault"));
    } else {
        h.promise().produced = t.value;
    }
    run_frame(t.job, h);
}

void Conductor::run_frame(JobId job, Duty::handle_type h) {
    const JobId previous = running_;
    ++depth_;
    running_ = job;
    try {
        h.resume();
    } catch (...) {
        running_ = previous;
        --depth_;
        throw;
    }
    running_ = previous;
    --depth_;
    if (depth_ == 0) {
        finish();
    }
}

void Conductor::finish() {
    bool again = true;
    while (again) {
        again = false;
        std::vector<JobId> ids;
        for (const auto& kv : jobs_) {
            const JobState& st = kv.second;
            if (st.cancel_pending || st.release_pending) {
                ids.push_back(kv.first);
            }
        }
        std::sort(ids.begin(), ids.end());
        for (JobId id : ids) {
            JobState* st = find(id);
            if (st == nullptr) {
                continue;
            }
            if (st->cancel_pending) {
                st->cancel_pending = false;
                teardown(id);
                again = true;
            }
            if (st->release_pending) {
                release_frames(id);
                again = true;
            }
        }
    }
}

bool Conductor::step() {
    if (queue_.empty()) {
        return false;
    }
    // Earliest ready time, then lowest job identifier, then earliest queued.
    auto chosen = queue_.begin();
    for (auto it = std::next(queue_.begin()); it != queue_.end(); ++it) {
        if (it->ready != chosen->ready) {
            if (it->ready < chosen->ready) {
                chosen = it;
            }
            continue;
        }
        if (it->job != chosen->job) {
            if (it->job < chosen->job) {
                chosen = it;
            }
            continue;
        }
        if (it->seq < chosen->seq) {
            chosen = it;
        }
    }
    const Pending t = *chosen;
    queue_.erase(chosen);
    if (t.ready > tick_) {
        tick_ = t.ready;
    }
    deliver(t);
    finish();
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
