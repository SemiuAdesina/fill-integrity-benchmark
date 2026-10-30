
#include "roundel/runtime.hpp"

#include <algorithm>
#include <new>

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
    const SlotId slot = FrameArena::slot_of(child_.address());
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
