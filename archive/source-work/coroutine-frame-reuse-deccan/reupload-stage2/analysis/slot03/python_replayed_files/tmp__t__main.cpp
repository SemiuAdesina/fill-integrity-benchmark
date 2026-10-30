// Behavioural tests for the roundel protocol. Not part of the deliverable.
#include "trace.hpp"

#include <stdexcept>
#include <vector>

namespace {

// ---------------------------------------------------------------- duties ---
Duty simple() { co_return co_await park(); }

Duty two_parks() {
    std::int64_t a = co_await park();
    std::int64_t b = co_await park();
    co_return a + b;
}

Duty child() { co_return co_await park(); }

Duty parent_one() {
    const std::int64_t v = co_await child();
    co_return v * 2;
}

Duty parent_two() {
    const std::int64_t a = co_await child();
    const std::int64_t b = co_await child();
    co_return a + b;
}

Duty parent_then_park() {
    const std::int64_t v = co_await child();
    co_return v + co_await park();
}

Duty grandparent() {
    const std::int64_t v = co_await parent_one();
    co_return v + 1;
}

Duty thrower() {
    co_await park();
    throw std::runtime_error("boom");
}

Duty catcher() {
    try {
        co_await park();
    } catch (...) {
    }
    co_return 5;
}

Duty catching_parent() {
    std::int64_t v = 0;
    try {
        v = co_await child();
    } catch (...) {
        v = -1;
    }
    co_return v;
}

// Holds a resource so that frame destruction is observable.
struct Tracked {
    int* counter;
    explicit Tracked(int* c) : counter(c) { ++*counter; }
    ~Tracked() { --*counter; }
};

int g_owned = 0;

Duty holder() {
    Tracked t{&g_owned};
    co_return co_await park();
}

Duty holding_child() {
    Tracked t{&g_owned};
    co_return co_await child();
}

// Cancels itself while running.
Duty self_canceller(Conductor* c, JobId* me) {
    co_await park();
    c->cancel(*me);
    co_return 7;
}

Duty self_canceller_then_park(Conductor* c, JobId* me) {
    co_await park();
    c->cancel(*me);
    co_return co_await park();
}

// Posts a ticket for itself before it has parked: that ticket is bound to no
// frame and can never be accepted.
Duty early_poster(Conductor* c, JobId* me) {
    co_await park();
    c->post(*me, 7);  // while running: not parked, so bound to no frame
    co_return co_await park();
}

// Posts from inside a resumed job: the delay runs from the clock as it stands.
Duty re_poster(Conductor* c, JobId* other, std::vector<std::uint64_t>* seen) {
    co_await park();
    seen->push_back(c->now());
    c->post_after(*other, 2, 3);
    co_return 1;
}

// ---------------------------------------------------------------- tests ----

void t_basic() {
    Conductor c{4};
    const JobId j = c.launch(simple());
    check_eq((long long)j, 1, "first job id");
    check(!c.settled(j), "running after launch");
    check_eq((long long)c.arena().live_slots(), 1, "one slot out");
    c.post(j, 5);
    check(!c.settled(j), "nothing runs before a step");
    Tracer tr;
    check(c.step(), "step delivered");
    tr.sync(c);
    check(c.settled(j), "settled after delivery");
    check_eq((long long)*c.value_of(j), 5, "value");
    check_eq((long long)c.arena().live_slots(), 0, "slot back at settle");
    check_eq(c.journal().count_for(Mark::JobSettled, j), 1, "settled once");
    check_eq(c.journal().count_for(Mark::FrameReleased, j), 1, "released once");
    check_eq((long long)c.now(), 0, "clock still 0");
    check(!c.step(), "empty queue");
    tr.sync(c);
    check(tr.text() == "A1@0;S1=D", "basic trace", tr.text());
    audit_journal(c, "basic");
}

void t_order_ready_then_id() {
    Conductor c{8};
    const JobId a = c.launch(simple());
    const JobId b = c.launch(simple());
    const JobId d = c.launch(simple());
    c.post_after(b, 1, 5);   // ready 5
    c.post_after(d, 2, 5);   // ready 5, id 3
    c.post_after(a, 3, 9);   // ready 9
    c.post(b, 4);            // ready 0
    Tracer tr;
    check(c.step(), "1");
    tr.sync(c);
    check_eq((long long)c.now(), 0, "clock 0");
    check(c.step(), "2");  // b's late ready-5 ticket: lowest id among ready 5
    tr.sync(c);
    check_eq((long long)c.now(), 5, "clock 5");
    check(c.step(), "3");
    tr.sync(c);
    check(c.settled(d), "d done");
    check(!c.settled(a), "a still parked");
    check(c.step(), "4");
    tr.sync(c);
    check_eq((long long)c.now(), 9, "clock 9");
    check(c.settled(a), "a done");
    check_eq(c.pending_tickets(), 0, "queue empty");
    check(tr.text() == "A2@0;S2=D;R2@5;A3@5;S3=D;A1@9;S1=D", "order trace", tr.text());
    audit_journal(c, "order");
}

void t_order_same_job_fifo() {
    Conductor c{4};
    const JobId j = c.launch(two_parks);
    c.post(j, 10);
    c.post(j, 20);
    Tracer tr;
    c.step();
    tr.sync(c);
    c.step();
    tr.sync(c);
    check_eq((long long)*c.value_of(j), 30, "sum of two tickets in order");
    check(tr.text() == "A1@0;A1@0;S1=D", "fifo trace", tr.text());
    // Out-of-order queueing still obeys ready time first.
    Conductor c2{4};
    const JobId k = c2.launch(two_parks);
    c2.post_after(k, 1, 4);
    c2.post(k, 100);
    Tracer tr2;
    c2.step();
    tr2.sync(c2);
    check_eq((long long)c2.now(), 0, "earliest ready first");
    c2.step();
    tr2.sync(c2);
    check_eq((long long)c2.now(), 4, "then the later one");
    check_eq((long long)*c2.value_of(k), 101, "value order");
    audit_journal(c2, "fifo");
}

void t_clock_from_inside() {
    Conductor c{4};
    JobId other = kNoJob;
    std::vector<std::uint64_t> seen;
    const JobId a = c.launch(re_poster(&c, &other, &seen));
    other = c.launch(simple());
    c.post(a, 1);
    c.step();  // a resumes at 0 and posts a ticket for b, ready 0+3
    check_eq((long long)c.now(), 0, "clock unmoved by the job's own post");
    check_eq(c.pending_tickets(), 1, "one ticket queued from inside");
    check_eq((long long)*c.value_of(a), 1, "a delivered");
    c.step();
    check_eq((long long)c.now(), 3, "clock moves to the new ready time");
    check_eq((long long)*c.value_of(other), 2, "the inner ticket's value");
    check_eq((long long)seen.size(), 1, "job resumed once");
    check_eq((long long)seen.at(0), 0, "saw the clock of its own delivery");
    audit_journal(c, "clock inside");
}

void t_reject_settled() {
    Conductor c{4};
    const JobId j = c.launch(simple());
    c.post(j, 1);
    c.cancel(j);
    check(c.settled(j), "cancelled settles at once");
    check_eq((long long)c.arena().live_slots(), 0, "storage released at once");
    c.post(j, 2);
    c.cancel(j);
    Tracer tr;
    check(c.step(), "deliver the late ticket");
    tr.sync(c);
    check_eq(c.journal().count_for(Mark::CancelRequested, j), 2, "both cancels recorded");
    check_eq(c.journal().count_for(Mark::JobSettled, j), 1, "settled once");
    check(outcome_str(c.outcome_of(j)) == "X", "cancelled", outcome_str(c.outcome_of(j)));
    check(tr.text() == "S1=X;R1@0", "reject after settle", tr.text());
    audit_journal(c, "reject settled");
}

void t_reject_unknown() {
    Conductor c{4};
    c.post(99, 1);
    c.cancel(99);
    Tracer tr;
    check(c.step(), "deliver for a job that never was");
    tr.sync(c);
    check_eq(c.journal().count_for(Mark::CancelRequested, 99), 1, "cancel recorded anyway");
    check(tr.text() == "R99@0", "unknown rejected", tr.text());
    audit_journal(c, "unknown");
}

void t_reject_recycled_slot() {
    Conductor c{1};
    const JobId first = c.launch(simple());
    c.post(first, 11);   // bound to slot 0 epoch 1
    c.cancel(first);
    const JobId second = c.launch(simple());
    check(first != second, "ids differ");
    check_eq((long long)c.arena().total_check_outs(), 2, "slot handed out again");
    Tracer tr;
    check(c.step(), "late ticket for the dead job");
    tr.sync(c);
    check(!c.settled(second), "the new job was not woken by it");
    check_eq(c.journal().count_for(Mark::TicketAccepted, second), 0, "no delivery to job 2");
    check(tr.text() == "S1=X;R1@0", "recycled slot rejected", tr.text());
    audit_journal(c, "recycled");
}

void t_reject_same_index_new_epoch() {
    // The job stays alive; its child frame is destroyed and the slot is handed
    // to a second child. Index matches, epoch does not.
    Conductor c{4};
    const JobId j = c.launch(parent_two());
    c.post(j, 10);  // bound to child 1
    c.post(j, 20);  // bound to child 1 as well
    Tracer tr;
    check(c.step(), "first ticket");
    tr.sync(c);
    check(c.step(), "second ticket, now parked in the next child");
    tr.sync(c);
    check(!c.settled(j), "job still waiting");
    check(tr.text() == "A1@0;C1;P1:1;R1@0", "index matches, epoch does not", tr.text());
    audit_journal(c, "epoch mismatch");
}

void t_reject_same_epoch_other_slot() {
    // Parent parks, then a child parks: two live frames share an epoch.
    Conductor c{4};
    const JobId j = c.launch(parent_then_park());
    c.post(j, 1);  // bound to the root frame (epoch 1, slot 0)
    c.post(j, 2);  // also bound to the root frame
    Tracer tr;
    check(c.step(), "resume the parent");
    tr.sync(c);
    check(c.step(), "second ticket against the child frame");
    tr.sync(c);
    check(!c.settled(j), "still waiting in the child");
    check(tr.text() == "A1@0;C1;P1:1;R1@0", "epoch matches, index does not", tr.text());
    audit_journal(c, "index mismatch");
}

void t_unbound_never_accepted() {
    Conductor c{4};
    JobId me = kNoJob;
    me = c.launch(early_poster(&c, &me));
    c.post(me, 9);  // bound: queued while the job is parked
    Tracer tr;
    check(c.step(), "deliver the bound one first");
    tr.sync(c);
    check(!c.settled(me), "parked again");
    check_eq(c.pending_tickets(), 1, "the self-posted one is still queued");
    check(c.step(), "then the unbound one");
    tr.sync(c);
    check(!c.settled(me), "unbound ticket resumed nothing, even parked in the same frame");
    check_eq(c.journal().count_for(Mark::TicketAccepted, me), 1, "one acceptance");
    check(tr.text() == "A1@0;R1@0", "unbound rejected", tr.text());
    c.post(me, 9);
    c.step();
    check_eq((long long)*c.value_of(me), 9, "bound value");
    audit_journal(c, "unbound");
}

void t_repark_same_frame() {
    Conductor c{4};
    const JobId j = c.launch(two_parks);
    c.post(j, 4);
    c.post(j, 6);
    Tracer tr;
    c.step();
    tr.sync(c);
    c.step();
    tr.sync(c);
    check_eq((long long)*c.value_of(j), 10, "both delivered");
    check_eq(c.journal().count_for(Mark::TicketAccepted, j), 2, "two acceptances");
    check(tr.text() == "A1@0;A1@0;S1=D", "re-park accepts", tr.text());
    audit_journal(c, "re-park");
}

void t_child_destroyed_before_parent_sees() {
    Conductor c{4};
    const JobId j = c.launch(parent_one());
    c.post(j, 21);
    Tracer tr;
    c.step();
    tr.sync(c);
    check_eq((long long)*c.value_of(j), 42, "doubled");
    check_eq((long long)c.arena().live_slots(), 0, "all slots back");
    check(tr.text() == "A1@0;C1;P1:1;S1=D", "child first", tr.text());
    const auto& es = c.journal().entries();
    std::size_t cd = 0, pc = 0;
    for (std::size_t i = 0; i < es.size(); ++i) {
        if (es[i].mark == Mark::ChildDestroyed) cd = i;
        if (es[i].mark == Mark::ParentContinued) pc = i;
    }
    check(cd < pc, "ChildDestroyed precedes ParentContinued");
    audit_journal(c, "child order");
}

void t_three_levels() {
    Conductor c{8};
    const JobId j = c.launch(grandparent());
    c.post(j, 5);
    check_eq((long long)c.arena().live_slots(), 3, "three frames out");
    check_eq((long long)c.arena().peak_live(), 3, "peak three");
    Tracer tr;
    c.step();
    tr.sync(c);
    check_eq((long long)*c.value_of(j), 11, "5*2+1");
    check_eq((long long)c.arena().live_slots(), 0, "all back");
    check_eq(c.journal().count_for(Mark::ChildDestroyed, j), 2, "two children destroyed");
    check(tr.text() == "A1@0;C1;P1:2;C1;P1:1;S1=D", "inner first", tr.text());
    audit_journal(c, "three levels");
}

void t_fault_escapes() {
    Conductor c{4};
    const JobId j = c.launch(thrower());
    c.post(j, 1);
    Tracer tr;
    c.step();
    tr.sync(c);
    check(outcome_str(c.outcome_of(j)) == "F", "faulted", outcome_str(c.outcome_of(j)));
    check_eq(c.journal().count_for(Mark::FaultReported, j), 1, "fault reported once");
    check_eq((long long)c.arena().live_slots(), 0, "storage back");
    check(tr.text() == "A1@0;S1=F", "fault trace", tr.text());
    audit_journal(c, "fault");
}

void t_fault_ticket() {
    Conductor c{4};
    const JobId j = c.launch(simple());
    c.post_fault(j);
    Tracer tr;
    c.step();
    tr.sync(c);
    check(outcome_str(c.outcome_of(j)) == "F", "faulted by ticket");
    check_eq(c.journal().count_for(Mark::FaultReported, j), 1, "reported once");
    check_eq((long long)c.arena().live_slots(), 0, "storage back");
    audit_journal(c, "fault ticket");

    // A job that catches it carries on.
    Conductor c2{4};
    const JobId k = c2.launch(catcher());
    c2.post_fault(k);
    c2.step();
    check(outcome_str(c2.outcome_of(k)) == "D", "caught, delivered", outcome_str(c2.outcome_of(k)));
    check_eq((long long)*c2.value_of(k), 5, "value after catch");
    check_eq(c2.journal().count_for(Mark::FaultReported, k), 0, "no fault reported");
    audit_journal(c2, "caught fault");
}

void t_fault_in_child() {
    Conductor c{4};
    const JobId j = c.launch(parent_one());
    c.post_fault(j);
    Tracer tr;
    c.step();
    tr.sync(c);
    check(outcome_str(c.outcome_of(j)) == "F", "child fault reaches parent");
    check_eq(c.journal().count_for(Mark::FaultReported, j), 1, "reported once");
    check_eq((long long)c.arena().live_slots(), 0, "both frames gone");
    check(tr.text() == "A1@0;C1;P1:1;S1=F", "child fault trace", tr.text());
    audit_journal(c, "child fault");

    Conductor c2{4};
    const JobId k = c2.launch(catching_parent());
    c2.post_fault(k);
    c2.step();
    check(outcome_str(c2.outcome_of(k)) == "D", "parent caught", outcome_str(c2.outcome_of(k)));
    check_eq((long long)*c2.value_of(k), -1, "caught value");
    check_eq(c2.journal().count_for(Mark::FaultReported, k), 0, "no report when caught");
    audit_journal(c2, "child fault caught");
}

void t_cancel_runs_cleanup() {
    g_owned = 0;
    {
        Conductor c{4};
        const JobId j = c.launch(holder());
        check_eq((long long)g_owned, 1, "frame holds its resource");
        c.cancel(j);
        check_eq((long long)g_owned, 0, "cleanup ran");
        check_eq((long long)c.arena().live_slots(), 0, "slot back");
        check(outcome_str(c.outcome_of(j)) == "X", "cancelled");
        c.cancel(j);
        check_eq(c.journal().count_for(Mark::CancelRequested, j), 2, "second cancel recorded");
        check_eq(c.journal().count_for(Mark::JobSettled, j), 1, "still settled once");
    }
    check_eq((long long)g_owned, 0, "nothing left owned");
}

void t_cancel_nested_cleanup() {
    g_owned = 0;
    Conductor c{8};
    const JobId j = c.launch(holding_child());
    check_eq((long long)c.arena().live_slots(), 2, "parent and child frames");
    check_eq((long long)g_owned, 1, "one tracked object");
    c.cancel(j);
    check_eq((long long)g_owned, 0, "cleanup ran");
    check_eq((long long)c.arena().live_slots(), 0, "both slots back");
    check(outcome_str(c.outcome_of(j)) == "X", "cancelled");
    audit_journal(c, "nested cancel");
}

void t_cancel_from_inside() {
    {
        Conductor c{4};
        JobId me = kNoJob;
        me = c.launch(self_canceller(&c, &me));
        c.post(me, 1);
        Tracer tr;
        c.step();
        tr.sync(c);
        check(c.settled(me), "settled");
        check(outcome_str(c.outcome_of(me)) == "X", "self cancel is decisive",
              outcome_str(c.outcome_of(me)));
        check_eq((long long)c.arena().live_slots(), 0, "slot back");
        check(tr.text() == "A1@0;S1=X", "self cancel trace", tr.text());
        audit_journal(c, "self cancel");
    }
    {
        Conductor c{4};
        JobId me = kNoJob;
        me = c.launch(self_canceller_then_park(&c, &me));
        c.post(me, 1);
        c.post(me, 2);
        c.step();  // resumes, cancels itself, parks again: stopped at the park
        check(c.settled(me), "cancelled at its next park");
        check(outcome_str(c.outcome_of(me)) == "X", "cancelled, never resumed",
              outcome_str(c.outcome_of(me)));
        check_eq((long long)c.arena().live_slots(), 0, "slot back");
        c.step();
        check_eq(c.journal().count_for(Mark::TicketAccepted, me), 1, "only one acceptance");
        check_eq(c.journal().count_for(Mark::TicketRejected, me), 1, "the rest refused");
        check_eq((long long)c.arena().live_slots(), 0, "slot back");
        audit_journal(c, "self cancel then park");
    }
}

void t_cancel_while_delivering() {
    // A job cancelled from inside another job, while it is parked.
    Conductor c{8};
    const JobId a = c.launch(simple());
    const JobId b = c.launch(simple());
    c.cancel(a);
    check_eq((long long)c.arena().live_slots(), 1, "only b's frame left");
    c.post(b, 3);
    c.step();
    check_eq((long long)*c.value_of(b), 3, "the other job is undisturbed");
    check(outcome_str(c.outcome_of(a)) == "X", "a cancelled");
    audit_journal(c, "independence");
}

void t_capacity() {
    Conductor c{2};
    const JobId a = c.launch(simple());
    const JobId b = c.launch(simple());
    check_eq((long long)c.arena().live_slots(), 2, "two out");
    bool threw = false;
    try {
        c.launch(simple());
    } catch (const std::bad_alloc&) {
        threw = true;
    }
    check(threw, "exhaustion is reported");
    check_eq((long long)c.arena().live_slots(), 2, "still two out");
    c.post(a, 1);
    c.step();
    check_eq((long long)c.arena().live_slots(), 1, "one back");
    const JobId d = c.launch(simple());
    check(d != a && d != b, "new id");
    check_eq((long long)c.arena().total_check_outs(), 3, "recycled");
    check(c.arena().live_slots() <= c.arena().capacity(), "within capacity");
    audit_journal(c, "capacity");
}

void t_drain_and_stepping() {
    Conductor c{8};
    const JobId a = c.launch(simple());
    const JobId b = c.launch(two_parks);
    c.post(a, 1);
    c.post(b, 2);
    c.post(b, 3);
    check_eq(c.pending_tickets(), 3, "three queued");
    check(c.step(), "one per step");
    check_eq(c.pending_tickets(), 2, "one delivered per step");
    const std::size_t n = c.drain();
    check_eq((long long)n, 2, "drain took the rest");
    check_eq(c.pending_tickets(), 0, "empty");
    check_eq((long long)*c.value_of(a), 1, "a");
    check_eq((long long)*c.value_of(b), 5, "b");
    check_eq((long long)c.arena().live_slots(), 0, "all back");
    audit_journal(c, "drain");
}

void t_shutdown_releases() {
    g_owned = 0;
    {
        Conductor c{8};
        c.launch(holder());
        c.launch(holding_child());
        check_eq((long long)c.arena().live_slots(), 3, "three out");
        check_eq((long long)c.live_jobs(), 2, "two live");
    }
    check_eq((long long)g_owned, 0, "destructor ran the cleanup");
}

void t_late_ticket_after_child_gone() {
    // A ticket bound to a child frame whose slot has since gone to another job
    // must not wake that other job.
    Conductor c{4};
    const JobId a = c.launch(parent_one());  // root 0/1, child 1/1
    c.post(a, 1);                            // bound to child 1/1
    c.post(a, 2);                            // bound to child 1/1
    c.step();                                // child done, parent finishes
    check(c.settled(a), "a finished");
    check_eq((long long)c.arena().live_slots(), 0, "both slots free");
    const JobId b = c.launch(simple());  // takes slot 0 epoch 2
    (void)b;
    Tracer tr;
    check(c.step(), "late ticket for a");
    tr.sync(c);
    check(tr.text() == "A1@0;C1;P1:1;S1=D;R1@0", "late child ticket", tr.text());
    audit_journal(c, "late child ticket");
}

}  // namespace

int main() {
    t_basic();
    t_order_ready_then_id();
    t_order_same_job_fifo();
    t_clock_from_inside();
    t_reject_settled();
    t_reject_unknown();
    t_reject_recycled_slot();
    t_reject_same_index_new_epoch();
    t_reject_same_epoch_other_slot();
    t_unbound_never_accepted();
    t_repark_same_frame();
    t_child_destroyed_before_parent_sees();
    t_three_levels();
    t_fault_escapes();
    t_fault_ticket();
    t_fault_in_child();
    t_cancel_runs_cleanup();
    t_cancel_nested_cleanup();
    t_cancel_from_inside();
    t_cancel_while_delivering();
    t_capacity();
    t_drain_and_stepping();
    t_shutdown_releases();
    t_late_ticket_after_child_gone();

    std::printf("%d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
