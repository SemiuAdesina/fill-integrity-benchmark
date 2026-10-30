// Scheduling from inside a running job. Not part of the deliverable.
#include "trace.hpp"

#include <stdexcept>
#include <vector>

namespace {

Duty simple() { co_return co_await park(); }
Duty child() { co_return co_await park(); }

Duty parent_one() {
    const std::int64_t v = co_await child();
    co_return v * 2;
}

// Launches another job from inside itself and posts to it.
Duty launcher(Conductor* c, JobId* made) {
    co_await park();
    *made = c->launch(simple());
    c->post(*made, 9);
    co_return static_cast<std::int64_t>(*made);
}

// Cancels another job from inside itself.
Duty canceller(Conductor* c, JobId victim) {
    co_await park();
    c->cancel(victim);
    co_return 1;
}

// Steps the conductor from inside itself.
Duty stepper(Conductor* c) {
    co_await park();
    c->step();
    co_return 2;
}

void t_launch_from_inside() {
    Conductor c{8};
    JobId made = kNoJob;
    const JobId a = c.launch(launcher(&c, &made));
    c.post(a, 1);
    check(c.step(), "resume a");
    check(made != kNoJob && made != a, "a job was made inside a job");
    check(c.settled(a), "a delivered");
    check_eq((long long)*c.value_of(a), (long long)made, "returned the new id");
    check(!c.settled(made), "the new job is parked, not run yet");
    check_eq(c.pending_tickets(), 1, "its ticket is queued");
    check(c.step(), "deliver to the new job");
    check_eq((long long)*c.value_of(made), 9, "new job value");
    check_eq((long long)c.arena().live_slots(), 0, "all back");
    audit_journal(c, "launch inside");
}

void t_cancel_other_from_inside() {
    Conductor c{8};
    const JobId victim = c.launch(parent_one());
    const JobId a = c.launch(canceller(&c, victim));
    check_eq((long long)c.arena().live_slots(), 3, "parent+child and a");
    c.post(a, 1);
    c.step();
    check(outcome_str(c.outcome_of(victim)) == "X", "victim cancelled from inside");
    check_eq((long long)c.arena().live_slots(), 0, "victim's two frames back at once");
    check(outcome_str(c.outcome_of(a)) == "D", "the canceller carried on");
    audit_journal(c, "cancel inside");
}

void t_step_from_inside() {
    Conductor c{8};
    const JobId other = c.launch(simple());
    const JobId a = c.launch(stepper(&c));
    c.post_after(other, 4, 1);  // later, so the stepper's ticket is first
    c.post(a, 0);
    check(c.step(), "resume the stepper");
    check(c.settled(a), "stepper delivered");
    check_eq((long long)*c.value_of(a), 2, "stepper value");
    // The inner step delivered the other job's ticket.
    check(c.settled(other), "the other job finished");
    check_eq((long long)*c.value_of(other), 4, "other value");
    check_eq((long long)c.now(), 1, "clock at the inner delivery");
    check_eq(c.pending_tickets(), 0, "queue drained by the inner step");
    check_eq((long long)c.arena().live_slots(), 0, "all back");
    audit_journal(c, "step inside");
}

}  // namespace

int main() {
    t_launch_from_inside();
    t_cancel_other_from_inside();
    t_step_from_inside();
    std::printf("%d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
