//
// Scheduler and clock: which ticket a step delivers, and what now() reads.

#include "common.hpp"

using namespace roundel;

namespace {

Duty parks_once() {
    const std::int64_t v = co_await park();
    co_return v;
}

Duty parks_twice(std::vector<std::int64_t>* got) {
    got->push_back(co_await park());
    got->push_back(co_await park());
    co_return static_cast<std::int64_t>(got->size());
}

// Queues delayed work for another job from inside a resumed job.
Duty posts_for_other(Conductor* c, const JobId* other, std::uint64_t delay) {
    const std::int64_t v = co_await park();
    c->post_after(*other, v + 1, delay);
    co_return v;
}

std::vector<JobId> accepted_order(const Journal& j) {
    std::vector<JobId> out;
    for (const Entry& e : j.entries()) {
        if (e.mark == Mark::TicketAccepted) {
            out.push_back(e.job);
        }
    }
    return out;
}

std::string show(const std::vector<JobId>& v) {
    std::string s;
    for (JobId id : v) {
        if (!s.empty()) {
            s += ",";
        }
        s += diag::num(static_cast<long long>(id));
    }
    return s.empty() ? "none" : s;
}

void ready_time_comes_first() {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    c.post_after(a, 10, 5);  // queued first, ready later
    c.post(b, 20);           // queued second, ready now
    c.step();
    diag::check(c.settled(b) && !c.settled(a), "the ticket with the lowest ready time is delivered first, whatever was queued first",
                "delivered " + show(accepted_order(c.journal())));
    diag::check(c.now() == 0, "delivering a ticket that is already ready leaves the clock where it is",
                "now " + diag::num(static_cast<long long>(c.now())));
    c.step();
    diag::check(c.now() == 5, "delivering a delayed ticket moves the clock to its ready time",
                "now " + diag::num(static_cast<long long>(c.now())));
}

void equal_time_then_lowest_identifier() {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    const JobId d = c.launch(parks_once());
    c.post_after(d, 3, 2);
    c.post_after(b, 2, 2);
    c.post_after(a, 1, 2);
    c.drain();
    const std::vector<JobId> got = accepted_order(c.journal());
    diag::check(got.size() == 3 && got[0] == a && got[1] == b && got[2] == d,
                "tickets sharing a ready time go by lowest job identifier", "delivered " + show(got));
}

void same_job_same_time_keeps_queue_order() {
    std::vector<std::int64_t> got;
    Conductor c(4);
    const JobId job = c.launch(parks_twice(&got));
    c.post(job, 7);
    c.post(job, 8);
    c.drain();
    diag::check(got.size() == 2 && got[0] == 7 && got[1] == 8,
                "two tickets for one job at one time arrive in the order they were queued, and both arrive",
                got.size() == 2 ? diag::num(got[0]) + " then " + diag::num(got[1]) : "delivered " + diag::num(static_cast<long long>(got.size())));
}

void clock_never_moves_on_its_own() {
    Conductor c(4);
    const JobId a = c.launch(parks_once());
    const JobId b = c.launch(parks_once());
    c.post_after(a, 1, 7);
    c.post_after(b, 2, 2);
    c.step();
    const std::uint64_t after_first = c.now();
    c.step();
    const std::uint64_t after_second = c.now();
    diag::check(after_first == 2 && after_second == 7, "the clock reads the ready time of what was delivered",
                "now " + diag::num(static_cast<long long>(after_first)) + " then " + diag::num(static_cast<long long>(after_second)));
    const JobId d = c.launch(parks_once());
    c.post(d, 3);
    c.step();
    c.step();
    diag::check(c.now() == 7, "a ticket ready now, and a step with nothing to deliver, leave the clock where it is",
                "now " + diag::num(static_cast<long long>(c.now())));
}

void delay_is_measured_from_the_queueing_clock() {
    Conductor c(4);
    JobId other = kNoJob;
    const JobId actor = c.launch(posts_for_other(&c, &other, 2));
    other = c.launch(parks_once());
    c.post_after(actor, 5, 4);
    c.step();  // actor at 4; from inside it queues for `other` with delay 2
    c.step();
    diag::check(c.now() == 6 && c.outcome_of(other) == Outcome::Delivered,
                "a delay queued from inside a resumed job counts from the clock as it stood then",
                "now " + diag::num(static_cast<long long>(c.now())) + ", other " + diag::outcome_text(c, other));
}

}  // namespace

int main() {
    std::printf("== scheduler_clock ==\n");
    diag::scenario("ready_time_comes_first", ready_time_comes_first);
    diag::scenario("equal_time_then_lowest_identifier", equal_time_then_lowest_identifier);
    diag::scenario("same_job_same_time_keeps_queue_order", same_job_same_time_keeps_queue_order);
    diag::scenario("clock_never_moves_on_its_own", clock_never_moves_on_its_own);
    diag::scenario("delay_is_measured_from_the_queueing_clock", delay_is_measured_from_the_queueing_clock);
    return diag::finish("scheduler_clock");
}
