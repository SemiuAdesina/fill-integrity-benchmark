//
// Nested coroutines: children, their frames, and the order things go away in.

#include <stdexcept>

#include "common.hpp"

using namespace roundel;

namespace {

Duty child_value(int tag, std::int64_t v) {
    diag::Probe p{tag};
    (void)p;
    co_return v;
}

Duty child_throws(int tag) {
    diag::Probe p{tag};
    (void)p;
    if (p.tag >= 0) {
        throw std::runtime_error("child failed");
    }
    co_return 0;
}

Duty child_parks(int tag) {
    diag::Probe p{tag};
    (void)p;
    const std::int64_t v = co_await park();
    co_return v;
}

// Samples how many probes are alive at the instant the child's value arrives.
Duty parent_counting(int ptag, int ctag, std::vector<int>* seen) {
    diag::Probe p{ptag};
    (void)p;
    const std::int64_t got = co_await child_value(ctag, 30);
    seen->push_back(diag::probes_alive());
    co_return got + 1;
}

Duty parent_of_throwing(int ptag, int ctag) {
    diag::Probe p{ptag};
    (void)p;
    const std::int64_t got = co_await child_throws(ctag);
    co_return got;
}

Duty parent_of_parking(int ptag, int ctag) {
    diag::Probe p{ptag};
    (void)p;
    const std::int64_t got = co_await child_parks(ctag);
    co_return got;
}

Duty middle(int tag, int ltag) {
    diag::Probe p{tag};
    (void)p;
    const std::int64_t got = co_await child_value(ltag, 5);
    co_return got + 1;
}

Duty top(int tag, int mtag, int ltag) {
    diag::Probe p{tag};
    (void)p;
    const std::int64_t got = co_await middle(mtag, ltag);
    co_return got + 1;
}

void child_gone_before_parent_continues() {
    diag::reset_probes();
    std::vector<int> seen;
    Conductor c(4);
    const JobId job = c.launch(parent_counting(1, 2, &seen));
    diag::check(c.outcome_of(job) == Outcome::Delivered && c.value_of(job).has_value() && *c.value_of(job) == 31,
                "a child's value reaches the parent", diag::outcome_text(c, job));
    diag::check(seen.size() == 1 && seen[0] == 1, "the child's frame is already destroyed when the parent sees the value",
                seen.empty() ? "no sample" : "probes alive at that instant " + diag::num(seen[0]));
    const std::vector<int>& order = diag::destroyed_in_order();
    diag::check(order.size() == 2 && order[0] == 2 && order[1] == 1, "the child's contents go before the parent's");
    diag::check(c.arena().live_slots() == 0 && c.arena().peak_live() == 2, "both slots are back and only two were ever live at once",
                "live_slots " + diag::num(static_cast<long long>(c.arena().live_slots())) + ", peak " + diag::num(static_cast<long long>(c.arena().peak_live())));
}

void deeper_nesting_unwinds_inner_first() {
    diag::reset_probes();
    Conductor c(4);
    const JobId job = c.launch(top(1, 2, 3));
    diag::check(c.value_of(job).has_value() && *c.value_of(job) == 7, "values pass up through every level",
                c.value_of(job) ? diag::num(*c.value_of(job)) : "none");
    const std::vector<int>& order = diag::destroyed_in_order();
    diag::check(order.size() == 3 && order[0] == 3 && order[1] == 2 && order[2] == 1, "frames are destroyed innermost first");
    diag::check(c.arena().live_slots() == 0, "every slot is back", "live_slots " + diag::num(static_cast<long long>(c.arena().live_slots())));
}

void cancelling_a_parent_takes_the_child_first() {
    diag::reset_probes();
    Conductor c(4);
    const JobId job = c.launch(parent_of_parking(1, 2));
    c.cancel(job);
    diag::check(c.outcome_of(job) == Outcome::Cancelled, "cancelling a job parked in a child settles it as cancelled",
                diag::outcome_text(c, job));
    const std::vector<int>& order = diag::destroyed_in_order();
    diag::check(order.size() == 2 && order[0] == 2 && order[1] == 1, "the child's frame is destroyed before the frame that awaited it",
                "destroyed " + diag::num(static_cast<long long>(order.size())) + " of 2");
    diag::check(c.arena().live_slots() == 0, "and both slots are back at once",
                "live_slots " + diag::num(static_cast<long long>(c.arena().live_slots())));
}

void a_failing_child_faults_the_parent_once() {
    diag::reset_probes();
    Conductor c(4);
    const JobId job = c.launch(parent_of_throwing(1, 2));
    diag::check(c.outcome_of(job) == Outcome::Faulted, "an exception escaping a child faults the job",
                diag::outcome_text(c, job));
    diag::check(c.journal().count_for(Mark::FaultReported, job) == 1 && c.journal().count_for(Mark::JobSettled, job) == 1,
                "the failure is announced once and the job settles once",
                "announced " + diag::num(static_cast<long long>(c.journal().count_for(Mark::FaultReported, job))) +
                    ", settled " + diag::num(static_cast<long long>(c.journal().count_for(Mark::JobSettled, job))));
    diag::check(diag::probes_alive() == 0 && c.arena().live_slots() == 0, "the failing child's frame is destroyed like a returning one",
                "probes alive " + diag::num(diag::probes_alive()) + ", live_slots " + diag::num(static_cast<long long>(c.arena().live_slots())));
}

}  // namespace

int main() {
    std::printf("== nested_coroutines ==\n");
    diag::scenario("child_gone_before_parent_continues", child_gone_before_parent_continues);
    diag::scenario("deeper_nesting_unwinds_inner_first", deeper_nesting_unwinds_inner_first);
    diag::scenario("cancelling_a_parent_takes_the_child_first", cancelling_a_parent_takes_the_child_first);
    diag::scenario("a_failing_child_faults_the_parent_once", a_failing_child_faults_the_parent_once);
    return diag::finish("nested_coroutines");
}
