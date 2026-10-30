// Randomised stress test (not part of /app).
#include "harness.hpp"

#include <cstdint>
#include <random>

namespace {

int g_frames = 0;  // Duty frames whose body has started and not yet finished

struct Tracked {
    Tracked() { ++g_frames; }
    ~Tracked() { --g_frames; }
    Tracked(const Tracked&) = delete;
    Tracked& operator=(const Tracked&) = delete;
};

Conductor* g_c = nullptr;
JobId g_self = kNoJob;

Duty leaf() {
    Tracked t;
    co_return co_await park();
}

Duty double_wait() {
    Tracked t;
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a - b;
}

Duty inner() {
    Tracked t;
    co_return co_await park();
}

Duty middle() {
    Tracked t;
    const std::int64_t v = co_await inner();
    co_return v + 1;
}

Duty outer() {
    Tracked t;
    const std::int64_t v = co_await middle();
    co_return v * 2;
}

Duty risky() {
    Tracked t;
    const std::int64_t v = co_await park();
    if (v % 3 == 0) {
        throw std::runtime_error("risky");
    }
    co_return v;
}

Duty self_scheduler() {
    Tracked t;
    const std::int64_t v = co_await park();
    if (g_c != nullptr && v % 2 == 0) {
        const JobId other = g_c->launch(leaf());
        g_c->post(other, 4);
    }
    co_return v;
}

Duty catcher() {
    Tracked t;
    try {
        co_await inner();
    } catch (const std::exception&) {
        co_return -3;
    }
    co_return 3;
}

void run(std::uint32_t seed) {
    std::mt19937 rng{seed};
    g_frames = 0;
    {
        Conductor c{6};
        g_c = &c;
        std::vector<JobId> jobs;
        for (int step_no = 0; step_no < 400; ++step_no) {
            const int op = static_cast<int>(rng() % 100);
            if (op < 18 && jobs.size() < 5) {
                switch (rng() % 7) {
                    case 0: jobs.push_back(c.launch(leaf())); break;
                    case 1: jobs.push_back(c.launch(double_wait())); break;
                    case 2: jobs.push_back(c.launch(outer())); break;
                    case 3: jobs.push_back(c.launch(risky())); break;
                    case 4: jobs.push_back(c.launch(self_scheduler())); break;
                    case 5: jobs.push_back(c.launch(catcher())); break;
                    default: jobs.push_back(c.launch(middle())); break;
                }
            } else if (op < 45 && !jobs.empty()) {
                const JobId j = jobs[rng() % jobs.size()];
                c.post(j, static_cast<std::int64_t>(rng() % 50));
            } else if (op < 55 && !jobs.empty()) {
                const JobId j = jobs[rng() % jobs.size()];
                c.post_after(j, static_cast<std::int64_t>(rng() % 50), rng() % 7);
            } else if (op < 63 && !jobs.empty()) {
                const JobId j = jobs[rng() % jobs.size()];
                if (rng() % 2 == 0) {
                    c.post_fault(j);
                } else {
                    c.post_fault_after(j, rng() % 5);
                }
            } else if (op < 78) {
                // Cancel something: a real job, or an identifier that means
                // nothing at all.
                const JobId j = (rng() % 8 == 0) ? (jobs.empty() ? 1 : jobs.back() + 100)
                                                 : (jobs.empty() ? 1 : jobs[rng() % jobs.size()]);
                c.cancel(j);
            } else if (op < 96) {
                c.step();
            } else {
                c.drain();
            }
            // Frame accounting must agree with the arena at every quiet point.
            check_eq(g_frames, static_cast<long long>(c.arena().live_slots()),
                     "frames alive match slots checked out (seed " + std::to_string(seed) +
                         " op " + std::to_string(step_no) + ")");
            check(c.arena().live_slots() <= c.arena().capacity(), "within capacity");
            check_eq(static_cast<long long>(c.arena().double_reclaims()), 0, "no double reclaims");
            if (g_fail > 0) {
                return;
            }
        }
        // Settle everything that can still be settled.
        for (JobId j : jobs) {
            c.cancel(j);
        }
        c.drain();
        for (JobId j : jobs) {
            check(c.settled(j), "every job settled at the end");
            check_eq(static_cast<long long>(c.journal().count_for(Mark::JobSettled, j)), 1,
                     "settled exactly once");
            if (c.outcome_of(j) == Outcome::Faulted) {
                check_eq(static_cast<long long>(c.journal().count_for(Mark::FaultReported, j)), 1,
                         "one failure announcement");
            } else {
                check_eq(static_cast<long long>(c.journal().count_for(Mark::FaultReported, j)), 0,
                         "no failure announcement");
            }
            check_eq(static_cast<long long>(c.journal().count_for(Mark::FrameReleased, j)), 1,
                     "frames released once");
        }
        check_eq(static_cast<long long>(c.arena().live_slots()), 0, "arena empty at the end");
        check_eq(g_frames, 0, "no frames alive at the end");
        check_eq(static_cast<long long>(c.live_jobs()), 0, "no live jobs");
        // Nothing about a job is recorded after its frames went back.
        const auto& es = c.journal().entries();
        for (JobId j : jobs) {
            const std::size_t released = c.journal().first_index(Mark::FrameReleased, j);
            for (std::size_t i = released; i < es.size(); ++i) {
                if (es[i].job != j) {
                    continue;
                }
                // Only requests and queueings for a settled job may follow.
                check(es[i].mark == Mark::CancelRequested || es[i].mark == Mark::TicketQueued ||
                          es[i].mark == Mark::TicketRejected,
                      "no lifecycle mark after frame released (got " +
                          std::string(mark_name(es[i].mark)) + ")");
            }
        }
    }
    g_c = nullptr;
    check_eq(g_frames, 0, "no frames left after the conductor is gone");
}

}  // namespace

int main() {
    for (std::uint32_t seed = 1; seed <= 40; ++seed) {
        run(seed);
        if (g_fail > 0) {
            std::printf("stopping at seed %u\n", seed);
            break;
        }
    }
    std::printf("%d checks, %d failures\n", g_checks, g_fail);
    return g_fail == 0 ? 0 : 1;
}
