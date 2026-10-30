// Scratch tests, round two (not part of /app).
#include "harness.hpp"

#include <functional>

namespace {

Duty waiter() { co_return co_await park(); }

Duty child_duty() { co_return co_await park(); }

Duty parent_duty() {
    const std::int64_t v = co_await child_duty();
    co_return v * 2;
}

Duty catching_parent() {
    try {
        co_await child_duty();
    } catch (const std::exception&) {
        co_return -7;
    }
    co_return 1;
}

Duty throwing_child() {
    co_await park();
    throw std::runtime_error("child boom");
}

int g_cleanup = 0;
struct Tracked {
    Tracked() { ++g_cleanup; }
    ~Tracked() { --g_cleanup; }
    Tracked(const Tracked&) = delete;
    Tracked& operator=(const Tracked&) = delete;
};

Duty child_with_cleanup() {
    Tracked t;
    co_return co_await park();
}

Duty parent_of_cleanup() {
    const std::int64_t v = co_await child_with_cleanup();
    co_return v + 1;
}

Conductor* g_c = nullptr;
JobId g_self = kNoJob;
JobId g_victim = kNoJob;
std::vector<JobId> g_launched;

// Schedules from inside a running job: launches another job, posts to it, and
// drives the conductor itself.
Duty scheduler_job() {
    const std::int64_t v = co_await park();
    const JobId other = g_c->launch(waiter());
    g_launched.push_back(other);
    g_c->post(other, 77);
    g_c->drain();  // nested driving from inside a job
    co_return v + static_cast<std::int64_t>(other) * 0;
}

// Cancels another, parked job from inside its own run.
Duty canceller_job() {
    const std::int64_t v = co_await park();
    g_c->cancel(g_victim);
    co_return v;
}

// Cancels itself, then swings through another Duty before waiting again.
Duty self_cancel_then_child() {
    co_await park();
    g_c->cancel(g_self);
    const std::int64_t v = co_await child_duty();
    co_return v;
}

// Asks to stop while running, then catches the unwind and carries on.
Duty self_cancel_catcher() {
    const std::int64_t a = co_await park();
    g_c->cancel(g_self);
    try {
        co_await park();
    } catch (const Abandoned&) {
        co_return a + 1000;
    }
    co_return a;
}

Duty park_forever() {
    for (;;) {
        co_await park();
    }
}

// ------------------------------------------------- scheduling from inside

void test_schedule_from_inside() {
    Conductor c{8};
    g_c = &c;
    g_launched.clear();
    const JobId j = c.launch(scheduler_job());
    c.post(j, 5);
    c.drain();
    check(c.outcome_of(j) == Outcome::Delivered, "scheduler job delivered");
    check_eq(static_cast<long long>(g_launched.size()), 1, "one job launched inside");
    if (!g_launched.empty()) {
        check(c.outcome_of(g_launched[0]) == Outcome::Delivered, "inner job delivered");
        check_eq(static_cast<long long>(c.value_of(g_launched[0]).value_or(-1)), 77,
                 "inner job value");
    }
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "all storage back");
    check_eq(static_cast<long long>(c.arena().double_reclaims()), 0, "no double reclaims");
}

void test_cancel_other_from_inside() {
    Conductor c{8};
    g_c = &c;
    g_victim = c.launch(waiter());
    const JobId j = c.launch(canceller_job());
    c.post(j, 3);
    c.drain();
    check(c.outcome_of(g_victim) == Outcome::Cancelled, "victim cancelled from inside a job");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "both frames gone");
    check(c.outcome_of(j) == Outcome::Delivered, "canceller delivered");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::JobSettled, g_victim)), 1,
             "victim settled once");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::JobSettled, j)), 1,
             "canceller settled once");
}

void test_self_cancel_then_child() {
    Conductor c{8};
    g_c = &c;
    g_self = c.launch(self_cancel_then_child());
    c.post(g_self, 1);
    c.drain();
    check(c.outcome_of(g_self) == Outcome::Cancelled, "cancellation reaches the new child");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "all frames gone");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::ChildDestroyed)), 1,
             "child destroyed");
}

void test_self_cancel_caught() {
    Conductor c{8};
    g_c = &c;
    g_self = c.launch(self_cancel_catcher());
    c.post(g_self, 9);
    c.drain();
    check(c.outcome_of(g_self) == Outcome::Delivered, "a job that catches it carries on");
    check_eq(static_cast<long long>(c.value_of(g_self).value_or(-1)), 1009, "carried on value");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "storage back");
}

// ----------------------------------------------------------- child faults

void test_child_fault_caught() {
    Conductor c{8};
    Trace tr{c};
    const JobId j = c.launch(catching_parent());
    c.post_fault(j);
    drain_traced(c, tr);
    check(c.outcome_of(j) == Outcome::Delivered, "parent caught the child failure");
    check_eq(static_cast<long long>(c.value_of(j).value_or(-1)), -7, "caught value");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "child frame gone");
    // The child is destroyed before the parent observes the failure.
    check(tr.text() == "A1@0;C1;S1=D", "caught child fault trace: " + tr.text());
}

void test_throwing_child() {
    Conductor c{8};
    Trace tr{c};
    const JobId j = c.launch(parent_duty());
    c.post(j, 21);
    drain_traced(c, tr);
    check_eq(static_cast<long long>(c.value_of(j).value_or(-1)), 42, "child value doubled");
    check(tr.text() == "A1@0;C1;P1:1;S1=D", "parent trace: " + tr.text());
}

// --------------------------------------------------------------- teardown

void test_teardown_with_parked_child() {
    g_cleanup = 0;
    {
        Conductor c{8};
        const JobId j = c.launch(parent_of_cleanup());
        check_eq(g_cleanup, 1, "child frame holds its object");
        check(!c.settled(j), "job parked in its child");
        check_eq(static_cast<long long>(c.arena().live_slots()), 2, "two frames live");
        (void)j;
    }
    // Destroying the conductor must not leave a frame alive: the arena deletes
    // only the blocks that came back, so a leftover shows up as a leak.
    check_eq(g_cleanup, 0, "cleanup ran at teardown");
}

void test_teardown_with_parked_job() {
    g_cleanup = 0;
    {
        Conductor c{4};
        c.launch(park_forever());
        check_eq(static_cast<long long>(c.arena().live_slots()), 1, "frame live");
    }
    check_eq(g_cleanup, 0, "no leftovers");
}

// ----------------------------------------------------------- arena limits

void test_child_allocation_failure() {
    // One slot for the root, none left for the child.
    Conductor c{1};
    const JobId j = c.launch(parent_duty());
    check(c.settled(j), "the parent cannot get a frame for its child");
    check(c.outcome_of(j) == Outcome::Faulted, "reported as a failure");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::FaultReported, j)), 1,
             "one failure announcement");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "storage back");
    check_eq(static_cast<long long>(c.arena().peak_live()), 1, "never over capacity");
}

void test_peak_within_capacity() {
    Conductor c{8};
    std::vector<JobId> jobs;
    for (int i = 0; i < 4; ++i) {
        jobs.push_back(c.launch(parent_duty()));
    }
    check_eq(static_cast<long long>(c.arena().live_slots()), 8, "eight frames live");
    check(c.arena().peak_live() <= c.arena().capacity(), "peak within capacity");
    for (JobId j : jobs) {
        c.post(j, 2);
    }
    c.drain();
    for (JobId j : jobs) {
        check_eq(static_cast<long long>(c.value_of(j).value_or(-1)), 4, "value doubled");
    }
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "all storage back");
    check_eq(static_cast<long long>(c.arena().total_check_outs()), 8, "no more than needed");
    check_eq(static_cast<long long>(c.arena().double_reclaims()), 0, "no double reclaims");
}

// --------------------------------------------------------------- duty misc

void test_unawaited_duty() {
    Conductor c{4};
    {
        Duty d = child_duty();  // made, never awaited
        check(static_cast<bool>(d), "frame taken");
        check_eq(static_cast<long long>(c.arena().live_slots()), 1, "one frame live");
    }
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "frame given back");
    check_eq(static_cast<long long>(c.arena().double_reclaims()), 0, "no double reclaims");
}

void test_nested_conductors() {
    Conductor outer{4};
    const JobId a = outer.launch(waiter());
    {
        Conductor inner{2};
        const JobId b = inner.launch(waiter());
        check_eq(static_cast<long long>(inner.arena().live_slots()), 1, "inner arena used");
        inner.post(b, 3);
        inner.drain();
        check_eq(static_cast<long long>(inner.value_of(b).value_or(-1)), 3, "inner job ran");
        check_eq(static_cast<long long>(inner.arena().live_slots()), 0, "inner storage back");
    }
    check(Conductor::active_arena() == &outer.arena(), "outer arena restored");
    outer.post(a, 4);
    outer.drain();
    check_eq(static_cast<long long>(outer.value_of(a).value_or(-1)), 4, "outer job ran");
}

Duty two_children() {
    const std::int64_t a = co_await child_duty();
    const std::int64_t b = co_await child_duty();
    co_return a * 10 + b;
}

// A ticket that named the previous child must not land on the next one, even
// though the next child took the same slot: the epoch moved on.
void test_ticket_for_previous_child() {
    Conductor c{2};
    Trace tr{c};
    const JobId j = c.launch(two_children());
    check_eq(static_cast<long long>(c.arena().live_slots()), 2, "root and first child live");
    c.post(j, 1);
    c.post(j, 2);
    drain_traced(c, tr);
    check(!c.settled(j), "second child is still waiting");
    check(tr.text() == "A1@0;C1;P1:1;R1@0", "previous child trace: " + tr.text());
    c.post(j, 2);  // now bound to the child that is actually waiting
    drain_traced(c, tr);
    check_eq(static_cast<long long>(c.value_of(j).value_or(-1)), 12, "value 12");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "all storage back");
    check_eq(static_cast<long long>(c.arena().double_reclaims()), 0, "no double reclaims");
}

void test_fault_after_delay() {
    Conductor c{4};
    Trace tr{c};
    const JobId j = c.launch(waiter());
    const JobId k = c.launch(waiter());
    c.post_fault_after(j, 4);
    c.post(k, 1);
    c.step();
    tr.fold(c.now());
    check(c.settled(k), "the earlier ticket went first");
    check(!c.settled(j), "the fault is not ready yet");
    check_eq(static_cast<long long>(c.now()), 0, "clock still 0");
    c.step();
    tr.fold(c.now());
    check(c.outcome_of(j) == Outcome::Faulted, "fault delivered at its ready time");
    check_eq(static_cast<long long>(c.now()), 4, "clock moved to the fault's ready time");
    check(tr.text() == "A2@0;S2=D;A1@4;S1=F", "fault-after trace: " + tr.text());
    check_eq(static_cast<long long>(c.journal().count(Mark::FaultReported)), 1,
             "one failure announcement");
}

}  // namespace

int main() {
    test_ticket_for_previous_child();
    test_fault_after_delay();
    test_schedule_from_inside();
    test_cancel_other_from_inside();
    test_self_cancel_then_child();
    test_self_cancel_caught();
    test_child_fault_caught();
    test_throwing_child();
    test_teardown_with_parked_child();
    test_teardown_with_parked_job();
    test_child_allocation_failure();
    test_peak_within_capacity();
    test_unawaited_duty();
    test_nested_conductors();
    std::printf("%d checks, %d failures\n", g_checks, g_fail);
    return g_fail == 0 ? 0 : 1;
}
