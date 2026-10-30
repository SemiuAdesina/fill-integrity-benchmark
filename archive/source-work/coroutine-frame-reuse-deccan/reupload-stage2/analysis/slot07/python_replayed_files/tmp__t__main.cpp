// Scratch tests (not part of /app).
#include "harness.hpp"

#include <functional>
#include <memory>

namespace {

Duty waiter() { co_return co_await park(); }

Duty twice() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a * 10 + b;
}

Duty immediate() { co_return 5; }

Duty thrower() {
    co_await park();
    throw std::runtime_error("boom");
}

Duty catcher() {
    try {
        co_await park();
    } catch (const std::exception&) {
        co_return -1;
    }
    co_return 1;
}

Duty child_duty() { co_return co_await park(); }

Duty grandchild_duty() { co_return co_await park(); }

Duty middle_duty() {
    const std::int64_t v = co_await grandchild_duty();
    co_return v + 1;
}

Duty deep_duty() {
    const std::int64_t v = co_await middle_duty();
    co_return v * 2;
}

Duty parent_then_park() {
    const std::int64_t v = co_await child_duty();
    const std::int64_t w = co_await park();
    co_return v + w;
}

int g_cleanup = 0;

struct Tracked {
    Tracked() { ++g_cleanup; }
    ~Tracked() { --g_cleanup; }
    Tracked(const Tracked&) = delete;
    Tracked& operator=(const Tracked&) = delete;
};

Duty holder() {
    Tracked t;
    co_await park();
    co_return 1;
}

Conductor* g_c = nullptr;
JobId g_self = kNoJob;
std::vector<std::uint64_t> g_seen_now;

Conductor* g_c2 = nullptr;
JobId g_other = kNoJob;

// Resumes, schedules another (parked) job with a delay measured from the clock
// as it stands now, and also posts to itself while it is running -- a ticket
// for a job that is not parked is bound to no frame and can never arrive.
Duty reposter() {
    const std::int64_t v = co_await park();
    g_seen_now.push_back(g_c->now());
    g_c->post_after(g_self, 7, 3);
    g_c->post_after(g_other, 8, 3);
    co_return v;
}

Duty self_canceller() {
    const std::int64_t v = co_await park();
    g_c->cancel(g_self);
    co_return v;
}

Duty self_cancel_then_park() {
    co_await park();
    g_c->cancel(g_self);
    co_await park();
    co_return 99;
}

// ---------------------------------------------------------------- ordering

void test_order_ready_time() {
    Conductor c{8};
    Trace tr{c};
    const JobId j1 = c.launch(waiter());
    const JobId j2 = c.launch(waiter());
    const JobId j3 = c.launch(waiter());
    // Queued out of order on purpose.
    c.post_after(j3, 30, 5);
    c.post_after(j1, 10, 9);
    c.post_after(j2, 20, 5);
    drain_traced(c, tr);
    // ready 5: job 2 before job 3; then job 1 at 9.
    check_eq(static_cast<long long>(c.value_of(j2).value_or(-1)), 20, "order: job2 value");
    check_eq(static_cast<long long>(c.value_of(j3).value_or(-1)), 30, "order: job3 value");
    check_eq(static_cast<long long>(c.value_of(j1).value_or(-1)), 10, "order: job1 value");
    check(tr.text() == "A2@5;S2=D;A3@5;S3=D;A1@9;S1=D", "order trace: " + tr.text());
    check_eq(static_cast<long long>(c.now()), 9, "clock ends at last ready time");
}

void test_order_job_id_then_seq() {
    Conductor c{4};
    Trace tr{c};
    const JobId j1 = c.launch(twice());
    const JobId j2 = c.launch(waiter());
    c.post(j1, 1);  // seq 0
    c.post(j1, 2);  // seq 1
    c.post(j2, 5);  // same ready, higher job id
    drain_traced(c, tr);
    check_eq(static_cast<long long>(c.value_of(j1).value_or(-1)), 12, "seq order value");
    check_eq(static_cast<long long>(c.value_of(j2).value_or(-1)), 5, "other job value");
    check(tr.text() == "A1@0;A1@0;S1=D;A2@0;S2=D", "job-id/seq trace: " + tr.text());
    check_eq(static_cast<long long>(c.now()), 0, "clock stays 0");
}

void test_clock_and_repost() {
    Conductor c{4};
    Trace tr{c};
    g_c = &c;
    g_seen_now.clear();
    g_other = c.launch(waiter());
    g_self = c.launch(reposter());
    c.post_after(g_self, 4, 5);
    check_eq(static_cast<long long>(c.now()), 0, "clock before step");
    c.step();
    tr.fold(c.now());
    check_eq(static_cast<long long>(c.now()), 5, "clock moved to ready time");
    check_eq(static_cast<long long>(g_seen_now.size()), 1, "job resumed once");
    check_eq(static_cast<long long>(g_seen_now.empty() ? -1 : g_seen_now[0]), 5,
             "now inside resumed job");
    check_eq(static_cast<long long>(c.pending_tickets()), 2, "two tickets queued inside");
    check(c.settled(g_self), "job settled after returning");
    // The ticket for the other job is ready 3 after the clock as it stood when
    // it was queued, which is 5.
    c.step();
    tr.fold(c.now());
    check_eq(static_cast<long long>(c.now()), 8, "delay measured from current clock");
    check_eq(static_cast<long long>(c.value_of(g_other).value_or(-1)), 8, "other job value");
    // The ticket the job queued for itself while running was bound to no frame.
    c.step();
    tr.fold(c.now());
    check(c.settled(g_self), "self ticket never resumed anything");
    check(tr.text() == "A2@5;S2=D;A1@8;S1=D;R2@8", "repost trace: " + tr.text());
    check_eq(static_cast<long long>(c.now()), 8, "clock never moves backwards");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "storage back");
}

// ------------------------------------------------------------ cancellation

void test_cancel_parked() {
    Conductor c{4};
    Trace tr{c};
    const JobId j = c.launch(waiter());
    c.post(j, 3);
    c.cancel(j);
    tr.fold(c.now());
    check(c.settled(j), "cancel: settled at once");
    check(c.outcome_of(j) == Outcome::Cancelled, "cancel: outcome cancelled");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "cancel: storage released now");
    check_eq(static_cast<long long>(c.value_of(j).has_value() ? 1 : 0), 0, "cancel: no value");
    c.step();  // the ticket that was already queued
    tr.fold(c.now());
    check_eq(static_cast<long long>(c.journal().count_for(Mark::TicketAccepted, j)), 0,
             "cancel: no delivery after settle");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::JobSettled, j)), 1,
             "cancel: settled once");
    check(tr.text() == "S1=X;R1@0", "cancel trace: " + tr.text());
}

void test_cancel_idempotent_and_unknown() {
    Conductor c{4};
    const JobId j = c.launch(waiter());
    c.cancel(j);
    c.cancel(j);
    c.cancel(999);
    check_eq(static_cast<long long>(c.journal().count_for(Mark::CancelRequested, j)), 2,
             "two requests recorded for the job");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::CancelRequested, 999)), 1,
             "request recorded for an unknown job");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::JobSettled, j)), 1,
             "still one outcome");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::JobSettled, 999)), 0,
             "unknown job never settles");
    check(!c.settled(999), "unknown job not settled");
    check(c.outcome_of(999) == std::nullopt, "unknown job has no outcome");
    check_eq(static_cast<long long>(c.live_jobs()), 0, "no live jobs");
}

void test_cancel_already_settled() {
    Conductor c{4};
    const JobId j = c.launch(immediate());
    check(c.settled(j), "immediate job settled in launch");
    check(c.outcome_of(j) == Outcome::Delivered, "delivered");
    check_eq(static_cast<long long>(c.value_of(j).value_or(-1)), 5, "value 5");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "storage back at once");
    c.cancel(j);
    check(c.outcome_of(j) == Outcome::Delivered, "cancel after settle changes nothing");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::CancelRequested, j)), 1,
             "request recorded after settle");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::JobSettled, j)), 1,
             "still one outcome");
}

void test_cancel_runs_cleanup() {
    g_cleanup = 0;
    {
        Conductor c{4};
        const JobId j = c.launch(holder());
        check_eq(g_cleanup, 1, "frame holds its object");
        c.cancel(j);
        check_eq(g_cleanup, 0, "cleanup ran on cancel");
        check(c.outcome_of(j) == Outcome::Cancelled, "cancelled");
        check_eq(static_cast<long long>(c.arena().live_slots()), 0, "storage back");
    }
    check_eq(g_cleanup, 0, "no leftovers");
}

void test_cancel_from_inside() {
    Conductor c{4};
    Trace tr{c};
    g_c = &c;
    g_self = c.launch(self_canceller());
    c.post(g_self, 42);
    drain_traced(c, tr);
    // Cancel asked for while the job was running: it had already produced its
    // value, so it delivers.
    check(c.outcome_of(g_self) == Outcome::Delivered, "self-cancel while running: delivered");
    check_eq(static_cast<long long>(c.value_of(g_self).value_or(-1)), 42, "value kept");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::CancelRequested, g_self)), 1,
             "request recorded");

    Conductor c2{4};
    Trace tr2{c2};
    g_c = &c2;
    g_self = c2.launch(self_cancel_then_park());
    c2.post(g_self, 1);
    drain_traced(c2, tr2);
    check(c2.outcome_of(g_self) == Outcome::Cancelled, "self-cancel then park: cancelled");
    check_eq(static_cast<long long>(c2.journal().count_for(Mark::JobSettled, g_self)), 1,
             "one outcome");
    check_eq(static_cast<long long>(c2.arena().live_slots()), 0, "storage back");
    check_eq(static_cast<long long>(c2.journal().count(Mark::FaultReported)), 0,
             "cancellation is not a failure");
}

// ------------------------------------------------------------------ faults

void test_post_fault_uncaught() {
    Conductor c{4};
    Trace tr{c};
    const JobId j = c.launch(waiter());
    c.post_fault(j);
    drain_traced(c, tr);
    check(c.outcome_of(j) == Outcome::Faulted, "fault: faulted");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::FaultReported, j)), 1,
             "one failure announcement");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::JobSettled, j)), 1, "one outcome");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "storage back");
    check(tr.text() == "A1@0;S1=F", "fault trace: " + tr.text());
}

void test_post_fault_caught() {
    Conductor c{4};
    Trace tr{c};
    const JobId j = c.launch(catcher());
    c.post_fault(j);
    drain_traced(c, tr);
    check(c.outcome_of(j) == Outcome::Delivered, "caught fault: delivered");
    check_eq(static_cast<long long>(c.value_of(j).value_or(-1)), -1, "caught fault value");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "storage back");
}

void test_throw_inside_job() {
    Conductor c{4};
    Trace tr{c};
    const JobId j = c.launch(thrower());
    c.post(j, 1);
    drain_traced(c, tr);
    check(c.outcome_of(j) == Outcome::Faulted, "throw: faulted");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::FaultReported, j)), 1,
             "one failure announcement");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "storage back");
}

// ----------------------------------------------------------------- nesting

void test_deep_nesting() {
    Conductor c{4};
    Trace tr{c};
    const JobId j = c.launch(deep_duty());
    check_eq(static_cast<long long>(c.arena().live_slots()), 3, "three frames live");
    c.post(j, 10);
    drain_traced(c, tr);
    check_eq(static_cast<long long>(c.value_of(j).value_or(-1)), 22, "deep value");
    // Each child is gone before the frame above it observes the result.
    check(tr.text() == "A1@0;C1;P1:2;C1;P1:1;S1=D", "deep trace: " + tr.text());
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "all storage back");
}

void test_nested_cancel() {
    Conductor c{4};
    Trace tr{c};
    const JobId j = c.launch(deep_duty());
    c.cancel(j);
    tr.fold(c.now());
    check(c.outcome_of(j) == Outcome::Cancelled, "nested cancel: cancelled");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "all frames destroyed");
    check_eq(static_cast<long long>(c.journal().count(Mark::ChildDestroyed)), 2,
             "both children destroyed");
    check_eq(static_cast<long long>(c.journal().count_for(Mark::JobSettled, j)), 1, "one outcome");
    // Inner frames go first: C1 (grandchild), then C1 (middle) at teardown.
    check(tr.text() == "C1;C1;S1=X", "nested cancel trace: " + tr.text());
    check_eq(static_cast<long long>(c.journal().count(Mark::FaultReported)), 0, "no failure");
}

// -------------------------------------------------------- ticket targeting

void test_ticket_for_other_frame() {
    Conductor c{4};
    Trace tr{c};
    const JobId j = c.launch(parent_then_park());
    c.post(j, 1);  // bound to the child frame
    c.post(j, 2);  // bound to the child frame as well
    drain_traced(c, tr);
    // The second ticket named the child; the job is now parked in the parent.
    check_eq(static_cast<long long>(c.journal().count_for(Mark::TicketAccepted, j)), 1,
             "only the first ticket was delivered");
    check(!c.settled(j), "job still waiting");
    check(tr.text() == "A1@0;C1;P1:1;R1@0", "frame mismatch trace: " + tr.text());
    c.post(j, 3);  // now bound to the parent frame
    drain_traced(c, tr);
    check_eq(static_cast<long long>(c.value_of(j).value_or(-1)), 4, "value 1 + 3");
}

void test_ticket_unbound() {
    Conductor c{4};
    Trace tr{c};
    const JobId j = c.launch(immediate());  // never waits
    c.post(j, 1);                           // nothing was parked: bound to no frame
    drain_traced(c, tr);
    check_eq(static_cast<long long>(c.journal().count_for(Mark::TicketAccepted, j)), 0,
             "unbound ticket never accepted");
    check(c.outcome_of(j) == Outcome::Delivered, "job unaffected");
    check(tr.text() == "S1=D;R1@0", "unbound trace: " + tr.text());
}

void test_recycled_slot_ticket() {
    Conductor c{1};
    Trace tr{c};
    const JobId first = c.launch(waiter());
    c.post(first, 11);
    c.cancel(first);
    tr.fold(c.now());
    const JobId second = c.launch(waiter());
    tr.fold(c.now());
    check(first != second, "different identifier for the new job");
    check_eq(static_cast<long long>(c.arena().live_slots()), 1, "slot reused");
    drain_traced(c, tr);
    check(!c.settled(second), "the new job is untouched by the old ticket");
    check(c.outcome_of(first) == Outcome::Cancelled, "first cancelled");
    check(tr.text() == "S1=X;R1@0", "recycled slot trace: " + tr.text());
}

// ------------------------------------------------------------ independence

void test_independence() {
    Conductor c{8};
    const JobId a = c.launch(waiter());
    const JobId b = c.launch(waiter());
    const JobId d = c.launch(thrower());
    c.post(a, 1);
    c.post(b, 2);
    c.post(d, 3);
    c.cancel(b);
    c.drain();
    check(c.outcome_of(a) == Outcome::Delivered, "a delivered");
    check_eq(static_cast<long long>(c.value_of(a).value_or(-1)), 1, "a value");
    check(c.outcome_of(b) == Outcome::Cancelled, "b cancelled");
    check(c.outcome_of(d) == Outcome::Faulted, "d faulted");
    check_eq(static_cast<long long>(c.arena().live_slots()), 0, "all storage back");
    check_eq(static_cast<long long>(c.live_jobs()), 0, "no live jobs");
}

void test_arena_exhaustion() {
    Conductor c{2};
    const JobId a = c.launch(waiter());
    const JobId b = c.launch(waiter());
    bool threw = false;
    try {
        c.launch(waiter());
    } catch (const std::bad_alloc&) {
        threw = true;
    }
    check(threw, "exhausted arena reports rather than ignores");
    check_eq(static_cast<long long>(c.arena().live_slots()), 2, "still two checked out");
    c.cancel(a);
    c.cancel(b);
    check_eq(static_cast<long long>(c.arena().free_slots()), 2, "all slots back");
    const JobId d = c.launch(waiter());
    check(d != kNoJob, "a slot can be handed out again");
    check_eq(static_cast<long long>(c.arena().total_check_outs()), 3, "genuine reuse");
    check_eq(static_cast<long long>(c.arena().double_reclaims()), 0, "no double reclaims");
}

void test_no_progress_without_step() {
    Conductor c{4};
    const JobId j = c.launch(waiter());
    c.post(j, 9);
    check(!c.settled(j), "nothing runs before a step");
    check_eq(static_cast<long long>(c.pending_tickets()), 1, "ticket queued");
    check(!c.step() == false, "a step delivers one");
    check(c.settled(j), "settled after the step");
}

}  // namespace

int main() {
    test_order_ready_time();
    test_order_job_id_then_seq();
    test_clock_and_repost();
    test_cancel_parked();
    test_cancel_idempotent_and_unknown();
    test_cancel_already_settled();
    test_cancel_runs_cleanup();
    test_cancel_from_inside();
    test_post_fault_uncaught();
    test_post_fault_caught();
    test_throw_inside_job();
    test_deep_nesting();
    test_nested_cancel();
    test_ticket_for_other_frame();
    test_ticket_unbound();
    test_recycled_slot_ticket();
    test_independence();
    test_arena_exhaustion();
    test_no_progress_without_step();
    std::printf("%d checks, %d failures\n", g_checks, g_fail);
    return g_fail == 0 ? 0 : 1;
}
