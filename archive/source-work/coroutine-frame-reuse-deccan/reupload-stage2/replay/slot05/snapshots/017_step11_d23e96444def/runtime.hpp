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
// of the Duty, so the temporary Duty no longer owns the frame. Once the child
// has been started the conductor owns its frame, and the awaiter destroys it
// when the child's result is observed.
//
// Required behaviour: by the time the awaiting job observes the child's result
// -- a value or a rethrown failure -- the child's frame must already be gone
// and its storage back in the arena.
struct Duty::ChildAwaiter {
    explicit ChildAwaiter(handle_type child) noexcept : child_(child) {}

    ~ChildAwaiter() {
        if (child_ && !detached_) {
            child_.destroy();
        }
    }

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
struct ParkAwaiter {
    bool await_ready() const noexcept { return false; }
    void await_suspend(Duty::handle_type me) noexcept;
    std::int64_t await_resume();

    Duty::promise_type* promise = nullptr;
};

inline ParkAwaiter park() noexcept { return ParkAwaiter{}; }

// -------------------------------------------------------------------------
// Conductor
// -------------------------------------------------------------------------

class Conductor {
  public:
    // `capacity` frames may be checked out at the same time.
    explicit Conductor(std::size_t capacity = 8);
    ~Conductor();

    Conductor(const Conductor&) = delete;
    Conductor& operator=(const Conductor&) = delete;

    // Adopt a freshly made Duty as a root job and run it to its first
    // suspension. Returns the job's identifier, which is never reused.
    JobId launch(Duty duty);

    // The conductor's logical clock. It starts at 0 and only ever moves
    // forward, to the ready time of the ticket a step delivers.
    std::uint64_t now() const noexcept { return tick_; }

    // Queue a completion for `job`, ready at now(), to be delivered by a later
    // step().
    void post(JobId job, std::int64_t value);
    // Queue a completion for `job` ready at now() + delay.
    void post_after(JobId job, std::int64_t value, std::uint64_t delay);
    // Queue a completion that raises an exception where `job` is parked, ready
    // at now(). A job that does not catch it ends up Faulted.
    void post_fault(JobId job);
    // The same, ready at now() + delay.
    void post_fault_after(JobId job, std::uint64_t delay);

    // Ask `job` to stop. Calling this more than once, or for a job that has
    // already settled, must be harmless.
    void cancel(JobId job);

    // Deliver at most one queued ticket: the one that comes first in the
    // delivery order (earliest ready time; then lowest job identifier; then
    // earliest queued). Returns false when the queue was empty. Nothing else
    // runs.
    bool step();
    // Deliver queued tickets until the queue is empty. Returns how many ran.
    std::size_t drain();

    std::size_t pending_tickets() const noexcept { return queue_.size(); }

    // Outcome of a settled job, or nullopt while it is still running.
    std::optional<Outcome> outcome_of(JobId job) const;
    // Value produced by a job that delivered one.
    std::optional<std::int64_t> value_of(JobId job) const;
    // True once the job has settled, whatever the outcome.
    bool settled(JobId job) const;
    // Jobs launched but not yet settled.
    std::size_t live_jobs() const;

    Journal& journal() noexcept { return journal_; }
    const Journal& journal() const noexcept { return journal_; }
    FrameArena& arena() noexcept { return arena_; }
    const FrameArena& arena() const noexcept { return arena_; }

    // Installed while a Conductor is alive so promise_type::operator new can
    // reach the arena.
    static FrameArena* active_arena() noexcept;

    // --- called by the coroutine machinery -----------------------------
    void note_parked(JobId job, std::coroutine_handle<> h);
    void settle(JobId job, Outcome outcome, std::int64_t value);
    JobId next_job_id() noexcept { return ++last_job_; }

  private:
    // A queued completion. A ticket authorises one wait: the job it names,
    // parked in the frame it was parked in when the ticket was queued. That
    // frame is named by its slot index and epoch together; a ticket queued for
    // a job that was not parked at that moment is bound to no frame at all and
    // can never be accepted.
    struct Pending {
        JobId job = kNoJob;
        std::int64_t value = 0;
        bool faulted = false;
        bool bound = false;
        SlotId slot{};
        std::uint64_t ready = 0;
        std::uint64_t seq = 0;
    };

    // The frames of one job, outermost first. The conductor owns every frame on
    // the stack, so it can tear a job down from the inside out without ever
    // resuming a frame that was asked to stop.
    struct JobState {
        std::vector<Duty::handle_type> stack;
        bool parked = false;         // the innermost frame is waiting for a ticket
        bool settled = false;
        bool cancel_asked = false;   // cancellation was requested for this job
        bool cancel_pending = false;  // it still has to be torn down
        bool release_pending = false;  // its frames still have to be released
        Outcome outcome = Outcome::Delivered;
        std::int64_t value = 0;
    };

    friend struct Duty::promise_type;
    friend struct Duty::promise_type::FinalStage;
    friend struct Duty::ChildAwaiter;
    friend struct ParkAwaiter;

    FrameArena arena_;
    Journal journal_;
    std::deque<Pending> queue_;
    std::unordered_map<JobId, JobState> jobs_;
    JobId last_job_ = kNoJob;
    JobId running_ = kNoJob;   // the job whose frame is on the stack right now
    std::size_t depth_ = 0;    // how deep the conductor is inside a resume
    std::uint64_t tick_ = 0;
    std::uint64_t next_seq_ = 0;
    FrameArena* previous_arena_ = nullptr;

    void enqueue(JobId job, std::int64_t value, bool faulted, std::uint64_t delay);
    bool acceptable(const Pending& t) const;
    void deliver(const Pending& t);
    // Resume one frame, then settle and release whatever that made ready.
    void run_frame(JobId job, Duty::handle_type h);
    // Tear down and release everything that is waiting for the stack to unwind.
    void finish();
    // Stop a job that is not running: settle it as Cancelled and release its
    // frames, innermost first.
    void teardown(JobId job);
    void release_frames(JobId job);
    void push_frame(JobId job, Duty::handle_type child);
    void finish_child(JobId job, Duty::handle_type child);
    JobState* find(JobId job);
    const JobState* find(JobId job) const;
};

}  // namespace roundel

#endif  // ROUNDEL_RUNTIME_HPP
