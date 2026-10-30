// Scratch test harness, round five: randomised schedules.
#include <cstdio>
#include <cstdlib>
#include <exception>
#include <random>
#include <vector>

#include "roundel/runtime.hpp"

using namespace roundel;

static int failures = 0;

static void fail(const char* what, unsigned seed) {
    ++failures;
    std::printf("  FAIL seed=%u: %s\n", seed, what);
}

static Duty simple() { co_return co_await park(); }
static Duty child() { co_return co_await park(); }
static Duty nested() {
    std::int64_t t = 0;
    for (int i = 0; i < 2; ++i) t += co_await child();
    co_return t;
}
static Duty mixed() {
    std::int64_t t = co_await park();
    try {
        t += co_await child();
    } catch (const std::exception&) {
        t += 100;
    }
    t += co_await park();
    co_return t;
}
static Duty two_parks() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a * 10 + b;
}

static void invariants(const Conductor& c, unsigned seed) {
    const Journal& j = c.journal();
    for (JobId id = 1; id <= 40; ++id) {
        if (!j.has(Mark::JobStarted, id)) continue;
        if (j.count_for(Mark::JobStarted, id) != 1) fail("started once", seed);
        if (j.count_for(Mark::JobSettled, id) > 1) fail("settled once", seed);
        if (j.count_for(Mark::FrameReleased, id) > 1) fail("released once", seed);
        if (j.count_for(Mark::FaultReported, id) > 1) fail("fault once", seed);
        const std::size_t rel = j.first_index(Mark::FrameReleased, id);
        if (rel != j.entries().size()) {
            const std::size_t set = j.first_index(Mark::JobSettled, id);
            if (set == j.entries().size() || set > rel) fail("settled before released", seed);
        }
    }
    if (c.arena().double_reclaims() != 0) fail("double reclaim", seed);
    if (c.arena().live_slots() > c.arena().capacity()) fail("over capacity", seed);
    if (c.arena().peak_live() > c.arena().capacity()) fail("peak over capacity", seed);
}

static void one_run(unsigned seed) {
    std::mt19937 rng{seed};
    const std::size_t capacity = 1 + rng() % 6;
    Conductor c{capacity};
    std::vector<JobId> jobs;
    std::uint64_t last_now = 0;

    for (int op = 0; op < 40; ++op) {
        const int pick = static_cast<int>(rng() % 100);
        JobId target = kNoJob;
        if (!jobs.empty() && (rng() % 4 != 0)) {
            target = jobs[rng() % jobs.size()];
        } else if (!jobs.empty()) {
            target = jobs.back();
        }
        try {
            if (pick < 12 && jobs.size() < 6) {
                switch (rng() % 4) {
                    case 0: jobs.push_back(c.launch(simple())); break;
                    case 1: jobs.push_back(c.launch(nested())); break;
                    case 2: jobs.push_back(c.launch(mixed())); break;
                    default: jobs.push_back(c.launch(two_parks())); break;
                }
            } else if (pick < 34 && target != kNoJob) {
                c.post(target, static_cast<std::int64_t>(rng() % 5));
            } else if (pick < 50 && target != kNoJob) {
                c.post_after(target, static_cast<std::int64_t>(rng() % 5), rng() % 6);
            } else if (pick < 58 && target != kNoJob) {
                c.post_fault(target);
            } else if (pick < 66 && target != kNoJob) {
                c.post_fault_after(target, rng() % 4);
            } else if (pick < 78 && target != kNoJob) {
                c.cancel(target);
            } else if (pick < 84) {
                c.cancel(1 + rng() % 8);  // possibly never launched
            } else if (pick < 96) {
                c.step();
            } else {
                c.drain();
            }
        } catch (const std::bad_alloc&) {
            // Arena exhaustion is reported, not ignored.
        }
        if (c.now() < last_now) fail("clock went backwards", seed);
        last_now = c.now();
        invariants(c, seed);
        if (failures > 0) return;
    }

    // Everything left over must unwind and give its storage back.
    for (JobId id : jobs) {
        if (!c.settled(id)) c.cancel(id);
    }
    if (c.arena().live_slots() != 0) {
        std::printf("  live=%zu capacity=%zu\n", c.arena().live_slots(), c.arena().capacity());
        fail("storage not returned at shutdown", seed);
    }
    if (c.live_jobs() != 0) fail("jobs still live", seed);
    invariants(c, seed);
}

int main() {
    for (unsigned seed = 1; seed <= 400; ++seed) {
        one_run(seed);
        if (failures > 3) break;
    }
    std::printf("%d failures\n", failures);
    return failures == 0 ? 0 : 1;
}
