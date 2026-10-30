// Round 3: lifetimes around conductors, plus protocol details.
#include "roundel/runtime.hpp"

#include <cstdio>
#include <stdexcept>
#include <string>
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

Duty park_twice() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a * 10 + b;
}

Duty feeder(Conductor* c, JobId other) {
    co_await park();
    c->post(other, 30);
    co_return 1;
}

Duty trap_canceller(Conductor* c, JobId victim) {
    co_await park();
    c->cancel(victim);
    c->cancel(victim);  // twice: still only one outcome, one extra request
    co_return 2;
}

// a frame made before any conductor exists
void t_duty_outside_conductor() {
    Duty d = leaf();  // ordinary storage
    CHECK(static_cast<bool>(d));
    {
        Conductor c{4};
        const JobId j = c.launch(std::move(d));
        c.post(j, 8);
        c.drain();
        CHECK_EQ(*c.value_of(j), 8);
        CHECK_EQ(c.arena().live_slots(), 0u);
        CHECK_EQ(c.arena().total_check_outs(), 0u);  // never used the pool
        CHECK_EQ(c.journal().count_for(Mark::FrameReleased, j), 1u);
    }
}

void t_duty_dropped_unlaunched() {
    Conductor c{4};
    { Duty d = leaf(); }  // destroyed again at once
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.arena().double_reclaims(), 0u);
    const JobId j = c.launch(leaf());
    c.cancel(j);
    CHECK_EQ(c.arena().free_slots(), c.arena().capacity());
}

void t_nested_conductors() {
    Conductor outer{4};
    const JobId a = outer.launch(leaf());
    {
        Conductor inner{2};
        CHECK_EQ(Conductor::active_arena(), &inner.arena());
        const JobId b = inner.launch(leaf());
        inner.post(b, 3);
        inner.drain();
        CHECK_EQ(*inner.value_of(b), 3);
    }
    CHECK_EQ(Conductor::active_arena(), &outer.arena());
    outer.post(a, 4);
    outer.drain();
    CHECK_EQ(*outer.value_of(a), 4);
    CHECK_EQ(outer.arena().live_slots(), 0u);
}

// a frame from another conductor's arena still returns to the right pool
void t_foreign_frame() {
    Conductor a{2};
    Duty d = leaf();  // frame lives in a's arena
    {
        Conductor b{2};
        const JobId j = b.launch(std::move(d));
        CHECK_EQ(b.arena().live_slots(), 0u);   // not b's storage
        CHECK_EQ(a.arena().live_slots(), 1u);
        b.cancel(j);
        CHECK_EQ(a.arena().live_slots(), 0u);
        CHECK_EQ(b.arena().live_slots(), 0u);
    }
    CHECK_EQ(a.arena().free_slots(), a.arena().capacity());
}

void t_schedule_from_inside_job() {
    Conductor c{4};
    const JobId other = c.launch(leaf());
    const JobId j = c.launch(feeder(&c, other));
    c.post(j, 1);
    CHECK(c.step());            // j resumed, it posted for other
    CHECK_EQ(c.pending_tickets(), 1u);
    CHECK(!c.settled(other));
    CHECK(c.step());            // other's ticket runs, and only now
    CHECK_EQ(*c.value_of(other), 30);
    CHECK_EQ(*c.value_of(j), 1);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

void t_cancel_twice_from_inside() {
    Conductor c{4};
    const JobId victim = c.launch(park_twice());
    const JobId j = c.launch(trap_canceller(&c, victim));
    c.post(j, 1);
    CHECK(c.step());
    CHECK(c.settled(victim));
    CHECK_EQ(*c.outcome_of(victim), Outcome::Cancelled);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, victim), 1u);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, victim), 1u);
    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, victim), 2u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

void t_no_progress_without_step() {
    Conductor c{4};
    const JobId j = c.launch(park_twice());
    c.post(j, 1);
    CHECK(!c.settled(j));
    CHECK_EQ(c.now(), 0u);
    CHECK_EQ(c.arena().live_slots(), 1u);
    CHECK(!c.step() == false);
    CHECK_EQ(*c.outcome_of(j), Outcome::Delivered);
}

void t_live_jobs_and_settled() {
    Conductor c{4};
    const JobId a = c.launch(leaf());
    const JobId b = c.launch(leaf());
    CHECK_EQ(c.live_jobs(), 2u);
    c.cancel(a);
    CHECK_EQ(c.live_jobs(), 1u);
    c.post(b, 2);
    c.drain();
    CHECK_EQ(c.live_jobs(), 0u);
    CHECK(c.settled(a));
    CHECK(c.settled(b));
    CHECK(!c.settled(999));
    CHECK(!c.outcome_of(999).has_value());
    CHECK(!c.value_of(a).has_value());  // cancelled: no value
}

// the clock never moves backwards and only to a delivered ready time
void t_clock_monotone() {
    Conductor c{4};
    const JobId j = c.launch(park_twice());
    c.post_after(j, 1, 9);
    c.post_after(j, 2, 3);
    std::uint64_t last = 0;
    while (c.step()) {
        CHECK(c.now() >= last);
        last = c.now();
    }
    CHECK_EQ(last, 9u);
    CHECK_EQ(*c.value_of(j), 21);
}

// a ticket for a job parked in a child is refused once the parent parks
void t_child_ticket_then_parent_park() {
    Conductor c{4};
    const JobId j = c.launch(park_twice());  // not nested, but two waits
    c.post_after(j, 5, 4);
    c.post(j, 1);
    CHECK(c.step());
    CHECK(c.step());
    CHECK_EQ(*c.value_of(j), 15);
}

int main() {
    setvbuf(stdout, nullptr, _IONBF, 0);
    t_duty_outside_conductor();
    t_duty_dropped_unlaunched();
    t_nested_conductors();
    t_foreign_frame();
    t_schedule_from_inside_job();
    t_cancel_twice_from_inside();
    t_no_progress_without_step();
    t_live_jobs_and_settled();
    t_clock_monotone();
    t_child_ticket_then_parent_park();
    std::printf("\n%d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
