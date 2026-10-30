// Local test harness (not part of /app).
#include "roundel/runtime.hpp"

#include <cstdio>
#include <stdexcept>
#include <string>
#include <vector>

using namespace roundel;

static int g_fail = 0;
#define CHECK(cond, msg)                                                            \
    do {                                                                            \
        if (!(cond)) {                                                              \
            std::printf("  FAIL line %d: %s\n", __LINE__, msg);                     \
            ++g_fail;                                                               \
        }                                                                           \
    } while (0)

static Conductor* g_c = nullptr;
static JobId g_self = 0;

static std::string trace(const Journal& j) {
    std::string out;
    for (const auto& e : j.entries()) {
        switch (e.mark) {
            case Mark::TicketAccepted:
                out += "A" + std::to_string(e.job) + ";";
                break;
            case Mark::TicketRejected:
                out += "R" + std::to_string(e.job) + ";";
                break;
            case Mark::ChildDestroyed:
                out += "C" + std::to_string(e.job) + ":" + std::to_string(e.slot.index) + "/" +
                       std::to_string(e.slot.epoch) + ";";
                break;
            case Mark::ParentContinued:
                out += "P" + std::to_string(e.job) + ":" + std::to_string(e.detail) + ";";
                break;
            case Mark::JobSettled:
                out += "S" + std::to_string(e.job) + "=" +
                       std::string(1, e.detail == 0 ? 'D' : (e.detail == 1 ? 'X' : 'F')) + ";";
                break;
            default:
                break;
        }
    }
    return out;
}

// ---------------------------------------------------------------- duties
static Duty leaf() { co_return co_await park(); }

static Duty mid() { co_return co_await leaf() + 1; }

static Duty deep() { co_return co_await mid() * 2; }

// awaits a child, then parks in its own frame
static Duty then_park() {
    const std::int64_t v = co_await leaf();
    co_return v + co_await park();
}

// awaits two children in sequence
static Duty two_children() {
    const std::int64_t a = co_await leaf();
    const std::int64_t b = co_await leaf();
    co_return a + b;
}

static Duty thrower() {
    co_await park();
    throw std::runtime_error("boom");
}

static Duty catcher() {
    try {
        co_await park();
    } catch (const std::exception&) {
    }
    co_return 5;
}

static Duty self_canceller() {
    g_c->cancel(g_self);
    co_return 7;
}

static Duty self_cancel_then_park() {
    g_c->cancel(g_self);
    const std::int64_t v = co_await park();
    co_return v;
}

static Duty poster() {
    const std::int64_t v = co_await park();
    g_c->post(g_self, 100);          // ready now (the clock moved to this step)
    g_c->post_after(g_self, 200, 5); // ready 5 later
    co_return v;
}

static Duty two_parks() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a * 10 + b;
}

static Duty canceller_of(JobId other) {
    g_c->cancel(other);
    co_return 1;
}

// ---------------------------------------------------------------- tests

static void t_recycled_epoch() {
    std::printf("test: ticket bound to a released child slot is refused\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(then_park());
    // parked in the child (slot 1/1)
    c.post(j, 1);            // ready 0 -> delivered first
    c.post_after(j, 2, 10);  // ready 10 -> bound to the child slot 1/1
    c.drain();
    // first ticket resumed the child; its frame was destroyed; the parent now
    // parks in its own frame (0/1). The stale ticket names (1,1): same epoch,
    // different index -> refused.
    CHECK(!c.settled(j), "still parked in the parent frame");
    CHECK(c.arena().live_slots() == 1, "child storage back in the pool");
    CHECK(c.journal().count_for(Mark::TicketRejected, j) == 1, "stale ticket rejected");
    c.post(j, 5);
    c.drain();
    CHECK(c.settled(j), "job settled");
    CHECK(*c.outcome_of(j) == Outcome::Delivered, "delivered");
    CHECK(*c.value_of(j) == 6, "value 1+5");
    CHECK(c.arena().live_slots() == 0, "no frames left");
    std::printf("  trace=%s\n", trace(c.journal()).c_str());
}

static void t_epoch_reuse() {
    std::printf("test: recycled slot keeps its own generation\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(two_children());
    // child #1 in slot 1/1
    c.post_after(j, 3, 10);  // bound to child #1 (1/1), delivered later
    c.post(j, 4);            // ready 0, delivered first -> child #1 completes
    c.step();                // deliver 4 to child #1: it returns, frame destroyed
    // child #2 now takes slot 1 again, epoch 2
    CHECK(c.arena().live_slot_ids().size() == 2, "root + child2 live");
    const SlotId parked = c.arena().live_slot_ids().back();
    CHECK(parked.index == 1 && parked.epoch == 2, "slot 1 reused with epoch 2");
    c.step();  // the ticket bound to (1,1): index matches, epoch does not
    CHECK(c.journal().count_for(Mark::TicketRejected, j) == 1, "stale-epoch ticket rejected");
    CHECK(c.journal().count_for(Mark::TicketAccepted, j) == 1, "only one accepted");
    CHECK(!c.settled(j), "job still waiting in child #2");
    c.post(j, 5);
    c.drain();
    CHECK(c.settled(j) && *c.value_of(j) == 9, "4+5");
    CHECK(c.arena().live_slots() == 0, "clean");
}

static void t_park_again_same_frame() {
    std::printf("test: a job that parks again in the same frame still receives\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(two_parks());
    c.post(j, 2);            // bound (0,1) ready 0
    c.post_after(j, 3, 4);   // bound (0,1) ready 4
    c.drain();
    CHECK(c.settled(j), "settled");
    CHECK(*c.value_of(j) == 23, "2*10+3");
    CHECK(c.journal().count_for(Mark::TicketAccepted, j) == 2, "both accepted");
    CHECK(c.journal().count_for(Mark::TicketRejected, j) == 0, "none rejected");
    CHECK(c.arena().live_slots() == 0, "clean");
}

static void t_nested_order() {
    std::printf("test: inner frames go before outer ones\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(deep());
    c.post(j, 10);
    c.drain();
    CHECK(c.settled(j), "settled");
    CHECK(*c.value_of(j) == 22, "(10+1)*2");
    const std::string tr = trace(c.journal());
    std::printf("  trace=%s\n", tr.c_str());
    CHECK(tr == "A1;C1:2/1;P1:2;C1:1/1;P1:1;S1=D;", "nested destruction order");
    CHECK(c.arena().live_slots() == 0, "clean");
    CHECK(c.arena().double_reclaims() == 0, "no double reclaim");
}

static void t_child_throws() {
    std::printf("test: child that throws\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(mid());
    c.post_fault(j);
    c.drain();
    CHECK(c.settled(j), "settled");
    CHECK(*c.outcome_of(j) == Outcome::Faulted, "faulted");
    CHECK(c.journal().count_for(Mark::FaultReported, j) == 1, "fault reported once");
    CHECK(c.journal().count_for(Mark::JobSettled, j) == 1, "settled once");
    CHECK(c.arena().live_slots() == 0, "child frame returned");
    const std::size_t cd = c.journal().first_index(Mark::ChildDestroyed, j);
    const std::size_t st = c.journal().first_index(Mark::JobSettled, j);
    CHECK(cd < st, "child destroyed before settle");
    std::printf("  trace=%s\n", trace(c.journal()).c_str());
}

static void t_thrower() {
    std::printf("test: exception inside a job body\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(thrower());
    c.post(j, 1);
    c.drain();
    CHECK(*c.outcome_of(j) == Outcome::Faulted, "faulted");
    CHECK(c.journal().count_for(Mark::FaultReported, j) == 1, "reported once");
    CHECK(c.arena().live_slots() == 0, "clean");
}

static void t_catcher() {
    std::printf("test: a job that catches the failure carries on\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(catcher());
    c.post_fault(j);
    c.drain();
    CHECK(*c.outcome_of(j) == Outcome::Delivered, "delivered");
    CHECK(*c.value_of(j) == 5, "value 5");
    CHECK(c.journal().count_for(Mark::FaultReported, j) == 0, "no fault reported");
    CHECK(c.arena().live_slots() == 0, "clean");
}

static void t_cancel_parked() {
    std::printf("test: cancel while parked\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(leaf());
    c.post(j, 9);
    c.cancel(j);
    CHECK(c.settled(j), "settled at once");
    CHECK(*c.outcome_of(j) == Outcome::Cancelled, "cancelled");
    CHECK(c.arena().live_slots() == 0, "storage back at once");
    c.drain();
    CHECK(c.journal().count_for(Mark::JobSettled, j) == 1, "settled once");
    CHECK(c.journal().count_for(Mark::TicketRejected, j) == 1, "late ticket rejected");
    c.cancel(j);
    CHECK(c.journal().count_for(Mark::CancelRequested, j) == 2, "request counted twice");
    CHECK(c.journal().count_for(Mark::JobSettled, j) == 1, "still settled once");
    CHECK(*c.outcome_of(j) == Outcome::Cancelled, "still cancelled");
}

static void t_cancel_unknown() {
    std::printf("test: cancel a job that never existed\n");
    Conductor c{4};
    g_c = &c;
    c.cancel(999);
    c.cancel(999);
    CHECK(c.journal().count_for(Mark::CancelRequested, 999) == 2, "both recorded");
    CHECK(c.live_jobs() == 0, "no jobs");
}

static void t_cancel_nested() {
    std::printf("test: cancel a job parked three frames deep\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(deep());
    CHECK(c.arena().live_slots() == 3, "three frames live");
    c.cancel(j);
    CHECK(c.settled(j), "settled");
    CHECK(*c.outcome_of(j) == Outcome::Cancelled, "cancelled");
    CHECK(c.arena().live_slots() == 0, "all frames returned");
    CHECK(c.journal().count_for(Mark::ChildDestroyed, j) == 2, "both children destroyed");
    const std::string tr = trace(c.journal());
    std::printf("  trace=%s\n", tr.c_str());
    CHECK(tr == "C1:2/1;C1:1/1;S1=X;", "inner frames first");
}

static void t_self_cancel() {
    std::printf("test: cancel from inside the job\n");
    {
        Conductor c{4};
        g_c = &c;
        g_self = 1;
        const JobId j = c.launch(self_canceller());
        CHECK(j == 1, "id 1");
        CHECK(c.settled(j), "settled");
        CHECK(*c.outcome_of(j) == Outcome::Cancelled, "cancelled");
        CHECK(c.arena().live_slots() == 0, "clean");
    }
    {
        Conductor c{4};
        g_c = &c;
        g_self = 1;
        const JobId j = c.launch(self_cancel_then_park());
        CHECK(c.settled(j), "settled, not left hanging");
        CHECK(*c.outcome_of(j) == Outcome::Cancelled, "cancelled");
        CHECK(c.arena().live_slots() == 0, "clean");
    }
}

static void t_cancel_other_from_job() {
    std::printf("test: a job cancels an unrelated job\n");
    Conductor c{4};
    g_c = &c;
    const JobId a = c.launch(leaf());
    const JobId b = c.launch(canceller_of(a));
    (void)b;
    (void)a;
    CHECK(c.settled(1), "first cancelled by the second");
    CHECK(*c.outcome_of(1) == Outcome::Cancelled, "cancelled");
    CHECK(c.arena().live_slots() == 0, "clean");
}

static void t_independence() {
    std::printf("test: jobs are independent\n");
    Conductor c{8};
    g_c = &c;
    const JobId a = c.launch(leaf());
    const JobId b = c.launch(thrower());
    const JobId d = c.launch(leaf());
    c.cancel(a);
    c.post(b, 1);
    c.post(d, 4);
    c.drain();
    CHECK(*c.outcome_of(a) == Outcome::Cancelled, "a cancelled");
    CHECK(*c.outcome_of(b) == Outcome::Faulted, "b faulted");
    CHECK(*c.outcome_of(d) == Outcome::Delivered, "d delivered");
    CHECK(*c.value_of(d) == 4, "d value");
    CHECK(c.arena().live_slots() == 0, "clean");
    CHECK(c.live_jobs() == 0, "no live jobs");
}

static void t_order_and_clock() {
    std::printf("test: delivery order and the clock\n");
    Conductor c{8};
    g_c = &c;
    const JobId a = c.launch(leaf());
    const JobId b = c.launch(leaf());
    const JobId d = c.launch(leaf());
    c.post_after(b, 1, 5);
    c.post_after(a, 2, 5);
    c.post_after(d, 3, 1);
    c.post_after(a, 4, 5);  // same time, same job: queue order decides
    CHECK(c.now() == 0, "clock starts at 0");
    CHECK(c.step(), "step 1");
    CHECK(c.now() == 1, "clock follows delivery");
    CHECK(c.settled(d), "d got the earliest");
    CHECK(c.step(), "step 2");
    CHECK(c.now() == 5, "clock is at 5");
    CHECK(c.settled(a) && *c.value_of(a) == 2, "lowest id first, then queue order");
    CHECK(!c.settled(b), "b still waiting");
    CHECK(c.step(), "step 3");
    CHECK(c.now() == 5, "clock does not move on its own");
    // a's second ticket (ready 5, queued later) comes before b's (job 2): it
    // is refused because a has already settled.
    CHECK(c.journal().count_for(Mark::TicketRejected, a) == 1, "a's second ticket refused");
    CHECK(c.step(), "step 4");
    CHECK(c.settled(b), "b done");
    CHECK(!c.step(), "queue empty");
    CHECK(c.now() == 5, "clock unchanged");
    CHECK(c.arena().live_slots() == 0, "clean");
    std::printf("  trace=%s\n", trace(c.journal()).c_str());
}

static void t_post_from_inside() {
    std::printf("test: scheduling from inside a running job\n");
    Conductor c{4};
    g_c = &c;
    g_self = 1;
    const JobId j = c.launch(poster());
    c.post(j, 1);          // ready 0 -> resumes the job, which posts 100 (ready 0?) ...
    c.drain();
    CHECK(c.settled(j), "settled");
    CHECK(*c.value_of(j) == 1, "the first value is what the await saw");
    const std::vector<Entry>& es = c.journal().entries();
    std::size_t accepted = 0;
    for (const auto& e : es) {
        if (e.mark == Mark::TicketAccepted) {
            ++accepted;
        }
    }
    CHECK(accepted == 1, "later tickets for a settled or running job are refused");
    CHECK(c.arena().live_slots() == 0, "clean");
}

static void t_delay_from_now() {
    std::printf("test: a delay is measured from the clock when queued\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(two_parks());
    c.post(j, 1);                 // ready 0
    c.step();                     // clock -> 0; the job resumes and parks again
    CHECK(c.now() == 0, "clock at 0");
    c.post_after(j, 2, 3);        // queued at now()=0 -> ready 3
    c.step();                     // must not deliver before the clock reaches 3
    CHECK(c.settled(j), "delivered");
    CHECK(c.now() == 3, "clock moved to the ready time");
    CHECK(*c.value_of(j) == 12, "1*10+2");
}

static void t_exhaustion() {
    std::printf("test: the pool is bounded and reports exhaustion\n");
    Conductor c{2};
    g_c = &c;
    const JobId a = c.launch(leaf());
    const JobId b = c.launch(leaf());
    CHECK(c.arena().live_slots() == 2, "two frames out");
    bool threw = false;
    try {
        Duty d = leaf();  // no slot left
        (void)d;
    } catch (const std::bad_alloc&) {
        threw = true;
    }
    CHECK(threw, "exhaustion reported");
    c.cancel(a);
    c.cancel(b);
    CHECK(c.arena().live_slots() == 0, "back to empty");
    CHECK(c.arena().peak_live() == 2, "peak within capacity");
    const JobId d2 = c.launch(leaf());
    CHECK(c.settled(d2) == false, "reused storage");
    CHECK(c.arena().total_check_outs() == 3, "storage really reused");
    c.cancel(d2);
    CHECK(c.arena().live_slots() == 0, "clean");
}

static void t_shutdown() {
    std::printf("test: shutdown releases everything\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(deep());
    c.post(j, 1);
    (void)j;
    CHECK(c.arena().live_slots() == 3, "frames live");
}

static void t_many() {
    std::printf("test: churn reuses storage and stays clean\n");
    Conductor c{3};
    g_c = &c;
    for (int i = 0; i < 50; ++i) {
        const JobId j = c.launch(leaf());
        if (i % 3 == 0) {
            c.cancel(j);
        } else if (i % 3 == 1) {
            c.post(j, i);
            c.drain();
        } else {
            c.post_fault(j);
            c.drain();
        }
        CHECK(c.arena().live_slots() == 0, "nothing held between rounds");
    }
    CHECK(c.arena().live_slots() == 0, "clean at the end");
    CHECK(c.arena().peak_live() <= 3, "peak within capacity");
    CHECK(c.arena().double_reclaims() == 0, "no double reclaim");
    CHECK(c.live_jobs() == 0, "no live jobs");
}

static Duty cat_abandoned() {
    try {
        co_await park();
    } catch (const Abandoned&) {
    }
    co_return 3;
}

static Duty stubborn() {
    for (int i = 0; i < 64; ++i) {
        try {
            co_await park();
            break;
        } catch (const Abandoned&) {
            // park again: a single request must not chase this for ever
        }
    }
    co_return 4;
}

static Duty self_poster_then_park() {
    const std::int64_t a = co_await park();
    g_c->post(g_self, 42);  // queued while running: bound to no frame
    const std::int64_t b = co_await park();
    co_return a * 10 + b;
}

static Duty inner_step() {
    g_c->step();  // drive the runtime from inside a running job
    co_return 8;
}

static Duty inner_launcher() {
    const JobId other = g_c->launch(leaf());
    co_return static_cast<std::int64_t>(other);
}

static void t_stubborn() {
    std::printf("test: a job that catches Abandoned is not chased for ever\n");
    {
        Conductor c{4};
        g_c = &c;
        const JobId j = c.launch(cat_abandoned());
        c.cancel(j);
        CHECK(c.settled(j), "settled");
        CHECK(*c.outcome_of(j) == Outcome::Cancelled, "cancelled, no value");
        CHECK(c.arena().live_slots() == 0, "clean");
    }
    {
        Conductor c{4};
        g_c = &c;
        const JobId j = c.launch(stubborn());
        c.cancel(j);
        CHECK(c.arena().live_slots() == 0 || !c.settled(j), "no crash");
        CHECK(c.arena().double_reclaims() == 0, "no double reclaim");
    }
}

static void t_post_while_running() {
    std::printf("test: a ticket queued for a running job authorises nothing\n");
    Conductor c{4};
    g_c = &c;
    g_self = 1;
    const JobId j = c.launch(self_poster_then_park());
    c.post(j, 1);  // resumes the job, which posts to itself and parks again
    c.step();
    CHECK(!c.settled(j), "still parked");
    c.step();  // the ticket queued while the job was running
    CHECK(c.journal().count_for(Mark::TicketRejected, j) == 1, "refused, forever");
    CHECK(!c.settled(j), "still parked");
    c.post(j, 5);  // a ticket queued while parked is bound
    c.drain();
    CHECK(c.settled(j) && *c.value_of(j) == 15, "delivered");
    CHECK(c.arena().live_slots() == 0, "clean");
}

static void t_inner_drive() {
    std::printf("test: driving the runtime from inside a job\n");
    {
        Conductor c{4};
        g_c = &c;
        const JobId j = c.launch(leaf());
        c.post(j, 1);
        const JobId k = c.launch(inner_step());  // steps the queue from inside
        CHECK(c.settled(j) && *c.value_of(j) == 1, "inner step delivered it");
        CHECK(c.settled(k) && *c.value_of(k) == 8, "the driving job finished");
        CHECK(c.arena().live_slots() == 0, "clean");
    }
    {
        Conductor c{4};
        g_c = &c;
        g_self = 1;
        const JobId j = c.launch(inner_launcher());
        CHECK(c.settled(j), "launcher settled");
        CHECK(*c.value_of(j) == 2, "launched job 2");
        CHECK(c.live_jobs() == 1, "the launched job is still parked");
        CHECK(c.arena().live_slots() == 1, "its frame is checked out");
    }
}

static void t_child_alloc_fails() {
    std::printf("test: a child that cannot get a frame faults the job\n");
    Conductor c{1};
    g_c = &c;
    const JobId j = c.launch(mid());  // root takes the only slot
    CHECK(c.settled(j), "settled");
    CHECK(*c.outcome_of(j) == Outcome::Faulted, "faulted");
    CHECK(c.journal().count_for(Mark::FaultReported, j) == 1, "reported once");
    CHECK(c.arena().live_slots() == 0, "root frame returned");
}

static void t_unlaunched_duty() {
    std::printf("test: a duty that is never launched\n");
    Conductor c{4};
    g_c = &c;
    {
        Duty d = leaf();
        CHECK(c.arena().live_slots() == 1, "frame checked out");
    }
    CHECK(c.arena().live_slots() == 0, "frame returned with the duty");
    CHECK(c.arena().double_reclaims() == 0, "no double reclaim");
}

static void t_heap_frame() {
    std::printf("test: a duty made with no conductor alive\n");
    // No Conductor is constructed while this duty lives, so its frame cannot
    // come from an arena and must still be freed exactly once.
    Duty d = leaf();
    CHECK(static_cast<bool>(d.get()), "frame allocated");
}

static int g_dtors = 0;
struct Guard {
    ~Guard() { ++g_dtors; }
};

static Duty guarded() {
    Guard g;
    co_return co_await park();
}

static Duty guarded_child() {
    Guard g;
    co_return co_await park();
}

static Duty guarded_parent() {
    Guard g;
    co_return co_await guarded_child();
}

static Duty catch_child_fault() {
    try {
        co_await leaf();
    } catch (const std::exception&) {
    }
    co_return 9;
}

static Duty cancel_on_resume() {
    const std::int64_t v = co_await park();
    g_c->cancel(g_self);  // from inside a delivery
    co_return v;
}

static void t_cleanup() {
    std::printf("test: cancelling runs the cleanup the frames hold\n");
    {
        Conductor c{4};
        g_c = &c;
        g_dtors = 0;
        const JobId j = c.launch(guarded());
        c.cancel(j);
        CHECK(g_dtors == 1, "frame cleanup ran once");
        CHECK(c.arena().live_slots() == 0, "storage back");
    }
    {
        Conductor c{4};
        g_c = &c;
        g_dtors = 0;
        const JobId j = c.launch(guarded_parent());
        c.cancel(j);
        CHECK(g_dtors == 2, "both frames cleaned up");
        CHECK(c.arena().live_slots() == 0, "both slots back");
    }
    {
        Conductor c{4};
        g_c = &c;
        g_dtors = 0;
        const JobId j = c.launch(guarded_parent());
        c.post(j, 4);
        c.drain();
        CHECK(g_dtors == 2, "cleanup on the success path too");
        CHECK(*c.value_of(j) == 4, "value");
        CHECK(c.arena().live_slots() == 0, "clean");
    }
}

static void t_caught_child_fault() {
    std::printf("test: a child failure the parent catches\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(catch_child_fault());
    c.post_fault(j);
    c.drain();
    CHECK(c.settled(j), "settled");
    CHECK(*c.outcome_of(j) == Outcome::Delivered, "carried on normally");
    CHECK(*c.value_of(j) == 9, "value 9");
    CHECK(c.journal().count_for(Mark::FaultReported, j) == 0, "no fault reported");
    CHECK(c.journal().count_for(Mark::ChildDestroyed, j) == 1, "child destroyed");
    CHECK(c.arena().live_slots() == 0, "child storage back");
    std::printf("  trace=%s\n", trace(c.journal()).c_str());
}

static void t_cancel_during_delivery() {
    std::printf("test: cancel from inside a delivery\n");
    Conductor c{4};
    g_c = &c;
    g_self = 1;
    const JobId j = c.launch(cancel_on_resume());
    c.post(j, 3);
    c.drain();
    CHECK(c.settled(j), "settled");
    CHECK(*c.outcome_of(j) == Outcome::Cancelled, "cancelled");
    CHECK(c.value_of(j) == std::nullopt, "no value");
    CHECK(c.journal().count_for(Mark::JobSettled, j) == 1, "settled once");
    CHECK(c.arena().live_slots() == 0, "storage back");
}

static void t_fault_after() {
    std::printf("test: a delayed fault\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(leaf());
    c.post_fault_after(j, 7);
    CHECK(c.pending_tickets() == 1, "queued");
    CHECK(c.step(), "stepped");
    CHECK(c.now() == 7, "clock followed the ready time");
    CHECK(*c.outcome_of(j) == Outcome::Faulted, "faulted");
    CHECK(c.journal().count_for(Mark::FaultReported, j) == 1, "reported once");
    CHECK(c.arena().live_slots() == 0, "clean");
}

static void t_value_after_caught_fault() {
    std::printf("test: a value delivered to a frame that already caught a fault\n");
    Conductor c{4};
    g_c = &c;
    const JobId j = c.launch(two_parks());
    c.post_fault(j);  // ready 0 -> raises in the first park, job catches nothing
    c.drain();
    CHECK(*c.outcome_of(j) == Outcome::Faulted, "faulted");
    CHECK(c.arena().live_slots() == 0, "clean");
    const JobId k = c.launch(two_parks());
    c.post(k, 1);
    c.post_fault(k);
    c.post(k, 2);
    c.drain();
    CHECK(*c.outcome_of(k) == Outcome::Faulted, "the fault ended it");
    CHECK(c.arena().live_slots() == 0, "clean");
}

int main() {
    t_heap_frame();  // first: no conductor exists yet
    t_recycled_epoch();
    t_epoch_reuse();
    t_park_again_same_frame();
    t_nested_order();
    t_child_throws();
    t_thrower();
    t_catcher();
    t_cancel_parked();
    t_cancel_unknown();
    t_cancel_nested();
    t_self_cancel();
    t_cancel_other_from_job();
    t_independence();
    t_order_and_clock();
    t_post_from_inside();
    t_delay_from_now();
    t_exhaustion();
    t_shutdown();
    t_many();
    t_stubborn();
    t_post_while_running();
    t_inner_drive();
    t_child_alloc_fails();
    t_unlaunched_duty();
    t_cleanup();
    t_caught_child_fault();
    t_cancel_during_delivery();
    std::printf(g_fail == 0 ? "\nall ok\n" : "\n%d FAILURES\n", g_fail);
    return g_fail == 0 ? 0 : 1;
}
