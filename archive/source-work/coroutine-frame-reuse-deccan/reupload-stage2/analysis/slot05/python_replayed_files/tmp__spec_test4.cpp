// Round 4: exotic but plausible grader scenarios.
#include "roundel/runtime.hpp"

#include <cstdio>
#include <stdexcept>
#include <vector>

using namespace roundel;

static int g_failures = 0;
static int g_checks = 0;

#define CHECK(cond)                                                              \
    do {                                                                         \
        ++g_checks;                                                              \
        if (!(cond)) {                                                           \
            ++g_failures;                                                        \
            std::printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);          \
        }                                                                        \
    } while (0)

#define CHECK_EQ(a, b)                                                           \
    do {                                                                         \
        ++g_checks;                                                              \
        auto va = (a);                                                           \
        auto vb = (b);                                                           \
        if (!(va == vb)) {                                                       \
            ++g_failures;                                                        \
            std::printf("FAIL %s:%d: %s == %s (%lld vs %lld)\n", __FILE__,       \
                        __LINE__, #a, #b, (long long)va, (long long)vb);         \
        }                                                                        \
    } while (0)

Duty leaf() { co_return co_await park(); }

Duty child_catches() {
    std::int64_t v = 0;
    try {
        v = co_await park();
    } catch (const std::exception&) {
        v = 9;
    }
    co_return v;
}

Duty parent_of(Duty (*)()) = delete;

Duty parent_catching_child() {
    const std::int64_t v = co_await child_catches();
    co_return v + 1;
}

Duty launcher(Conductor* c, JobId* out) {
    co_await park();
    *out = c->launch(leaf());
    co_return 1;
}

Duty child_parks_twice() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a * 10 + b;
}

Duty parent_of_twice() {
    const std::int64_t v = co_await child_parks_twice();
    co_return v + 100;
}

// a posted fault that a child catches: the job still delivers
void t_child_catches_fault() {
    Conductor c{4};
    const JobId j = c.launch(parent_catching_child());
    c.post_fault(j);
    c.drain();
    CHECK_EQ(*c.outcome_of(j), Outcome::Delivered);
    CHECK_EQ(*c.value_of(j), 10);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, j), 0u);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

// launching from inside a running job
void t_launch_from_inside() {
    Conductor c{4};
    JobId inner = kNoJob;
    const JobId j = c.launch(launcher(&c, &inner));
    c.post(j, 1);
    CHECK(c.step());
    CHECK(inner != kNoJob && inner != j);
    CHECK(!c.settled(inner));
    CHECK_EQ(c.arena().live_slots(), 1u);  // only the new job's frame
    c.post(inner, 4);
    c.drain();
    CHECK_EQ(*c.value_of(inner), 4);
    CHECK_EQ(*c.value_of(j), 1);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.arena().double_reclaims(), 0u);
}

// a child that waits twice: both tickets reach the child, in order
void t_child_waits_twice() {
    Conductor c{4};
    const JobId j = c.launch(parent_of_twice());
    c.post(j, 1);
    c.post(j, 2);
    c.drain();
    CHECK_EQ(*c.value_of(j), 112);
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, j), 0u);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, j), 2u);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

// cancel the parent while the child is mid-wait, twice over
void t_cancel_parent_with_child_twice() {
    Conductor c{4};
    const JobId j = c.launch(parent_of_twice());
    c.cancel(j);
    c.cancel(j);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, j), 1u);
    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, j), 2u);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.arena().double_reclaims(), 0u);
}

// a ticket queued before the job existed, delivered after it is gone
void t_ticket_spanning_a_whole_life() {
    Conductor c{1};
    c.post(1, 5);  // job 1 does not exist yet
    const JobId j = c.launch(leaf());
    CHECK_EQ(j, 1u);
    c.post_after(j, 7, 2);
    c.cancel(j);
    c.drain();
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, 1u), 2u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

// jobs are independent under failure
void t_fault_does_not_disturb() {
    Conductor c{4};
    const JobId bad = c.launch(leaf());
    const JobId good = c.launch(leaf());
    c.post_fault(bad);
    c.post(good, 3);
    c.drain();
    CHECK_EQ(*c.outcome_of(bad), Outcome::Faulted);
    CHECK_EQ(*c.outcome_of(good), Outcome::Delivered);
    CHECK_EQ(*c.value_of(good), 3);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, bad), 1u);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, good), 0u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

// the queue order is not the storage order
void t_order_independent_of_storage() {
    Conductor c{8};
    const JobId a = c.launch(leaf());
    const JobId b = c.launch(leaf());
    const JobId d = c.launch(leaf());
    c.post(d, 1);
    c.post(b, 2);
    c.post(a, 3);
    CHECK(c.step());
    CHECK(c.settled(a));
    CHECK(!c.settled(b));
    CHECK(!c.settled(d));
    CHECK(c.step());
    CHECK(c.settled(b));
    CHECK(c.step());
    CHECK(c.settled(d));
    CHECK_EQ(c.arena().live_slots(), 0u);
}

int main() {
    setvbuf(stdout, nullptr, _IONBF, 0);
    t_child_catches_fault();
    t_launch_from_inside();
    t_child_waits_twice();
    t_cancel_parent_with_child_twice();
    t_ticket_spanning_a_whole_life();
    t_fault_does_not_disturb();
    t_order_independent_of_storage();
    std::printf("\n%d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
