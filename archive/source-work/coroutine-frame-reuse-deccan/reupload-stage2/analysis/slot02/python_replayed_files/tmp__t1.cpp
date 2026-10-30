// Scratch test harness. Not part of the deliverable.
#include <cstdio>
#include <exception>
#include <string>
#include <vector>

#include "roundel/runtime.hpp"

using namespace roundel;

static int failures = 0;
static int checks = 0;

static void check(bool ok, const char* what) {
    ++checks;
    if (!ok) {
        ++failures;
        std::printf("  FAIL: %s\n", what);
    }
}

static void eq(long long got, long long want, const char* what) {
    ++checks;
    if (got != want) {
        ++failures;
        std::printf("  FAIL: %s (got %lld want %lld)\n", what, got, want);
    }
}

// The trace, accumulated incrementally: <now> on A/R marks is now() read after
// the step that produced them, which the journal itself does not carry.
static std::string g_trace;
static std::size_t g_seen = 0;

static void observe(const Conductor& c) {
    const auto& es = c.journal().entries();
    for (std::size_t i = g_seen; i < es.size(); ++i) {
        const Entry& e = es[i];
        switch (e.mark) {
            case Mark::TicketAccepted:
                g_trace += "A" + std::to_string(e.job) + "@" + std::to_string(c.now());
                break;
            case Mark::TicketRejected:
                g_trace += "R" + std::to_string(e.job) + "@" + std::to_string(c.now());
                break;
            case Mark::ChildDestroyed:
                g_trace += "C" + std::to_string(e.job);
                break;
            case Mark::ParentContinued:
                g_trace += "P" + std::to_string(e.job) + ":" + std::to_string(e.detail);
                break;
            case Mark::JobSettled:
                g_trace += "S" + std::to_string(e.job) + "=";
                g_trace += (e.detail == 0 ? 'D' : (e.detail == 1 ? 'X' : 'F'));
                break;
            default:
                continue;
        }
        g_trace += ";";
    }
    g_seen = es.size();
}

static std::string trace(const Conductor& c) {
    observe(c);
    std::string out = g_trace;
    if (!out.empty()) out.pop_back();
    g_trace.clear();
    g_seen = 0;
    return out;
}

// Drain with per-step observation so each A/R carries its own clock reading.
static std::size_t observe_drain(Conductor& c) {
    std::size_t n = 0;
    while (c.step()) {
        observe(c);
        ++n;
    }
    observe(c);
    return n;
}

// -------------------------------------------------------------------------

static Duty simple() { co_return co_await park(); }
static Duty child() { co_return co_await park(); }
static Duty parent() {
    const std::int64_t v = co_await child();
    co_return v * 2;
}
static Duty grandparent() {
    const std::int64_t v = co_await parent();
    co_return v + 1;
}
static Duty trivial() { co_return 7; }

static Duty two_children() {
    std::int64_t total = 0;
    for (int i = 0; i < 2; ++i) total += co_await child();
    co_return total;
}

static Duty park_then_child() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await child();
    co_return a + b;
}

static Duty catcher() {
    try {
        const std::int64_t v = co_await child();
        co_return v;
    } catch (const std::exception&) {
        co_return -1;
    }
}

static Duty fault_catcher() {
    try {
        const std::int64_t v = co_await park();
        co_return v;
    } catch (const std::exception&) {
        co_return -1;
    }
}

static Duty thrower() {
    const std::int64_t v = co_await park();
    if (v == 0) throw std::runtime_error("boom");
    co_return v;
}

// A frame holding something with a destructor: cleanup has to run.
static int cleanup_count = 0;
struct Guard {
    Guard() { ++cleanup_count; }
    ~Guard() { --cleanup_count; }
};
static Duty guarded() {
    Guard g;
    co_return co_await park();
}

// Job 1, cancelling itself before it waits.
static Duty self_canceller(Conductor* c) {
    c->cancel(1);
    const std::int64_t v = co_await park();
    co_return v;
}

// Job 1, cancelling itself and then returning without ever waiting.
static Duty self_canceller_returning(Conductor* c) {
    c->cancel(1);
    co_return 5;
}

// Job 1, queuing a completion for itself from inside a running frame: the spec
// says such a ticket authorises no frame and can never be delivered.
static Duty self_reposter(Conductor* c) {
    c->post_after(1, 99, 5);
    const std::int64_t v = co_await park();
    co_return v;
}

// Job 1, queuing a completion for job 2 from inside a running frame.
static Duty cross_poster(Conductor* c) {
    const std::int64_t v = co_await park();
    c->post(2, v + 1);
    co_return v;
}

// -------------------------------------------------------------------------

static void t_basic() {
    std::printf("[basic]\n");
    Conductor c{4};
    const JobId j = c.launch(simple());
    eq((long long)j, 1, "first id is 1");
    check(!c.settled(j), "running after launch");
    eq((long long)c.arena().live_slots(), 1, "one frame live");
    c.post(j, 5);
    eq((long long)c.now(), 0, "clock starts at 0");
    eq((long long)c.pending_tickets(), 1, "one ticket");
    check(c.step(), "step delivered");
    check(c.settled(j), "settled");
    check(c.outcome_of(j) == Outcome::Delivered, "delivered");
    eq(c.value_of(j).value_or(-1), 5, "value");
    eq((long long)c.arena().live_slots(), 0, "storage back at once");
    eq((long long)c.journal().count_for(Mark::JobSettled, j), 1, "settled once");
    eq((long long)c.journal().count_for(Mark::FrameReleased, j), 1, "released once");
    eq((long long)c.arena().double_reclaims(), 0, "no double reclaim");
}

static void t_trace_basic() {
    std::printf("[trace basic]\n");
    Conductor c{4};
    const JobId j = c.launch(simple());
    c.post(j, 5);
    c.drain();
    const std::string t = trace(c.journal());
    std::printf("  trace=%s\n", t.c_str());
    check(t == "A1@0;S1=D", "simple trace");
}

static void t_cancel_parked() {
    std::printf("[cancel parked]\n");
    Conductor c{4};
    const JobId j = c.launch(simple());
    c.post(j, 5);
    c.cancel(j);
    check(c.settled(j), "settled by cancel");
    check(c.outcome_of(j) == Outcome::Cancelled, "cancelled");
    check(!c.value_of(j).has_value(), "no value");
    eq((long long)c.arena().live_slots(), 0, "storage released immediately");
    check(c.drain() >= 1, "drain processed the stale ticket");
    eq((long long)c.journal().count_for(Mark::TicketRejected, j), 1, "ticket rejected");
    eq((long long)c.journal().count_for(Mark::JobSettled, j), 1, "settled once");
    eq((long long)c.journal().count_for(Mark::FaultReported, j), 0, "no fault report");
    std::printf("  trace=%s\n", trace(c.journal()).c_str());
}

static void t_cancel_unknown_and_twice() {
    std::printf("[cancel unknown / twice / after settle]\n");
    Conductor c{4};
    c.cancel(999);  // never launched
    eq((long long)c.journal().count_for(Mark::CancelRequested, 999), 1, "request recorded");
    const JobId j = c.launch(simple());
    c.cancel(j);
    c.cancel(j);
    c.cancel(j);
    eq((long long)c.journal().count_for(Mark::CancelRequested, j), 3, "three requests");
    eq((long long)c.journal().count_for(Mark::JobSettled, j), 1, "one settle");
    check(c.outcome_of(j) == Outcome::Cancelled, "still cancelled");
    c.cancel(j);  // after settle
    eq((long long)c.journal().count_for(Mark::CancelRequested, j), 4, "request after settle");
    check(c.outcome_of(j) == Outcome::Cancelled, "unchanged");
    c.post(j, 3);
    c.drain();
    eq((long long)c.journal().count_for(Mark::TicketRejected, j), 1, "late ticket refused");
}

static void t_child_value() {
    std::printf("[child value]\n");
    Conductor c{4};
    const JobId j = c.launch(parent());
    eq((long long)c.arena().live_slots(), 2, "two frames live");
    c.post(j, 21);
    c.drain();
    check(c.value_of(j) == 42, "value 42");
    eq((long long)c.arena().live_slots(), 0, "all storage back");
    const std::string t = trace(c.journal());
    std::printf("  trace=%s\n", t.c_str());
    check(t == "A1@0;C1;P1:1;S1=D", "child trace");
    check(c.journal().first_index(Mark::ChildDestroyed, j) <
              c.journal().first_index(Mark::ParentContinued, j),
          "child destroyed before parent continued");
}

static void t_child_fault() {
    std::printf("[child fault]\n");
    Conductor c{4};
    const JobId j = c.launch(parent());
    c.post_fault(j);
    c.drain();
    check(c.outcome_of(j) == Outcome::Faulted, "faulted");
    eq((long long)c.journal().count_for(Mark::FaultReported, j), 1, "fault reported once");
    eq((long long)c.arena().live_slots(), 0, "all storage back");
    std::printf("  trace=%s\n", trace(c.journal()).c_str());
}

static void t_child_fault_caught() {
    std::printf("[child fault caught]\n");
    Conductor c{4};
    const JobId j = c.launch(catcher());
    c.post_fault(j);
    c.drain();
    check(c.outcome_of(j) == Outcome::Delivered, "delivered after catch");
    eq(c.value_of(j).value_or(0), -1, "caught value");
    eq((long long)c.arena().live_slots(), 0, "storage back");
}

static void t_fault_caught_at_park() {
    std::printf("[fault caught at park]\n");
    Conductor c{4};
    const JobId j = c.launch(fault_catcher());
    c.post_fault(j);
    c.drain();
    check(c.outcome_of(j) == Outcome::Delivered, "delivered after catch");
    eq(c.value_of(j).value_or(0), -1, "caught value");
    eq((long long)c.arena().live_slots(), 0, "storage back");
}

static void t_throw_in_body() {
    std::printf("[throw in body]\n");
    Conductor c{4};
    const JobId j = c.launch(thrower());
    c.post(j, 0);
    c.drain();
    check(c.outcome_of(j) == Outcome::Faulted, "faulted");
    eq((long long)c.journal().count_for(Mark::FaultReported, j), 1, "reported once");
    eq((long long)c.arena().live_slots(), 0, "storage back");
}

static void t_three_levels() {
    std::printf("[three levels]\n");
    Conductor c{4};
    const JobId j = c.launch(grandparent());
    eq((long long)c.arena().live_slots(), 3, "three frames live");
    c.post(j, 10);
    c.drain();
    eq(c.value_of(j).value_or(0), 21, "(10*2)+1");
    eq((long long)c.arena().live_slots(), 0, "storage back");
    std::printf("  trace=%s\n", trace(c.journal()).c_str());
    check(trace(c.journal()) == "A1@0;C1;P1:2;C1;P1:1;S1=D", "nested trace");
}

static void t_three_levels_cancel() {
    std::printf("[three levels, cancelled deep]\n");
    Conductor c{4};
    const JobId j = c.launch(grandparent());
    eq((long long)c.arena().live_slots(), 3, "three frames live");
    c.cancel(j);
    check(c.outcome_of(j) == Outcome::Cancelled, "cancelled");
    eq((long long)c.arena().live_slots(), 0, "all three frames gone at once");
    eq((long long)c.journal().count_for(Mark::FaultReported, j), 0, "no fault for a cancel");
    std::printf("  trace=%s\n", trace(c.journal()).c_str());
}

static void t_cleanup_runs() {
    std::printf("[cleanup runs]\n");
    cleanup_count = 0;
    {
        Conductor c{4};
        const JobId j = c.launch(guarded());
        eq((long long)cleanup_count, 1, "guard alive while parked");
        c.cancel(j);
        eq((long long)cleanup_count, 0, "guard destroyed by cancel");
    }
    {
        Conductor c{4};
        const JobId j = c.launch(guarded());
        c.post(j, 3);
        c.drain();
        eq((long long)cleanup_count, 0, "guard destroyed after delivery");
    }
    eq((long long)cleanup_count, 0, "no guard leaked");
}

static void t_trivial() {
    std::printf("[job that never parks]\n");
    Conductor c{4};
    const JobId j = c.launch(trivial());
    check(c.settled(j), "settled during launch");
    eq(c.value_of(j).value_or(0), 7, "value 7");
    eq((long long)c.arena().live_slots(), 0, "storage back during launch");
    eq((long long)c.journal().count_for(Mark::FrameReleased, j), 1, "released");
}

static void t_ordering() {
    std::printf("[delivery order]\n");
    Conductor c{8};
    const JobId a = c.launch(simple());
    const JobId b = c.launch(simple());
    const JobId d = c.launch(simple());
    // Same ready time: lowest job id first.
    c.post(d, 1);
    c.post(b, 2);
    c.post(a, 3);
    std::vector<JobId> order;
    for (int i = 0; i < 3; ++i) {
        const std::size_t before = c.journal().entries().size();
        c.step();
        for (std::size_t k = before; k < c.journal().entries().size(); ++k) {
            if (c.journal().entries()[k].mark == Mark::TicketAccepted) {
                order.push_back(c.journal().entries()[k].job);
            }
        }
    }
    check(order.size() == 3 && order[0] == a && order[1] == b && order[2] == d,
          "lowest job id first at equal ready time");

    // Two tickets for the same job at the same ready time: queued order.
    Conductor c2{8};
    const JobId j = c2.launch(simple());
    // only one can be bound while parked once; queue two anyway
    c2.post_after(j, 100, 0);
    c2.post_after(j, 111, 0);
    c2.step();
    const auto& e = c2.journal().entries();
    long long first = -1;
    for (const auto& en : e) {
        if (en.mark == Mark::TicketAccepted) {
            first = en.detail;
            break;
        }
    }
    eq(first, 100, "earlier queued wins for one job");

    // Ready time dominates job id.
    Conductor c3{8};
    const JobId x = c3.launch(simple());
    const JobId y = c3.launch(simple());
    c3.post_after(x, 1, 10);
    c3.post_after(y, 2, 5);
    c3.step();
    const auto& e3 = c3.journal().entries();
    JobId served = 0;
    for (const auto& en : e3) {
        if (en.mark == Mark::TicketAccepted) served = en.job;
    }
    eq((long long)served, (long long)y, "lower ready time wins over job id");
    eq((long long)c3.now(), 5, "clock at ready time");
    c3.step();
    eq((long long)c3.now(), 10, "clock moved to next ready time");
}

static void t_clock() {
    std::printf("[clock]\n");
    // A ticket queued from inside a running job for itself authorises no frame
    // and is refused, however the job's state changes afterwards.
    {
        Conductor c{4};
        const JobId j = c.launch(self_reposter(&c));
        c.post(j, 1);
        eq((long long)c.pending_tickets(), 2, "two tickets queued");
        observe_drain(c);
        eq(c.value_of(j).value_or(0), 1, "first ticket delivered");
        eq((long long)c.journal().count_for(Mark::TicketRejected, j), 1, "self ticket refused");
        eq((long long)c.now(), 5, "clock followed the refused ticket's ready time");
    }
    // A ticket queued from inside a running job for another, parked job is
    // measured from the clock as it stands then.
    {
        Conductor c{4};
        const JobId a = c.launch(cross_poster(&c));
        const JobId b = c.launch(simple());
        c.post_after(a, 4, 3);
        c.step();
        eq((long long)c.now(), 3, "clock at 3");
        eq((long long)c.pending_tickets(), 1, "ticket queued from inside");
        c.step();
        eq(c.value_of(b).value_or(0), 5, "cross-posted value");
        eq(c.value_of(a).value_or(0), 4, "a delivered");
    }
}

static void t_reject_different_frame() {
    std::printf("[ticket naming another frame]\n");
    // Two tickets queued while parked in the root; the second arrives after the
    // job has moved into a child frame (same epoch, different index).
    Conductor c{4};
    const JobId j = c.launch(park_then_child());
    c.post(j, 10);
    c.post(j, 20);
    c.step();  // first accepted: root resumes and awaits a child, which parks
    c.step();  // second names the root frame; the job is in the child
    eq((long long)c.journal().count_for(Mark::TicketAccepted, j), 1, "one accepted");
    eq((long long)c.journal().count_for(Mark::TicketRejected, j), 1, "one rejected");
    c.cancel(j);
    eq((long long)c.arena().live_slots(), 0, "storage back");
}

static void t_reject_recycled_child() {
    std::printf("[ticket naming a previous child]\n");
    Conductor c{2};
    const JobId j = c.launch(two_children());
    c.post(j, 10);
    c.post(j, 20);
    eq((long long)c.arena().live_slots(), 2, "root and first child");
    c.step();  // first child completes and is destroyed
    c.step();  // second child parks in the recycled slot: epoch differs
    eq((long long)c.journal().count_for(Mark::TicketAccepted, j), 1, "one accepted");
    eq((long long)c.journal().count_for(Mark::TicketRejected, j), 1, "one rejected");
    eq((long long)c.arena().double_reclaims(), 0, "no double reclaim");
    c.cancel(j);
    eq((long long)c.arena().live_slots(), 0, "storage back");
}

static void t_repark_same_frame() {
    std::printf("[repark in the same frame]\n");
    Conductor c{4};
    const JobId j = c.launch(simple());
    c.post(j, 1);
    c.post(j, 2);
    c.step();  // accepted: job settles
    // Now check the "parks again in the very same frame" rule with two children
    Conductor c2{4};
    const JobId k = c2.launch(park_then_child());
    c2.post(k, 5);
    c2.step();
    // job is parked in the child now; a ticket bound to the child must arrive
    c2.post(k, 6);
    c2.step();
    eq((long long)c2.journal().count_for(Mark::TicketAccepted, k), 2, "both accepted");
    eq(c2.value_of(k).value_or(0), 11, "sum");
}

static void t_unbound_ticket() {
    std::printf("[ticket queued while not parked]\n");
    Conductor c{4};
    const JobId j = c.launch(trivial());  // settles at once
    c.post(j, 1);                         // queued with nobody parked
    c.drain();
    eq((long long)c.journal().count_for(Mark::TicketRejected, j), 1, "rejected");
    check(c.outcome_of(j) == Outcome::Delivered, "outcome untouched");
}

static void t_self_cancel() {
    std::printf("[cancel from inside]\n");
    {
        Conductor c{4};
        const JobId j = c.launch(self_canceller(&c));
        check(c.settled(j), "settled");
        check(c.outcome_of(j) == Outcome::Cancelled, "cancelled");
        eq((long long)c.arena().live_slots(), 0, "storage back");
        eq((long long)c.journal().count_for(Mark::JobSettled, j), 1, "settled once");
        eq((long long)c.journal().count_for(Mark::FaultReported, j), 0, "no fault");
    }
    {
        Conductor c{4};
        const JobId j = c.launch(self_canceller_returning(&c));
        check(c.settled(j), "settled");
        check(c.outcome_of(j) == Outcome::Cancelled, "cancelled even though it returned");
        check(!c.value_of(j).has_value(), "no value");
    }
}

static void t_independent() {
    std::printf("[jobs independent]\n");
    Conductor c{8};
    const JobId a = c.launch(simple());
    const JobId b = c.launch(simple());
    const JobId d = c.launch(parent());
    c.cancel(a);
    c.post_fault(b);
    c.post(d, 4);
    c.drain();
    check(c.outcome_of(a) == Outcome::Cancelled, "a cancelled");
    check(c.outcome_of(b) == Outcome::Faulted, "b faulted");
    eq(c.value_of(d).value_or(0), 8, "d delivered");
    eq((long long)c.arena().live_slots(), 0, "storage back");
    eq((long long)c.live_jobs(), 0, "no live jobs");
}

static void t_capacity() {
    std::printf("[capacity]\n");
    Conductor c{2};
    const JobId a = c.launch(parent());  // two frames
    bool threw = false;
    try {
        c.launch(simple());
    } catch (const std::bad_alloc&) {
        threw = true;
    }
    check(threw, "exhaustion reported");
    c.cancel(a);
    eq((long long)c.arena().live_slots(), 0, "storage back");
    const JobId b = c.launch(simple());  // now fits
    check(b != a, "new job");
    c.cancel(b);
    eq((long long)c.arena().peak_live(), 2, "peak within capacity");
    eq((long long)c.arena().live_slots(), 0, "empty at rest");
}

static void t_reuse() {
    std::printf("[slot reuse]\n");
    Conductor c{1};
    const JobId a = c.launch(simple());
    c.cancel(a);
    const JobId b = c.launch(simple());
    c.post(b, 9);
    c.drain();
    eq(c.value_of(b).value_or(0), 9, "reused slot works");
    eq((long long)c.arena().total_check_outs(), 2, "slot handed out twice");
    check(c.arena().total_check_outs() > c.arena().capacity(), "genuine recycling");
    eq((long long)c.arena().double_reclaims(), 0, "no double reclaim");
    eq((long long)c.arena().live_slots(), 0, "empty at rest");
}

static void t_pending_left_over() {
    std::printf("[jobs left parked at shutdown]\n");
    Conductor c{4};
    const JobId a = c.launch(simple());
    const JobId b = c.launch(parent());
    c.post(a, 1);  // never delivered
    (void)b;
    // destructor must return everything
}

int main() {
    t_basic();
    t_trace_basic();
    t_cancel_parked();
    t_cancel_unknown_and_twice();
    t_child_value();
    t_child_fault();
    t_child_fault_caught();
    t_fault_caught_at_park();
    t_throw_in_body();
    t_three_levels();
    t_three_levels_cancel();
    t_cleanup_runs();
    t_trivial();
    t_ordering();
    t_clock();
    t_reject_different_frame();
    t_reject_recycled_child();
    t_repark_same_frame();
    t_unbound_ticket();
    t_self_cancel();
    t_independent();
    t_capacity();
    t_reuse();
    t_pending_left_over();
    std::printf("\n%d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}
