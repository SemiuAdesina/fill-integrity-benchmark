// Local test harness for the roundel runtime. Not part of the deliverable.
#include "roundel/runtime.hpp"

#include <cstdio>
#include <string>
#include <vector>

using namespace roundel;

static int g_failures = 0;
static const char* g_section = "";

#define CHECK(cond)                                                              \
    do {                                                                         \
        if (!(cond)) {                                                           \
            std::printf("FAIL [%s] %s:%d: %s\n", g_section, __FILE__, __LINE__,  \
                        #cond);                                                  \
            ++g_failures;                                                        \
        }                                                                        \
    } while (0)

#define CHECK_EQ(a, b)                                                           \
    do {                                                                         \
        long long va = static_cast<long long>(a);                                \
        long long vb = static_cast<long long>(b);                                \
        if (va != vb) {                                                          \
            std::printf("FAIL [%s] %s:%d: %s == %s (%lld vs %lld)\n", g_section, \
                        __FILE__, __LINE__, #a, #b, va, vb);                     \
            ++g_failures;                                                        \
        }                                                                        \
    } while (0)

// ---------------------------------------------------------------------------
// trace reconstruction
// ---------------------------------------------------------------------------

class Runner {
  public:
    explicit Runner(Conductor& c) : c_(c) {}

    void flush() {
        const auto& es = c_.journal().entries();
        for (; pos_ < es.size(); ++pos_) {
            const Entry& e = es[pos_];
            switch (e.mark) {
                case Mark::TicketAccepted:
                    add("A" + std::to_string(e.job) + "@" + std::to_string(c_.now()));
                    break;
                case Mark::TicketRejected:
                    add("R" + std::to_string(e.job) + "@" + std::to_string(c_.now()));
                    break;
                case Mark::ChildDestroyed:
                    add("C" + std::to_string(e.job));
                    break;
                case Mark::ParentContinued:
                    add("P" + std::to_string(e.job) + ":" + std::to_string(e.detail));
                    break;
                case Mark::JobSettled:
                    add("S" + std::to_string(e.job) + "=" +
                        std::string(1, "DXF"[static_cast<int>(e.detail)]));
                    break;
                default:
                    break;
            }
        }
    }

    void step() {
        c_.step();
        flush();
    }
    void drain() {
        while (c_.step()) {
            flush();
        }
        flush();
    }
    void launch_done() { flush(); }

    std::string text() const {
        std::string out;
        for (const auto& s : lines_) {
            if (!out.empty()) {
                out += ";";
            }
            out += s;
        }
        return out;
    }

  private:
    void add(std::string s) { lines_.push_back(std::move(s)); }

    Conductor& c_;
    std::size_t pos_ = 0;
    std::vector<std::string> lines_;
};

#define CHECK_TRACE(r, expected)                                                     \
    do {                                                                             \
        std::string got = (r).text();                                                \
        if (got != (expected)) {                                                     \
            std::printf("FAIL [%s] %s:%d trace\n  got      %s\n  expected %s\n",      \
                        g_section, __FILE__, __LINE__, got.c_str(), (expected));     \
            ++g_failures;                                                            \
        }                                                                            \
    } while (0)

// ---------------------------------------------------------------------------
// jobs
// ---------------------------------------------------------------------------

static Duty one_park() { co_return co_await park(); }

static Duty two_parks() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a + b;
}

static Duty child_park() { co_return co_await park(); }

static Duty parent_then_park() {
    const std::int64_t v = co_await child_park();
    co_return v + co_await park();
}

static Duty two_children() {
    const std::int64_t a = co_await child_park();
    const std::int64_t b = co_await child_park();
    co_return a + b;
}

static Duty grandchild_park() { co_return co_await park(); }
static Duty mid_child() { co_return co_await grandchild_park(); }
static Duty top_parent() { co_return co_await mid_child(); }

static Duty thrower() {
    co_await park();
    throw std::runtime_error("boom");
}

static Duty catcher() {
    try {
        co_await park();
    } catch (const std::exception&) {
        co_return -1;
    }
    co_return 1;
}

static Duty child_throws() {
    co_await park();
    throw std::runtime_error("child boom");
}

static Duty parent_catches_child() {
    try {
        co_await child_throws();
    } catch (const std::exception&) {
        co_return 5;
    }
    co_return 6;
}

static Duty parent_lets_child_throw() { co_return co_await child_throws(); }

static std::vector<std::string> g_log;

struct Tracer {
    std::string name;
    explicit Tracer(std::string n) : name(std::move(n)) {}
    ~Tracer() { g_log.push_back(name); }
};

static Duty traced_child(std::string n) {
    Tracer t{n};
    co_return co_await park();
}

static Duty traced_mid(std::string n) {
    Tracer t{n};
    co_return co_await traced_child(n + "/inner");
}

static Duty traced_root(std::string n) {
    Tracer t{n};
    co_return co_await traced_mid(n + "/mid");
}

static Conductor* g_c = nullptr;
static JobId g_self = kNoJob;

// Cancels itself from inside the job body, before parking.
static Duty self_canceller() {
    g_c->cancel(g_self);
    co_return co_await park();
}

// Cancels itself from inside, after a delivery.
static Duty late_self_canceller() {
    const std::int64_t v = co_await park();
    g_c->cancel(g_self);
    co_return v;
}

static Duty cancel_other(JobId other) {
    g_c->cancel(other);
    co_return co_await park();
}

// ---------------------------------------------------------------------------

static void section(const char* name) { g_section = name; }

static void test_order_by_job() {
    section("order/job");
    Conductor c{4};
    Runner r{c};
    const JobId a = c.launch(one_park());
    const JobId b = c.launch(one_park());
    r.launch_done();
    c.post(b, 20);
    c.post(a, 10);
    r.drain();
    CHECK_EQ(c.value_of(a).value_or(-1), 10);
    CHECK_EQ(c.value_of(b).value_or(-1), 20);
    CHECK_EQ(c.now(), 0u);
    CHECK_TRACE(r, "A1@0;S1=D;A2@0;S2=D");
    CHECK_EQ(c.arena().live_slots(), 0u);
}

static void test_order_by_seq_same_job() {
    section("order/seq");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(two_parks());
    r.launch_done();
    c.post(j, 7);
    c.post(j, 9);
    r.drain();
    CHECK_EQ(c.value_of(j).value_or(-1), 16);
    CHECK_TRACE(r, "A1@0;A1@0;S1=D");
}

static void test_order_by_ready_time() {
    section("order/ready");
    Conductor c{4};
    Runner r{c};
    const JobId a = c.launch(one_park());
    const JobId b = c.launch(one_park());
    r.launch_done();
    c.post_after(b, 1, 0);
    c.post_after(a, 2, 5);
    r.step();
    CHECK_EQ(c.now(), 0u);
    CHECK_TRACE(r, "A2@0;S2=D");
    r.step();
    CHECK_EQ(c.now(), 5u);
    CHECK_TRACE(r, "A2@0;S2=D;A1@5;S1=D");
    CHECK_EQ(c.value_of(a).value_or(-1), 2);
}

static void test_clock_from_queue_time() {
    section("clock");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(two_parks());
    r.launch_done();
    c.post(j, 1);  // ready 0
    r.step();
    CHECK_EQ(c.now(), 0u);
    // Queued from inside a running job? no: queued now, clock is 0.
    c.post_after(j, 2, 3);  // ready 3
    r.step();
    CHECK_EQ(c.now(), 3u);
    CHECK_EQ(c.value_of(j).value_or(-1), 3);
    CHECK_TRACE(r, "A1@0;A1@3;S1=D");
}

static void test_delay_measured_at_queue_time() {
    section("clock/inner");
    // A ticket queued from inside a resumed job is never ready before the step
    // that resumed it.
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(one_park());
    r.launch_done();
    c.post_after(j, 4, 10);  // ready 10
    r.step();                // clock -> 10, job delivers and settles
    CHECK_EQ(c.now(), 10u);
    CHECK_TRACE(r, "A1@10;S1=D");
}

static void test_late_ticket_same_frame() {
    section("bind/repark");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(two_parks());
    r.launch_done();
    c.post_after(j, 5, 10);
    c.post(j, 1);
    r.step();
    r.step();
    CHECK_EQ(c.value_of(j).value_or(-1), 6);
    CHECK_TRACE(r, "A1@0;A1@10;S1=D");
}

static void test_reject_job_settled() {
    section("bind/settled");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(one_park());
    r.launch_done();
    c.post(j, 1);
    c.post_after(j, 2, 4);
    r.drain();
    CHECK_EQ(c.value_of(j).value_or(-1), 1);
    CHECK(c.settled(j));
    CHECK_TRACE(r, "A1@0;S1=D;R1@4");
    CHECK_EQ(c.now(), 4u);
}

static void test_reject_slot_recycled() {
    section("bind/recycled");
    Conductor c{1};
    Runner r{c};
    const JobId first = c.launch(one_park());
    r.launch_done();
    c.post(first, 11);
    c.cancel(first);
    r.flush();
    CHECK(c.settled(first));
    CHECK_EQ(c.outcome_of(first).value_or(Outcome::Delivered), Outcome::Cancelled);
    CHECK_EQ(c.arena().live_slots(), 0u);
    const JobId second = c.launch(one_park());
    r.launch_done();
    CHECK(second != first);
    c.post(second, 3);
    r.drain();
    CHECK_EQ(c.value_of(second).value_or(-1), 3);
    CHECK_EQ(c.arena().double_reclaims(), 0u);
    // first's ticket is refused: job 1 has settled
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, first), 1u);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, first), 1u);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, second), 1u);
}

static void test_reject_epoch_mismatch() {
    section("bind/epoch");
    // Ticket bound to the first child's slot; the second child reuses the same
    // slot index with a new epoch.
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(two_children());
    r.launch_done();
    CHECK_EQ(c.arena().live_slots(), 2u);  // root + child
    c.post_after(j, 2, 5);                 // bound to first child slot
    c.post(j, 1);                          // bound to first child slot
    r.step();                              // child one returns 1, next child parks
    CHECK_EQ(c.arena().live_slots(), 2u);
    r.step();  // stale ticket: same index, new epoch
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, j), 1u);
    CHECK_TRACE(r, "A1@0;C1;P1:1;R1@5");
    CHECK_EQ(c.now(), 5u);
}

static void test_reject_index_mismatch() {
    section("bind/index");
    // Ticket queued while parked in the child; job then parks in the parent.
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(parent_then_park());
    r.launch_done();
    c.post_after(j, 99, 5);  // bound to the child frame
    c.post(j, 7);            // bound to the child frame
    r.step();                // child delivers, parent parks
    CHECK_EQ(c.journal().count_for(Mark::JobParked, j), 2u);
    r.step();  // stale ticket: parent frame now
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, j), 1u);
    CHECK_TRACE(r, "A1@0;C1;P1:1;R1@5");
}

static void test_reject_never_parked() {
    section("bind/unbound");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(one_park());
    r.launch_done();
    c.cancel(j);
    r.flush();
    CHECK(c.settled(j));
    c.post(j, 5);  // queued while the job is not parked at all
    r.drain();
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, j), 1u);
    CHECK_EQ(c.outcome_of(j).value_or(Outcome::Delivered), Outcome::Cancelled);
}

static void test_cancel_unknown_and_settled() {
    section("cancel/total");
    Conductor c{4};
    const JobId j = c.launch(one_park());
    c.cancel(j);
    c.cancel(j);
    c.cancel(4242);
    c.cancel(kNoJob);
    CHECK_EQ(c.journal().count(Mark::CancelRequested), 4u);
    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, j), 2u);
    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, 4242), 1u);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, j), 1u);
    CHECK_EQ(c.outcome_of(j).value_or(Outcome::Delivered), Outcome::Cancelled);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

static void test_cancel_runs_cleanup_nested() {
    section("cancel/cleanup");
    g_log.clear();
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(traced_root("root"));
    r.launch_done();
    CHECK_EQ(c.arena().live_slots(), 3u);
    c.cancel(j);
    r.flush();
    CHECK_EQ(c.outcome_of(j).value_or(Outcome::Delivered), Outcome::Cancelled);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(g_log.size(), 3u);
    if (g_log.size() == 3) {
        // innermost first
        CHECK(g_log[0] == "root/mid/inner");
        CHECK(g_log[1] == "root/mid");
        CHECK(g_log[2] == "root");
    }
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, j), 1u);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, j), 1u);
    CHECK_EQ(c.journal().count(Mark::FaultReported), 0u);
}

static void test_nested_child_destroy_order() {
    section("child/nested");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(top_parent());
    r.launch_done();
    CHECK_EQ(c.arena().live_slots(), 3u);
    c.post(j, 3);
    r.drain();
    CHECK_EQ(c.value_of(j).value_or(-1), 3);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_TRACE(r, "A1@0;C1;P1:2;C1;P1:1;S1=D");
}

static void test_child_fault_reaches_parent() {
    section("child/fault");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(parent_lets_child_throw());
    r.launch_done();
    c.post(j, 0);
    r.drain();
    CHECK_EQ(c.outcome_of(j).value_or(Outcome::Delivered), Outcome::Faulted);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_TRACE(r, "A1@0;C1;S1=F");
}

static void test_child_fault_caught() {
    section("child/caught");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(parent_catches_child());
    r.launch_done();
    c.post(j, 0);
    r.drain();
    CHECK_EQ(c.value_of(j).value_or(-1), 5);
    CHECK_EQ(c.journal().count(Mark::FaultReported), 0u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

static void test_job_throws() {
    section("fault/throw");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(thrower());
    r.launch_done();
    c.post(j, 1);
    r.drain();
    CHECK_EQ(c.outcome_of(j).value_or(Outcome::Delivered), Outcome::Faulted);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, j), 1u);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_TRACE(r, "A1@0;S1=F");
}

static void test_posted_fault_uncaught() {
    section("fault/posted");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(one_park());
    r.launch_done();
    c.post_fault(j);
    r.drain();
    CHECK_EQ(c.outcome_of(j).value_or(Outcome::Delivered), Outcome::Faulted);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.value_of(j).has_value(), false);
}

static void test_posted_fault_caught() {
    section("fault/caught");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(catcher());
    r.launch_done();
    c.post_fault(j);
    r.drain();
    CHECK_EQ(c.value_of(j).value_or(0), -1);
    CHECK_EQ(c.outcome_of(j).value_or(Outcome::Faulted), Outcome::Delivered);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

static void test_posted_fault_after() {
    section("fault/delayed");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(one_park());
    r.launch_done();
    c.post_fault_after(j, 8);
    r.step();
    CHECK_EQ(c.now(), 8u);
    CHECK_EQ(c.outcome_of(j).value_or(Outcome::Delivered), Outcome::Faulted);
}

static void test_cancel_from_inside_before_park() {
    section("cancel/self");
    g_c = nullptr;
    g_self = kNoJob;
    Conductor c{4};
    g_c = &c;
    // The body cannot know its own id before it runs, so launch first with a
    // placeholder, then let a second run use it.
    const JobId j = c.launch(one_park());
    g_self = j;
    const JobId k = c.launch(self_canceller());
    CHECK_EQ(c.settled(k), true);
    CHECK_EQ(c.outcome_of(k).value_or(Outcome::Delivered), Outcome::Cancelled);
    CHECK_EQ(c.arena().live_slots(), 1u);  // job j still parked
    CHECK(c.settled(j) == false);
}

static void test_cancel_from_inside_after_delivery() {
    section("cancel/self-late");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(one_park());
    g_self = j;
    const JobId k = c.launch(late_self_canceller());
    CHECK(!c.settled(k));
    c.post(k, 4);
    c.drain();
    CHECK(c.settled(k));
    CHECK_EQ(c.outcome_of(k).value_or(Outcome::Delivered), Outcome::Cancelled);
    CHECK_EQ(c.value_of(k).has_value(), false);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, k), 1u);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, k), 1u);
}

static void test_cancel_other_from_inside() {
    section("cancel/other");
    Conductor c{4};
    g_c = &c;
    const JobId victim = c.launch(one_park());
    const JobId other = c.launch(cancel_other(victim));
    CHECK(c.settled(victim));
    CHECK_EQ(c.outcome_of(victim).value_or(Outcome::Delivered), Outcome::Cancelled);
    CHECK(!c.settled(other));
    CHECK_EQ(c.arena().live_slots(), 1u);
    c.post(other, 2);
    c.drain();
    CHECK_EQ(c.value_of(other).value_or(-1), 2);
}

static void test_jobs_independent() {
    section("independence");
    Conductor c{4};
    Runner r{c};
    const JobId a = c.launch(one_park());
    const JobId b = c.launch(one_park());
    const JobId d = c.launch(one_park());
    r.launch_done();
    c.cancel(b);
    r.flush();
    c.post_fault(a);
    c.post(d, 9);
    r.drain();
    CHECK_EQ(c.outcome_of(a).value_or(Outcome::Delivered), Outcome::Faulted);
    CHECK_EQ(c.outcome_of(b).value_or(Outcome::Delivered), Outcome::Cancelled);
    CHECK_EQ(c.value_of(d).value_or(-1), 9);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.live_jobs(), 0u);
}

static void test_pool_exhaustion() {
    section("pool/exhaust");
    Conductor c{2};
    const JobId a = c.launch(one_park());
    const JobId b = c.launch(one_park());
    CHECK(a != kNoJob && b != kNoJob);
    CHECK_EQ(c.arena().live_slots(), 2u);
    bool threw = false;
    try {
        c.launch(one_park());
    } catch (const std::bad_alloc&) {
        threw = true;
    }
    CHECK(threw);
    CHECK_EQ(c.arena().live_slots(), 2u);
    CHECK_EQ(c.arena().peak_live(), 2u);
}

static void test_pool_reuse_bounded() {
    section("pool/reuse");
    Conductor c{2};
    Runner r{c};
    for (int i = 0; i < 6; ++i) {
        const JobId j = c.launch(one_park());
        r.launch_done();
        c.post(j, i);
        r.drain();
        CHECK_EQ(c.value_of(j).value_or(-99), i);
        CHECK_EQ(c.arena().live_slots(), 0u);
    }
    CHECK_EQ(c.arena().peak_live(), 1u);
    CHECK_EQ(c.arena().double_reclaims(), 0u);
    CHECK(c.arena().total_check_outs() >= 6u);
}

static void test_shutdown_releases() {
    section("shutdown");
    {
        Conductor c{4};
        c.launch(one_park());
        c.launch(traced_root("x"));
        c.launch(two_parks());
        CHECK_EQ(c.arena().live_slots(), 5u);
    }
    // No way to inspect the arena after destruction; rely on asan/leak checks.
}

static void test_settled_job_untouchable() {
    section("settled/stays");
    Conductor c{4};
    Runner r{c};
    const JobId j = c.launch(one_park());
    r.launch_done();
    c.post(j, 3);
    r.drain();
    const std::size_t settled_marks = c.journal().count_for(Mark::JobSettled, j);
    c.cancel(j);
    c.post(j, 100);
    c.post_fault(j);
    r.drain();
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, j), settled_marks);
    CHECK_EQ(c.value_of(j).value_or(-1), 3);
    CHECK_EQ(c.outcome_of(j).value_or(Outcome::Cancelled), Outcome::Delivered);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

static void test_no_self_drain() {
    section("schedule/step");
    Conductor c{4};
    const JobId a = c.launch(one_park());
    const JobId b = c.launch(one_park());
    c.post(b, 1);
    c.post(a, 2);
    CHECK_EQ(c.pending_tickets(), 2u);
    CHECK(c.step());
    CHECK(c.settled(a));
    CHECK(!c.settled(b));  // one delivery per step
    CHECK_EQ(c.pending_tickets(), 1u);
    CHECK(c.step());
    CHECK_EQ(c.pending_tickets(), 0u);
    CHECK(!c.step());
}

int main() {
    test_order_by_job();
    test_order_by_seq_same_job();
    test_order_by_ready_time();
    test_clock_from_queue_time();
    test_delay_measured_at_queue_time();
    test_late_ticket_same_frame();
    test_reject_job_settled();
    test_reject_slot_recycled();
    test_reject_epoch_mismatch();
    test_reject_index_mismatch();
    test_reject_never_parked();
    test_cancel_unknown_and_settled();
    test_cancel_runs_cleanup_nested();
    test_nested_child_destroy_order();
    test_child_fault_reaches_parent();
    test_child_fault_caught();
    test_job_throws();
    test_posted_fault_uncaught();
    test_posted_fault_caught();
    test_posted_fault_after();
    test_cancel_from_inside_before_park();
    test_cancel_from_inside_after_delivery();
    test_cancel_other_from_inside();
    test_jobs_independent();
    test_pool_exhaustion();
    test_pool_reuse_bounded();
    test_shutdown_releases();
    test_settled_job_untouchable();
    test_no_self_drain();

    if (g_failures == 0) {
        std::printf("all checks passed\n");
        return 0;
    }
    std::printf("%d failure(s)\n", g_failures);
    return 1;
}
