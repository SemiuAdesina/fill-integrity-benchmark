//
// Smoke: the plain path through the runtime. Launch, park, queue, step, settle.

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

void launch_parks_and_waits() {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    diag::check(job != kNoJob, "launch returns an identifier");
    diag::check(!c.settled(job), "a launched job is not settled before any ticket is delivered",
                diag::outcome_text(c, job));
    diag::check(c.live_jobs() == 1, "one launched job counts as one live job", diag::num(static_cast<long long>(c.live_jobs())));
    diag::check(c.arena().live_slots() == 1, "one parked root job holds exactly one slot",
                "live_slots " + diag::num(static_cast<long long>(c.arena().live_slots())));
    diag::check(c.journal().has(Mark::JobParked, job), "parking is recorded");
}

void delivery_settles_with_value() {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    c.post(job, 41);
    diag::check(!c.settled(job), "queueing a ticket does not deliver it", diag::outcome_text(c, job));
    diag::check(c.step(), "step delivers a queued ticket");
    diag::check(c.outcome_of(job) == Outcome::Delivered, "a job that returns settles as delivered",
                diag::outcome_text(c, job));
    diag::check(c.value_of(job).has_value() && *c.value_of(job) == 41, "the delivered value is the ticket's value",
                c.value_of(job) ? diag::num(*c.value_of(job)) : "none");
    diag::check(c.live_jobs() == 0, "a settled job is no longer live", diag::num(static_cast<long long>(c.live_jobs())));
}

void one_ticket_per_step() {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    c.post(a, 1);
    c.post(b, 2);
    c.step();
    diag::check(c.pending_tickets() == 1, "a step delivers one ticket and leaves the rest queued",
                "pending " + diag::num(static_cast<long long>(c.pending_tickets())));
    diag::check(c.settled(a) && !c.settled(b), "the other job is untouched by that step");
    c.step();
    diag::check(!c.step(), "a step with nothing queued reports that");
}

void storage_returns_when_the_job_settles() {
    diag::reset_probes();
    Conductor c(4);
    const JobId job = c.launch(parks_probed(1));
    c.post(job, 3);
    c.step();
    diag::check(diag::probes_alive() == 0, "objects living in a settled job's frame have been destroyed",
                "probes alive " + diag::num(diag::probes_alive()));
    diag::check(c.arena().live_slots() == 0, "a settled job's slot is back in the pool",
                "live_slots " + diag::num(static_cast<long long>(c.arena().live_slots())));
    diag::check(c.journal().count_for(Mark::FrameReleased, job) == 1, "the release is recorded once",
                diag::num(static_cast<long long>(c.journal().count_for(Mark::FrameReleased, job))));
}

void identifiers_increase() {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    const JobId d = c.launch(parks_once());
    diag::check(a < b && b < d, "job identifiers are handed out strictly increasing");
}

}  // namespace

int main() {
    std::printf("== runtime_smoke ==\n");
    diag::scenario("launch_parks_and_waits", launch_parks_and_waits);
    diag::scenario("delivery_settles_with_value", delivery_settles_with_value);
    diag::scenario("one_ticket_per_step", one_ticket_per_step);
    diag::scenario("storage_returns_when_the_job_settles", storage_returns_when_the_job_settles);
    diag::scenario("identifiers_increase", identifiers_increase);
    return diag::finish("runtime_smoke");
}
