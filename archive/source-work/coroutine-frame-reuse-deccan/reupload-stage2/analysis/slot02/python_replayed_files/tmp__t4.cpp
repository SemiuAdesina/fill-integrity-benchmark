// Scratch test harness, round four: re-parking and cross-job cancellation.
#include <cstdio>
#include <cstdlib>
#include <exception>

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

static Duty simple() { co_return co_await park(); }
static Duty two_parks() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a * 10 + b;
}

// Both tickets are queued while the job waits in one frame; the second has to
// arrive when the job waits in that same frame again.
static void t_repark_same_frame() {
    std::printf("[two tickets for one frame]\n");
    Conductor c{4};
    const JobId j = c.launch(two_parks());
    c.post(j, 1);
    c.post(j, 2);
    eq((long long)c.pending_tickets(), 2, "two queued");
    c.drain();
    eq((long long)c.journal().count_for(Mark::TicketAccepted, j), 2, "both accepted");
    eq((long long)c.journal().count_for(Mark::TicketRejected, j), 0, "none rejected");
    eq(c.value_of(j).value_or(0), 12, "1 then 2");
    eq((long long)c.arena().live_slots(), 0, "storage back");
}

// Job 1 cancels job 2 while job 1 is being resumed by a delivery.
static Duty canceller(Conductor* c, JobId victim) {
    const std::int64_t v = co_await park();
    c->cancel(victim);
    co_return v;
}

static void t_cancel_other_from_inside() {
    std::printf("[cancel another job from inside a delivery]\n");
    Conductor c{4};
    const JobId victim = c.launch(simple());
    const JobId j = c.launch(canceller(&c, victim));
    c.post(j, 9);
    c.step();
    check(c.settled(victim), "victim settled during the other job's step");
    check(c.outcome_of(victim) == Outcome::Cancelled, "victim cancelled");
    eq((long long)c.arena().live_slots(), 0, "both frames released");
    eq(c.value_of(j).value_or(0), 9, "canceller delivered");
    eq((long long)c.journal().count_for(Mark::CancelRequested, victim), 1, "request recorded");
}

// A job that outlives a cancelled sibling keeps working.
static Duty three_parks() {
    std::int64_t t = co_await park();
    t += co_await park();
    t += co_await park();
    co_return t;
}

static void t_interleaved() {
    std::printf("[interleaved jobs, one cancelled midway]\n");
    Conductor c{4};
    const JobId a = c.launch(three_parks());
    const JobId b = c.launch(three_parks());
    c.post(a, 1);
    c.post_after(b, 10, 2);
    c.step();  // a at 0
    c.step();  // a? no: a has no ticket now; b's is ready at 2
    eq((long long)c.now(), 2, "clock at 2");
    c.cancel(b);
    c.post(a, 2);
    c.post(a, 3);
    c.drain();
    eq(c.value_of(a).value_or(0), 6, "a finished");
    check(c.outcome_of(b) == Outcome::Cancelled, "b cancelled");
    eq((long long)c.arena().live_slots(), 0, "storage back");
    eq((long long)c.live_jobs(), 0, "no live jobs");
}

int main() {
    t_repark_same_frame();
    t_cancel_other_from_inside();
    t_interleaved();
    std::printf("\n%d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}
