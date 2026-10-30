// Randomised stress test (not part of /app).
#include "roundel/runtime.hpp"

#include <cstdio>
#include <map>
#include <random>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

using namespace roundel;

static int g_fail = 0;
#define CHECK(cond, msg)                                                 \
    do {                                                                 \
        if (!(cond)) {                                                    \
            std::printf("  FAIL line %d: %s\n", __LINE__, msg);           \
            ++g_fail;                                                     \
        }                                                                 \
    } while (0)

static Conductor* g_c = nullptr;
static JobId g_self = 0;

static Duty leaf() { co_return co_await park(); }
static Duty two_parks() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a * 10 + b;
}
static Duty mid() { co_return co_await leaf() + 1; }
static Duty deep() { co_return co_await mid() * 2; }
static Duty then_park() {
    const std::int64_t v = co_await leaf();
    co_return v + co_await park();
}
static Duty two_children() {
    const std::int64_t a = co_await leaf();
    const std::int64_t b = co_await leaf();
    co_return a + b;
}
static Duty catcher() {
    try {
        co_await park();
    } catch (const std::exception&) {
    }
    co_return 5;
}
static Duty thrower() {
    co_await park();
    throw std::runtime_error("boom");
}
static Duty self_cancel() {
    g_c->cancel(g_self);
    co_return 1;
}
static Duty poster() {
    const std::int64_t v = co_await park();
    g_c->post(g_self, v);
    co_return v;
}

static Duty make(int kind) {
    switch (kind % 9) {
        case 0: return leaf();
        case 1: return two_parks();
        case 2: return mid();
        case 3: return deep();
        case 4: return then_park();
        case 5: return two_children();
        case 6: return catcher();
        case 7: return thrower();
        default: return poster();
    }
}

static void run(unsigned seed, std::size_t capacity, int rounds) {
    Conductor c{capacity};
    g_c = &c;
    std::mt19937 rng{seed};
    std::vector<JobId> ids;
    for (int i = 0; i < rounds; ++i) {
        const int op = static_cast<int>(rng() % 100);
        JobId j = ids.empty() ? kNoJob : ids[rng() % ids.size()];
        if (op < 22 || ids.empty()) {
            g_self = static_cast<JobId>(ids.size() + 1);
            const JobId id = c.launch(make(static_cast<int>(rng() % 9)));
            ids.push_back(id);
        } else if (op < 50) {
            c.post(j, static_cast<std::int64_t>(rng() % 100));
        } else if (op < 62) {
            c.post_after(j, static_cast<std::int64_t>(rng() % 100), rng() % 5);
        } else if (op < 70) {
            c.post_fault(j);
        } else if (op < 76) {
            c.post_fault_after(j, rng() % 5);
        } else if (op < 88) {
            c.cancel(j);
        } else if (op < 97) {
            c.step();
        } else {
            c.drain();
        }
        CHECK(c.arena().live_slots() <= capacity, "within capacity");
        CHECK(c.arena().double_reclaims() == 0, "no double reclaim");
        CHECK(c.live_jobs() + 0 == c.live_jobs(), "live_jobs sane");
        std::size_t settled = 0;
        for (JobId id : ids) {
            if (c.settled(id)) {
                ++settled;
            }
        }
        CHECK(settled + c.live_jobs() == ids.size(), "settled + live == launched");
    }

    // Journal invariants.
    const std::vector<Entry>& es = c.journal().entries();
    std::map<JobId, std::size_t> release_at;
    std::set<std::pair<std::uint32_t, std::uint32_t>> destroyed_slots;
    std::map<JobId, int> counts[static_cast<int>(Mark::FaultReported) + 1];
    std::map<JobId, std::size_t> first_start;
    for (std::size_t i = 0; i < es.size(); ++i) {
        const Entry& e = es[i];
        counts[static_cast<int>(e.mark)][e.job]++;
        if (e.mark == Mark::JobStarted && !first_start.count(e.job)) {
            first_start[e.job] = i;
        }
        if (e.mark == Mark::FrameReleased) {
            CHECK(!release_at.count(e.job), "one release per job");
            release_at[e.job] = i;
        }
        if (e.mark == Mark::ChildDestroyed) {
            const bool fresh =
                destroyed_slots.insert({e.slot.index, e.slot.epoch}).second;
            CHECK(fresh, "a slot is destroyed at most once per generation");
        }
    }
    for (JobId id : ids) {
        CHECK(counts[static_cast<int>(Mark::JobSettled)][id] <= 1, "settled at most once");
        CHECK(counts[static_cast<int>(Mark::FaultReported)][id] <= 1, "fault reported at most once");
        CHECK(counts[static_cast<int>(Mark::FrameReleased)][id] <= 1, "released at most once");
        if (counts[static_cast<int>(Mark::JobSettled)][id] == 1) {
            CHECK(release_at.count(id) == 1, "every settled job released");
            CHECK(c.journal().first_index(Mark::JobSettled, id) < release_at[id],
                  "settled before released");
        }
        const std::size_t rel = release_at.count(id) ? release_at[id] : es.size();
        for (std::size_t i = rel + 1; i < es.size(); ++i) {
            const Entry& e = es[i];
            if (e.job != id) {
                continue;
            }
            const bool ok = e.mark == Mark::TicketQueued || e.mark == Mark::CancelRequested ||
                            e.mark == Mark::TicketRejected;
            CHECK(ok, "nothing but queueing and requests after release");
        }
        CHECK(first_start.count(id) == 1 || id == kNoJob, "started once");
    }
    CHECK(c.arena().live_slots() == 0, "everything back at the end");
    CHECK(c.live_jobs() == 0, "no live jobs at the end");
}

int main() {
    for (unsigned seed = 1; seed <= 300; ++seed) {
        g_fail = 0;
        run(seed, 1 + (seed % 6), 60);
        if (g_fail) {
            std::printf("seed %u FAILED (%d)\n", seed, g_fail);
            return 1;
        }
    }
    std::printf("fuzz: all ok\n");
    return 0;
}
