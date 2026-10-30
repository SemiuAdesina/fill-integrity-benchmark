//
// Lifecycle: settling once, cancellation, failure, and storage coming back.

#include <stdexcept>

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

// Keeps going after anything it is handed at the park. A cancelled job must
// never get to run this body again.
Duty keeps_going(std::vector<std::int64_t>* got) {
    for (int i = 0; i < 3; ++i) {
        try {
            got->push_back(co_await park());
        } catch (const std::exception&) {
            got->push_back(-1);
        }
    }
    co_return static_cast<std::int64_t>(got->size());
}

Duty throws_after_park(int tag) {
    diag::Probe p{tag};
    (void)p;
    const std::int64_t v = co_await park();
    if (v >= 0) {
        throw std::runtime_error("job body failed");
    }
    co_return v;
}

Duty handles_posted_fault(std::vector<int>* seen) {
    try {
        const std::int64_t v = co_await park();
        seen->push_back(1);
        co_return v;
    } catch (const std::exception&) {
        seen->push_back(3);
        co_return -3;
    }
}

void cancel_while_parked() {
    diag::reset_probes();
    std::vector<std::int64_t> got;
    Conductor c(4);
    const JobId job = c.launch(keeps_going(&got));
    c.cancel(job);
    diag::check(c.outcome_of(job) == Outcome::Cancelled, "a job cancelled while parked settles as cancelled",
                diag::outcome_text(c, job));
    diag::check(got.empty(), "a cancelled job's body does not run again", "body ran " + diag::num(static_cast<long long>(got.size())) + " more time(s)");
    diag::check(c.arena().live_slots() == 0, "a cancelled job's storage is back in the pool at once",
                "live_slots " + diag::num(static_cast<long long>(c.arena().live_slots())));
    diag::check(c.journal().count_for(Mark::FaultReported, job) == 0, "cancellation is not announced as a failure",
                diag::num(static_cast<long long>(c.journal().count_for(Mark::FaultReported, job))));
}

void cancellation_runs_cleanup() {
    diag::reset_probes();
    Conductor c(4);
    const JobId job = c.launch(parks_probed(11));
    c.cancel(job);
    diag::check(diag::probes_alive() == 0, "cancelling destroys what the frame holds",
                "probes alive " + diag::num(diag::probes_alive()));
    diag::check(c.journal().count_for(Mark::FrameReleased, job) == 1, "the release is recorded once",
                diag::num(static_cast<long long>(c.journal().count_for(Mark::FrameReleased, job))));
}

void settle_once_whatever_arrives() {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    c.post(job, 5);
    c.step();
    c.cancel(job);
    c.post(job, 6);
    c.step();
    diag::check(c.journal().count_for(Mark::JobSettled, job) == 1, "a job settles exactly once",
                diag::num(static_cast<long long>(c.journal().count_for(Mark::JobSettled, job))));
    diag::check(c.value_of(job).has_value() && *c.value_of(job) == 5, "the first outcome stands",
                c.value_of(job) ? diag::num(*c.value_of(job)) : "none");
    diag::check(c.journal().count_for(Mark::FrameReleased, job) == 1, "storage is released exactly once",
                diag::num(static_cast<long long>(c.journal().count_for(Mark::FrameReleased, job))));
    diag::check(c.arena().double_reclaims() == 0, "no slot is handed back twice",
                diag::num(static_cast<long long>(c.arena().double_reclaims())));
}

void repeated_and_unknown_cancellation() {
    Conductor c(4);
    const JobId job = c.launch(parks_once());
    c.cancel(job);
    c.cancel(job);
    c.cancel(job + 500);
    diag::check(c.journal().count_for(Mark::CancelRequested, job) == 2, "every cancel call is recorded, including one for a settled job",
                diag::num(static_cast<long long>(c.journal().count_for(Mark::CancelRequested, job))));
    diag::check(c.journal().count_for(Mark::CancelRequested, job + 500) == 1, "a cancel naming an unknown identifier is recorded too",
                diag::num(static_cast<long long>(c.journal().count_for(Mark::CancelRequested, job + 500))));
    diag::check(c.journal().count_for(Mark::JobSettled, job) == 1, "repeated cancellation settles once",
                diag::num(static_cast<long long>(c.journal().count_for(Mark::JobSettled, job))));
    diag::check(c.live_jobs() == 0 && c.arena().double_reclaims() == 0, "and disturbs nothing else");
}

void failure_settles_once_and_releases() {
    diag::reset_probes();
    Conductor c(4);
    const JobId job = c.launch(throws_after_park(6));
    c.post(job, 1);
    c.step();
    diag::check(c.outcome_of(job) == Outcome::Faulted, "an exception escaping the body settles the job as faulted",
                diag::outcome_text(c, job));
    diag::check(c.journal().count_for(Mark::FaultReported, job) == 1, "the failure is announced exactly once",
                diag::num(static_cast<long long>(c.journal().count_for(Mark::FaultReported, job))));
    diag::check(diag::probes_alive() == 0 && c.arena().live_slots() == 0, "a failed job's storage comes back like any other",
                "probes alive " + diag::num(diag::probes_alive()) + ", live_slots " + diag::num(static_cast<long long>(c.arena().live_slots())));
    c.cancel(job);
    diag::check(c.journal().count_for(Mark::JobSettled, job) == 1 && c.outcome_of(job) == Outcome::Faulted,
                "a cancellation after the failure changes nothing");
}

void a_job_may_handle_a_posted_fault() {
    std::vector<int> seen;
    Conductor c(4);
    const JobId job = c.launch(handles_posted_fault(&seen));
    c.post_fault(job);
    c.step();
    diag::check(seen.size() == 1 && seen[0] == 3, "a failing ticket raises where the job is parked");
    diag::check(c.outcome_of(job) == Outcome::Delivered, "a job that catches it carries on and delivers",
                diag::outcome_text(c, job));
    diag::check(c.journal().count_for(Mark::FaultReported, job) == 0, "a caught failure is not announced",
                diag::num(static_cast<long long>(c.journal().count_for(Mark::FaultReported, job))));
}

}  // namespace

int main() {
    std::printf("== lifecycle ==\n");
    diag::scenario("cancel_while_parked", cancel_while_parked);
    diag::scenario("cancellation_runs_cleanup", cancellation_runs_cleanup);
    diag::scenario("settle_once_whatever_arrives", settle_once_whatever_arrives);
    diag::scenario("repeated_and_unknown_cancellation", repeated_and_unknown_cancellation);
    diag::scenario("failure_settles_once_and_releases", failure_settles_once_and_releases);
    diag::scenario("a_job_may_handle_a_posted_fault", a_job_may_handle_a_posted_fault);
    return diag::finish("lifecycle");
}
