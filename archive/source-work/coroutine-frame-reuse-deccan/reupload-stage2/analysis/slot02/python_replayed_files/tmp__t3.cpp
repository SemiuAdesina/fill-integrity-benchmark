// Scratch test harness, round three: global journal invariants.
#include <cstdio>
#include <cstdlib>
#include <exception>
#include <string>
#include <vector>

#include "roundel/runtime.hpp"

using namespace roundel;

static int failures = 0;
static int checks = 0;
static void check(bool ok, const char* what) {
    ++checks;
    if (!ok) {
        ++failures;
        std::printf("  FAIL: %s\n", what);
    }
}
static void eq(long long got, long long want, const char* what) {
    ++checks;
    if (got != want) {
        ++failures;
        std::printf("  FAIL: %s (got %lld want %lld)\n", what, got, want);
    }
}

// Every job in the journal obeys the ordering rules.
static void invariants(const Conductor& c, const char* what) {
    const auto& es = c.journal().entries();
    std::vector<JobId> ids;
    for (const auto& e : es) {
        bool known = false;
        for (JobId id : ids) known = known || id == e.job;
        if (!known && e.job != kNoJob) ids.push_back(e.job);
    }
    for (JobId id : ids) {
        const std::size_t started = c.journal().first_index(Mark::JobStarted, id);
        if (started == es.size()) continue;
        for (std::size_t i = 0; i < es.size(); ++i) {
            if (es[i].job != id || es[i].mark == Mark::JobStarted) continue;
            if (i < started && es[i].mark != Mark::CancelRequested) {
                check(false, "mark before job-started");
                return;
            }
        }
        if (c.journal().count_for(Mark::JobStarted, id) != 1) {
            std::printf("  [%s] job %llu started %zu times\n", what, (unsigned long long)id,
                        c.journal().count_for(Mark::JobStarted, id));
            check(false, "started once");
        }
        if (c.journal().count_for(Mark::JobSettled, id) > 1) {
            std::printf("  [%s] job %llu settled %zu times\n", what, (unsigned long long)id,
                        c.journal().count_for(Mark::JobSettled, id));
            check(false, "settled once");
        }
        if (c.journal().count_for(Mark::FrameReleased, id) > 1) {
            check(false, "released once");
        }
        if (c.journal().count_for(Mark::FaultReported, id) > 1) {
            std::printf("  [%s] job %llu fault reported %zu times\n", what,
                        (unsigned long long)id, c.journal().count_for(Mark::FaultReported, id));
            check(false, "fault reported once");
        }
        const std::size_t released = c.journal().first_index(Mark::FrameReleased, id);
        if (released != es.size()) {
            const std::size_t settled = c.journal().first_index(Mark::JobSettled, id);
            if (settled == es.size() || settled > released) check(false, "settled before release");
            for (std::size_t i = released + 1; i < es.size(); ++i) {
                if (es[i].job == id && es[i].mark != Mark::CancelRequested) {
                    std::printf("  [%s] job %llu has %s after frame-released\n", what,
                                (unsigned long long)id, mark_name(es[i].mark));
                    check(false, "nothing after frame-released");
                }
            }
        }
        // A job that accepted a ticket must have parked first.
        for (std::size_t i = 0; i < es.size(); ++i) {
            if (es[i].job != id || es[i].mark != Mark::TicketAccepted) continue;
            bool parked = false;
            for (std::size_t k = 0; k < i; ++k) {
                if (es[k].job == id && es[k].mark == Mark::JobParked) parked = true;
            }
            if (!parked) check(false, "parked before accepted");
        }
    }
    check(true, "invariants");
}

// -------------------------------------------------------------------------

static Duty simple() { co_return co_await park(); }
static Duty child() { co_return co_await park(); }

static Duty four_children() {
    std::int64_t total = 0;
    for (int i = 0; i < 4; ++i) total += co_await child();
    co_return total;
}

static Duty nested() {
    const std::int64_t a = co_await child();
    const std::int64_t b = co_await child();
    co_return a + b;
}

static Duty mixed() {
    std::int64_t total = co_await park();
    try {
        total += co_await child();
    } catch (const std::exception&) {
        total += 100;
    }
    total += co_await park();
    co_return total;
}

static Duty fanout(Conductor* c) {
    // Launch a second job from inside a running one.
    const JobId other = c->launch(simple());
    c->post(other, 3);
    const std::int64_t v = co_await park();
    co_return v + (c->settled(other) ? 1 : 0);
}

static void t_peak() {
    std::printf("[peak live slots]\n");
    Conductor c{4};
    const JobId j = c.launch(four_children());
    for (int i = 0; i < 4; ++i) {
        c.post_after(j, 1, static_cast<std::uint64_t>(i + 1));
    }
    c.drain();
    eq(c.value_of(j).value_or(0), 4, "four children");
    eq((long long)c.arena().peak_live(), 2, "never more than a parent and one child");
    eq((long long)c.arena().live_slots(), 0, "storage back");
    invariants(c, "peak");
}

static void t_nested_cancel_then_more() {
    std::printf("[cancel one of two jobs, keep going]\n");
    Conductor c{4};
    const JobId a = c.launch(nested());
    const JobId b = c.launch(nested());
    c.post(a, 1);
    c.cancel(b);
    eq((long long)c.arena().live_slots(), 1, "only a's frames left");
    c.post(a, 2);
    c.drain();
    eq(c.value_of(a).value_or(0), 3, "a finished normally");
    check(c.outcome_of(b) == Outcome::Cancelled, "b cancelled");
    eq((long long)c.arena().live_slots(), 0, "storage back");
    invariants(c, "two jobs");
}

static void t_mixed() {
    std::printf("[park, child that fails, park again]\n");
    Conductor c{4};
    const JobId j = c.launch(mixed());
    c.post(j, 1);
    c.step();
    c.post_fault(j);  // queued while parked in the child
    c.step();
    eq((long long)c.journal().count_for(Mark::FaultReported, j), 1, "fault reported once");
    check(!c.settled(j), "still running after catching");
    c.post(j, 5);
    c.drain();
    eq(c.value_of(j).value_or(0), 106, "1 + 100 + 5");
    eq((long long)c.arena().live_slots(), 0, "storage back");
    invariants(c, "mixed");
}

static void t_launch_from_inside() {
    std::printf("[launch and post from inside a running job]\n");
    Conductor c{4};
    const JobId j = c.launch(fanout(&c));
    eq((long long)c.live_jobs(), 2, "two jobs live");
    c.post(j, 10);
    c.drain();
    eq(c.value_of(j).value_or(0), 11, "child of the step finished");
    eq((long long)c.live_jobs(), 0, "both settled");
    eq((long long)c.arena().live_slots(), 0, "storage back");
    invariants(c, "fanout");
}

static void t_delayed_fault_deep() {
    std::printf("[delayed fault into a nested job]\n");
    Conductor c{4};
    const JobId j = c.launch(nested());
    c.post_after(j, 7, 5);
    eq((long long)c.step(), 1, "step ran");
    eq((long long)c.now(), 5, "clock followed the delivery");
    c.post_fault_after(j, 2);
    c.drain();
    eq((long long)c.now(), 7, "clock at the later ready time");
    check(c.outcome_of(j) == Outcome::Faulted, "faulted");
    eq((long long)c.arena().live_slots(), 0, "storage back");
    invariants(c, "delayed fault");
}

static void t_many_rounds() {
    std::printf("[many rounds of everything]\n");
    for (int round = 0; round < 30; ++round) {
        Conductor c{3};
        const JobId a = c.launch(mixed());
        const JobId b = c.launch(nested());
        const JobId d = c.launch(simple());
        c.post_after(a, 1, 1);
        c.post_after(b, 2, 1);
        c.post_after(d, 3, 2);
        if (round % 3 == 1) c.cancel(b);
        if (round % 5 == 2) c.post_fault_after(d, 1);
        c.drain();
        invariants(c, "rounds");
        if (c.arena().live_slots() != 0) {
            check(false, "storage back every round");
            return;
        }
        if (c.arena().double_reclaims() != 0) {
            check(false, "no double reclaims");
            return;
        }
        if (c.arena().peak_live() > 3) {
            check(false, "peak within capacity");
            return;
        }
    }
    check(true, "thirty rounds");
}

int main() {
    t_peak();
    t_nested_cancel_then_more();
    t_mixed();
    t_launch_from_inside();
    t_delayed_fault_deep();
    t_many_rounds();
    std::printf("\n%d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}
