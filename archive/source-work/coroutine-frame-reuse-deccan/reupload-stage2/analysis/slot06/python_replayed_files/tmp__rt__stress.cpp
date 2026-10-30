// Deterministic stress test. Not part of the deliverable.
#include "roundel/runtime.hpp"

#include <cstdio>
#include <string>
#include <vector>

using namespace roundel;

static int g_failures = 0;
#define CHECK(cond)                                                              \
    do {                                                                         \
        if (!(cond)) {                                                           \
            std::printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);          \
            ++g_failures;                                                        \
        }                                                                        \
    } while (0)

static std::vector<std::string> g_log;

struct Tracer {
    int id;
    explicit Tracer(int i) : id(i) {}
    ~Tracer() { g_log.push_back(std::to_string(id)); }
};

static Duty leaf(int) { co_return co_await park(); }

static Duty leaf_throws(int) {
    co_await park();
    throw std::runtime_error("leaf");
}

static Duty leaf_catches(int) {
    try {
        co_await park();
    } catch (const std::exception&) {
        co_return -1;
    }
    co_return 1;
}

static Duty two_parks(int) {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a + b;
}

static Duty one_child(int) {
    Tracer t{1};
    co_return co_await leaf(0);
}

static Duty two_children(int) {
    Tracer t{2};
    const std::int64_t a = co_await leaf(0);
    const std::int64_t b = co_await leaf(0);
    co_return a + b;
}

static Duty deep(int) {
    Tracer t{3};
    co_return co_await one_child(0);
}

static Duty faulty_child(int) {
    Tracer t{4};
    try {
        co_return co_await leaf_throws(0);
    } catch (const std::exception&) {
        co_return 7;
    }
}

static Duty faulty_child_uncaught(int) {
    Tracer t{5};
    co_return co_await leaf_throws(0);
}

static Duty mixed(int) {
    Tracer t{6};
    const std::int64_t a = co_await leaf_catches(0);
    const std::int64_t b = co_await park();
    co_return a + b;
}

typedef Duty (*Maker)(int);

static const Maker kMakers[] = {&leaf,      &leaf_throws, &leaf_catches,
                                &two_parks, &one_child,   &two_children,
                                &deep,      &faulty_child, &faulty_child_uncaught,
                                &mixed};

int main() {
    std::uint64_t seed = 12345;
    auto next = [&seed](std::uint64_t n) {
        seed = seed * 6364136223846793005ull + 1442695040888963407ull;
        return static_cast<std::size_t>((seed >> 33) % n);
    };

    for (int round = 0; round < 40; ++round) {
        const std::size_t capacity = 1 + next(8);
        Conductor c{capacity};
        std::vector<JobId> jobs;
        const std::size_t ops = 300;
        for (std::size_t i = 0; i < ops; ++i) {
            const std::size_t pick = next(10);
            if (pick < 3) {
                if (c.arena().free_slots() > 1) {
                    const Maker m = kMakers[next(sizeof(kMakers) / sizeof(kMakers[0]))];
                    JobId id = kNoJob;
                    try {
                        id = c.launch(m(0));
                    } catch (const std::bad_alloc&) {
                        id = kNoJob;
                    }
                    if (id != kNoJob) {
                        jobs.push_back(id);
                    }
                }
            } else if (pick < 7 && !jobs.empty()) {
                const JobId j = jobs[next(jobs.size())];
                const std::size_t kind = next(4);
                if (kind == 0) {
                    c.post(j, static_cast<std::int64_t>(i));
                } else if (kind == 1) {
                    c.post_after(j, static_cast<std::int64_t>(i), next(5));
                } else if (kind == 2) {
                    c.post_fault(j);
                } else {
                    c.post_fault_after(j, next(5));
                }
            } else if (pick < 8 && !jobs.empty()) {
                c.cancel(jobs[next(jobs.size())]);
            } else if (pick < 9) {
                c.cancel(next(2) == 0 ? kNoJob : 999999u);
            } else {
                c.step();
            }
            // invariants that must hold at every point
            CHECK(c.arena().live_slots() <= c.arena().capacity());
            CHECK(c.arena().double_reclaims() == 0);
            CHECK(c.arena().peak_live() <= c.arena().capacity());
        }
        c.drain();
        for (JobId j : jobs) {
            c.cancel(j);
        }
        for (JobId j : jobs) {
            CHECK(c.settled(j));
            CHECK(c.journal().count_for(Mark::JobSettled, j) == 1);
            CHECK(c.journal().count_for(Mark::FrameReleased, j) == 1);
            const Outcome o = c.outcome_of(j).value_or(Outcome::Delivered);
            if (o == Outcome::Faulted) {
                CHECK(c.journal().count_for(Mark::FaultReported, j) == 1);
            } else {
                CHECK(c.journal().count_for(Mark::FaultReported, j) == 0);
            }
        }
        CHECK_EQ_LIVE:
        CHECK(c.arena().live_slots() == 0);
        CHECK(c.live_jobs() == 0);
        // ordering rule: settled before release, for each job
        for (JobId j : jobs) {
            CHECK(c.journal().first_index(Mark::JobSettled, j) <
                  c.journal().first_index(Mark::FrameReleased, j));
            CHECK(c.journal().first_index(Mark::JobStarted, j) <
                  c.journal().first_index(Mark::JobSettled, j));
            // nothing after the release
            const auto& es = c.journal().entries();
            const std::size_t rel = c.journal().first_index(Mark::FrameReleased, j);
            for (std::size_t k = rel + 1; k < es.size(); ++k) {
                CHECK(es[k].job != j);
            }
        }
    }
    if (g_failures == 0) {
        std::printf("stress ok\n");
        return 0;
    }
    std::printf("%d failure(s)\n", g_failures);
    return 1;
}
