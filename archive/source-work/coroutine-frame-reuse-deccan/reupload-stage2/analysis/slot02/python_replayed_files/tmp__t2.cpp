// Scratch test harness, round two. Not part of the deliverable.
#include <cstdio>
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

// -------------------------------------------------------------------------
// Destruction order: each level records when it goes.
static std::vector<int> death_order;
struct Marker {
    int id;
    explicit Marker(int i) : id(i) {}
    ~Marker() { death_order.push_back(id); }
};

static Duty leaf(int id) {
    Marker m{id};
    co_return co_await park();
}
static Duty mid(int id) {
    Marker m{id};
    const std::int64_t v = co_await leaf(id + 1);
    co_return v + 1;
}
static Duty top(int id) {
    Marker m{id};
    const std::int64_t v = co_await mid(id + 1);
    co_return v + 1;
}

static void t_destruction_order_value() {
    std::printf("[destruction order: value path]\n");
    death_order.clear();
    {
        Conductor c{4};
        const JobId j = c.launch(top(1));
        c.post(j, 1);
        c.drain();
        eq(c.value_of(j).value_or(0), 3, "value 3");
    }
    // innermost first: 3 (leaf), 2 (mid), 1 (top)
    check(death_order.size() == 3, "three guards destroyed");
    std::printf("  order=%zu", death_order.size());
    for (int v : death_order) std::printf(" %d", v);
    std::printf("\n");
    check(death_order.size() == 3 && death_order[0] == 3 && death_order[1] == 2 &&
              death_order[2] == 1,
          "innermost frame first");
}

static void t_destruction_order_cancel() {
    std::printf("[destruction order: cancel path]\n");
    death_order.clear();
    {
        Conductor c{4};
        const JobId j = c.launch(top(1));
        c.cancel(j);
        check(c.outcome_of(j) == Outcome::Cancelled, "cancelled");
        eq((long long)c.arena().live_slots(), 0, "all storage back");
    }
    check(death_order.size() == 3 && death_order[0] == 3 && death_order[1] == 2 &&
              death_order[2] == 1,
          "innermost frame first on cancel");
}

static void t_destruction_order_fault() {
    std::printf("[destruction order: fault path]\n");
    death_order.clear();
    {
        Conductor c{4};
        const JobId j = c.launch(top(1));
        c.post_fault_after(j, 4);
        c.drain();
        check(c.outcome_of(j) == Outcome::Faulted, "faulted");
        eq((long long)c.now(), 4, "clock followed the delayed fault");
        eq((long long)c.arena().live_slots(), 0, "all storage back");
    }
    check(death_order.size() == 3 && death_order[0] == 3, "innermost frame first on fault");
}

// -------------------------------------------------------------------------
// A child whose frame cannot be allocated.
static Duty parent_of_many() {
    std::int64_t total = 0;
    for (int i = 0; i < 4; ++i) {
        total += co_await leaf(99);
    }
    co_return total;
}

static void t_child_alloc_failure() {
    std::printf("[child frame cannot be allocated]\n");
    Conductor c{1};  // room for the root only
    const JobId j = c.launch(parent_of_many());
    c.post(j, 1);
    c.drain();
    // The job faults: the failure is reported once and storage comes back.
    check(c.outcome_of(j) == Outcome::Faulted, "faulted");
    eq((long long)c.journal().count_for(Mark::FaultReported, j), 1, "reported once");
    eq((long long)c.journal().count_for(Mark::JobSettled, j), 1, "settled once");
    eq((long long)c.arena().live_slots(), 0, "storage back");
    eq((long long)c.arena().double_reclaims(), 0, "no double reclaim");
}

// -------------------------------------------------------------------------
static Duty simple() { co_return co_await park(); }

static void t_unlaunched_duty() {
    std::printf("[a Duty that is never launched]\n");
    Conductor c{2};
    {
        Duty d = simple();
        eq((long long)c.arena().live_slots(), 1, "frame taken");
    }
    eq((long long)c.arena().live_slots(), 0, "frame returned");
    eq((long long)c.arena().double_reclaims(), 0, "no double reclaim");
}

// -------------------------------------------------------------------------
// A child that swallows the abandonment and returns anyway.
static Duty swallow() {
    try {
        const std::int64_t v = co_await simple();
        co_return v;
    } catch (const Abandoned&) {
        co_return -7;
    }
}

static void t_swallow_abandon() {
    std::printf("[child swallows abandonment]\n");
    Conductor c{4};
    const JobId j = c.launch(swallow());
    c.cancel(j);
    check(c.settled(j), "settled");
    check(c.outcome_of(j) == Outcome::Cancelled, "still cancelled");
    check(!c.value_of(j).has_value(), "no value");
    eq((long long)c.arena().live_slots(), 0, "storage back");
    eq((long long)c.journal().count_for(Mark::JobSettled, j), 1, "settled once");
    eq((long long)c.journal().count_for(Mark::FaultReported, j), 0, "no fault reported");
}

// -------------------------------------------------------------------------
// Exact delivery order with ties on all three keys.
static Duty two_parks() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a * 10 + b;
}

static void t_queue_order() {
    std::printf("[queue storage order is not the tie-break]\n");
    Conductor c{8};
    const JobId a = c.launch(simple());
    const JobId b = c.launch(simple());
    const JobId d = c.launch(simple());
    // Queue in reverse id order with equal ready times, plus a later one.
    c.post(d, 1);
    c.post(b, 2);
    c.post(a, 3);
    std::vector<JobId> served;
    while (c.step()) {
        const auto& es = c.journal().entries();
        for (std::size_t i = es.size(); i-- > 0;) {
            if (es[i].mark == Mark::TicketAccepted) {
                served.push_back(es[i].job);
                break;
            }
        }
        break;  // one per step; re-scan is not needed because marks append
    }
    eq((long long)served.size(), 1, "one per step");
    eq((long long)served[0], (long long)a, "lowest id first");
    c.step();
    c.step();
    eq((long long)c.live_jobs(), 0, "all three settled");
    eq(c.value_of(a).value_or(0), 3, "a got 3");
    eq(c.value_of(b).value_or(0), 2, "b got 2");
    eq(c.value_of(d).value_or(0), 1, "d got 1");
}

static void t_two_parks_one_frame() {
    std::printf("[two waits in the same frame]\n");
    Conductor c{4};
    const JobId j = c.launch(two_parks());
    c.post_after(j, 1, 2);
    c.step();
    eq((long long)c.now(), 2, "clock at 2");
    check(!c.settled(j), "still running");
    // queued while the job is parked, after it came back: bound to the same frame
    c.post_after(j, 2, 3);
    c.step();
    eq((long long)c.now(), 5, "clock at 5");
    eq(c.value_of(j).value_or(0), 12, "1*10+2");
    eq((long long)c.arena().live_slots(), 0, "storage back");
}

// -------------------------------------------------------------------------
// Many jobs, interleaved, every slot recycled repeatedly.
static void t_churn() {
    std::printf("[churn: many short jobs in a small arena]\n");
    Conductor c{2};
    for (int round = 0; round < 50; ++round) {
        const JobId a = c.launch(simple());
        c.post(a, round);
        c.drain();
        if (c.value_of(a).value_or(-1) != round) {
            check(false, "churn value");
            return;
        }
        const JobId b = c.launch(simple());
        c.cancel(b);
        if (c.outcome_of(b) != Outcome::Cancelled) {
            check(false, "churn cancel");
            return;
        }
        if (c.arena().live_slots() != 0) {
            check(false, "churn leak");
            return;
        }
    }
    check(true, "churn ok");
    eq((long long)c.arena().double_reclaims(), 0, "no double reclaim");
    check(c.arena().total_check_outs() >= 100, "slots genuinely recycled");
    eq((long long)c.arena().peak_live(), 1, "never more than one frame at a time here");
    check(c.arena().live_slot_ids().empty(), "nothing checked out");
}

// -------------------------------------------------------------------------
// Two conductors alive at once (nested scopes), each with its own arena.
static void t_two_conductors() {
    std::printf("[two conductors]\n");
    Conductor outer{2};
    {
        Conductor inner{2};
        const JobId j = inner.launch(simple());
        inner.post(j, 4);
        inner.drain();
        eq(inner.value_of(j).value_or(0), 4, "inner works");
    }
    const JobId k = outer.launch(simple());
    outer.post(k, 6);
    outer.drain();
    eq(outer.value_of(k).value_or(0), 6, "outer still works");
}

// -------------------------------------------------------------------------
// A job left parked when the conductor is destroyed.
static void t_destroy_parked() {
    std::printf("[conductor destroyed with jobs parked]\n");
    Conductor c{4};
    const JobId a = c.launch(top(1));
    const JobId b = c.launch(simple());
    c.post(b, 3);  // never stepped
    (void)a;
    // No explicit cancel: the destructor has to give every slot back.
}

int main() {
    t_destruction_order_value();
    t_destruction_order_cancel();
    t_destruction_order_fault();
    t_child_alloc_failure();
    t_unlaunched_duty();
    t_swallow_abandon();
    t_queue_order();
    t_two_parks_one_frame();
    t_churn();
    t_two_conductors();
    t_destroy_parked();
    std::printf("\n%d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}
