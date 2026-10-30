//
// Visible diagnostic program.
//
// Three short scenarios that show the runtime misbehaving. Each one prints the
// journal and a few counters. This program is for your own investigation; it is
// not what the work is graded on, and you may change or delete it.
//
// Build and run:
//   cmake -S /app -B /app/build -DCMAKE_BUILD_TYPE=Debug
//   cmake --build /app/build
//   /app/build/roundel_demo

#include <cstdio>
#include <exception>
#include <string>

#include "roundel/runtime.hpp"

using namespace roundel;

namespace {

Duty simple_job() { co_return co_await park(); }

Duty child_job() { co_return co_await park(); }

Duty parent_job() {
    const std::int64_t v = co_await child_job();
    co_return v * 2;
}

void banner(const char* title) { std::printf("\n=== %s ===\n", title); }

void report(const Conductor& c) {
    std::printf("%s", c.journal().render().c_str());
    std::printf("[arena] live=%zu free=%zu capacity=%zu peak=%zu check_outs=%zu "
                "double_reclaims=%zu\n",
                c.arena().live_slots(), c.arena().free_slots(), c.arena().capacity(),
                c.arena().peak_live(), c.arena().total_check_outs(),
                c.arena().double_reclaims());
}

// A cancelled job releases its storage, a new job takes the same slot, and the
// ticket queued for the old job is delivered afterwards.
void scenario_recycled_slot() {
    banner("cancelled job, slot reused, old ticket delivered late");
    Conductor c{1};
    const JobId first = c.launch(simple_job());
    c.post(first, 11);
    c.cancel(first);
    std::printf("[after cancel] settled=%d live_slots=%zu\n", c.settled(first) ? 1 : 0,
                c.arena().live_slots());

    const JobId second = c.launch(simple_job());
    std::printf("[second launched] id_differs=%d\n", first != second ? 1 : 0);
    c.drain();
    std::printf("[second] settled=%d\n", c.settled(second) ? 1 : 0);
    report(c);
}

// A job is cancelled and then a completion arrives, then cancelled again.
void scenario_cancel_then_complete() {
    banner("cancel, then completion, then cancel again");
    Conductor c{4};
    const JobId job = c.launch(simple_job());
    c.cancel(job);
    c.post(job, 7);
    c.cancel(job);
    c.drain();
    report(c);
}

// A parent awaits a child that throws.
void scenario_child_throws() {
    banner("child fails");
    Conductor c{4};
    const JobId job = c.launch(parent_job());
    c.post_fault(job);
    c.drain();
    auto o = c.outcome_of(job);
    std::printf("[outcome] %s\n", o ? outcome_name(*o) : "still running");
    report(c);
}

// A parent awaits a child that produces a value. `live` on parent-continued is
// how many arena slots were checked out at the moment the parent read the
// child's result.
void scenario_child_completes() {
    banner("child succeeds");
    Conductor c{4};
    const JobId job = c.launch(parent_job());
    c.post(job, 21);
    c.drain();
    auto v = c.value_of(job);
    std::printf("[value] %s\n", v ? std::to_string(*v).c_str() : "none");
    report(c);
}

}  // namespace

int main() {
    try {
        scenario_recycled_slot();
    } catch (const std::exception& e) {
        std::printf("scenario 1 threw: %s\n", e.what());
    }
    try {
        scenario_cancel_then_complete();
    } catch (const std::exception& e) {
        std::printf("scenario 2 threw: %s\n", e.what());
    }
    try {
        scenario_child_throws();
    } catch (const std::exception& e) {
        std::printf("scenario 3 threw: %s\n", e.what());
    }
    try {
        scenario_child_completes();
    } catch (const std::exception& e) {
        std::printf("scenario 4 threw: %s\n", e.what());
    }
    std::printf("\ndemo finished\n");
    return 0;
}
