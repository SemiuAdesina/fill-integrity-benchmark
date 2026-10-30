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

// Candidate compound cases (author-side lab copy). Spliced after the helper
// section of graded.cpp by tools/splice_cases.py.

namespace {

// Awaits a child that throws immediately, catches the failure, then parks in
// its own frame and returns whatever ticket reaches it.
Duty catches_child_then_parks(int ptag, int ctag, std::vector<int>* seen) {
    Probe p{ptag};
    (void)p;
    try {
        co_await child_throws(ctag);
    } catch (const std::exception&) {
        seen->push_back(probe_alive());
    }
    const std::int64_t v = co_await park();
    co_return v + 1;
}

// Parks in a first child, catches that child's failure, then parks in a second
// child. The second child takes the slot the first one gave back.
Duty catches_parking_child_then_awaits_another(int ctag1, int ctag2, std::vector<int>* seen) {
    std::int64_t a = 0;
    try {
        a = co_await child_parks(ctag1);
    } catch (const std::exception&) {
        seen->push_back(probe_alive());
        a = -1;
    }
    const std::int64_t b = co_await child_parks(ctag2);
    co_return a * 100 + b;
}

// Parks in a child, catches that child's failure, then parks in its own root
// frame.
Duty catches_parking_child_then_parks(int ptag, int ctag, std::vector<int>* seen) {
    Probe p{ptag};
    (void)p;
    std::int64_t a = 0;
    try {
        a = co_await child_parks(ctag);
    } catch (const std::exception&) {
        seen->push_back(probe_alive());
        a = -1;
    }
    const std::int64_t b = co_await park();
    co_return a * 100 + b;
}

// Three levels. The leaf parks and fails on the ticket it receives; the middle
// catches and returns a value; the top then awaits a fresh leaf.
Duty leaf_parks(int tag) {
    Probe p{tag};
    (void)p;
    const std::int64_t v = co_await park();
    co_return v;
}

Duty middle_catches(int tag, int ltag, std::vector<int>* seen) {
    Probe p{tag};
    (void)p;
    std::int64_t got = 0;
    try {
        got = co_await leaf_parks(ltag);
    } catch (const std::exception&) {
        seen->push_back(probe_alive());
        got = 3;
    }
    co_return got;
}

Duty top_recovers(int tag, int mtag, int ltag, int ltag2, std::vector<int>* seen) {
    Probe p{tag};
    (void)p;
    const std::int64_t m = co_await middle_catches(mtag, ltag, seen);
    seen->push_back(probe_alive());
    const std::int64_t l = co_await child_value(ltag2, 4);
    co_return m + l;
}

// Runs to its end at once, without ever parking.
Duty returns_at_once(int tag, std::int64_t v) {
    Probe p{tag};
    (void)p;
    co_return v;
}

// Parks; when resumed, launches a second root job that finishes at once and
// records what it can see of that job from inside its own frame; then parks
// again.
Duty launches_from_inside(Conductor* c, std::vector<std::int64_t>* seen) {
    const std::int64_t v = co_await park();
    const JobId inner = c->launch(returns_at_once(52, 8));
    seen->push_back(c->settled(inner) ? 1 : 0);
    seen->push_back(static_cast<std::int64_t>(c->arena().live_slots()));
    seen->push_back(static_cast<std::int64_t>(c->journal().count_for(Mark::FrameReleased, inner)));
    seen->push_back(probe_alive());
    const std::int64_t w = co_await park();
    co_return v + w;
}

// Catches a child's failure, carries on, then fails on its own.
Duty catches_child_then_throws(int ptag, int ctag) {
    Probe p{ptag};
    (void)p;
    try {
        co_await child_throws(ctag);
    } catch (const std::exception&) {
    }
    if (p.tag >= 0) {
        throw std::runtime_error("parent failed afterwards");
    }
    co_return 0;
}

// A child that parks, and when resumed queues a ticket for its own job while
// it is running, then parks again in the same frame.
Duty child_posts_for_self(Conductor* c, const JobId* self, std::uint64_t delay) {
    const std::int64_t v = co_await park();
    c->post_after(*self, 9, delay);
    const std::int64_t w = co_await park();
    co_return v + w;
}

Duty parent_of_self_posting_child(Conductor* c, const JobId* self, std::uint64_t delay) {
    const std::int64_t got = co_await child_posts_for_self(c, self, delay);
    co_return got;
}

}  // namespace

// =========================================================================
// Family N -- candidate compound cases
// =========================================================================

ROUNDEL_TEST(n1_caught_child_fault_then_repark_trace) {
    std::vector<int> seen;
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(catches_child_then_parks(41, 42, &seen));
    tr.absorb();

    CHECK(!c.settled(job));
    CHECK_EQ(seen.size(), kOne);
    if (!seen.empty()) {
        CHECK_EQ(seen[0], 1);  // only the parent's probe is alive when it catches
    }
    CHECK_EQ(c.arena().live_slots(), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);

    c.post(job, 5);
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("C1;P1:1;A1@0;S1=D"));
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(6));
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, job), kOne);
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 42);
        CHECK_EQ(destroy_order()[1], 41);
    }
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

ROUNDEL_TEST(n2_stale_child_ticket_after_child_fault_and_reuse_trace) {
    std::vector<int> seen;
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(catches_parking_child_then_awaits_another(43, 44, &seen));
    tr.absorb();

    c.post_after(job, 100, 6);  // for the first child's frame, ready later
    c.post_fault(job);          // for the first child's frame, ready now

    CHECK(tr.step());  // the fault: first child fails, parent catches, second child parks
    CHECK(!c.settled(job));
    CHECK_EQ(seen.size(), kOne);
    if (!seen.empty()) {
        CHECK_EQ(seen[0], 0);  // the failed child is gone before the parent catches
    }
    CHECK_EQ(probe_alive(), 1);
    CHECK_EQ(c.arena().live_slots(), kTwo);

    // The second child took the first one's index, one generation on.
    const std::vector<Entry>& es = c.journal().entries();
    std::vector<SlotId> reused;
    for (const Entry& e : es) {
        if (e.mark == Mark::SlotReused) {
            reused.push_back(e.slot);
        }
    }
    CHECK_EQ(reused.size(), kOne);

    CHECK(tr.step());  // the stale ticket: same index, older generation
    CHECK(!c.settled(job));
    CHECK_EQ(probe_alive(), 1);

    c.post(job, 7);
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;C1;P1:1;R1@6;A1@6;C1;P1:1;S1=D"));
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(-93));
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, job), kTwo);
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 43);
        CHECK_EQ(destroy_order()[1], 44);
    }
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(6));
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

ROUNDEL_TEST(n3_deep_child_failure_recovery_trace) {
    std::vector<int> seen;
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(top_recovers(45, 46, 47, 48, &seen));
    tr.absorb();
    CHECK_EQ(probe_alive(), 3);
    CHECK_EQ(c.arena().live_slots(), kThree);

    c.post_fault(job);  // for the leaf's frame
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;C1;P1:2;C1;P1:1;C1;P1:1;S1=D"));
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(7));
    CHECK_EQ(seen.size(), kTwo);
    if (seen.size() == 2) {
        CHECK_EQ(seen[0], 2);  // middle catching: leaf gone, top and middle alive
        CHECK_EQ(seen[1], 1);  // top continuing: middle gone as well
    }
    CHECK_EQ(destroy_order().size(), static_cast<std::size_t>(4));
    if (destroy_order().size() == 4) {
        CHECK_EQ(destroy_order()[0], 47);
        CHECK_EQ(destroy_order()[1], 46);
        CHECK_EQ(destroy_order()[2], 48);
        CHECK_EQ(destroy_order()[3], 45);
    }
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, job), kThree);
    CHECK_EQ(c.arena().total_check_outs(), static_cast<std::size_t>(4));
    CHECK_EQ(c.arena().peak_live(), kThree);
    CHECK(c.journal().count(Mark::SlotReused) >= kOne);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

ROUNDEL_TEST(n4_job_launched_from_inside_a_running_job_settles_and_releases_at_once) {
    std::vector<std::int64_t> seen;
    Conductor c(4);
    Tracer tr(&c);
    const JobId outer = c.launch(launches_from_inside(&c, &seen));
    tr.absorb();

    c.post(outer, 1);
    CHECK(tr.step());
    CHECK(!c.settled(outer));
    const JobId inner = outer + 1;
    CHECK(c.settled(inner));
    CHECK_EQ(outcome_int(c, inner), kDelivered);
    CHECK_EQ(*c.value_of(inner), static_cast<std::int64_t>(8));

    // Seen from inside the outer job, right after launch returned.
    CHECK_EQ(seen.size(), static_cast<std::size_t>(4));
    if (seen.size() == 4) {
        CHECK_EQ(seen[0], static_cast<std::int64_t>(1));  // settled
        CHECK_EQ(seen[1], static_cast<std::int64_t>(1));  // only the outer frame is live
        CHECK_EQ(seen[2], static_cast<std::int64_t>(1));  // released already
        CHECK_EQ(seen[3], static_cast<std::int64_t>(0));  // its probe is gone
    }
    // And the record agrees: the inner job's release precedes the outer job's
    // next park.
    const Journal& j = c.journal();
    const std::size_t released = j.first_index(Mark::FrameReleased, inner);
    std::size_t second_park = j.entries().size();
    std::size_t parks = 0;
    for (std::size_t i = 0; i < j.entries().size(); ++i) {
        if (j.entries()[i].mark == Mark::JobParked && j.entries()[i].job == outer) {
            ++parks;
            if (parks == 2) {
                second_park = i;
            }
        }
    }
    CHECK(released < second_park);

    c.post(outer, 2);
    CHECK(tr.step());
    CHECK(!tr.step());
    CHECK_EQ(tr.text(), std::string("A1@0;S2=D;A1@0;S1=D"));
    CHECK_EQ(*c.value_of(outer), static_cast<std::int64_t>(3));
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

ROUNDEL_TEST(n5_fault_into_child_caught_then_root_ticket_trace) {
    std::vector<int> seen;
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(catches_parking_child_then_parks(49, 50, &seen));
    tr.absorb();

    c.post_after(job, 100, 5);  // for the child's frame, ready later
    c.post_fault(job);          // for the child's frame, ready now

    CHECK(tr.step());  // child fails, parent catches and parks in its root
    CHECK(!c.settled(job));
    CHECK_EQ(seen.size(), kOne);
    if (!seen.empty()) {
        CHECK_EQ(seen[0], 1);
    }
    CHECK_EQ(c.arena().live_slots(), kOne);

    CHECK(tr.step());  // the child's old ticket: the job is parked in a different frame
    CHECK(!c.settled(job));
    CHECK_EQ(probe_alive(), 1);

    c.post(job, 9);
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;C1;P1:1;R1@5;A1@5;S1=D"));
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(-91));
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(5));
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

ROUNDEL_TEST(n6_cancel_parent_with_child_ticket_queued_then_reuse) {
    Conductor c(3);
    const JobId job = c.launch(parent_of_parking(53, 54));
    c.post_after(job, 100, 4);  // for the child's frame
    CHECK_EQ(probe_alive(), 2);

    c.cancel(job);
    c.cancel(job);
    CHECK_EQ(outcome_int(c, job), kCancelled);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 54);
        CHECK_EQ(destroy_order()[1], 53);
    }
    CHECK_EQ(c.arena().live_slots(), kZero);

    // Two new jobs take both slots back, the second at the child's old index.
    const JobId n1 = c.launch(parks_probed(55));
    const JobId n2 = c.launch(parks_probed(56));
    c.post(n2, 2);
    c.post(n1, 1);
    CHECK_EQ(c.drain(), kThree);

    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, job), kZero);
    CHECK_EQ(outcome_int(c, n1), kDelivered);
    CHECK_EQ(outcome_int(c, n2), kDelivered);
    CHECK_EQ(*c.value_of(n1), static_cast<std::int64_t>(1));
    CHECK_EQ(*c.value_of(n2), static_cast<std::int64_t>(2));
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(4));

    c.cancel(job);
    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, job), kThree);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, job), kOne);
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(4));
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
    CHECK(c.journal().count(Mark::SlotReused) >= kTwo);
}

ROUNDEL_TEST(n7_child_fault_caught_then_parent_faults_once) {
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(catches_child_then_throws(57, 58));
    tr.absorb();

    CHECK_EQ(tr.text(), std::string("C1;P1:1;S1=F"));
    CHECK_EQ(outcome_int(c, job), kFaulted);
    CHECK(!c.value_of(job).has_value());
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::ParentContinued, job), kOne);
    CHECK(ordered(c.journal(), Mark::ChildDestroyed, Mark::ParentContinued, job));
    CHECK(ordered(c.journal(), Mark::ParentContinued, Mark::JobSettled, job));
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 58);
        CHECK_EQ(destroy_order()[1], 57);
    }
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
}

ROUNDEL_TEST(n8_self_post_from_inside_child_frame_is_unbound_trace) {
    Conductor c(4);
    Tracer tr(&c);
    JobId self = roundel::kNoJob;
    self = c.launch(parent_of_self_posting_child(&c, &self, 2));
    tr.absorb();

    c.post(self, 4);   // for the child frame that is parked now
    CHECK(tr.step());  // child resumes, queues an unbound ticket, parks again
    CHECK(!c.settled(self));

    c.post_after(self, 7, 5);  // bound to the child frame, which is waiting
    CHECK(tr.step());          // the unbound ticket at 2: refused
    CHECK(!c.settled(self));
    CHECK(tr.step());          // the bound ticket at 5
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;R1@2;A1@5;C1;P1:1;S1=D"));
    CHECK_EQ(*c.value_of(self), static_cast<std::int64_t>(11));
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(5));
    CHECK_EQ(c.arena().live_slots(), kZero);
}

ROUNDEL_TEST(n9_parent_continued_on_failure_path_trace) {
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(parent_of_throwing(59, 60));
    tr.absorb();

    CHECK_EQ(tr.text(), std::string("C1;P1:1;S1=F"));
    CHECK_EQ(outcome_int(c, job), kFaulted);
    const std::vector<std::int64_t> live = details_of(c.journal(), Mark::ParentContinued);
    CHECK_EQ(live.size(), kOne);
    if (!live.empty()) {
        CHECK_EQ(live[0], static_cast<std::int64_t>(1));
    }
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kOne);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
}

int main(int argc, char** argv) { return harness::main_impl(argc, argv); }
