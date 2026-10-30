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

        // Picks up the identity of the storage that was just handed out.
        promise_type() noexcept;

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

        // Identity of the storage this frame lives in: the pair (index, epoch)
        // names the frame, and a ticket is bound to that pair. Neither number
        // is identity on its own.
        SlotId slot{};

        // While this frame is suspended awaiting a child, the child it owns.
        // Emptying it hands the child over, so nobody destroys it twice.
        std::coroutine_handle<> awaiting_child{};
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

    ~ChildAwaiter();

    ChildAwaiter(const ChildAwaiter&) = delete;
    ChildAwaiter& operator=(const ChildAwaiter&) = delete;

    bool await_ready() const noexcept { return false; }

    // Start the child. Control comes back to the parent through the child's
    // final suspension.
    std::coroutine_handle<> await_suspend(handle_type parent) noexcept;

    std::int64_t await_resume();

  private:
    // Give up the child frame: it is destroyed here, and the awaiter and the
    // parent promise both stop owning it.
    void dispose() noexcept;

    handle_type child_;
    promise_type* parent_promise_ = nullptr;
    bool detached_ = false;
    bool skipped_ = false;
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
    // Returning false leaves the job running when it was cancelled; it then
    // unwinds from the parking spot instead of waiting for anything.
    bool await_suspend(Duty::handle_type me) noexcept;
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

    // --- state the coroutine machinery asks about ----------------------
    // True once cancellation has been asked for `job`. Asking is recorded
    // once per call, whatever the job's state.
    bool cancel_asked(JobId job) const noexcept;

    friend struct Duty::promise_type::FinalStage;
    friend struct Duty::ChildAwaiter;
    friend struct ParkAwaiter;

  private:
    // A queued completion.
    struct Pending {
        JobId job = kNoJob;
        std::int64_t value = 0;
        bool faulted = false;
        // The frame this ticket authorises, or none at all.
        bool bound = false;
        SlotId slot{};
        std::uint64_t ready = 0;
        std::uint64_t seq = 0;
    };

    struct JobState {
        std::coroutine_handle<> root{};    // the frame the conductor owns
        std::coroutine_handle<> parked{};  // the frame waiting for a ticket
        SlotId root_slot{};
        bool settled = false;
        bool cancel_asked = false;
        bool released = false;
        Outcome outcome = Outcome::Delivered;
        std::int64_t value = 0;
    };

    FrameArena arena_;
    Journal journal_;
    std::deque<Pending> queue_;
    std::unordered_map<JobId, JobState> jobs_;
    std::vector<JobId> pending_release_;
    JobId last_job_ = kNoJob;
    JobId running_job_ = kNoJob;
    std::uint64_t tick_ = 0;
    std::uint64_t next_seq_ = 0;
    FrameArena* previous_arena_ = nullptr;

    void enqueue(JobId job, std::int64_t value, bool faulted, std::uint64_t delay);
    void deliver(const Pending& t);
    // Destroy the frames of every job waiting to be released.
    void release_pending();
    // Destroy a frame and everything it is waiting on, innermost first.
    void destroy_stack(std::coroutine_handle<> innermost);
    // Settle and release at once, for a job that will not get there on its own.
    void finish_now(JobId job, Outcome outcome);
    void resume_frame(Duty::handle_type h, JobId job);
    JobState* find(JobId job);
    const JobState* find(JobId job) const;
};

}  // namespace roundel

#endif  // ROUNDEL_RUNTIME_HPP
