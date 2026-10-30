//
// The runtime: jobs, tickets and the conductor that drives them.
//
// A Duty is a coroutine whose frame lives in a FrameArena. A job is a stack of
// Duty frames: the conductor owns the outermost one, and the innermost one is
// the frame that actually parks. Completions arrive as tickets, which the
// conductor delivers one at a time when it is stepped.
//
// The names and signatures on Conductor, Duty and park() are the contract the
// runtime is graded on. How they are implemented is yours to change.

#ifndef ROUNDEL_RUNTIME_HPP
#define ROUNDEL_RUNTIME_HPP

#include <coroutine>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <exception>
#include <optional>
#include <stdexcept>
#include <unordered_map>
#include <utility>
#include <vector>

#include "roundel/arena.hpp"
#include "roundel/journal.hpp"

namespace roundel {

class Conductor;

// Raised inside a job that is pushed along after cancellation was asked for.
class Abandoned : public std::runtime_error {
  public:
    Abandoned() : std::runtime_error("job abandoned") {}
};

// -------------------------------------------------------------------------
// Duty
// -------------------------------------------------------------------------

// The coroutine type. A Duty owns nothing on its own: whoever holds the handle
// owns the frame. That is the conductor for a root job, and the awaiting parent
// for a child.
class Duty {
  public:
    struct promise_type;
    using handle_type = std::coroutine_handle<promise_type>;

    struct promise_type {
        // Frame storage comes from the arena the live Conductor installed. The
        // language gives operator new no other way to reach it.
        static void* operator new(std::size_t bytes);
        static void operator delete(void* p, std::size_t bytes) noexcept;

        Duty get_return_object() noexcept { return Duty{handle_type::from_promise(*this)}; }

        std::suspend_always initial_suspend() noexcept { return {}; }

        // Final suspension hands control to the parent when there is one, so a
        // finished child continues its awaiter without growing the stack.
        struct FinalStage {
            bool await_ready() const noexcept { return false; }
            std::coroutine_handle<> await_suspend(handle_type me) noexcept;
            void await_resume() const noexcept {}
        };
        FinalStage final_suspend() noexcept { return {}; }

        void return_value(std::int64_t v) noexcept { produced = v; }
        void unhandled_exception() noexcept { fault = std::current_exception(); }

        Conductor* conductor = nullptr;
        JobId job = kNoJob;
        std::coroutine_handle<> parent{};
        std::int64_t produced = 0;
        std::exception_ptr fault{};
        bool cancel_flagged = false;
    };

    Duty() noexcept = default;
    explicit Duty(handle_type h) noexcept : handle_(h) {}

    Duty(Duty&& other) noexcept : handle_(std::exchange(other.handle_, {})) {}
    Duty& operator=(Duty&& other) noexcept {
        if (this != &other) {
            if (handle_) {
                handle_.destroy();
            }
            handle_ = std::exchange(other.handle_, {});
        }
        return *this;
    }

    Duty(const Duty&) = delete;
    Duty& operator=(const Duty&) = delete;

    ~Duty() {
        if (handle_) {
            handle_.destroy();
        }
    }

    handle_type release() noexcept { return std::exchange(handle_, {}); }
    handle_type get() const noexcept { return handle_; }
    explicit operator bool() const noexcept { return static_cast<bool>(handle_); }

    struct ChildAwaiter;
    ChildAwaiter operator co_await() && noexcept;

  private:
    handle_type handle_{};
};

// -------------------------------------------------------------------------
// Awaiting a child
// -------------------------------------------------------------------------

// Runs a Duty as a child of the awaiting job. The awaiter takes the handle out
// of the Duty, so the temporary Duty no longer owns the frame.
//
// Required behaviour: by the time the awaiting job observes the child's result
// -- a value or a rethrown failure -- the child's frame must already be gone
// and its storage back in the arena.
struct Duty::ChildAwaiter {
    explicit ChildAwaiter(handle_type child) noexcept : child_(child) {}

    // Defined out of line: releasing the child has to unhook it from the
    // conductor, whose definition is not available here.
    ~ChildAwaiter();

    ChildAwaiter(const ChildAwaiter&) = delete;
    ChildAwaiter& operator=(const ChildAwaiter&) = delete;

    bool await_ready() const noexcept { return false; }

    // Start the child. Control comes back to the parent through the child's
    // final suspension.
    std::coroutine_handle<> await_suspend(handle_type parent) noexcept;

    std::int64_t await_resume();

  private:
    handle_type child_;
    bool detached_ = false;
};

inline Duty::ChildAwaiter Duty::operator co_await() && noexcept {
    return ChildAwaiter{release()};
}

// -------------------------------------------------------------------------
// Parking
// -------------------------------------------------------------------------

// `co_await park()` suspends the job until a ticket for it is delivered. The
// awaited expression evaluates to the ticket's value.
//
// Required behaviour: a job that is cancelled while parked is never resumed.
// Its frame is destroyed, which cleans up everything the frame holds, and its
// storage goes back to the arena straight away.
//
// A job that was already asked to stop does not park at all: `await_suspend`
// returns false and the wait expression raises instead, so the job unwinds at
// the point it tried to wait rather than lingering for a ticket that can no
// longer be delivered to it.
struct ParkAwaiter {
    bool await_ready() const noexcept { return false; }
    bool await_suspend(Duty::handle_type me) noexcept;
    std::int64_t await_resume();

    Duty::promise_type* promise = nullptr;
};

inline ParkAwaiter park() noexcept { return ParkAwaiter{}; }
