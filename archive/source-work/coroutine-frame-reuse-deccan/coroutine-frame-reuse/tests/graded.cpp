//
// Verifier-owned scenario definitions.
//
// Every test drives the runtime through the public contract only and then
// observes real state: outcomes returned by the conductor, arena counters, and
// destructor side effects recorded by RAII probes that live inside coroutine
// frames. The candidate-side process serializes actual values only. A separate
// verifier process owns and checks the expected transcript digest. Journal
// marks are observed for ordering and multiplicity, never as a
// substitute for the state they describe -- where a mark could be printed
// without the work behind it, the same fact is also checked against the arena
// or a probe.
//
// Nothing here depends on pointer values, addresses, timing, allocator
// behaviour, sanitizer output formatting or compiler diagnostics.

#include <cstdint>
#include <exception>
#include <stdexcept>
#include <string>
#include <vector>

// The candidate's header is included first, deliberately. harness.hpp comes
// last, so its ROUNDEL_TEST / CHECK definitions cannot be pre-empted by a
// candidate header that tries to define them into no-ops: a candidate that
// defines any of these names loses to -Wmacro-redefined, which is an error.
#include "roundel/arena.hpp"
#include "roundel/journal.hpp"
#include "roundel/runtime.hpp"

#include "harness.hpp"

using roundel::Conductor;
using roundel::Duty;
using roundel::Entry;
using roundel::FrameArena;
using roundel::JobId;
using roundel::Journal;
using roundel::Mark;
using roundel::Outcome;
using roundel::park;
using roundel::SlotId;

namespace {

// Outcome values as plain integers, so failures print something readable.
constexpr int kDelivered = static_cast<int>(Outcome::Delivered);
constexpr int kCancelled = static_cast<int>(Outcome::Cancelled);
constexpr int kFaulted = static_cast<int>(Outcome::Faulted);
constexpr int kUnsettled = -1;

constexpr std::size_t kZero = 0;
constexpr std::size_t kOne = 1;
constexpr std::size_t kTwo = 2;
constexpr std::size_t kThree = 3;

// -------------------------------------------------------------------------
// Probes: the ground truth for "this frame was really destroyed"
// -------------------------------------------------------------------------

int& probe_alive() {
    static int n = 0;
    return n;
}

std::vector<int>& destroy_order() {
    static std::vector<int> v;
    return v;
}

struct Probe {
    int tag;

    explicit Probe(int t) : tag(t) { ++probe_alive(); }
    Probe(const Probe&) = delete;
    Probe& operator=(const Probe&) = delete;
    ~Probe() {
        destroy_order().push_back(tag);
        --probe_alive();
    }
};

// -------------------------------------------------------------------------
// Journal readers
// -------------------------------------------------------------------------

int outcome_int(const Conductor& c, JobId job) {
    const auto o = c.outcome_of(job);
    return o.has_value() ? static_cast<int>(*o) : kUnsettled;
}

const Entry* first_of(const Journal& j, Mark m, JobId job) {
    for (const Entry& e : j.entries()) {
        if (e.mark == m && e.job == job) {
            return &e;
        }
    }
    return nullptr;
}

std::vector<std::int64_t> details_of(const Journal& j, Mark m) {
    std::vector<std::int64_t> out;
    for (const Entry& e : j.entries()) {
        if (e.mark == m) {
            out.push_back(e.detail);
        }
    }
    return out;
}

std::vector<JobId> jobs_of(const Journal& j, Mark m) {
    std::vector<JobId> out;
    for (const Entry& e : j.entries()) {
        if (e.mark == m) {
            out.push_back(e.job);
        }
    }
    return out;
}

// True when both marks are present for `job` and `before` comes first.
// first_index reports entries().size() for a mark that never appeared, so an
// absent `after` has to be rejected explicitly rather than compared.
bool ordered(const Journal& j, Mark before, Mark after, JobId job) {
    const std::size_t n = j.entries().size();
    const std::size_t a = j.first_index(before, job);
    const std::size_t b = j.first_index(after, job);
    return a < n && b < n && a < b;
}

// -------------------------------------------------------------------------
// Jobs used by the tests
// -------------------------------------------------------------------------

Duty parks_once() {
    const std::int64_t v = co_await park();
    co_return v;
}

Duty parks_probed(int tag) {
    Probe p{tag};
    (void)p;
    const std::int64_t v = co_await park();
    co_return v;
}

// Catches every abandonment it is handed and keeps going. A correct runtime
// never lets this body run again after cancellation.
Duty stubborn(std::vector<std::int64_t>* got) {
    for (int i = 0; i < 3; ++i) {
        try {
            got->push_back(co_await park());
        } catch (const roundel::Abandoned&) {
            got->push_back(-1);
        }
    }
    co_return static_cast<std::int64_t>(got->size());
}

Duty throws_after_park() {
    const std::int64_t v = co_await park();
    if (v >= 0) {
        throw std::runtime_error("job body failed");
    }
    co_return v;
}

Duty throws_after_park_probed(int tag) {
    Probe p{tag};
    (void)p;
    const std::int64_t v = co_await park();
    if (v >= 0) {
        throw std::runtime_error("job body failed");
    }
    co_return v;
}

// Records which kind of failure reached it, then finishes normally.
Duty catches_posted_fault(std::vector<int>* seen) {
    try {
        const std::int64_t v = co_await park();
        seen->push_back(1);
        co_return v;
    } catch (const roundel::Abandoned&) {
        seen->push_back(2);
        co_return -2;
    } catch (const std::exception&) {
        seen->push_back(3);
        co_return -3;
    }
}

Duty child_value(int tag, std::int64_t v) {
    Probe p{tag};
    (void)p;
    co_return v;
}

Duty child_throws(int tag) {
    Probe p{tag};
    (void)p;
    if (p.tag >= 0) {
        throw std::runtime_error("child failed");
    }
    co_return 0;
}

Duty child_parks(int tag) {
    Probe p{tag};
    (void)p;
    const std::int64_t v = co_await park();
    co_return v;
}

Duty parent_of_value(int ptag, int ctag, std::int64_t v) {
    Probe p{ptag};
    (void)p;
    const std::int64_t got = co_await child_value(ctag, v);
    co_return got + 1;
}

// Looks at how many probes are still alive the instant the child's result
// becomes visible. If the child frame is really gone, only this frame's probe
// remains. No amount of journal writing can change that number.
Duty parent_counting(int ptag, int ctag, std::int64_t v, std::vector<int>* seen) {
    Probe p{ptag};
    (void)p;
    const std::int64_t got = co_await child_value(ctag, v);
    seen->push_back(probe_alive());
    co_return got;
}

Duty parent_of_throwing(int ptag, int ctag) {
    Probe p{ptag};
    (void)p;
    const std::int64_t got = co_await child_throws(ctag);
    co_return got;
}

Duty parent_of_parking(int ptag, int ctag) {
    Probe p{ptag};
    (void)p;
    const std::int64_t got = co_await child_parks(ctag);
    co_return got;
}

Duty middle_level(int tag, int ltag) {
    Probe p{tag};
    (void)p;
    const std::int64_t got = co_await child_value(ltag, 5);
    co_return got + 1;
}

Duty top_level(int tag, int mtag, int ltag) {
    Probe p{tag};
    (void)p;
    const std::int64_t got = co_await middle_level(mtag, ltag);
    co_return got + 1;
}

// Parks twice in one frame. Both waits happen at the same slot and the same
// generation, so a ticket queued during the first wait is still the right
// ticket for the second.
Duty parks_twice(std::vector<std::int64_t>* got) {
    got->push_back(co_await park());
    got->push_back(co_await park());
    co_return static_cast<std::int64_t>(got->size());
}

// Parks in its own frame, then parks again inside a child's frame. The job is
// the same job throughout; the frame that is waiting is not.
Duty parks_then_awaits_parking_child(int ctag) {
    const std::int64_t v = co_await park();
    const std::int64_t w = co_await child_parks(ctag);
    co_return v + w;
}

// Awaits two parking children one after the other. The first child's slot is
// back in the pool before the second is taken, and the pool hands out its
// lowest free slot, so both children live at the same index one generation
// apart.
Duty awaits_two_parking_children(int ctag1, int ctag2) {
    const std::int64_t a = co_await child_parks(ctag1);
    const std::int64_t b = co_await child_parks(ctag2);
    co_return a + b;
}

// Queues a ticket for itself while it is running rather than parked, then parks
// again in the very same frame. The ticket was queued at a moment when nothing
// was waiting, so it is bound to no frame at all.
Duty posts_for_self_then_parks(Conductor* c, const JobId* self, std::uint64_t delay) {
    const std::int64_t v = co_await park();
    c->post_after(*self, 9, delay);
    const std::int64_t w = co_await park();
    co_return v + w;
}

// Queues a ticket for another job from inside a resumed job, so the delay is
// measured from a clock that has already moved.
Duty posts_for_other(Conductor* c, const JobId* other, std::uint64_t delay) {
    const std::int64_t v = co_await park();
    c->post_after(*other, v + 1, delay);
    co_return v;
}

// -------------------------------------------------------------------------
// The trace
// -------------------------------------------------------------------------

char outcome_letter(std::int64_t outcome) {
    switch (static_cast<Outcome>(outcome)) {
        case Outcome::Delivered:
            return 'D';
        case Outcome::Cancelled:
            return 'X';
        case Outcome::Faulted:
            return 'F';
    }
    return '?';
}

// Reduces a run to the string the protocol describes. Delivery marks are
// stamped with the clock as it stands when they are absorbed, which is why
// absorb() has to be called after every step and after anything else that
// records.
class Tracer {
  public:
    explicit Tracer(Conductor* c) : c_(c) { absorb(); }

    bool step() {
        const bool ran = c_->step();
        absorb();
        return ran;
    }

    void absorb() {
        const std::vector<Entry>& es = c_->journal().entries();
        for (std::size_t i = seen_; i < es.size(); ++i) {
            append(es[i]);
        }
        seen_ = es.size();
    }

    const std::string& text() const { return out_; }

  private:
    void append(const Entry& e) {
        const std::string job = std::to_string(e.job);
        std::string line;
        switch (e.mark) {
            case Mark::TicketAccepted:
                line = "A" + job + "@" + std::to_string(c_->now());
                break;
            case Mark::TicketRejected:
                line = "R" + job + "@" + std::to_string(c_->now());
                break;
            case Mark::ChildDestroyed:
                line = "C" + job;
                break;
            case Mark::ParentContinued:
                line = "P" + job + ":" + std::to_string(e.detail);
                break;
            case Mark::JobSettled:
                line = "S" + job + "=" + std::string(1, outcome_letter(e.detail));
                break;
            default:
                return;
        }
        if (!out_.empty()) {
            out_ += ";";
        }
        out_ += line;
    }

    Conductor* c_;
    std::size_t seen_ = 0;
    std::string out_;
};

Duty cancels_other(Conductor* c, const JobId* other) {
    const std::int64_t v = co_await park();
    c->cancel(*other);
    co_return v;
}

Duty cancels_self(Conductor* c, const JobId* self) {
    const std::int64_t v = co_await park();
    c->cancel(*self);
    const std::int64_t w = co_await park();
    co_return v + w;
}

}  // namespace

// =========================================================================
// Family A -- basic lifecycle
// =========================================================================

// A launched job runs as far as its first park and stops there: unsettled,
// counted as live, holding one slot, with its start and its park both on the
// record.
ROUNDEL_TEST(a_launch_parks_and_reports_unsettled) {
    Conductor c(4);
    const JobId job = c.launch(parks_once());

    CHECK(job != roundel::kNoJob);
    CHECK(!c.settled(job));
    CHECK_EQ(outcome_int(c, job), kUnsettled);
    CHECK_EQ(c.live_jobs(), kOne);
    CHECK_EQ(c.arena().live_slots(), kOne);
    CHECK(c.journal().has(Mark::JobStarted, job));
    CHECK(c.journal().has(Mark::JobParked, job));
}

// The ordinary path. A completion resumes the parked job, the job returns,
// and what it produced is readable through the settled outcome.
ROUNDEL_TEST(a_delivered_value_and_outcome) {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    c.post(job, 41);
    CHECK(c.step());

    CHECK(c.settled(job));
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK(c.value_of(job).has_value());
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(41));
    CHECK_EQ(c.live_jobs(), kZero);
}

// Storage comes back when the job settles -- not at shutdown, not on the next
// allocation. An object living in the frame reports whether it was really
// destroyed at that moment.
ROUNDEL_TEST(a_settle_releases_storage_immediately) {
    Conductor c(4);
    const JobId job = c.launch(parks_probed(7));
    CHECK_EQ(probe_alive(), 1);

    c.post(job, 3);
    CHECK(c.step());

    // Not at shutdown, not on the next launch: now.
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().free_slots(), c.arena().capacity());
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, job), kOne);
}

// Job identifiers are handed out strictly increasing and never reused, so a
// later job can always be told from an earlier one.
ROUNDEL_TEST(a_job_ids_are_unique_and_increasing) {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    const JobId d = c.launch(parks_once());

    CHECK(a != roundel::kNoJob);
    CHECK(a < b);
    CHECK(b < d);
    CHECK_EQ(c.live_jobs(), kThree);
}

// A step delivers one ticket and no more. The rest of the queue stays where
// it was, and a step with nothing left to deliver says so.
ROUNDEL_TEST(a_step_delivers_one_ticket_only) {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    c.post(a, 1);
    c.post(b, 2);
    CHECK_EQ(c.pending_tickets(), kTwo);

    CHECK(c.step());
    CHECK_EQ(c.pending_tickets(), kOne);
    CHECK(c.settled(a));
    CHECK(!c.settled(b));

    CHECK(c.step());
    CHECK(c.settled(b));
    CHECK(!c.step());
}

// Queueing is not delivering. Until a step is asked for the job stays parked,
// nothing is accepted, and its frame is still checked out.
ROUNDEL_TEST(a_no_progress_without_stepping) {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    c.post(job, 9);
    c.post(job, 10);

    CHECK_EQ(c.pending_tickets(), kTwo);
    CHECK(!c.settled(job));
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, job), kZero);
    CHECK_EQ(c.arena().live_slots(), kOne);
}

// The lifecycle order of a plain job -- started, parked, accepted, settled,
// released -- and the rule that once its storage is back, nothing further is
// ever recorded against it.
ROUNDEL_TEST(a_journal_orders_start_park_accept_settle_release) {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    c.post(job, 4);
    CHECK(c.step());

    const Journal& j = c.journal();
    CHECK(ordered(j, Mark::JobStarted, Mark::JobParked, job));
    CHECK(ordered(j, Mark::JobParked, Mark::TicketAccepted, job));
    CHECK(ordered(j, Mark::TicketAccepted, Mark::JobSettled, job));
    CHECK(ordered(j, Mark::JobSettled, Mark::FrameReleased, job));

    // Nothing at all is recorded for the job after its storage went back.
    const std::size_t released = j.first_index(Mark::FrameReleased, job);
    for (std::size_t i = released + 1; i < j.entries().size(); ++i) {
        CHECK(j.entries()[i].job != job);
    }
}

// =========================================================================
// Family B -- cancellation ordering
// =========================================================================

// Cancelling a parked job settles it as cancelled. It carries no value, and
// cancellation is not a failure, so nothing is announced as one.
ROUNDEL_TEST(b_cancel_while_parked_settles_cancelled) {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    c.cancel(job);

    CHECK(c.settled(job));
    CHECK_EQ(outcome_int(c, job), kCancelled);
    CHECK(!c.value_of(job).has_value());
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);
}

// Cancellation destroys the frame rather than abandoning it: objects living
// in the frame are destroyed, the slot returns to the pool, and the release
// is recorded once.
ROUNDEL_TEST(b_cancel_releases_storage_and_runs_cleanup) {
    Conductor c(4);
    const JobId job = c.launch(parks_probed(11));
    CHECK_EQ(probe_alive(), 1);

    c.cancel(job);

    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(destroy_order().size(), kOne);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().free_slots(), c.arena().capacity());
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, job), kOne);
}

// A cancelled job's body does not run again. It is neither resumed one more
// step nor handed an abandonment it could catch and continue past.
ROUNDEL_TEST(b_cancelled_job_never_resumes) {
    std::vector<std::int64_t> got;
    Conductor c(4);
    const JobId job = c.launch(stubborn(&got));
    c.cancel(job);

    // The body must not have run one more step, neither normally nor by
    // being handed an abandonment it could swallow.
    CHECK_EQ(got.size(), kZero);
    CHECK(c.settled(job));
    CHECK_EQ(outcome_int(c, job), kCancelled);
    CHECK_EQ(c.arena().live_slots(), kZero);
}

// Cancelling the same job repeatedly settles it once and destroys its frame
// once. The pool sees no slot handed back twice.
ROUNDEL_TEST(b_repeated_cancel_settles_once) {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    c.cancel(job);
    c.cancel(job);
    c.cancel(job);

    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
    CHECK_EQ(outcome_int(c, job), kCancelled);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, job), kOne);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// A cancellation arriving after the job has already completed changes
// nothing: the outcome and value it settled with stand, with no second settle
// and no second release.
ROUNDEL_TEST(b_cancel_after_settle_is_harmless) {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    c.post(job, 21);
    CHECK(c.step());
    CHECK_EQ(outcome_int(c, job), kDelivered);

    c.cancel(job);

    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(21));
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, job), kOne);
}

// Cancelling an identifier that was never launched settles nothing, disturbs
// no live job, and hands nothing back to the pool.
ROUNDEL_TEST(b_cancel_unknown_job_is_harmless) {
    Conductor c(4);
    const JobId job = c.launch(parks_once());

    c.cancel(9999);
    c.cancel(roundel::kNoJob);

    CHECK(!c.settled(9999));
    CHECK(!c.settled(job));
    CHECK_EQ(c.live_jobs(), kOne);
    CHECK_EQ(c.arena().live_slots(), kOne);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// The first completion wins. A cancellation and a second completion
// afterwards leave the outcome as it was, and the late ticket is refused
// rather than accepted.
ROUNDEL_TEST(b_complete_then_cancel_then_complete) {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    c.post(job, 5);
    CHECK(c.step());
    c.cancel(job);
    c.post(job, 6);
    CHECK(c.step());

    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(5));
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, job), kOne);
}

// A ticket for a job cancelled before that ticket came due is refused, not
// delivered, and the job stays cancelled.
ROUNDEL_TEST(b_cancel_then_ticket_is_rejected) {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    c.cancel(job);
    c.post(job, 77);
    CHECK(c.step());

    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, job), kZero);
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, job), kOne);
    CHECK_EQ(outcome_int(c, job), kCancelled);
}

// =========================================================================
// Family C -- deliberate address reuse
// =========================================================================

// The pool really does reuse storage: a settled job's slot goes to the next
// job at the same index with its generation raised, and capacity is reused
// rather than grown.
ROUNDEL_TEST(c_released_slot_is_handed_out_again) {
    Conductor c(1);
    const JobId a = c.launch(parks_once());
    c.post(a, 1);
    CHECK(c.step());
    CHECK_EQ(c.arena().live_slots(), kZero);

    const JobId b = c.launch(parks_once());

    const Entry* first = first_of(c.journal(), Mark::JobStarted, a);
    const Entry* second = first_of(c.journal(), Mark::JobStarted, b);
    CHECK(first != nullptr);
    CHECK(second != nullptr);
    if (first != nullptr && second != nullptr) {
        CHECK_EQ(second->slot.index, first->slot.index);
        CHECK(second->slot.epoch > first->slot.epoch);
    }
    CHECK_EQ(c.arena().total_check_outs(), kTwo);
    CHECK_EQ(c.arena().peak_live(), kOne);
    CHECK(c.journal().count(Mark::SlotReused) >= kOne);
}

// The central hazard. A ticket queued for a job that has since settled must
// not resume whatever coroutine occupies that storage now; it is refused, and
// the new occupant is not delivered into.
ROUNDEL_TEST(c_stale_ticket_does_not_resume_new_occupant) {
    Conductor c(1);
    const JobId a = c.launch(parks_once());
    c.post(a, 500);
    c.cancel(a);

    const JobId b = c.launch(parks_once());  // takes over a's storage
    CHECK(c.step());                         // a's ticket comes due

    CHECK(!c.settled(b));
    CHECK_EQ(outcome_int(c, b), kUnsettled);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, a), kZero);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, b), kZero);
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, a), kOne);
}

// Refusing a stale ticket must not tear the new occupant down either. The
// frame at the reused address stays alive, stays parked, and keeps its slot.
ROUNDEL_TEST(c_stale_ticket_does_not_destroy_new_occupant) {
    Conductor c(1);
    const JobId a = c.launch(parks_once());
    c.post(a, 12);
    c.cancel(a);

    const JobId b = c.launch(parks_probed(3));
    CHECK_EQ(probe_alive(), 1);

    CHECK(c.step());

    // The new occupant is untouched: still alive, still parked, still holding
    // its slot.
    CHECK_EQ(probe_alive(), 1);
    CHECK(!c.settled(b));
    CHECK_EQ(c.arena().live_slots(), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, b), kZero);
}

// A failing ticket carries the same hazard as a value. A stale fault must not
// be raised inside the coroutine that inherited the address, and must not be
// announced against it.
ROUNDEL_TEST(c_stale_fault_ticket_does_not_fault_new_occupant) {
    Conductor c(1);
    const JobId a = c.launch(parks_once());
    c.post_fault(a);
    c.cancel(a);

    const JobId b = c.launch(parks_once());
    CHECK(c.step());

    CHECK_EQ(outcome_int(c, b), kUnsettled);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, b), kZero);
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, a), kOne);
}

// Several stale tickets against one reissued slot are each refused on their
// own. Refusal does not depend on being the first or the only one.
ROUNDEL_TEST(c_many_stale_tickets_all_rejected) {
    Conductor c(1);
    const JobId a = c.launch(parks_once());
    for (int i = 0; i < 5; ++i) {
        c.post(a, i);
    }
    c.cancel(a);

    const JobId b = c.launch(parks_probed(4));
    CHECK_EQ(c.drain(), static_cast<std::size_t>(5));

    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, a), static_cast<std::size_t>(5));
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, a), kZero);
    CHECK(!c.settled(b));
    CHECK_EQ(probe_alive(), 1);
}

// Repeated reuse of one slot keeps the index and raises the generation every
// time, which is what lets two frames at one address be told apart.
ROUNDEL_TEST(c_slot_epoch_advances_per_reuse) {
    Conductor c(1);
    std::vector<std::uint32_t> epochs;
    std::vector<std::uint32_t> indexes;
    for (int i = 0; i < 3; ++i) {
        const JobId job = c.launch(parks_once());
        const Entry* e = first_of(c.journal(), Mark::JobStarted, job);
        CHECK(e != nullptr);
        if (e != nullptr) {
            epochs.push_back(e->slot.epoch);
            indexes.push_back(e->slot.index);
        }
        c.post(job, i);
        CHECK(c.step());
    }

    CHECK_EQ(epochs.size(), kThree);
    if (epochs.size() == 3) {
        CHECK_EQ(indexes[0], indexes[1]);
        CHECK_EQ(indexes[1], indexes[2]);
        CHECK(epochs[0] < epochs[1]);
        CHECK(epochs[1] < epochs[2]);
    }
    CHECK_EQ(c.arena().total_check_outs(), kThree);
    CHECK_EQ(c.arena().live_slots(), kZero);
}

// =========================================================================
// Family D -- nested child tasks
// =========================================================================

// A parent awaiting a child receives the child's value. Both frames are live
// while the child runs, and both are back in the pool once the parent
// returns.
ROUNDEL_TEST(d_child_value_reaches_parent) {
    Conductor c(4);
    const JobId job = c.launch(parent_of_value(1, 2, 30));

    CHECK(c.settled(job));
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(31));
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().peak_live(), kTwo);
}

// The child frame is destroyed before the parent learns anything. The
// destruction is recorded before the continuation, and the child's contents
// are destroyed before the parent's.
ROUNDEL_TEST(d_child_destroyed_before_parent_continues) {
    Conductor c(4);
    const JobId job = c.launch(parent_of_value(1, 2, 30));

    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, job), kOne);
    CHECK(ordered(c.journal(), Mark::ChildDestroyed, Mark::ParentContinued, job));

    // The child's probe went first, and it went before the parent's.
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 2);
        CHECK_EQ(destroy_order()[1], 1);
    }
}

// Ground truth rather than a report: the pool's live count is sampled from
// inside the parent at the instant the value arrives, so a parent told the
// child is gone while its frame still exists is caught.
ROUNDEL_TEST(d_parent_continued_reports_child_gone) {
    std::vector<int> seen;
    Conductor c(4);
    const JobId job = c.launch(parent_counting(1, 2, 30, &seen));

    // Counted from inside the parent, at the instant the value arrived.
    CHECK_EQ(seen.size(), kOne);
    if (!seen.empty()) {
        CHECK_EQ(seen[0], 1);
    }

    const std::vector<std::int64_t> live = details_of(c.journal(), Mark::ParentContinued);
    CHECK_EQ(live.size(), kOne);
    if (!live.empty()) {
        CHECK_EQ(live[0], static_cast<std::int64_t>(1));
    }
    CHECK_EQ(outcome_int(c, job), kDelivered);
}

// Cancelling a parent that is waiting on a parked child destroys the inner
// frame before the outer one, returns both slots, and settles as a
// cancellation rather than a failure.
ROUNDEL_TEST(d_cancel_with_child_destroys_inner_first) {
    Conductor c(4);
    const JobId job = c.launch(parent_of_parking(1, 2));
    CHECK_EQ(probe_alive(), 2);
    CHECK_EQ(c.arena().live_slots(), kTwo);

    c.cancel(job);

    CHECK_EQ(outcome_int(c, job), kCancelled);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 2);  // the inner frame
        CHECK_EQ(destroy_order()[1], 1);  // then the outer one
    }
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);
}

// Nesting composes. Three frames are live at the deepest point, they are
// destroyed innermost first, and each level observes one fewer frame alive
// than the level below it.
ROUNDEL_TEST(d_nested_two_levels) {
    Conductor c(4);
    const JobId job = c.launch(top_level(1, 2, 3));

    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(7));
    CHECK_EQ(c.arena().peak_live(), kThree);
    CHECK_EQ(c.arena().live_slots(), kZero);

    CHECK_EQ(destroy_order().size(), kThree);
    if (destroy_order().size() == 3) {
        CHECK_EQ(destroy_order()[0], 3);
        CHECK_EQ(destroy_order()[1], 2);
        CHECK_EQ(destroy_order()[2], 1);
    }

    // Each level sees one fewer frame alive than the level below it.
    const std::vector<std::int64_t> live = details_of(c.journal(), Mark::ParentContinued);
    CHECK_EQ(live.size(), kTwo);
    if (live.size() == 2) {
        CHECK_EQ(live[0], static_cast<std::int64_t>(2));
        CHECK_EQ(live[1], static_cast<std::int64_t>(1));
    }
}

// A child's slot returns to the pool alongside its parent's, so a second job
// of the same shape fits in the same capacity without the pool growing or
// returning a slot twice.
ROUNDEL_TEST(d_child_slot_returns_to_arena) {
    Conductor c(2);
    const JobId a = c.launch(parent_of_value(1, 2, 8));
    CHECK_EQ(outcome_int(c, a), kDelivered);
    CHECK_EQ(c.arena().live_slots(), kZero);

    // Both slots are free again, so a two-frame job fits a second time.
    const JobId b = c.launch(parent_of_value(3, 4, 9));
    CHECK_EQ(outcome_int(c, b), kDelivered);
    CHECK_EQ(*c.value_of(b), static_cast<std::int64_t>(10));
    CHECK_EQ(c.arena().total_check_outs(), static_cast<std::size_t>(4));
    CHECK_EQ(c.arena().peak_live(), kTwo);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// =========================================================================
// Family E -- exceptions
// =========================================================================

// A child that throws faults the parent that awaited it, once: one settle,
// one announcement, and no value readable.
ROUNDEL_TEST(e_child_throw_faults_parent_once) {
    Conductor c(4);
    const JobId job = c.launch(parent_of_throwing(1, 2));

    CHECK_EQ(outcome_int(c, job), kFaulted);
    CHECK(!c.value_of(job).has_value());
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
}

// The failing path destroys the child frame exactly as the succeeding path
// does. The frame is not left checked out while the exception travels.
ROUNDEL_TEST(e_child_throw_destroys_child_frame) {
    Conductor c(4);
    const JobId job = c.launch(parent_of_throwing(1, 2));

    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, job), kOne);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 2);
        CHECK_EQ(destroy_order()[1], 1);
    }
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().free_slots(), c.arena().capacity());
}

// A job whose body throws after being resumed settles as a failure, announced
// once.
ROUNDEL_TEST(e_body_throw_faults_job) {
    Conductor c(4);
    const JobId job = c.launch(throws_after_park());
    c.post(job, 1);
    CHECK(c.step());

    CHECK_EQ(outcome_int(c, job), kFaulted);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
}

// A failed job's storage comes back like any other job's, and it comes back
// after the settle rather than before it.
ROUNDEL_TEST(e_fault_releases_storage) {
    Conductor c(4);
    const JobId job = c.launch(throws_after_park_probed(6));
    c.post(job, 1);
    CHECK(c.step());

    CHECK_EQ(outcome_int(c, job), kFaulted);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, job), kOne);
    CHECK(ordered(c.journal(), Mark::JobSettled, Mark::FrameReleased, job));
}

// Failing and succeeding jobs alternating through a small pool: every frame
// is returned, none is returned twice, and the number of announcements
// matches the number of failures.
ROUNDEL_TEST(e_exception_does_not_break_arena_accounting) {
    Conductor c(2);
    for (int i = 0; i < 8; ++i) {
        if (i % 2 == 0) {
            const JobId job = c.launch(throws_after_park());
            c.post(job, 1);
            CHECK(c.step());
            CHECK_EQ(outcome_int(c, job), kFaulted);
        } else {
            const JobId job = c.launch(parks_once());
            c.post(job, i);
            CHECK(c.step());
            CHECK_EQ(outcome_int(c, job), kDelivered);
        }
        CHECK_EQ(c.arena().live_slots(), kZero);
    }

    CHECK_EQ(c.arena().total_check_outs(), static_cast<std::size_t>(8));
    CHECK_EQ(c.arena().peak_live(), kOne);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
    CHECK_EQ(c.journal().count(Mark::FaultReported), static_cast<std::size_t>(4));
}

// A failing ticket raises where the job was parked, so the job may catch it
// and complete normally. A caught failure is not the job's outcome and is not
// announced.
ROUNDEL_TEST(e_job_may_handle_posted_fault) {
    std::vector<int> seen;
    Conductor c(4);
    const JobId job = c.launch(catches_posted_fault(&seen));
    c.post_fault(job);
    CHECK(c.step());

    CHECK_EQ(seen.size(), kOne);
    if (!seen.empty()) {
        CHECK_EQ(seen[0], 3);
    }
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(-3));
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);
    CHECK_EQ(c.arena().live_slots(), kZero);
}

// A cancellation arriving after a job has already failed leaves the failure
// standing: no second settle, no second announcement, no second release.
ROUNDEL_TEST(e_fault_then_cancel_settles_once) {
    Conductor c(4);
    const JobId job = c.launch(throws_after_park());
    c.post(job, 1);
    CHECK(c.step());
    CHECK_EQ(outcome_int(c, job), kFaulted);

    c.cancel(job);

    CHECK_EQ(outcome_int(c, job), kFaulted);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, job), kOne);
}

// =========================================================================
// Family F -- multiple independent operations
// =========================================================================

// Delivering to one job leaves the other exactly where it was -- still
// parked, still unsettled, still holding its slot.
ROUNDEL_TEST(f_two_jobs_progress_independently) {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    c.post(b, 64);
    CHECK(c.step());

    CHECK(c.settled(b));
    CHECK_EQ(*c.value_of(b), static_cast<std::int64_t>(64));
    CHECK(!c.settled(a));
    CHECK_EQ(c.live_jobs(), kOne);
    CHECK_EQ(c.arena().live_slots(), kOne);
}

// Cancelling one job settles only that one. The other keeps its frame and
// later receives its own completion normally.
ROUNDEL_TEST(f_cancel_one_does_not_affect_other) {
    Conductor c(4);
    const JobId a = c.launch(parks_probed(1));
    const JobId b = c.launch(parks_probed(2));

    c.cancel(a);
    CHECK_EQ(probe_alive(), 1);

    c.post(b, 15);
    CHECK(c.step());

    CHECK_EQ(outcome_int(c, a), kCancelled);
    CHECK_EQ(outcome_int(c, b), kDelivered);
    CHECK_EQ(*c.value_of(b), static_cast<std::int64_t>(15));
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 1);
        CHECK_EQ(destroy_order()[1], 2);
    }
}

// A failure stays with the job it belongs to. The unrelated job completes
// normally and nothing is announced against it.
ROUNDEL_TEST(f_fault_one_does_not_affect_other) {
    Conductor c(4);
    const JobId a = c.launch(throws_after_park());
    const JobId b = c.launch(parks_once());

    c.post(a, 1);
    CHECK(c.step());
    c.post(b, 2);
    CHECK(c.step());

    CHECK_EQ(outcome_int(c, a), kFaulted);
    CHECK_EQ(outcome_int(c, b), kDelivered);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, b), kZero);
    CHECK_EQ(c.journal().count(Mark::FaultReported), kOne);
    CHECK_EQ(c.arena().live_slots(), kZero);
}

// Tickets ready at the same time are delivered by lowest job identifier, not
// in the order they happened to be queued.
ROUNDEL_TEST(f_interleaved_delivery_follows_delivery_order) {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    const JobId d = c.launch(parks_once());

    // Queued newest-job-first and all ready at 0, so the queue order and the
    // delivery order disagree: the lowest identifier goes first.
    c.post(d, 3);
    c.post(a, 1);
    c.post(b, 2);
    CHECK_EQ(c.drain(), kThree);

    const std::vector<JobId> accepted = jobs_of(c.journal(), Mark::TicketAccepted);
    CHECK_EQ(accepted.size(), kThree);
    if (accepted.size() == 3) {
        CHECK(accepted[0] == a);
        CHECK(accepted[1] == b);
        CHECK(accepted[2] == d);
    }
    CHECK_EQ(*c.value_of(a), static_cast<std::int64_t>(1));
    CHECK_EQ(*c.value_of(b), static_cast<std::int64_t>(2));
    CHECK_EQ(*c.value_of(d), static_cast<std::int64_t>(3));
}

// =========================================================================
// Family G -- pool invariants
// =========================================================================

// The pool's peak occupancy follows how many frames were live at once, not
// how many jobs the workload put through it.
ROUNDEL_TEST(g_peak_live_bounded_by_inflight) {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    CHECK_EQ(c.arena().peak_live(), kTwo);

    c.post(a, 1);
    CHECK(c.step());
    const JobId d = c.launch(parks_once());
    c.post(b, 2);
    c.post(d, 3);
    CHECK_EQ(c.drain(), kTwo);

    CHECK_EQ(c.arena().peak_live(), kTwo);
    CHECK_EQ(c.arena().live_slots(), kZero);
}

// Across a workload that mixes cancelling before delivery with cancelling
// after it, no slot is ever handed back to the pool twice.
ROUNDEL_TEST(g_no_double_reclaims_across_workload) {
    Conductor c(3);
    for (int i = 0; i < 10; ++i) {
        const JobId job = c.launch(parks_once());
        if (i % 3 == 0) {
            c.cancel(job);
            c.post(job, i);
            CHECK(c.step());
        } else {
            c.post(job, i);
            CHECK(c.step());
            c.cancel(job);
        }
    }
    CHECK_EQ(c.arena().double_reclaims(), kZero);
    CHECK_EQ(c.arena().live_slots(), kZero);
}

// Many jobs through a two-slot pool. The pool stays its declared size and
// satisfies them by reuse, and every slot is free at the end.
ROUNDEL_TEST(g_capacity_is_reused_not_grown) {
    Conductor c(2);
    for (int i = 0; i < 20; ++i) {
        const JobId job = c.launch(parks_once());
        c.post(job, i);
        CHECK(c.step());
        CHECK_EQ(outcome_int(c, job), kDelivered);
    }

    CHECK_EQ(c.arena().capacity(), kTwo);
    CHECK_EQ(c.arena().total_check_outs(), static_cast<std::size_t>(20));
    CHECK_LE(c.arena().peak_live(), kTwo);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().free_slots(), kTwo);
}

// A mixed set of jobs ended by delivery, cancellation and failure leaves
// nothing checked out and nothing alive inside a frame.
ROUNDEL_TEST(g_all_slots_returned_at_end) {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parent_of_parking(1, 2));
    const JobId d = c.launch(throws_after_park());

    c.post(d, 1);
    CHECK(c.step());
    c.cancel(b);
    c.post(a, 2);
    CHECK(c.step());

    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().live_slot_ids().size(), kZero);
    CHECK_EQ(c.arena().free_slots(), c.arena().capacity());
    CHECK_EQ(probe_alive(), 0);
}

// A full pool refuses a further frame by throwing rather than by
// overcommitting, and the refusal costs the job already running nothing.
ROUNDEL_TEST(g_exhaustion_is_reported) {
    Conductor c(1);
    const JobId job = c.launch(parks_once());
    CHECK_EQ(c.arena().live_slots(), kOne);

    bool threw = false;
    try {
        Duty extra = parks_once();
        (void)extra;
    } catch (const std::bad_alloc&) {
        threw = true;
    }
    CHECK(threw);

    // The failed attempt cost nothing: the running job is still fine.
    c.post(job, 5);
    CHECK(c.step());
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// =========================================================================
// Family H -- adversarial schedules
// =========================================================================

// A cancellation landing between queueing a ticket and delivering it wins:
// the job settles once as cancelled, the ticket is refused, and the frame is
// destroyed.
ROUNDEL_TEST(h_cancel_between_queue_and_delivery) {
    Conductor c(4);
    const JobId job = c.launch(parks_probed(1));
    c.post(job, 99);
    c.cancel(job);
    CHECK(c.step());

    CHECK_EQ(outcome_int(c, job), kCancelled);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, job), kZero);
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, job), kOne);
    CHECK_EQ(probe_alive(), 0);
}

// Two jobs trading the same storage round after round, one delivered and one
// cancelled each time, with no slot handed back twice and the pool never
// growing.
ROUNDEL_TEST(h_two_jobs_swap_slots_repeatedly) {
    Conductor c(2);
    for (int round = 0; round < 6; ++round) {
        const JobId a = c.launch(parks_once());
        const JobId b = c.launch(parks_once());
        c.post(a, round);
        c.cancel(b);
        CHECK(c.step());
        CHECK_EQ(outcome_int(c, a), kDelivered);
        CHECK_EQ(outcome_int(c, b), kCancelled);
        CHECK_EQ(c.arena().live_slots(), kZero);
    }
    CHECK_EQ(c.arena().total_check_outs(), static_cast<std::size_t>(12));
    CHECK_LE(c.arena().peak_live(), kTwo);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// A stale ticket that comes due after its slot has been reissued more than
// once is still refused, and the job living there by then is untouched.
ROUNDEL_TEST(h_stale_ticket_arrives_after_two_reuses) {
    Conductor c(1);
    const JobId a = c.launch(parks_once());
    c.post(a, 1000);
    c.cancel(a);

    const JobId b = c.launch(parks_once());
    c.post(b, 2);
    CHECK(c.step());  // a's stale ticket -- must be rejected
    CHECK(c.step());  // b's own ticket
    CHECK_EQ(outcome_int(c, b), kDelivered);
    CHECK_EQ(*c.value_of(b), static_cast<std::int64_t>(2));

    const JobId d = c.launch(parks_probed(9));
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, a), kOne);
    CHECK(!c.settled(d));
    CHECK_EQ(probe_alive(), 1);
    CHECK_EQ(c.arena().total_check_outs(), kThree);
}

// A job cancelling another from inside its own body: the cancelled job
// settles once and its frame goes back, while the running job completes
// normally.
ROUNDEL_TEST(h_cancel_during_delivery_of_another_job) {
    Conductor c(4);
    JobId victim = roundel::kNoJob;
    const JobId actor = c.launch(cancels_other(&c, &victim));
    victim = c.launch(parks_probed(5));

    c.post(actor, 7);
    CHECK(c.step());

    CHECK_EQ(outcome_int(c, actor), kDelivered);
    CHECK_EQ(*c.value_of(actor), static_cast<std::int64_t>(7));
    CHECK_EQ(outcome_int(c, victim), kCancelled);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, victim), kOne);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// A job that cancels itself while running cannot be destroyed from under
// itself. It settles once, and its frame is torn down at the next quiet
// point.
ROUNDEL_TEST(h_self_cancel_from_inside_job) {
    Conductor c(4);
    JobId self = roundel::kNoJob;
    self = c.launch(cancels_self(&c, &self));

    c.post(self, 4);
    CHECK(c.step());

    CHECK_EQ(outcome_int(c, self), kCancelled);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, self), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, self), kOne);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().free_slots(), c.arena().capacity());
}

// A cancellation naming a job that already settled must not reach the job
// that inherited its storage: no second settle for the dead job, and no
// destruction of the new occupant.
ROUNDEL_TEST(h_late_cancel_after_frame_reuse) {
    Conductor c(1);
    const JobId a = c.launch(parks_once());
    c.cancel(a);

    const JobId b = c.launch(parks_probed(8));  // takes over a's storage
    c.cancel(a);                                // late, and about a dead job

    CHECK_EQ(c.journal().count_for(Mark::JobSettled, a), kOne);
    CHECK(!c.settled(b));
    CHECK_EQ(probe_alive(), 1);
    CHECK_EQ(c.arena().live_slots(), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, b), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// A long mixed workload of plain, cancelled, failing and nested jobs. Every
// job settles once and releases once, and the pool ends empty with nothing
// returned twice.
ROUNDEL_TEST(h_mixed_workload_invariants) {
    Conductor c(4);
    std::vector<JobId> ids;
    int faulted = 0;

    for (int i = 0; i < 12; ++i) {
        switch (i % 4) {
            case 0: {
                const JobId job = c.launch(parks_once());
                ids.push_back(job);
                c.post(job, i);
                break;
            }
            case 1: {
                const JobId job = c.launch(parks_probed(i));
                ids.push_back(job);
                c.cancel(job);
                c.post(job, i);
                break;
            }
            case 2: {
                const JobId job = c.launch(throws_after_park());
                ids.push_back(job);
                c.post(job, i);
                ++faulted;
                break;
            }
            default: {
                const JobId job = c.launch(parent_of_value(100 + i, 200 + i, i));
                ids.push_back(job);
                break;
            }
        }
        c.drain();
    }

    CHECK_EQ(ids.size(), static_cast<std::size_t>(12));
    for (JobId job : ids) {
        CHECK(c.settled(job));
        CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
        CHECK_EQ(c.journal().count_for(Mark::FrameReleased, job), kOne);
    }
    CHECK_EQ(c.journal().count(Mark::FaultReported), static_cast<std::size_t>(faulted));
    CHECK_EQ(c.live_jobs(), kZero);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().free_slots(), c.arena().capacity());
    CHECK_EQ(c.arena().double_reclaims(), kZero);
    CHECK_EQ(probe_alive(), 0);
}

// =========================================================================
// Family I -- delivery order and the clock
// =========================================================================

// Ready time is the first key of the delivery order: a ticket queued first
// but ready later goes second, and delivering an already-ready ticket does
// not move the clock.
ROUNDEL_TEST(i_ready_time_beats_queue_order) {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());

    CHECK_EQ(c.now(), static_cast<std::uint64_t>(0));
    c.post_after(a, 10, 5);  // queued first, ready last
    c.post(b, 20);           // queued second, ready now

    CHECK(c.step());
    CHECK(c.settled(b));
    CHECK(!c.settled(a));
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(0));

    CHECK(c.step());
    CHECK(c.settled(a));
    CHECK_EQ(*c.value_of(a), static_cast<std::int64_t>(10));
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(5));
}

// Tickets sharing a ready time are ordered by lowest job identifier whatever
// order they were queued in, and the clock lands on that shared ready time.
ROUNDEL_TEST(i_equal_ready_time_breaks_on_lowest_job_id) {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    const JobId d = c.launch(parks_once());

    c.post_after(d, 3, 2);
    c.post_after(b, 2, 2);
    c.post_after(a, 1, 2);

    CHECK_EQ(c.drain(), kThree);
    const std::vector<JobId> accepted = jobs_of(c.journal(), Mark::TicketAccepted);
    CHECK_EQ(accepted.size(), kThree);
    if (accepted.size() == 3) {
        CHECK(accepted[0] == a);
        CHECK(accepted[1] == b);
        CHECK(accepted[2] == d);
    }
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(2));
}

// Two tickets for one job at one time fall back to queue order. Both wait on
// the same frame, so both are valid, and refusing the second is as wrong as
// reordering them.
ROUNDEL_TEST(i_same_job_same_time_keeps_queue_order) {
    Conductor c(4);
    std::vector<std::int64_t> got;
    const JobId job = c.launch(parks_twice(&got));

    c.post(job, 7);
    c.post(job, 8);
    CHECK_EQ(c.drain(), kTwo);

    // Both waits are the same frame, so both tickets are valid and the earlier
    // one goes first. Refusing the second is as wrong as reordering them.
    CHECK_EQ(got.size(), kTwo);
    if (got.size() == 2) {
        CHECK_EQ(got[0], static_cast<std::int64_t>(7));
        CHECK_EQ(got[1], static_cast<std::int64_t>(8));
    }
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, job), kTwo);
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, job), kZero);
    CHECK_EQ(outcome_int(c, job), kDelivered);
}

// The clock follows what was delivered and nothing else: it moves to a
// delivered ticket's ready time, never steps on its own, and never goes
// backwards for a ticket queued later.
ROUNDEL_TEST(i_clock_moves_to_ready_time_and_never_back) {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    c.post_after(a, 1, 7);
    c.post_after(b, 2, 2);

    CHECK(c.step());  // b, ready 2
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(2));
    CHECK(c.step());  // a, ready 7
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(7));

    // A ticket queued now is ready now, and delivering it leaves the clock
    // where it is rather than stepping it on.
    const JobId d = c.launch(parks_once());
    c.post(d, 3);
    CHECK(c.step());
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(7));
    CHECK(!c.step());
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(7));
}

// A delay is resolved against the clock as it stands when the ticket is
// queued, so a ticket posted from inside a running job is never ready before
// the step that resumed it.
ROUNDEL_TEST(i_delay_is_measured_from_the_queueing_clock) {
    Conductor c(4);
    JobId other = roundel::kNoJob;
    const JobId actor = c.launch(posts_for_other(&c, &other, 2));
    other = c.launch(parks_once());

    c.post_after(actor, 5, 4);
    CHECK(c.step());
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(4));
    CHECK_EQ(outcome_int(c, actor), kDelivered);

    // Queued at 4 with a delay of 2, so ready at 6 -- not at 2, and not at the
    // step that delivers it.
    CHECK(c.step());
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(6));
    CHECK_EQ(outcome_int(c, other), kDelivered);
    CHECK_EQ(*c.value_of(other), static_cast<std::int64_t>(6));
}

// The delayed fault. post_fault_after resolves its ready time against the clock
// as it stands when the ticket is queued, takes its turn in the delivery order
// like any other ticket, and is bound to the one frame the job was parked in at
// that moment -- so a delayed fault whose job has settled and whose slot has
// since been handed out again is refused, and refusing it neither faults nor
// destroys the frame that occupies that storage now. The job's own delayed fault
// then settles it once, announces the failure once, and hands the storage back.
// A further delayed fault for a job that has already failed is refused and adds
// no second announcement.
ROUNDEL_TEST(i_delayed_fault_keeps_its_ready_time_and_its_frame) {
    Conductor c(2);

    // A delayed fault for a job that will not be there when it comes due.
    const JobId gone = c.launch(parks_probed(51));
    c.post_fault_after(gone, 3);
    c.cancel(gone);
    CHECK_EQ(outcome_int(c, gone), kCancelled);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);

    // The pool hands the same storage out again, one generation on.
    const JobId heir = c.launch(parks_probed(52));
    c.post_fault_after(heir, 5);
    const JobId other = c.launch(parks_once());
    c.post(other, 7);
    CHECK_EQ(probe_alive(), 1);
    CHECK_EQ(c.pending_tickets(), kThree);
    CHECK(c.journal().count(Mark::SlotReused) >= kOne);

    // Ready at 0 goes first, whatever order the three were queued in, and the
    // clock has not moved.
    CHECK(c.step());
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(0));
    CHECK_EQ(outcome_int(c, other), kDelivered);
    CHECK_EQ(*c.value_of(other), static_cast<std::int64_t>(7));

    // The stale delayed fault. It names a settled job at a slot whose
    // generation has moved on, so it is refused and touches nothing: no fault
    // for the job it names, and none for the job living at that address.
    CHECK(c.step());
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(3));
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, gone), kOne);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, gone), kZero);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, gone), kZero);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, gone), kOne);
    CHECK_EQ(outcome_int(c, gone), kCancelled);
    CHECK_EQ(outcome_int(c, heir), kUnsettled);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, heir), kZero);
    CHECK_EQ(probe_alive(), 1);
    CHECK_EQ(c.arena().live_slots(), kOne);

    // The heir's own delayed fault, at its own ready time: accepted, and the
    // failure escapes the body it was raised in.
    CHECK(c.step());
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(5));
    CHECK_EQ(outcome_int(c, heir), kFaulted);
    CHECK(!c.value_of(heir).has_value());
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, heir), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, heir), kOne);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, heir), kOne);
    CHECK(ordered(c.journal(), Mark::JobSettled, Mark::FrameReleased, heir));
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK(!c.step());

    // A delayed fault for a job that has already failed changes nothing, and
    // the clock still follows the ticket that was delivered.
    c.post_fault_after(heir, 4);
    CHECK(c.step());
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(9));
    CHECK_EQ(outcome_int(c, heir), kFaulted);
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, heir), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, heir), kOne);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, heir), kOne);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// =========================================================================
// Family J -- a ticket names one frame, by index and generation together
// =========================================================================

// The job is alive, unsettled and parked when the ticket is delivered. Only the
// frame is wrong: the ticket was made for the root, and the job has moved on to
// a child. The generation of both frames is 1, so a runtime that compares
// generations alone accepts this, and a runtime that asks nothing but "is this
// job parked" accepts it too.
ROUNDEL_TEST(j_ticket_for_outer_frame_rejected_while_inner_parked) {
    Conductor c(4);
    const JobId job = c.launch(parks_then_awaits_parking_child(11));

    c.post_after(job, 100, 5);  // made for the root frame
    c.post(job, 1);             // also the root frame, delivered first

    CHECK(c.step());  // resumes the root, which then awaits a parking child
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, job), kOne);
    CHECK_EQ(probe_alive(), 1);
    CHECK_EQ(c.arena().live_slots(), kTwo);

    CHECK(c.step());  // the root's late ticket, now that the child is waiting
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, job), kOne);
    CHECK(!c.settled(job));
    CHECK_EQ(probe_alive(), 1);
    CHECK_EQ(c.arena().live_slots(), kTwo);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// The mirror image: the ticket was made for a child frame that has since
// returned, and the job is parked again in a *different* child at the same
// index, one generation on. Index alone accepts it; generation alone accepts
// it; both together do not.
ROUNDEL_TEST(j_ticket_for_returned_child_rejected_after_reuse) {
    Conductor c(4);
    const JobId job = c.launch(parks_then_awaits_parking_child(12));

    c.post(job, 1);
    CHECK(c.step());  // root resumed, first child now parked

    c.post_after(job, 100, 9);  // made for the first child's frame
    c.post(job, 2);             // also that frame, delivered first

    CHECK(c.step());  // first child returns 2; root settles at 1 + 2
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(3));

    // A fresh job takes the storage the child gave back, so the stale ticket's
    // index now belongs to somebody else entirely.
    const JobId later = c.launch(parks_probed(13));
    CHECK(c.step());
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, job), kOne);
    CHECK(!c.settled(later));
    CHECK_EQ(probe_alive(), 1);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, later), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// The job never settles, never leaves the pool, and is parked at the same slot
// index the ticket names. Only the generation differs: the frame the ticket was
// made for returned, its slot went back, and the job's next child took the same
// slot. Comparing the index alone accepts this.
ROUNDEL_TEST(j_same_index_new_generation_is_rejected) {
    Conductor c(4);
    const JobId job = c.launch(awaits_two_parking_children(15, 16));
    CHECK_EQ(probe_alive(), 1);

    c.post_after(job, 100, 9);  // made for the first child's frame
    c.post(job, 1);             // the same frame, delivered first

    CHECK(c.step());  // first child returns; its slot goes back; second child takes it
    CHECK_EQ(probe_alive(), 1);
    CHECK(!c.settled(job));
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, job), kOne);
    CHECK_EQ(c.arena().live_slots(), kTwo);

    CHECK(c.step());  // the stale ticket: right index, wrong generation
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, job), kOne);
    CHECK(!c.settled(job));
    CHECK(!c.value_of(job).has_value());
    CHECK_EQ(probe_alive(), 1);
    CHECK_EQ(c.arena().double_reclaims(), kZero);

    // The frame that is actually waiting can still be reached.
    c.post(job, 4);
    CHECK(c.step());
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(5));
    CHECK_EQ(c.arena().live_slots(), kZero);
}

// A ticket queued for an identifier that is not parked -- here, not yet
// launched -- is bound to no frame at all. The job it names exists and is
// parked by the time the ticket is delivered, which is exactly the state a
// runtime that resolves the frame at delivery would accept.
ROUNDEL_TEST(j_ticket_queued_before_launch_is_never_accepted) {
    Conductor c(4);
    const JobId first = c.launch(parks_once());
    const JobId predicted = first + 1;

    c.post(predicted, 77);
    const JobId second = c.launch(parks_probed(14));
    CHECK(second == predicted);

    CHECK(c.step());
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, second), kOne);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, second), kZero);
    CHECK(!c.settled(second));
    CHECK(!c.value_of(second).has_value());
    CHECK_EQ(probe_alive(), 1);

    // The other job is untouched by any of it.
    c.post(first, 5);
    CHECK(c.step());
    CHECK_EQ(outcome_int(c, first), kDelivered);
    CHECK_EQ(*c.value_of(first), static_cast<std::int64_t>(5));
}

// Slots churn underneath a parked job. Its own ticket still names its own
// frame, so it must still be delivered: rejecting everything that looks old is
// not an implementation of the rule.
ROUNDEL_TEST(j_binding_survives_unrelated_reuse) {
    Conductor c(2);
    const JobId keeper = c.launch(parks_once());
    c.post_after(keeper, 42, 4);

    for (int round = 0; round < 4; ++round) {
        const JobId churn = c.launch(parks_probed(round));
        c.cancel(churn);
        CHECK_EQ(probe_alive(), 0);
    }

    CHECK(c.step());
    CHECK_EQ(outcome_int(c, keeper), kDelivered);
    CHECK_EQ(*c.value_of(keeper), static_cast<std::int64_t>(42));
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, keeper), kZero);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
    CHECK(c.journal().count(Mark::SlotReused) >= kOne);
}

// =========================================================================
// Family K -- cancellation counts its requests exactly
// =========================================================================

// Requests and outcomes are counted separately: repeated cancellation calls
// are repeated recorded requests, but one settle, one release and one
// destruction.
ROUNDEL_TEST(k_cancel_records_one_request_per_call) {
    Conductor c(4);
    const JobId job = c.launch(parks_probed(21));

    c.cancel(job);  // the one that settles it
    c.cancel(job);  // after settling
    c.cancel(job);  // and again

    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, job), kThree);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, job), kOne);
    CHECK_EQ(outcome_int(c, job), kCancelled);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// A cancellation naming an identifier that was never launched is still
// recorded as a request, yet settles nothing, releases nothing, and leaves
// live jobs alone.
ROUNDEL_TEST(k_cancel_of_unknown_id_is_recorded_and_harmless) {
    Conductor c(4);
    const JobId live = c.launch(parks_once());
    const JobId never = live + 500;

    c.cancel(never);
    c.cancel(never);

    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, never), kTwo);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, never), kZero);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, never), kZero);
    CHECK_EQ(c.live_jobs(), kOne);

    // The real job is unaffected.
    c.post(live, 6);
    CHECK(c.step());
    CHECK_EQ(outcome_int(c, live), kDelivered);
    CHECK_EQ(*c.value_of(live), static_cast<std::int64_t>(6));
    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, live), kZero);
}

// =========================================================================
// Family L -- the compound schedule, read off the trace
// =========================================================================

// Reused storage, a delayed completion for the frame that used to live there, a
// new coroutine at that same address, a cancellation, and two tickets ready at
// the same time that have to be ordered by identifier. The trace is the
// assertion: it is fixed by the rules, and every one of those rules has to hold
// for the string to come out.
ROUNDEL_TEST(l_compound_schedule_trace_is_exact) {
    Conductor c(4);
    Tracer tr(&c);

    const JobId a = c.launch(parks_probed(31));
    c.post_after(a, 100, 3);  // for a's frame, ready later
    c.cancel(a);              // a settles; its slot goes back
    tr.absorb();
    CHECK_EQ(probe_alive(), 0);

    const JobId b = c.launch(parent_of_parking(32, 33));  // parent takes a's old slot
    const JobId d = c.launch(parks_once());
    tr.absorb();

    c.post(d, 9);  // ready 0, higher identifier, queued first
    c.post(b, 5);  // ready 0, lower identifier, queued second

    CHECK(tr.step());
    CHECK(tr.step());
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("S1=X;A2@0;C2;P2:2;S2=D;A3@0;S3=D;R1@3"));

    CHECK_EQ(outcome_int(c, a), kCancelled);
    CHECK_EQ(outcome_int(c, b), kDelivered);
    CHECK_EQ(*c.value_of(b), static_cast<std::int64_t>(5));
    CHECK_EQ(outcome_int(c, d), kDelivered);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
    CHECK(c.arena().peak_live() <= kThree);
}

// One job, two tickets for its root frame queued at the same time. The first is
// its own; delivering it moves the job into a child, which makes the second a
// ticket for a frame that is no longer waiting. Then a ticket for the child,
// after a delay, finishes the job.
ROUNDEL_TEST(l_equal_time_then_inner_frame_trace_is_exact) {
    Conductor c(4);
    Tracer tr(&c);

    const JobId job = c.launch(parks_then_awaits_parking_child(34));
    tr.absorb();

    c.post(job, 1);
    c.post(job, 2);
    CHECK(tr.step());
    CHECK(tr.step());
    CHECK(!c.settled(job));

    c.post_after(job, 6, 4);
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;R1@0;A1@4;C1;P1:1;S1=D"));
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(7));
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(4));
    CHECK_EQ(c.arena().live_slots(), kZero);
}

// =========================================================================
// Family M -- compound schedules
//
// Every case below needs at least three of the public rules to hold at once:
// getting any single one wrong changes the trace, so none of these can be
// satisfied by a shortcut that happens to be right elsewhere. Each case notes
// the realistic wrong implementations it separates.
// =========================================================================

// One job, two tickets, both queued while it was parked in its first child.
// Delivering the first lets that child return, so its slot goes back and the
// job's next child is handed the very same index one generation later. The
// second ticket then names a frame at the same address and the same index as
// the frame the job is parked in now -- only the generation separates them.
//
// Needs at once: both halves of the frame identity, the child frame being
// destroyed before the parent continues, and the clock following the delivered
// ticket's ready time rather than a step count.
//
// Separates: index-only binding and raw-address binding, which both resume the
// second ticket into a frame it was never made for; a parent told the child is
// gone before it is; and a step-counting clock, which mis-stamps every mark.
ROUNDEL_TEST(m_stale_ticket_across_two_generations_trace) {
    Conductor c(4);
    Tracer tr(&c);

    const JobId job = c.launch(awaits_two_parking_children(71, 72));
    tr.absorb();

    c.post(job, 1);           // ready 0, names the first child's frame
    c.post_after(job, 2, 3);  // ready 3, also names the first child's frame

    CHECK(tr.step());  // first child returns; second child takes its index
    CHECK(!c.settled(job));

    CHECK(tr.step());  // same index, older generation: refused
    CHECK(!c.settled(job));

    c.post_after(job, 5, 4);  // ready 7, for the frame actually waiting
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;C1;P1:1;R1@3;A1@7;C1;P1:1;S1=D"));

    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(6));
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(7));
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// A job resumed at a moved-on clock queues delayed work for another job from
// inside itself. That ticket lands at the same ready time as one queued earlier
// for a third job, so the identifier tie-break decides which goes first.
//
// Needs at once: the clock moving to the delivered ticket's ready time, a delay
// measured from the clock as it stands at queue time, and the equal-ready-time
// tie-break on lowest identifier.
//
// Separates: a delay resolved against a stale or pre-resume clock, which makes
// the queued ticket ready far too early; a clock driven by step count; and an
// equal-time tie broken by queue order, which would run the third job first.
ROUNDEL_TEST(m_delay_from_resumed_clock_then_identifier_tie_trace) {
    Conductor c(4);
    Tracer tr(&c);

    JobId other = roundel::kNoJob;
    const JobId poster = c.launch(posts_for_other(&c, &other, 1));
    other = c.launch(parks_once());
    const JobId third = c.launch(parks_once());
    tr.absorb();

    c.post_after(poster, 10, 5);  // ready 5
    c.post_after(third, 30, 6);   // ready 6

    CHECK(tr.step());  // poster at 5; queues for `other` ready 5 + 1 == 6
    CHECK(tr.step());  // tie at 6: `other` has the lower identifier
    CHECK(tr.step());  // then `third`, also ready 6
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@5;S1=D;A2@6;S2=D;A3@6;S3=D"));

    CHECK_EQ(*c.value_of(poster), static_cast<std::int64_t>(10));
    CHECK_EQ(*c.value_of(other), static_cast<std::int64_t>(11));
    CHECK_EQ(*c.value_of(third), static_cast<std::int64_t>(30));
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(6));
    CHECK_EQ(c.arena().live_slots(), kZero);
}

// One job holds two tickets for its root frame, queued while that frame was the
// one waiting. Delivering the first moves the job down into a child, so the
// second now names a frame of the job's own that is no longer parked. A third
// job competes at the same ready time throughout.
//
// Needs at once: frame identity by index as well as epoch (the child shares the
// job and can share a generation, so only the index separates them), the
// identifier tie-break at equal ready times, and the child frame being gone
// before the parent observes its result.
//
// Separates: identifier-only binding, which resumes the root's second ticket
// into the child; epoch-only binding, which does the same; and a parent that is
// told the child is gone before the frame is actually destroyed, which reports
// the wrong live count on ParentContinued.
ROUNDEL_TEST(m_outer_ticket_rejected_while_inner_parked_trace) {
    Conductor c(4);
    Tracer tr(&c);

    const JobId job = c.launch(parks_then_awaits_parking_child(63));
    const JobId bystander = c.launch(parks_once());
    tr.absorb();

    c.post(job, 1);        // ready 0, names the root frame
    c.post(job, 2);        // ready 0, also names the root frame
    c.post(bystander, 5);  // ready 0, higher identifier

    CHECK(tr.step());  // job's first ticket; it descends into the child
    CHECK(tr.step());  // job's second ticket now names a frame that is not parked
    CHECK(tr.step());  // bystander
    CHECK(!tr.step());

    c.post_after(job, 6, 4);  // for the child frame
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(),
             std::string("A1@0;R1@0;A2@0;S2=D;A1@4;C1;P1:1;S1=D"));

    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(7));
    CHECK_EQ(outcome_int(c, bystander), kDelivered);
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(4));
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// A job queues a ticket for itself at a moment when it is running rather than
// parked, then parks again in the frame it started in. Nothing was waiting when
// that ticket was made, so it authorises no frame -- and the job parking again
// afterwards, in the very same slot and generation, does not give it one.
//
// Needs at once: an unbound ticket staying unbound for good, a later ticket for
// the frame that really is waiting still being delivered, and the clock moving
// only to each delivered ticket's ready time.
//
// Separates: binding a ticket at delivery time to whatever frame happens to be
// parked, which accepts a ticket made for no frame; and refusing every late
// ticket, which would also drop the valid one that follows.
ROUNDEL_TEST(m_unbound_ticket_never_binds_after_later_park_trace) {
    Conductor c(4);
    Tracer tr(&c);

    JobId self = roundel::kNoJob;
    self = c.launch(posts_for_self_then_parks(&c, &self, 2));
    tr.absorb();

    c.post(self, 4);  // ready 0, for the frame that is parked now

    CHECK(tr.step());  // resumes; while running it queues an unbound ticket
    CHECK(!c.settled(self));

    c.post_after(self, 7, 5);  // ready 5, properly bound to the waiting frame

    CHECK(tr.step());  // the unbound ticket, ready 2: refused
    CHECK(!c.settled(self));

    CHECK(tr.step());  // the bound ticket, ready 5
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;R1@2;A1@5;S1=D"));

    CHECK_EQ(outcome_int(c, self), kDelivered);
    CHECK_EQ(*c.value_of(self), static_cast<std::int64_t>(13));
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(5));
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// A job cancels itself from inside its own resumed frame, so the frame that has
// to be torn down is the one currently executing. Its slot is then taken by a
// new job, and a ticket queued for the cancelled job before any of this arrives
// afterwards.
//
// Needs at once: settling exactly once from inside the job, tearing the frame
// down only once control has left it, and refusing the older ticket on the
// generation that the reused slot now carries.
//
// Separates: destroying the executing frame at the point of settlement, which
// is a use-after-free the sanitizer catches; deferring the release to shutdown,
// which leaves the slot checked out and stops the next job reusing it; and
// address-based binding, which resumes the new occupant.
ROUNDEL_TEST(m_self_cancel_then_slot_reuse_rejects_stale_trace) {
    Conductor c(4);
    Tracer tr(&c);

    JobId self = roundel::kNoJob;
    self = c.launch(cancels_self(&c, &self));
    tr.absorb();

    c.post(self, 4);           // resumes it; it cancels itself while running
    c.post_after(self, 99, 5);  // queued while parked, delivered long after

    CHECK(tr.step());  // self-cancellation settles the job once
    tr.absorb();
    CHECK_EQ(outcome_int(c, self), kCancelled);
    CHECK_EQ(c.arena().live_slots(), kZero);

    const JobId next = c.launch(parks_probed(64));  // takes the freed slot
    c.post(next, 8);

    CHECK(tr.step());  // next's own ticket, ready 0
    CHECK(tr.step());  // the stale ticket at 5, rejected
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;S1=X;A2@0;S2=D;R1@5"));

    CHECK_EQ(outcome_int(c, next), kDelivered);
    CHECK_EQ(*c.value_of(next), static_cast<std::int64_t>(8));
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, self), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, self), kOne);
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(5));
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

// =========================================================================
// Negative control
//
// This test must fail. The runner requires it to fail and excludes it from
// the graded total. If it ever reports a pass, results are being forged --
// by a candidate that exits successfully before running anything, or that
// prints markers it did not earn -- and the run scores zero regardless of
// what every other test reported.
// =========================================================================

// Proves the suite can actually fail. A harness that reports a pass for this
// case is not running assertions at all, so every other pass in the run is
// unverifiable and the whole run is void.
ROUNDEL_TEST(zz_negative_control_must_fail) {
    CHECK(probe_alive() == 424242);
}

int main(int argc, char** argv) { return harness::main_impl(argc, argv); }
