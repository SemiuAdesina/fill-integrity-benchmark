// Second harness: edge cases.
#include "roundel/runtime.hpp"

#include <cstdio>
#include <new>
#include <stdexcept>
#include <string>
#include <vector>

using namespace roundel;

static int failures = 0;
static int checks = 0;
#define CHECK(cond)                                                       \
    do {                                                                  \
        ++checks;                                                         \
        if (!(cond)) {                                                    \
            ++failures;                                                   \
            std::printf("  FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond); \
        }                                                                 \
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

static Duty simple() { co_return co_await park(); }
static Duty child() { co_return co_await park(); }

static Duty triple() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    const std::int64_t c = co_await park();
    co_return a * 10000 + b * 100 + c;
}

static Duty grandchild() { co_return co_await park(); }
static Duty middle() { co_return co_await grandchild() + 1; }
static Duty outer() { co_return co_await middle() + 1; }

static Duty killer(Conductor* c, JobId victim) {
    const std::int64_t v = co_await park();
    c->cancel(victim);
    co_return v;
}

static Duty stepper(Conductor* c) {
    const std::int64_t v = co_await park();
    c->step();  // reentrant stepping
    co_return v;
}

static Duty catcher_child_then_park() {
    std::int64_t v = 0;
    try {
        v = co_await child();
    } catch (const std::exception&) {
        v = -1;
    }
    const std::int64_t w = co_await park();
    co_return v * 1000 + w;
}

// -------------------------------------------------------------------------

static void t_empty_duty() {
    std::printf("empty duty\n");
    Conductor c{4};
    const JobId a = c.launch(Duty{});
    check_eq("no job", (long long)a, (long long)kNoJob);
    check_eq("live", (long long)c.arena().live_slots(), 0);
}

static void t_exhaustion_inside_job() {
    std::printf("arena exhaustion inside a job\n");
    Conductor c{2};
    const JobId a = c.launch(outer());  // root + middle = 2 slots; grandchild fails
    check_eq("outcome", (long long)*c.outcome_of(a), (long long)Outcome::Faulted);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("JobSettled", (long long)count_for(c.journal(), Mark::JobSettled, a), 1);
    check_eq("FaultReported", (long long)count_for(c.journal(), Mark::FaultReported, a), 1);
    check_eq("double", (long long)c.arena().double_reclaims(), 0);
}

static void t_cancel_other_from_inside() {
    std::printf("cancel another job from inside a job\n");
    Conductor c{4};
    const JobId victim = c.launch(simple());
    const JobId a = c.launch(killer(&c, victim));
    c.post_after(victim, 9, 5);  // later, so the killer runs first
    c.post(a, 1);
    c.drain();
    check_eq("victim", (long long)*c.outcome_of(victim), (long long)Outcome::Cancelled);
    check_eq("killer", (long long)*c.value_of(a), 1);
    check_eq("victim rejected", (long long)count_for(c.journal(), Mark::TicketRejected, victim), 1);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("JobSettled victim", (long long)count_for(c.journal(), Mark::JobSettled, victim), 1);
}

static void t_reentrant_step() {
    std::printf("reentrant step from inside a job\n");
    Conductor c{4};
    const JobId a = c.launch(stepper(&c));  // id 1, so it goes first
    const JobId b = c.launch(simple());
    c.post(a, 1);
    c.post(b, 2);
    c.step();  // a resumes and steps again from the inside
    check_eq("a", (long long)*c.value_of(a), 1);
    check_eq("b", (long long)*c.value_of(b), 2);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("pending", (long long)c.pending_tickets(), 0);
}

static void t_three_parks() {
    std::printf("three tickets queued up front, one frame\n");
    Conductor c{4};
    const JobId a = c.launch(triple());
    c.post(a, 1);
    c.post(a, 2);
    c.post(a, 3);
    c.drain();
    check_eq("value", (long long)*c.value_of(a), 10203);
    check_eq("accepted", (long long)count_for(c.journal(), Mark::TicketAccepted, a), 3);
    check_eq("rejected", (long long)count_for(c.journal(), Mark::TicketRejected, a), 0);
}

static void t_child_fault_caught_then_park() {
    std::printf("child fault caught, then park again\n");
    Conductor c{4};
    const JobId a = c.launch(catcher_child_then_park());
    c.post_fault(a);  // child faults
    c.post(a, 42);    // queued while parked in the child -> rejected later
    c.step();
    c.post(a, 7);  // queued while parked in the root
    c.drain();
    check_eq("value", (long long)*c.value_of(a), -1 * 1000 + 7);
    check_eq("rejected", (long long)count_for(c.journal(), Mark::TicketRejected, a), 1);
    check_eq("accepted", (long long)count_for(c.journal(), Mark::TicketAccepted, a), 2);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("FaultReported", (long long)count_for(c.journal(), Mark::FaultReported, a), 0);
}

static void t_step_idle() {
    std::printf("step with an empty queue does nothing\n");
    Conductor c{4};
    const JobId a = c.launch(simple());
    CHECK(!c.step());
    check_eq("clock", (long long)c.now(), 0);
    check_eq("live", (long long)c.arena().live_slots(), 1);
    check_eq("live jobs", (long long)c.live_jobs(), 1);
    check_eq("unsettled", (long long)c.settled(a), 0);
    check_eq("no outcome", (long long)c.outcome_of(a).has_value(), 0);
}

static void t_unknown_job_queries() {
    std::printf("queries about unknown jobs\n");
    Conductor c{2};
    check_eq("outcome", (long long)c.outcome_of(99).has_value(), 0);
    check_eq("value", (long long)c.value_of(99).has_value(), 0);
    check_eq("settled", (long long)c.settled(99), 0);
    check_eq("live jobs", (long long)c.live_jobs(), 0);
}

static void t_nested_conductors() {
    std::printf("nested conductors\n");
    Conductor outer_c{4};
    const JobId a = outer_c.launch(simple());
    {
        Conductor inner{2};
        const JobId b = inner.launch(simple());
        inner.post(b, 3);
        inner.drain();
        check_eq("inner value", (long long)*inner.value_of(b), 3);
        check_eq("inner live", (long long)inner.arena().live_slots(), 0);
    }
    check_eq("outer arena restored", (long long)(Conductor::active_arena() == &outer_c.arena()), 1);
    outer_c.post(a, 4);
    outer_c.drain();
    check_eq("outer value", (long long)*outer_c.value_of(a), 4);
    check_eq("outer live", (long long)outer_c.arena().live_slots(), 0);
}

static void t_cancel_child_parked() {
    std::printf("cancel a job parked in a deep child\n");
    Conductor c{4};
    const JobId a = c.launch(outer());
    check_eq("live", (long long)c.arena().live_slots(), 3);
    c.post(a, 1);
    c.cancel(a);
    check_eq("live after", (long long)c.arena().live_slots(), 0);
    check_eq("outcome", (long long)*c.outcome_of(a), (long long)Outcome::Cancelled);
    c.drain();
    check_eq("rejected", (long long)count_for(c.journal(), Mark::TicketRejected, a), 1);
    check_eq("JobSettled", (long long)count_for(c.journal(), Mark::JobSettled, a), 1);
    check_eq("live", (long long)c.arena().live_slots(), 0);
}

static void t_cancel_before_waiting() {
    std::printf("cancel before the job starts waiting\n");
    Conductor c{4};
    // cancel for an id that was never launched, then launch and cancel
    c.cancel(1);
    const JobId a = c.launch(simple());
    check_eq("id", (long long)a, 1);
    check_eq("cancel recorded", (long long)count_for(c.journal(), Mark::CancelRequested, 1), 1);
    c.cancel(a);
    check_eq("cancel recorded x2", (long long)count_for(c.journal(), Mark::CancelRequested, 1), 2);
    check_eq("outcome", (long long)*c.outcome_of(a), (long long)Outcome::Cancelled);
    check_eq("live", (long long)c.arena().live_slots(), 0);
}

static void t_many_tickets_same_slot() {
    std::printf("many tickets bound to one recycled slot\n");
    Conductor c{1};
    for (int i = 0; i < 20; ++i) {
        const JobId a = c.launch(simple());
        c.post(a, i);  // bound to slot 0, epoch i+1
        if (i % 2 == 0) {
            c.cancel(a);  // settles; the ticket is refused later
        } else {
            c.post_after(a, 1000 + i, 2);  // bound too, but never reached
        }
        c.step();
    }
    c.drain();
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("double", (long long)c.arena().double_reclaims(), 0);
    check_eq("peak", (long long)c.arena().peak_live(), 1);
    std::size_t settled = 0;
    for (const Entry& e : c.journal().entries()) {
        if (e.mark == Mark::JobSettled) {
            ++settled;
        }
    }
    check_eq("each job settled once", (long long)settled, 20);
}

int main() {
    t_empty_duty();
    t_exhaustion_inside_job();
    t_cancel_other_from_inside();
    t_reentrant_step();
    t_three_parks();
    t_child_fault_caught_then_park();
    t_step_idle();
    t_unknown_job_queries();
    t_nested_conductors();
    t_cancel_child_parked();
    t_cancel_before_waiting();
    t_many_tickets_same_slot();
    std::printf("\n%d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}
