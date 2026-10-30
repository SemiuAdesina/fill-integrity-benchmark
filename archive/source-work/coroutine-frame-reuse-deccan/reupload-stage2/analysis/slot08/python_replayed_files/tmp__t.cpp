// My own conformance harness (not part of the deliverable).
#include "roundel/runtime.hpp"

#include <cstdio>
#include <new>
#include <stdexcept>
#include <string>
#include <vector>

using namespace roundel;

static int failures = 0;
static int checks = 0;

#define CHECK(cond)                                                        \
    do {                                                                   \
        ++checks;                                                          \
        if (!(cond)) {                                                     \
            ++failures;                                                    \
            std::printf("  FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond);  \
        }                                                                  \
    } while (0)

static void check_eq(const char* what, long long got, long long want) {
    ++checks;
    if (got != want) {
        ++failures;
        std::printf("  FAIL %s: got %lld want %lld\n", what, got, want);
    }
}

static std::size_t count(const Journal& j, Mark m) { return j.count(m); }
static std::size_t count_for(const Journal& j, Mark m, JobId id) { return j.count_for(m, id); }

// -------------------------------------------------------------------------

static Duty simple() { co_return co_await park(); }

static Duty adder() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a + b;
}

static Duty child() { co_return co_await park(); }

static Duty deep() {
    const std::int64_t a = co_await child();
    const std::int64_t b = co_await child();
    co_return a * 100 + b;
}

static Duty thrower() {
    const std::int64_t v = co_await park();
    if (v < 0) {
        throw std::runtime_error("boom");
    }
    co_return v;
}

static Duty catcher() {
    std::int64_t v = 0;
    try {
        v = co_await park();
    } catch (const std::exception&) {
        v = -1;
    }
    co_return v;
}

static Duty catching_parent() {
    std::int64_t v = 0;
    try {
        v = co_await child();
    } catch (const std::exception&) {
        v = -7;
    }
    co_return v;
}

struct Order {
    std::vector<int>* log;
    int id;
    Conductor* c;
    ~Order() {
        if (log != nullptr && c != nullptr) {
            log->push_back(id * 10 + static_cast<int>(c->arena().live_slots()));
        }
    }
};

static Duty grandchild(std::vector<int>* log, Conductor* c) {
    Order guard{log, 3, c};
    co_return co_await park();
}
static Duty middle(std::vector<int>* log, Conductor* c) {
    Order guard{log, 2, c};
    co_return co_await grandchild(log, c) + 1;
}
static Duty outer(std::vector<int>* log, Conductor* c) {
    Order guard{log, 1, c};
    co_return co_await middle(log, c) + 1;
}

static Duty grabber(Conductor* c, std::vector<int>* log) {
    Order guard{log, 9, c};
    co_return co_await park();
}

static Duty two_children_then_launch(Conductor* c, std::vector<int>* log) {
    const std::int64_t first = co_await child();
    c->launch(grabber(c, log));  // takes the slot the first child just freed
    const std::int64_t second = co_await child();
    co_return first * 100 + second;
}

static Duty self_canceller(Conductor* c, const JobId* self) {
    c->cancel(*self);
    co_return 5;
}

static Duty late_self_canceller(Conductor* c, const JobId* self) {
    const std::int64_t v = co_await park();
    c->cancel(*self);
    co_return v + 1;
}

static Duty poster(Conductor* c, JobId other) {
    const std::int64_t v = co_await park();
    c->post(other, 100);
    co_return v;
}

static Duty delay_poster(Conductor* c, JobId other) {
    const std::int64_t v = co_await park();
    c->post_after(other, 55, 4);
    co_return v;
}

// -------------------------------------------------------------------------

static void t_settle_once() {
    std::printf("settles exactly once\n");
    Conductor c{4};
    const JobId a = c.launch(simple());
    c.post(a, 5);
    c.drain();
    CHECK(c.settled(a));
    check_eq("JobSettled count", (long long)count_for(c.journal(), Mark::JobSettled, a), 1);
    check_eq("FrameReleased count", (long long)count_for(c.journal(), Mark::FrameReleased, a), 1);
    check_eq("value", (long long)*c.value_of(a), 5);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("double", (long long)c.arena().double_reclaims(), 0);
}

static void t_cancel_parked() {
    std::printf("cancel while parked\n");
    Conductor c{4};
    const JobId a = c.launch(simple());
    c.post(a, 5);
    c.cancel(a);
    check_eq("outcome cancelled", (long long)*c.outcome_of(a), (long long)Outcome::Cancelled);
    check_eq("live after cancel", (long long)c.arena().live_slots(), 0);
    check_eq("FrameReleased", (long long)count_for(c.journal(), Mark::FrameReleased, a), 1);
    c.drain();
    check_eq("JobSettled still 1", (long long)count_for(c.journal(), Mark::JobSettled, a), 1);
    check_eq("settled count", (long long)count(c.journal(), Mark::JobSettled), 1);
    check_eq("rejected", (long long)count_for(c.journal(), Mark::TicketRejected, a), 1);
    check_eq("no value", (long long)c.value_of(a).has_value(), 0);
    check_eq("live", (long long)c.arena().live_slots(), 0);
}

static void t_cancel_repeated_and_unknown() {
    std::printf("cancel is idempotent and total\n");
    Conductor c{4};
    const JobId a = c.launch(simple());
    c.cancel(a);
    c.cancel(a);
    c.cancel(a);
    check_eq("CancelRequested x3", (long long)count_for(c.journal(), Mark::CancelRequested, a), 3);
    check_eq("JobSettled once", (long long)count_for(c.journal(), Mark::JobSettled, a), 1);
    c.cancel(4242);
    check_eq("CancelRequested unknown",
             (long long)count_for(c.journal(), Mark::CancelRequested, 4242), 1);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    c.drain();
    check_eq("live after drain", (long long)c.arena().live_slots(), 0);
}

static void t_unparked_ticket_never_binds() {
    std::printf("ticket made for an unparked job binds nothing\n");
    Conductor c{4};
    c.post(7, 1);  // no such job
    const JobId a = c.launch(simple());
    CHECK(a != 7);
    c.drain();
    check_eq("rejected", (long long)count_for(c.journal(), Mark::TicketRejected, 7), 1);
    check_eq("a unsettled", (long long)c.settled(a), 0);
    check_eq("live", (long long)c.arena().live_slots(), 1);
}

static void t_order() {
    std::printf("delivery order: ready, then job, then queued\n");
    Conductor c{8};
    const JobId a = c.launch(adder());
    const JobId b = c.launch(adder());
    const JobId d = c.launch(adder());
    c.post(b, 1);           // ready 0
    c.post_after(a, 2, 5);  // ready 5
    c.post(d, 3);           // ready 0
    c.post(a, 4);           // ready 0, lowest job id
    c.post(d, 5);           // ready 0, second ticket for d
    while (c.step()) {
    }
    check_eq("rejected", (long long)count(c.journal(), Mark::TicketRejected), 0);
    std::vector<std::int64_t> values;
    for (const Entry& e : c.journal().entries()) {
        if (e.mark == Mark::TicketAccepted) {
            values.push_back(e.detail);
        }
    }
    CHECK(values.size() == 5);
    if (values.size() == 5) {
        check_eq("1st", (long long)values[0], 4);
        check_eq("2nd", (long long)values[1], 1);
        check_eq("3rd", (long long)values[2], 3);
        check_eq("4th", (long long)values[3], 5);
        check_eq("5th", (long long)values[4], 2);
    }
    check_eq("clock", (long long)c.now(), 5);
}

static void t_clock() {
    std::printf("clock follows delivery\n");
    Conductor c{4};
    const JobId a = c.launch(simple());
    check_eq("start", (long long)c.now(), 0);
    c.post_after(a, 1, 3);
    c.post_after(a, 2, 9);
    c.step();
    check_eq("after 1st", (long long)c.now(), 3);
    c.step();
    check_eq("after 2nd", (long long)c.now(), 9);
    check_eq("pending", (long long)c.pending_tickets(), 0);
}

static void t_delay_from_inside() {
    std::printf("a ticket queued inside a job is never ready early\n");
    Conductor c{4};
    const JobId a = c.launch(delay_poster(&c, 2));
    const JobId b = c.launch(simple());
    check_eq("ids", (long long)b, 2);
    c.post_after(a, 1, 7);
    c.step();  // clock -> 7, a resumes and posts for b at 7+4 = 11
    check_eq("clock", (long long)c.now(), 7);
    check_eq("pending", (long long)c.pending_tickets(), 1);
    c.step();
    check_eq("clock", (long long)c.now(), 11);
    check_eq("b", (long long)*c.value_of(b), 55);
}

static void t_same_frame_repark() {
    std::printf("ticket still good when the job parks in the same frame again\n");
    Conductor c{4};
    const JobId a = c.launch(adder());
    c.post(a, 10);
    c.post(a, 32);
    c.drain();
    check_eq("sum", (long long)*c.value_of(a), 42);
    check_eq("accepted", (long long)count_for(c.journal(), Mark::TicketAccepted, a), 2);
    check_eq("rejected", (long long)count_for(c.journal(), Mark::TicketRejected, a), 0);
}

static void t_inner_first_on_cancel() {
    std::printf("inner frames go first on cancel\n");
    std::vector<int> log;
    Conductor c{4};
    const JobId a = c.launch(outer(&log, &c));
    check_eq("live", (long long)c.arena().live_slots(), 3);
    c.cancel(a);
    check_eq("live after", (long long)c.arena().live_slots(), 0);
    CHECK(log.size() == 3);
    if (log.size() == 3) {
        check_eq("first destroyed", (long long)log[0], 33);
        check_eq("second", (long long)log[1], 22);
        check_eq("third", (long long)log[2], 11);
    }
    check_eq("outcome", (long long)*c.outcome_of(a), (long long)Outcome::Cancelled);
    check_eq("ChildDestroyed", (long long)count_for(c.journal(), Mark::ChildDestroyed, a), 2);
    check_eq("FrameReleased", (long long)count_for(c.journal(), Mark::FrameReleased, a), 1);
    check_eq("JobSettled", (long long)count_for(c.journal(), Mark::JobSettled, a), 1);
}

static void t_inner_first_on_success() {
    std::printf("child gone before the parent sees the result\n");
    std::vector<int> log;
    Conductor c{4};
    const JobId a = c.launch(outer(&log, &c));
    c.post(a, 5);
    c.drain();
    check_eq("value", (long long)*c.value_of(a), 7);
    CHECK(log.size() == 3);
    if (log.size() == 3) {
        check_eq("grandchild first", (long long)log[0], 33);
        check_eq("middle", (long long)log[1], 22);
    }
    std::vector<std::int64_t> pc;
    for (const Entry& e : c.journal().entries()) {
        if (e.mark == Mark::ParentContinued) {
            pc.push_back(e.detail);
        }
    }
    CHECK(pc.size() == 2);
    if (pc.size() == 2) {
        check_eq("P live 1", (long long)pc[0], 2);
        check_eq("P live 2", (long long)pc[1], 1);
    }
    check_eq("live at end", (long long)c.arena().live_slots(), 0);
}

static void t_epoch_mismatch() {
    std::printf("same slot, new generation -> rejected\n");
    Conductor c{4};
    const JobId a = c.launch(deep());
    c.post(a, 1);
    c.post(a, 2);
    c.step();
    check_eq("live", (long long)c.arena().live_slots(), 2);  // root + second child
    c.step();
    check_eq("accepted", (long long)count_for(c.journal(), Mark::TicketAccepted, a), 1);
    check_eq("rejected", (long long)count_for(c.journal(), Mark::TicketRejected, a), 1);
    check_eq("live after", (long long)c.arena().live_slots(), 2);
    check_eq("unsettled", (long long)c.settled(a), 0);
}

static void t_index_mismatch() {
    std::printf("same generation, other slot -> rejected\n");
    std::vector<int> log;
    Conductor c{6};
    const JobId a = c.launch(two_children_then_launch(&c, &log));
    c.post(a, 1);
    c.post(a, 2);
    c.step();  // first child completes; the launched grabber takes its slot
    const std::size_t live = c.arena().live_slots();
    c.step();  // second ticket names (1,1); the job now sits in (2,1)
    std::printf("  live after first step=%zu\n", live);
    check_eq("accepted", (long long)count_for(c.journal(), Mark::TicketAccepted, a), 1);
    check_eq("rejected", (long long)count_for(c.journal(), Mark::TicketRejected, a), 1);
    check_eq("unsettled", (long long)c.settled(a), 0);
}

static void t_cancel_from_inside() {
    std::printf("cancel from inside the job\n");
    Conductor c{4};
    JobId self = 1;  // the first job on a fresh conductor
    const JobId a = c.launch(self_canceller(&c, &self));
    check_eq("id", (long long)a, 1);
    check_eq("outcome", (long long)*c.outcome_of(a), (long long)Outcome::Cancelled);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("CancelRequested", (long long)count_for(c.journal(), Mark::CancelRequested, a), 1);
    check_eq("JobSettled", (long long)count_for(c.journal(), Mark::JobSettled, a), 1);
    check_eq("FrameReleased", (long long)count_for(c.journal(), Mark::FrameReleased, a), 1);
}

static void t_cancel_from_inside_after_park() {
    std::printf("cancel from inside, after resuming\n");
    Conductor c{4};
    JobId self = 1;
    const JobId b = c.launch(late_self_canceller(&c, &self));
    c.post(b, 7);
    c.drain();
    check_eq("outcome", (long long)*c.outcome_of(b), (long long)Outcome::Cancelled);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("JobSettled", (long long)count_for(c.journal(), Mark::JobSettled, b), 1);
    check_eq("no value", (long long)c.value_of(b).has_value(), 0);
    check_eq("FrameReleased", (long long)count_for(c.journal(), Mark::FrameReleased, b), 1);
}

static void t_fault_paths() {
    std::printf("failure follows the same rules\n");
    {
        Conductor c{4};
        const JobId a = c.launch(thrower());
        c.post(a, -1);
        c.drain();
        check_eq("faulted", (long long)*c.outcome_of(a), (long long)Outcome::Faulted);
        check_eq("FaultReported", (long long)count_for(c.journal(), Mark::FaultReported, a), 1);
        check_eq("JobSettled", (long long)count_for(c.journal(), Mark::JobSettled, a), 1);
        check_eq("live", (long long)c.arena().live_slots(), 0);
    }
    {
        Conductor c{4};
        const JobId a = c.launch(thrower());
        c.post_fault(a);
        c.drain();
        check_eq("faulted", (long long)*c.outcome_of(a), (long long)Outcome::Faulted);
        check_eq("FaultReported", (long long)count_for(c.journal(), Mark::FaultReported, a), 1);
        check_eq("JobSettled", (long long)count_for(c.journal(), Mark::JobSettled, a), 1);
        check_eq("live", (long long)c.arena().live_slots(), 0);
    }
    {
        Conductor c{4};
        const JobId a = c.launch(thrower());
        c.post_fault_after(a, 3);
        c.drain();
        check_eq("faulted", (long long)*c.outcome_of(a), (long long)Outcome::Faulted);
        check_eq("clock", (long long)c.now(), 3);
        check_eq("live", (long long)c.arena().live_slots(), 0);
    }
    {
        Conductor c{4};
        const JobId a = c.launch(catcher());
        c.post_fault(a);
        c.drain();
        check_eq("caught", (long long)*c.value_of(a), -1);
        check_eq("FaultReported", (long long)count_for(c.journal(), Mark::FaultReported, a), 0);
        check_eq("live", (long long)c.arena().live_slots(), 0);
    }
    {
        Conductor c{4};
        const JobId a = c.launch(catching_parent());
        c.post_fault(a);
        c.drain();
        check_eq("parent caught", (long long)*c.value_of(a), -7);
        check_eq("FaultReported", (long long)count_for(c.journal(), Mark::FaultReported, a), 0);
        check_eq("ChildDestroyed", (long long)count_for(c.journal(), Mark::ChildDestroyed, a), 1);
        check_eq("live", (long long)c.arena().live_slots(), 0);
    }
    {
        // a cancelled job never faults, whatever was queued for it
        Conductor c{4};
        const JobId a = c.launch(thrower());
        c.post_fault(a);
        c.cancel(a);
        c.drain();
        check_eq("cancelled", (long long)*c.outcome_of(a), (long long)Outcome::Cancelled);
        check_eq("FaultReported", (long long)count_for(c.journal(), Mark::FaultReported, a), 0);
        check_eq("live", (long long)c.arena().live_slots(), 0);
    }
}

static void t_independence() {
    std::printf("jobs are independent\n");
    Conductor c{4};
    const JobId a = c.launch(simple());
    const JobId b = c.launch(simple());
    const JobId d = c.launch(simple());
    c.post(a, 1);
    c.post(b, 2);
    c.post(d, 3);
    c.cancel(b);
    c.drain();
    check_eq("a", (long long)*c.value_of(a), 1);
    check_eq("b cancelled", (long long)*c.outcome_of(b), (long long)Outcome::Cancelled);
    check_eq("d", (long long)*c.value_of(d), 3);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("live jobs", (long long)c.live_jobs(), 0);
}

static void t_exhaustion() {
    std::printf("arena exhaustion is reported\n");
    Conductor c{2};
    const JobId a = c.launch(simple());
    const JobId b = c.launch(simple());
    bool threw = false;
    try {
        const JobId d = c.launch(simple());
        (void)d;
    } catch (const std::bad_alloc&) {
        threw = true;
    }
    CHECK(threw);
    check_eq("peak", (long long)c.arena().peak_live(), 2);
    c.cancel(a);
    c.cancel(b);
    check_eq("free", (long long)c.arena().free_slots(), 2);
    const JobId d = c.launch(simple());
    check_eq("checkouts", (long long)c.arena().total_check_outs(), 3);
    c.post(d, 4);
    c.drain();
    check_eq("value", (long long)*c.value_of(d), 4);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("double", (long long)c.arena().double_reclaims(), 0);
}

static void t_one_per_step() {
    std::printf("one delivery per step, nothing runs unstepped\n");
    Conductor c{4};
    const JobId a = c.launch(simple());
    const JobId b = c.launch(simple());
    c.post(a, 1);
    c.post(b, 2);
    check_eq("pending", (long long)c.pending_tickets(), 2);
    check_eq("a running", (long long)c.settled(a), 0);
    CHECK(c.step());
    check_eq("pending", (long long)c.pending_tickets(), 1);
    CHECK(c.step());
    check_eq("pending", (long long)c.pending_tickets(), 0);
    CHECK(!c.step());
    check_eq("live", (long long)c.arena().live_slots(), 0);
}

static void t_schedule_from_inside() {
    std::printf("scheduling from inside a running job\n");
    Conductor c{4};
    const JobId a = c.launch(poster(&c, 2));
    const JobId b = c.launch(simple());
    check_eq("ids", (long long)b, 2);
    c.post(a, 1);
    c.drain();
    check_eq("a", (long long)*c.value_of(a), 1);
    check_eq("b", (long long)*c.value_of(b), 100);
    check_eq("clock", (long long)c.now(), 0);
    check_eq("live", (long long)c.arena().live_slots(), 0);
}

static void t_shutdown_releases() {
    std::printf("parked jobs release at shutdown\n");
    Conductor c{4};
    const JobId a = c.launch(outer(nullptr, &c));
    check_eq("live", (long long)c.arena().live_slots(), 3);
    (void)a;
}

static void t_many_jobs_reuse() {
    std::printf("slots are recycled, not accumulated\n");
    Conductor c{2};
    for (int i = 0; i < 50; ++i) {
        const JobId a = c.launch(simple());
        c.post(a, i);
        c.drain();
        check_eq("value", (long long)*c.value_of(a), i);
        check_eq("live", (long long)c.arena().live_slots(), 0);
    }
    check_eq("peak", (long long)c.arena().peak_live(), 1);
    check_eq("double", (long long)c.arena().double_reclaims(), 0);
}

int main() {
    t_settle_once();
    t_cancel_parked();
    t_cancel_repeated_and_unknown();
    t_unparked_ticket_never_binds();
    t_order();
    t_clock();
    t_delay_from_inside();
    t_same_frame_repark();
    t_inner_first_on_cancel();
    t_inner_first_on_success();
    t_epoch_mismatch();
    t_index_mismatch();
    t_cancel_from_inside();
    t_cancel_from_inside_after_park();
    t_fault_paths();
    t_independence();
    t_exhaustion();
    t_one_per_step();
    t_schedule_from_inside();
    t_shutdown_releases();
    t_many_jobs_reuse();
    std::printf("\n%d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}
