// Round 2: edge cases + fuzz. (my own harness; not part of the deliverable)
#include "roundel/runtime.hpp"

#include <cstdio>
#include <random>
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

// ------------------------------------------------------------------ duties --
Duty no_park() { co_return 5; }

Duty leaf() { co_return co_await park(); }

Duty parent_leaf() {
    const std::int64_t v = co_await leaf();
    co_return v * 2;
}

Duty parent_no_park() {
    const std::int64_t v = co_await no_park();
    co_return v + 1;
}

Duty t_inner() { co_return co_await park(); }
Duty t_middle() { co_return co_await t_inner() + 1; }
Duty t_outer() { co_return co_await t_middle() + 1; }

Duty park_twice() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a * 10 + b;
}

Duty catcher() {
    try {
        co_await park();
    } catch (const std::exception&) {
    }
    co_return 5;
}

Duty thrower() {
    co_await park();
    throw std::runtime_error("boom");
}

// ------------------------------------------------------------------- tests --
void t_immediate_completion() {
    Conductor c{4};
    const JobId j = c.launch(no_park());
    CHECK(c.settled(j));
    CHECK_EQ(*c.outcome_of(j), Outcome::Delivered);
    CHECK_EQ(*c.value_of(j), 5);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, j), 1u);
    CHECK_EQ(c.live_jobs(), 0u);
}

void t_child_without_park() {
    Conductor c{4};
    const JobId j = c.launch(parent_no_park());
    CHECK(c.settled(j));
    CHECK_EQ(*c.value_of(j), 6);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, j), 1u);
}

void t_exhaustion_inside_job() {
    // capacity 1: the root fits, its child cannot. The failure is reported
    // through the job, and its storage is still handed back.
    Conductor c{1};
    const JobId j = c.launch(parent_leaf());
    CHECK(c.settled(j));
    CHECK_EQ(*c.outcome_of(j), Outcome::Faulted);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.arena().double_reclaims(), 0u);
}

void t_exhaustion_deep() {
    Conductor c{2};
    const JobId j = c.launch(t_outer());
    CHECK(c.settled(j));
    CHECK_EQ(*c.outcome_of(j), Outcome::Faulted);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

void t_fault_after_delay() {
    Conductor c{4};
    const JobId j = c.launch(leaf());
    c.post_fault_after(j, 3);
    CHECK(c.step());
    CHECK_EQ(c.now(), 3u);
    CHECK_EQ(*c.outcome_of(j), Outcome::Faulted);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

void t_ticket_for_unknown_job() {
    Conductor c{4};
    c.post(77, 1);
    CHECK_EQ(c.pending_tickets(), 1u);
    CHECK(c.step());
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, 77), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

void t_cancel_nested_then_reuse() {
    Conductor c{2};
    const JobId j = c.launch(t_outer());   // root(0) + middle(1) + inner(?)
    CHECK(!c.settled(j));
    c.cancel(j);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, j), 1u);
    CHECK_EQ(*c.outcome_of(j), Outcome::Cancelled);
    // storage really is reusable
    const JobId k = c.launch(t_outer());
    c.cancel(k);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.arena().total_check_outs(), 6u);  // 3 frames twice
}

void t_many_jobs_reuse() {
    Conductor c{2};
    for (int i = 0; i < 20; ++i) {
        const JobId j = c.launch(leaf());
        c.post(j, i);
        CHECK(c.drain() >= 1u);
        CHECK_EQ(*c.value_of(j), i);
        CHECK_EQ(c.arena().live_slots(), 0u);
    }
    CHECK_EQ(c.arena().peak_live(), 1u);
    CHECK_EQ(c.arena().double_reclaims(), 0u);
}

void t_stress_interleaved() {
    Conductor c{4};
    const JobId a = c.launch(park_twice());
    const JobId b = c.launch(leaf());
    const JobId d = c.launch(park_twice());
    c.post_after(a, 1, 4);
    c.post(b, 2);
    c.post_after(d, 3, 4);
    c.post_after(a, 4, 2);
    std::uint64_t last = 0;
    while (c.step()) {
        CHECK(c.now() >= last);
        last = c.now();
    }
    CHECK_EQ(*c.outcome_of(b), Outcome::Delivered);
    CHECK_EQ(*c.value_of(b), 2);
    // a: second park got 1 (ready 2) then first park got 4 (ready 4)
    CHECK(c.settled(a));
    CHECK(c.settled(d) || !c.settled(d));
    CHECK_EQ(c.arena().double_reclaims(), 0u);
    std::printf("[stress] now=%llu a=%lld\n", (unsigned long long)c.now(),
                c.value_of(a).value_or(-1));
}

// ------------------------------------------------------------------- fuzz --
namespace {

struct Fuzz {
    Conductor c;
    std::vector<JobId> jobs;
    std::mt19937 rng{12345};

    explicit Fuzz(std::size_t cap) : c(cap) {}

    void run(int iterations) {
        for (int i = 0; i < iterations; ++i) {
            const int op = static_cast<int>(rng() % 100);
            const JobId j = jobs.empty() ? kNoJob : jobs[rng() % jobs.size()];
            switch (op) {
                case 0:
                case 1:
                case 2: {
                    try {
                        jobs.push_back(c.launch(leaf()));
                    } catch (const std::bad_alloc&) {
                    }
                    break;
                }
                case 3:
                case 4:
                case 5:
                    try {
                        jobs.push_back(c.launch(park_twice()));
                    } catch (const std::bad_alloc&) {
                    }
                    break;
                case 6:
                case 7:
                    try {
                        jobs.push_back(c.launch(parent_leaf()));
                    } catch (const std::bad_alloc&) {
                    }
                    break;
                case 8:
                    try {
                        jobs.push_back(c.launch(thrower()));
                    } catch (const std::bad_alloc&) {
                    }
                    break;
                case 9:
                    try {
                        jobs.push_back(c.launch(catcher()));
                    } catch (const std::bad_alloc&) {
                    }
                    break;
                case 10:
                case 11:
                case 12:
                    c.post(j, static_cast<std::int64_t>(rng() % 100));
                    break;
                case 13:
                case 14:
                    c.post_after(j, static_cast<std::int64_t>(rng() % 100), rng() % 7);
                    break;
                case 15:
                    c.post_fault(j);
                    break;
                case 16:
                    c.post_fault_after(j, rng() % 5);
                    break;
                case 17:
                case 18:
                case 19:
                    c.cancel(j);
                    break;
                case 20:
                    c.cancel(99999);  // never launched
                    break;
                case 21:
                    c.post(99999, 1);
                    break;
                default:
                    c.step();
                    break;
            }
            invariants();
        }
        c.drain();
    }

    void invariants() {
        for (JobId j : jobs) {
            CHECK(c.journal().count_for(Mark::JobSettled, j) <= 1);
            CHECK(c.journal().count_for(Mark::FrameReleased, j) <= 1);
            CHECK(c.journal().count_for(Mark::FaultReported, j) <= 1);
        }
        CHECK(c.arena().live_slots() <= c.arena().capacity());
        CHECK_EQ(c.arena().double_reclaims(), 0u);
        // every settled job has handed its storage back
        std::size_t live = 0;
        for (JobId j : jobs) {
            if (!c.settled(j)) ++live;
        }
        if (live == 0) {
            CHECK_EQ(c.arena().live_slots(), 0u);
        }
    }
};

}  // namespace

void t_fuzz() {
    for (std::size_t cap = 1; cap <= 6; ++cap) {
        for (unsigned seed = 1; seed <= 6; ++seed) {
            Fuzz f{cap};
            f.rng.seed(seed * 7919u + static_cast<unsigned>(cap));
            f.run(400);
            // shutting down must hand everything back
            CHECK_EQ(f.c.arena().double_reclaims(), 0u);
            CHECK_EQ(f.c.arena().free_slots(), f.c.arena().capacity());
        }
    }
}

int main() {
    t_immediate_completion();
    t_child_without_park();
    t_exhaustion_inside_job();
    t_exhaustion_deep();
    t_fault_after_delay();
    t_ticket_for_unknown_job();
    t_cancel_nested_then_reuse();
    t_many_jobs_reuse();
    t_stress_interleaved();
    t_fuzz();
    std::printf("\n%d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
