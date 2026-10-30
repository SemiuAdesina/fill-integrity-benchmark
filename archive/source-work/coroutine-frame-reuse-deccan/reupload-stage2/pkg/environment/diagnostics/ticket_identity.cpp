//
// Ticket identity: a ticket reaches the one frame it was made for, or nobody.

#include "common.hpp"

using namespace roundel;

namespace {

Duty parks_once() {
    const std::int64_t v = co_await park();
    co_return v;
}

Duty parks_probed(int tag) {
    diag::Probe p{tag};
    (void)p;
    const std::int64_t v = co_await park();
    co_return v;
}

Duty child_parks(int tag) {
    diag::Probe p{tag};
    (void)p;
    const std::int64_t v = co_await park();
    co_return v;
}

// Parks in its own frame, then parks again inside a child's frame.
Duty parks_then_awaits_parking_child(int ctag) {
    const std::int64_t v = co_await park();
    const std::int64_t w = co_await child_parks(ctag);
    co_return v + w;
}

// Queues a ticket for itself while it is running, then parks again.
Duty posts_for_self_then_parks(Conductor* c, const JobId* self) {
    const std::int64_t v = co_await park();
    c->post(*self, 9);
    const std::int64_t w = co_await park();
    co_return v + w;
}

void stale_ticket_after_reuse() {
    diag::reset_probes();
    Conductor c(1);
    const JobId a = c.launch(parks_once());
    c.post(a, 500);
    c.cancel(a);
    const JobId b = c.launch(parks_probed(3));  // takes over a's storage
    c.step();                                   // a's ticket comes due
    diag::check(c.journal().count_for(Mark::TicketRejected, a) == 1, "a ticket for a job that has settled is refused",
                "rejected " + diag::num(static_cast<long long>(c.journal().count_for(Mark::TicketRejected, a))) +
                    ", accepted " + diag::num(static_cast<long long>(c.journal().count_for(Mark::TicketAccepted, a))));
    diag::check(!c.settled(b), "the job now living in that storage is not resumed by it", diag::outcome_text(c, b));
    diag::check(diag::probes_alive() == 1 && c.arena().live_slots() == 1, "and is not destroyed by it either",
                "probes alive " + diag::num(diag::probes_alive()) + ", live_slots " + diag::num(static_cast<long long>(c.arena().live_slots())));
}

void valid_ticket_survives_unrelated_reuse() {
    diag::reset_probes();
    Conductor c(2);
    const JobId keeper = c.launch(parks_once());
    c.post_after(keeper, 42, 4);
    for (int round = 0; round < 3; ++round) {
        const JobId churn = c.launch(parks_probed(round));
        c.cancel(churn);
    }
    c.step();
    diag::check(c.outcome_of(keeper) == Outcome::Delivered && c.value_of(keeper).has_value() && *c.value_of(keeper) == 42,
                "a ticket for a job still parked in the frame it names is delivered however much the pool churned",
                std::string(diag::outcome_text(c, keeper)) + ", rejected " +
                    diag::num(static_cast<long long>(c.journal().count_for(Mark::TicketRejected, keeper))));
}

void ticket_for_outer_frame_while_inner_parked() {
    diag::reset_probes();
    Conductor c(4);
    const JobId job = c.launch(parks_then_awaits_parking_child(11));
    c.post_after(job, 100, 5);  // made while the root frame was the one parked
    c.post(job, 1);             // likewise, delivered first
    c.step();                   // the root resumes and descends into a parking child
    c.step();                   // the older ticket, now that the child is the frame waiting
    diag::check(c.journal().count_for(Mark::TicketRejected, job) == 1,
                "a ticket made for one of the job's frames is refused while the job waits in a different frame of its own",
                "rejected " + diag::num(static_cast<long long>(c.journal().count_for(Mark::TicketRejected, job))) +
                    ", accepted " + diag::num(static_cast<long long>(c.journal().count_for(Mark::TicketAccepted, job))));
    diag::check(!c.settled(job) && diag::probes_alive() == 1, "the waiting child frame is untouched",
                std::string(diag::outcome_text(c, job)) + ", probes alive " + diag::num(diag::probes_alive()));
}

void ticket_queued_while_running_never_binds() {
    Conductor c(4);
    JobId self = kNoJob;
    self = c.launch(posts_for_self_then_parks(&c, &self));
    c.post(self, 4);
    c.step();  // resumes; queues a ticket for itself while running; parks again
    c.step();  // that ticket comes due
    diag::check(c.journal().count_for(Mark::TicketRejected, self) == 1 && !c.settled(self),
                "a ticket queued while its job was not parked is never delivered, even once the job parks again",
                "rejected " + diag::num(static_cast<long long>(c.journal().count_for(Mark::TicketRejected, self))) + ", " + diag::outcome_text(c, self));
    c.post(self, 7);
    c.step();
    diag::check(c.outcome_of(self) == Outcome::Delivered && c.value_of(self).has_value() && *c.value_of(self) == 11,
                "a ticket queued while the job is parked in that frame still arrives", diag::outcome_text(c, self));
}

}  // namespace

int main() {
    std::printf("== ticket_identity ==\n");
    diag::scenario("stale_ticket_after_reuse", stale_ticket_after_reuse);
    diag::scenario("valid_ticket_survives_unrelated_reuse", valid_ticket_survives_unrelated_reuse);
    diag::scenario("ticket_for_outer_frame_while_inner_parked", ticket_for_outer_frame_while_inner_parked);
    diag::scenario("ticket_queued_while_running_never_binds", ticket_queued_while_running_never_binds);
    return diag::finish("ticket_identity");
}
