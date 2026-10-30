
#include "roundel/runtime.hpp"

#include <algorithm>

namespace roundel {
namespace {

// The arena the next coroutine frame should be taken from. A live Conductor
// installs itself here; nested Conductors save and restore the previous one.
FrameArena* g_arena = nullptr;

// Geometry of the header the arena puts in front of every block. A frame that
// was not taken from an arena gets the same header, with no owner, so a raw
// frame pointer can always be traced and never looked up out of bounds.
constexpr std::size_t kFrameAlign = alignof(std::max_align_t);
constexpr std::size_t kHeaderSpan =
    (sizeof(BlockHeader) + kFrameAlign - 1) / kFrameAlign * kFrameAlign;

// Distinct generation for frames that came off the general heap, so two such
// frames never answer to the same slot name.
std::uint32_t next_heap_epoch() noexcept {
    static std::uint32_t counter = 0;
    return ++counter;
}

}  // namespace

// -------------------------------------------------------------------------
// Frame storage
// -------------------------------------------------------------------------

void* Duty::promise_type::operator new(std::size_t bytes) {
    if (g_arena == nullptr) {
        unsigned char* base = static_cast<unsigned char*>(::operator new(bytes + kHeaderSpan));
        auto* h = reinterpret_cast<BlockHeader*>(base);
        h->owner = nullptr;
        h->index = 0xFFFFFFFFu;
        h->epoch = next_heap_epoch();
        return base + kHeaderSpan;
    }
    return g_arena->check_out(bytes);
}

void Duty::promise_type::operator delete(void* p, std::size_t bytes) noexcept {
    (void)bytes;
    if (p == nullptr) {
        return;
    }
    unsigned char* base = static_cast<unsigned char*>(p) - kHeaderSpan;
    auto* h = reinterpret_cast<BlockHeader*>(base);
    if (h->owner == nullptr) {
        ::operator delete(base);
        return;
    }
    h->owner->reclaim(p);
}

std::coroutine_handle<> Duty::promise_type::FinalStage::await_suspend(handle_type me) noexcept {
    promise_type& p = me.promise();
    if (p.parent) {
        // A child hands control back to whoever awaited it. Its frame is still
        // whole here: the awaiter destroys it before the parent sees a result.
        return p.parent;
    }
    // A root job has nobody to continue into, so it settles here.
    if (p.conductor != nullptr) {
        const Outcome o =
            p.conductor->resolve_outcome(p.job, static_cast<bool>(p.fault), p.cancel_flagged);
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

Duty::ChildAwaiter::~ChildAwaiter() {
    if (child_ && !detached_) {
        promise_type& cp = child_.promise();
        Conductor* c = cp.conductor;
        if (c != nullptr) {
            c->journal().record_slot(Mark::ChildDestroyed, cp.job,
                                     FrameArena::slot_of(child_.address()));
        }
        child_.destroy();
    }
}

std::int64_t Duty::ChildAwaiter::await_resume() {
    handle_type child = child_;
    child_ = handle_type{};

    promise_type& cp = child.promise();
    Conductor* c = cp.conductor;
    const JobId job = cp.job;
    // Read everything the parent may observe out of the child before the
    // child's frame is gone.
    const std::exception_ptr fault = cp.fault;
    const std::int64_t value = cp.produced;

    // The child is finished and suspended at its final suspension. It is
    // destroyed here, so the awaiting job never sees a live child frame: its
    // storage is back in the arena before the result is observed.
    if (c != nullptr) {
        c->journal().record_slot(Mark::ChildDestroyed, job,
                                 FrameArena::slot_of(child.address()));
    }
    child.destroy();
    detached_ = true;

    if (fault) {
        // The failure is re-raised where the parent was waiting. Whether it
        // escapes the job is the parent's business; either way the child is
        // already gone.
        std::rethrow_exception(fault);
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
    // Jobs still running have to be brought down: their frames are destroyed
    // and their storage goes back before the arena does.
    std::vector<JobId> open;
    for (const auto& kv : jobs_) {
        if (!kv.second.settled) {
            open.push_back(kv.first);
        }
    }
    std::sort(open.begin(), open.end());
    for (JobId job : open) {
        JobState* st = find(job);
        if (st == nullptr || st->settled) {
            continue;
        }
        if (st->parked) {
            unwind(job);
        } else {
            settle(job, Outcome::Cancelled, 0);
        }
        flush_releases();
    }
    flush_releases();
    for (auto& kv : jobs_) {
        release_frames(kv.first);
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

    {
        JobState st;
        st.root = h;
        jobs_.emplace(job, st);
    }

    journal_.record_slot(Mark::JobStarted, job, FrameArena::slot_of(h.address()));

    resume_frame(job, h);
    flush_releases();
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
    // A ticket is bound at queueing time to the one wait it authorises: the
    // frame the job is parked in now. A job that is not parked at this moment
    // gives the ticket nothing to authorise, ever.
    JobState* st = find(job);
    if (st != nullptr && !st->settled && st->parked) {
        t.bound = true;
        t.slot = FrameArena::slot_of(st->parked.address());
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
    // Every request is recorded, whether or not there is a job behind it and
    // whatever state that job is in.
    journal_.record(Mark::CancelRequested, job);

    JobState* st = find(job);
    if (st == nullptr) {
        return;
    }
    st->cancel_asked = true;
    if (st->settled) {
        return;
    }
    if (st->depth > 0) {
        // The job is running underneath this call -- it cancelled itself, or
        // something it started did. It is brought down when it comes to rest.
        return;
    }
    if (st->parked) {
        unwind(job);
    } else {
        settle(job, Outcome::Cancelled, 0);
    }
    flush_releases();
}

Outcome Conductor::resolve_outcome(JobId job, bool faulted, bool cancel_flagged) const noexcept {
    const JobState* st = find(job);
    if (st != nullptr && st->cancel_asked) {
        return Outcome::Cancelled;
    }
    if (cancel_flagged) {
        return Outcome::Cancelled;
    }
    if (faulted) {
        return Outcome::Faulted;
    }
    return Outcome::Delivered;
}

void Conductor::settle(JobId job, Outcome outcome, std::int64_t value) {
    JobState* st = find(job);
    // One outcome per job: the first one wins, and nothing after it changes
    // the outcome, the value or the record.
    if (st == nullptr || st->settled) {
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
    release_queue_.push_back(job);
}

void Conductor::flush_releases() {
    // Releasing a frame can settle another job, so this runs to a fixed point.
    while (!release_queue_.empty()) {
        std::vector<JobId> batch;
        batch.swap(release_queue_);
        for (JobId job : batch) {
            release_frames(job);
        }
    }
}

void Conductor::release_frames(JobId job) {
    JobState* st = find(job);
    if (st == nullptr || !st->root) {
        return;
    }
    // Every frame of the job is destroyed by the time it settles: children
    // went first, from the inside out, as each await returned. What is left is
    // the root, and it goes now rather than at some later step.
    std::coroutine_handle<> r = std::exchange(st->root, {});
    st->parked = {};
    const SlotId slot = FrameArena::slot_of(r.address());
    r.destroy();
    journal_.record_slot(Mark::FrameReleased, job, slot);
}

// Delivery order: earliest ready time, then lowest job identifier, then
// earliest queued.
bool Conductor::comes_before(const Pending& a, const Pending& b) noexcept {
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
    // The ticket names a job and the frame it was parked in when the ticket
    // was queued. Both parts of that slot name have to match: the index alone
    // would reach a different job living in recycled storage, and the epoch
    // alone would reach another frame of this job handed out as often.
    const bool live = st != nullptr && !st->settled && static_cast<bool>(st->parked);
    const bool accepted = t.bound && live && FrameArena::slot_of(st->parked.address()) == t.slot;
    if (!accepted) {
        // A rejection touches nothing: no frame is resumed and none is
        // destroyed.
        journal_.record(Mark::TicketRejected, t.job);
        return;
    }

    journal_.record(Mark::TicketAccepted, t.job, t.value);

    auto typed = Duty::handle_type::from_address(st->parked.address());
    if (t.faulted) {
        typed.promise().fault = std::make_exception_ptr(std::runtime_error("posted fault"));
    } else {
        typed.promise().produced = t.value;
    }

    std::coroutine_handle<> h = std::exchange(st->parked, {});
    resume_frame(t.job, h);
}

void Conductor::resume_frame(JobId job, std::coroutine_handle<> h) {
    {
        JobState* st = find(job);
        if (st != nullptr) {
            ++st->depth;
        }
    }
    h.resume();
    // The job may have launched or settled underneath, so look it up again.
    JobState* st = find(job);
    if (st == nullptr) {
        return;
    }
    if (st->depth > 0) {
        --st->depth;
    }
    if (st->depth == 0) {
        come_to_rest(job);
    }
}

void Conductor::come_to_rest(JobId job) {
    JobState* st = find(job);
    if (st == nullptr || st->settled) {
        return;
    }
    if (!st->cancel_asked) {
        return;
    }
    // A cancellation that arrived while the job was running takes effect the
    // moment the job stops, whether that is a new park or the end of the run.
    if (st->parked) {
        unwind(job);
    } else {
        settle(job, Outcome::Cancelled, 0);
    }
}

void Conductor::unwind(JobId job) {
    JobState* st = find(job);
    if (st == nullptr || st->settled || !st->parked) {
        return;
    }
    // Flag the parked frame so its await_resume raises, then push it along so
    // the job unwinds now instead of sitting there. The frames unwind from the
    // inside out and each awaiter destroys the child it was waiting on.
    auto typed = Duty::handle_type::from_address(st->parked.address());
    typed.promise().cancel_flagged = true;
    std::coroutine_handle<> h = std::exchange(st->parked, {});
    resume_frame(job, h);
}

bool Conductor::step() {
    if (queue_.empty()) {
        return false;
    }
    std::size_t best = 0;
    for (std::size_t i = 1; i < queue_.size(); ++i) {
        if (comes_before(queue_[i], queue_[best])) {
            best = i;
        }
    }
    Pending t = queue_[best];
    queue_.erase(queue_.begin() + static_cast<std::ptrdiff_t>(best));
    // The clock follows delivery, and only ever moves forward.
    if (t.ready > tick_) {
        tick_ = t.ready;
    }
    deliver(t);
    flush_releases();
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
