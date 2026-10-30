// Randomised stress: invariants must hold after every operation.
#include "roundel/runtime.hpp"

#include <cstdio>
#include <random>
#include <stdexcept>
#include <vector>

using namespace roundel;

static int failures = 0;
static long checks = 0;
#define CHECK(cond, msg)                                          \
    do {                                                          \
        ++checks;                                                 \
        if (!(cond)) {                                            \
            ++failures;                                           \
            std::printf("  FAIL seed=%u op=%d: %s\n", seed, opi, msg); \
        }                                                         \
    } while (0)

static unsigned seed = 0;
static int opi = 0;

static Duty simple() { co_return co_await park(); }
static Duty child() { co_return co_await park(); }

static Duty two_parks() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a + b;
}
static Duty with_child() { co_return co_await child() + 1; }
static Duty nested() {
    const std::int64_t a = co_await child();
    const std::int64_t b = co_await child();
    co_return a * 10 + b;
}
static Duty risky() {
    const std::int64_t v = co_await park();
    if (v == 13) {
        throw std::runtime_error("unlucky");
    }
    co_return v;
}
static Duty guarded() {
    std::int64_t v = 0;
    try {
        v = co_await child();
    } catch (const std::exception&) {
        v = -1;
    }
    co_return v;
}

static Duty pick(int kind) {
    switch (kind % 6) {
        case 0:
            return simple();
        case 1:
            return two_parks();
        case 2:
            return with_child();
        case 3:
            return nested();
        case 4:
            return risky();
        default:
            return guarded();
    }
}

static void one_run(unsigned s) {
    seed = s;
    std::mt19937 rng{s};
    const std::size_t cap = 1 + (rng() % 6);
    Conductor c{cap};
    std::vector<JobId> ids;
    std::uint64_t last_clock = 0;

    for (opi = 0; opi < 60; ++opi) {
        const int what = static_cast<int>(rng() % 100);
        try {
            if (what < 22 || ids.empty()) {
                const Duty d = pick(static_cast<int>(rng() % 6));
                const JobId id = c.launch(std::move(d));
                CHECK(id != kNoJob, "launch gave no job");
                ids.push_back(id);
            } else if (what < 45) {
                const JobId id = ids[rng() % ids.size()];
                c.post(id, static_cast<std::int64_t>(rng() % 20));
            } else if (what < 55) {
                const JobId id = ids[rng() % ids.size()];
                c.post_after(id, static_cast<std::int64_t>(rng() % 20), rng() % 5);
            } else if (what < 63) {
                c.post_fault(ids[rng() % ids.size()]);
            } else if (what < 70) {
                c.post_fault_after(ids[rng() % ids.size()], rng() % 5);
            } else if (what < 82) {
                const JobId id = (rng() % 8 == 0) ? 9000 + rng() % 5 : ids[rng() % ids.size()];
                c.cancel(id);
            } else if (what < 96) {
                c.step();
            } else {
                c.drain();
            }
        } catch (const std::bad_alloc&) {
            // arena exhausted; fine
        }

        // invariants
        CHECK(c.arena().live_slots() <= c.arena().capacity(), "live exceeds capacity");
        CHECK(c.arena().double_reclaims() == 0, "double reclaim");
        CHECK(c.now() >= last_clock, "clock went backwards");
        last_clock = c.now();
        for (JobId id : ids) {
            CHECK(c.journal().count_for(Mark::JobSettled, id) <= 1, "settled twice");
            CHECK(c.journal().count_for(Mark::FaultReported, id) <= 1, "fault reported twice");
            CHECK(c.journal().count_for(Mark::FrameReleased, id) <= 1, "released twice");
            if (c.settled(id)) {
                CHECK(c.journal().count_for(Mark::FrameReleased, id) == 1,
                      "settled but not released");
            }
            if (c.journal().count_for(Mark::FrameReleased, id) == 1) {
                const std::size_t si = c.journal().first_index(Mark::FrameReleased, id);
                const std::size_t ss = c.journal().first_index(Mark::JobSettled, id);
                CHECK(ss < si, "settled after release");
            }
        }
    }
    c.drain();
    // Jobs that are still parked are cancelled at shutdown; check the arena is whole.
    for (JobId id : ids) {
        CHECK(c.journal().count_for(Mark::JobSettled, id) <= 1, "settled twice at end");
        CHECK(c.journal().count_for(Mark::FaultReported, id) <= 1, "fault twice at end");
    }
    CHECK(c.arena().double_reclaims() == 0, "double reclaim at end");
    CHECK(c.arena().free_slots() + c.live_jobs() <= c.arena().capacity(), "accounting");
    // Cancel whatever is still parked; every slot must come back at once.
    std::size_t open = 0;
    for (JobId id : ids) {
        if (!c.settled(id)) {
            ++open;
            c.cancel(id);
        }
    }
    CHECK(c.live_jobs() == 0, "jobs still live after cancelling everything");
    CHECK(c.arena().live_slots() == 0, "slots still checked out after cancelling everything");
    for (JobId id : ids) {
        CHECK(c.settled(id), "job never settled");
        CHECK(c.journal().count_for(Mark::JobSettled, id) == 1, "not settled exactly once");
        CHECK(c.journal().count_for(Mark::FrameReleased, id) == 1, "not released exactly once");
    }
    std::printf("seed %u: cap=%zu jobs=%zu open=%zu peak=%zu checkouts=%zu\n", s, cap, ids.size(),
                open, c.arena().peak_live(), c.arena().total_check_outs());
    (void)open;
}

int main() {
    setvbuf(stdout, nullptr, _IONBF, 0);
    for (unsigned s = 1; s <= 400; ++s) {
        one_run(s);
        if (failures > 0) {
            std::printf("stopping at seed %u\n", s);
            break;
        }
    }
    std::printf("\n%ld invariant checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}
