// Further edge cases and a randomised schedule. Not part of the deliverable.
#include "trace.hpp"

#include <functional>
#include <random>
#include <stdexcept>
#include <vector>

namespace {

Duty immediate() { co_return 42; }

Duty child_now() { co_return 3; }

Duty parent_of_immediate() {
    const std::int64_t v = co_await child_now();
    co_return v + 1;
}

Duty simple() { co_return co_await park(); }

Duty thrower() {
    co_await park();
    throw std::runtime_error("boom");
}

Duty parent_one() {
    const std::int64_t v = co_await child();
    co_return v * 2;
}

Duty child() { co_return co_await park(); }

Duty catcher() {
    try {
        co_await park();
    } catch (...) {
    }
    co_return 5;
}

void t_immediate() {
    Conductor c{4};
    const JobId j = c.launch(immediate());
    check(c.settled(j), "settled inside launch");
    check_eq((long long)*c.value_of(j), 42, "value");
    check_eq((long long)c.arena().live_slots(), 0, "storage released at once");
    check_eq(c.journal().count_for(Mark::JobSettled, j), 1, "settled once");
    check_eq(c.journal().count_for(Mark::FrameReleased, j), 1, "released once");
    check_eq((long long)c.now(), 0, "clock still 0");
    audit_journal(c, "immediate");
}

void t_immediate_child() {
    Conductor c{4};
    const JobId j = c.launch(parent_of_immediate());
    check(c.settled(j), "settled inside launch");
    check_eq((long long)*c.value_of(j), 4, "value");
    check_eq((long long)c.arena().live_slots(), 0, "both slots back");
    check_eq(c.journal().count_for(Mark::ChildDestroyed, j), 1, "child destroyed");
    check_eq(c.journal().count_for(Mark::ParentContinued, j), 1, "parent continued");
    audit_journal(c, "immediate child");
}

void t_fault_later() {
    Conductor c{4};
    const JobId j = c.launch(simple());
    c.post_fault_after(j, 6);
    check(!c.settled(j), "nothing until a step");
    check(c.step(), "delivered");
    check_eq((long long)c.now(), 6, "clock at the ready time");
    check(outcome_str(c.outcome_of(j)) == "F", "faulted");
    check_eq((long long)c.arena().live_slots(), 0, "storage back");
    audit_journal(c, "fault later");
}

void t_fault_and_independence() {
    Conductor c{8};
    const JobId a = c.launch(thrower());
    const JobId b = c.launch(simple());
    c.post(a, 1);
    c.post(b, 2);
    c.step();
    c.step();
    check(outcome_str(c.outcome_of(a)) == "F", "a faulted");
    check(outcome_str(c.outcome_of(b)) == "D", "b unaffected");
    check_eq((long long)*c.value_of(b), 2, "b value");
    check_eq((long long)c.arena().live_slots(), 0, "all back");
    audit_journal(c, "independence of faults");
}

void t_many_jobs_reuse() {
    Conductor c{2};
    std::int64_t total = 0;
    for (int i = 0; i < 20; ++i) {
        const JobId j = c.launch(simple());
        c.post(j, i);
        c.drain();
        const auto v = c.value_of(j);
        check(v && *v == i, "each job got its own value");
        if (v) total += *v;
        check_eq((long long)c.arena().live_slots(), 0, "nothing held between jobs");
    }
    check_eq((long long)total, 190, "sum 0..19");
    check(c.arena().total_check_outs() >= 20, "storage was reused");
    check(c.arena().peak_live() <= 2, "never above capacity");
    check_eq((long long)c.arena().capacity(), 2, "capacity unchanged");
    audit_journal(c, "reuse");
}

// ------------------------------------------------------------------ fuzz ---

using Factory = std::function<Duty()>;

struct World {
    Conductor c{6};
    std::vector<JobId> ids;
    std::mt19937 rng{12345};
    std::size_t accepted_total = 0;

    JobId launch_one() {
        static const std::vector<Factory> fs = {
            [] { return simple(); },        [] { return parent_one(); },
            [] { return catcher(); },       [] { return thrower(); },
            [] { return parent_one(); },    [] { return simple(); },
            [] { return parent_of_immediate(); },
        };
        const Factory& f = fs[std::uniform_int_distribution<std::size_t>(0, fs.size() - 1)(rng)];
        JobId id = kNoJob;
        try {
            id = c.launch(f());
        } catch (const std::bad_alloc&) {
            return kNoJob;  // pool exhausted, which is allowed
        }
        ids.push_back(id);
        return id;
    }

    JobId pick() {
        if (ids.empty()) return kNoJob;
        return ids[std::uniform_int_distribution<std::size_t>(0, ids.size() - 1)(rng)];
    }
};

void t_fuzz() {
    for (int round = 0; round < 60; ++round) {
        World w;
        std::mt19937& rng = w.rng;
        for (int i = 0; i < 6; ++i) w.launch_one();

        for (int op = 0; op < 120; ++op) {
            const int what = std::uniform_int_distribution<int>(0, 99)(rng);
            const JobId j = (what < 30) ? w.launch_one() : w.pick();
            if (j == kNoJob) continue;

            if (what < 30) {
                // freshly launched; maybe queue something for it
                if (std::uniform_int_distribution<int>(0, 1)(rng)) w.c.post(j, op);
            } else if (what < 55) {
                switch (std::uniform_int_distribution<int>(0, 3)(rng)) {
                    case 0: w.c.post(j, op); break;
                    case 1: w.c.post_after(j, op, std::uniform_int_distribution<unsigned>(0, 5)(rng)); break;
                    case 2: w.c.post_fault(j); break;
                    default: w.c.post_fault_after(j, std::uniform_int_distribution<unsigned>(0, 5)(rng)); break;
                }
            } else if (what < 68) {
                w.c.cancel(j);
            } else if (what < 72) {
                w.c.cancel(std::uniform_int_distribution<std::uint64_t>(500, 600)(rng));
            } else if (what < 97) {
                const std::uint64_t before = w.c.now();
                const bool did = w.c.step();
                if (did) {
                    check(w.c.now() >= before, "clock never goes back");
                }
                check(w.c.arena().live_slots() <= w.c.arena().capacity(), "within capacity");
            } else {
                w.c.drain();
            }

            // invariants after every operation
            check_eq((long long)w.c.arena().double_reclaims(), 0, "no double reclaim");
            check(w.c.arena().live_slots() <= w.c.arena().capacity(), "capacity respected");
            for (JobId id : w.ids) {
                check(w.c.journal().count_for(Mark::JobSettled, id) <= 1, "settled once");
                check(w.c.journal().count_for(Mark::FrameReleased, id) <= 1, "released once");
                check(w.c.journal().count_for(Mark::FaultReported, id) <= 1, "fault once");
                if (w.c.settled(id)) {
                    const auto o = w.c.outcome_of(id);
                    check(o.has_value(), "settled has an outcome");
                    if (o && *o != Outcome::Delivered) {
                        check(!w.c.value_of(id).has_value(), "no value unless delivered");
                    }
                }
            }
        }
        audit_journal(w.c, "fuzz round");
    }
}

}  // namespace

int main() {
    t_immediate();
    t_immediate_child();
    t_fault_later();
    t_fault_and_independence();
    t_many_jobs_reuse();
    t_fuzz();
    std::printf("%d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
