// Spec conformance tests for roundel (my own harness; not part of the deliverable).
#include "roundel/runtime.hpp"

#include <cstdio>
#include <stdexcept>
#include <string>
#include <vector>

using namespace roundel;

static int g_failures = 0;
static int g_checks = 0;

#define CHECK(cond)                                                              \
    do {                                                                         \
        ++g_checks;                                                              \
        if (!(cond)) {                                                           \
            ++g_failures;                                                        \
            std::printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);          \
        }                                                                        \
    } while (0)

#define CHECK_EQ(a, b)                                                           \
    do {                                                                         \
        ++g_checks;                                                              \
        auto va = (a);                                                           \
        auto vb = (b);                                                           \
        if (!(va == vb)) {                                                       \
            ++g_failures;                                                        \
            std::printf("FAIL %s:%d: %s == %s (%lld vs %lld)\n", __FILE__,       \
                        __LINE__, #a, #b, (long long)va, (long long)vb);         \
        }                                                                        \
    } while (0)

// ---------------------------------------------------------------- tracing --
std::vector<int> g_log;

struct Tracer {
    explicit Tracer(int id) : id_(id) {}
    ~Tracer() { g_log.push_back(id_); }
    int id_;
};

// ------------------------------------------------------------------ duties --
Duty leaf() { co_return co_await park(); }

Duty park_twice() {
    const std::int64_t a = co_await park();
    const std::int64_t b = co_await park();
    co_return a * 10 + b;
}

Duty thrower() {
    co_await park();
    throw std::runtime_error("boom");
    co_return 0;  // not reached
}

Duty catcher() {
    std::int64_t v = 0;
    try {
        v = co_await park();
    } catch (const std::exception&) {
        v = 5;
    }
    co_return v;
}

Duty parent_leaf() {
    const std::int64_t v = co_await leaf();
    co_return v * 2;
}

Duty parent_thrower() {
    const std::int64_t v = co_await thrower();
    co_return v;
}

Duty parent_catches_thrower() {
    std::int64_t v = 0;
    try {
        v = co_await thrower();
    } catch (const std::exception&) {
        v = 7;
    }
    co_return v;
}

Duty two_children() {
    const std::int64_t a = co_await leaf();
    const std::int64_t b = co_await leaf();
    co_return a + b;
}

Duty self_poster(Conductor* c, JobId me) {
    co_await park();
    c->post(me, 9);
    const std::int64_t v = co_await park();
    co_return v;
}

Duty canceller(Conductor* c, JobId victim) {
    co_await park();
    c->cancel(victim);
    co_return 1;
}

Duty self_canceller(Conductor* c, JobId me) {
    co_await park();
    c->cancel(me);
    co_return 3;
}

// nested with destruction tracers
Duty t_inner() {
    Tracer t{3};
    co_return co_await park();
}
Duty t_middle() {
    Tracer t{2};
    co_return co_await t_inner() + 1;
}
Duty t_outer() {
    Tracer t{1};
    co_return co_await t_middle() + 1;
}

// ------------------------------------------------------------------ trace --
struct Trace {
    Conductor& c;
    std::size_t pos = 0;
    std::string s;

    explicit Trace(Conductor& cc) : c(cc) {}

    void flush(std::uint64_t now) {
        const auto& es = c.journal().entries();
        for (; pos < es.size(); ++pos) {
            const Entry& e = es[pos];
            switch (e.mark) {
                case Mark::TicketAccepted:
                    s += "A" + std::to_string(e.job) + "@" + std::to_string(now) + ";";
                    break;
                case Mark::TicketRejected:
                    s += "R" + std::to_string(e.job) + "@" + std::to_string(now) + ";";
                    break;
                case Mark::ChildDestroyed:
                    s += "C" + std::to_string(e.job) + ";";
                    break;
                case Mark::ParentContinued:
                    s += "P" + std::to_string(e.job) + ":" + std::to_string(e.detail) + ";";
                    break;
                case Mark::JobSettled:
                    s += "S" + std::to_string(e.job) + "=" +
                         std::string(1, outcome_name(static_cast<Outcome>(e.detail))[0] == 'd'
                                            ? 'D'
                                            : (outcome_name(static_cast<Outcome>(e.detail))[0] == 'c'
                                                   ? 'X'
                                                   : 'F')) +
                         ";";
                    break;
                default:
                    break;
            }
        }
    }
    void after_step() { flush(c.now()); }
    std::string all() {
        flush(c.now());
        if (!s.empty() && s.back() == ';') s.pop_back();
        return s;
    }
};

// ------------------------------------------------------------------ tests --
void t_cancel_releases_and_recycle() {
    Conductor c{1};
    const JobId a = c.launch(leaf());
    c.post(a, 11);
    c.cancel(a);
    CHECK(c.settled(a));
    CHECK_EQ(*c.outcome_of(a), Outcome::Cancelled);
    CHECK_EQ(c.arena().live_slots(), 0u);          // released at once
    CHECK_EQ(c.arena().free_slots(), 1u);
    const JobId b = c.launch(leaf());               // slot reused
    CHECK(b != a);
    c.drain();
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, a), 1u);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, a), 0u);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, b), 0u);
    CHECK(!c.settled(b));                           // still parked, untouched
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, a), 1u);
    CHECK_EQ(c.journal().count(Mark::SlotReused), 1u);
    CHECK_EQ(c.arena().double_reclaims(), 0u);
}

void t_settle_once_cancel_then_ticket() {
    Conductor c{4};
    const JobId j = c.launch(leaf());
    c.cancel(j);
    c.post(j, 7);
    c.cancel(j);
    c.drain();
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, j), 1u);
    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, j), 2u);
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, j), 1u);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, j), 1u);
    CHECK_EQ(*c.outcome_of(j), Outcome::Cancelled);
    CHECK(!c.value_of(j).has_value());
    CHECK_EQ(c.arena().live_slots(), 0u);
    // unknown job
    c.cancel(4242);
    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, 4242), 1u);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, 4242), 0u);
}

void t_child_success() {
    Conductor c{4};
    const JobId j = c.launch(parent_leaf());
    c.post(j, 21);
    c.drain();
    CHECK_EQ(*c.outcome_of(j), Outcome::Delivered);
    CHECK_EQ(*c.value_of(j), 42);
    // child destroyed before parent continues, live count == 1 (only root)
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, j), 1u);
    CHECK_EQ(c.journal().count_for(Mark::ParentContinued, j), 1u);
    CHECK_LIVE(c, 1);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.arena().double_reclaims(), 0u);
}

void t_child_fault() {
    Conductor c{4};
    const JobId j = c.launch(parent_thrower());
    c.post(j, 1);
    c.drain();
    CHECK_EQ(*c.outcome_of(j), Outcome::Faulted);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, j), 1u);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, j), 1u);
    CHECK_EQ(c.journal().count_for(Mark::ParentContinued, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
    CHECK_EQ(c.arena().double_reclaims(), 0u);
}

void t_child_fault_caught() {
    Conductor c{4};
    const JobId j = c.launch(parent_catches_thrower());
    c.post(j, 1);
    c.drain();
    CHECK_EQ(*c.outcome_of(j), Outcome::Delivered);
    CHECK_EQ(*c.value_of(j), 7);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

void t_fault_then_catch() {
    Conductor c{4};
    const JobId j = c.launch(catcher());
    c.post_fault(j);
    c.drain();
    CHECK_EQ(*c.outcome_of(j), Outcome::Delivered);
    CHECK_EQ(*c.value_of(j), 5);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, j), 0u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

void t_uncaught_throw() {
    Conductor c{4};
    const JobId j = c.launch(thrower());
    c.post(j, 1);
    c.drain();
    CHECK_EQ(*c.outcome_of(j), Outcome::Faulted);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, j), 1u);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

void t_repark_same_frame() {
    Conductor c{4};
    const JobId j = c.launch(park_twice());
    c.post_after(j, 1, 5);   // seq 0, ready 5
    c.post(j, 2);            // seq 1, ready 0
    CHECK(c.step());
    CHECK_EQ(c.now(), 0u);
    CHECK(c.step());
    CHECK_EQ(c.now(), 5u);
    CHECK_EQ(*c.value_of(j), 21);
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, j), 0u);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, j), 2u);
}

void t_epoch_mismatch_same_index() {
    // capacity 2: root in slot 0, child in slot 1. The first child's slot is
    // handed back and handed out again to the second child: same index, new
    // epoch. A ticket bound to the first child must be refused.
    Conductor c{2};
    const JobId j = c.launch(two_children());
    c.post_after(j, 111, 5);   // bound to child #1 (1,1)
    c.post(j, 1);              // ready 0 -> resumes child #1
    CHECK(c.step());           // child #1 done; child #2 parks in slot 1 epoch 2
    CHECK(c.step());           // the late ticket: index matches, epoch does not
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, j), 1u);
    CHECK(!c.settled(j));
    CHECK_EQ(c.arena().live_slots(), 2u);
}

void t_index_mismatch_same_epoch() {
    // capacity 3: child #1 in slot 1, child #2 in slot 2, both epoch 1.
    Conductor c{3};
    const JobId j = c.launch(two_children());
    c.post_after(j, 111, 5);   // bound to child #1 (1,1)
    c.post(j, 1);
    CHECK(c.step());
    CHECK(c.step());           // parked in child #2 (2,1): epoch matches, index not
    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, j), 1u);
    CHECK(!c.settled(j));
}

void t_unbound_never_accepted() {
    Conductor c{4};
    const JobId j = c.launch(self_poster(&c, 0));
    // job id is not known inside; use the real one:
    c.cancel(j);
    Conductor c2{4};
    (void)c2;
}

void t_self_post_while_running() {
    Conductor c{4};
    // self_poster needs its own id: park first, then post using the id we know.
    JobId j = kNoJob;
    {
        Conductor d{4};
        j = d.launch(leaf());
        CHECK_EQ(j, 1u);
    }
    Conductor c1{4};
    const JobId me = c1.launch(self_poster(&c1, 1));
    CHECK_EQ(me, 1u);
    c1.post(me, 1);  // resumes the job
    CHECK(c1.step());
    // the job posted to itself while running: bound to no frame
    CHECK_EQ(c1.journal().count_for(Mark::TicketQueued, me), 2u);
    CHECK(c1.step());  // must be rejected even though the job parks again
    CHECK_EQ(c1.journal().count_for(Mark::TicketRejected, me), 1u);
    CHECK(!c1.settled(me));
    CHECK_EQ(c1.arena().live_slots(), 1u);
}

void t_order_and_clock() {
    Conductor c{8};
    const JobId a = c.launch(leaf());
    const JobId b = c.launch(leaf());
    c.post(b, 20);
    c.post(a, 10);            // same ready time, lower id wins
    CHECK(c.step());
    CHECK_EQ(c.now(), 0u);
    const auto& es = c.journal().entries();
    std::size_t first_accepted = 0;
    for (std::size_t i = 0; i < es.size(); ++i) {
        if (es[i].mark == Mark::TicketAccepted) {
            first_accepted = es[i].job;
            break;
        }
    }
    CHECK_EQ(first_accepted, (std::size_t)a);

    // sequence tie-break: two tickets for the same job at the same ready time
    Conductor d{4};
    const JobId j = d.launch(park_twice());
    d.post(j, 1);
    d.post(j, 2);
    CHECK(d.step());
    std::size_t accepted_value = 0;
    for (const auto& e : d.journal().entries()) {
        if (e.mark == Mark::TicketAccepted) accepted_value = static_cast<std::size_t>(e.detail);
    }
    CHECK_EQ(accepted_value, 1u);
    CHECK(d.step());
    CHECK_EQ(*d.value_of(j), 12);

    // ready time dominates
    Conductor e2{4};
    const JobId x = e2.launch(leaf());
    const JobId y = e2.launch(leaf());
    e2.post_after(x, 1, 10);
    e2.post(y, 2);
    CHECK(e2.step());
    CHECK_EQ(e2.now(), 0u);
    CHECK(e2.settled(y));
    CHECK(!e2.settled(x));
    CHECK(e2.step());
    CHECK_EQ(e2.now(), 10u);
    CHECK(e2.settled(x));
}

void t_delay_from_current_clock() {
    Conductor c{4};
    const JobId a = c.launch(leaf());
    const JobId b = c.launch(leaf());
    c.post_after(a, 1, 7);
    c.post(b, 0);
    CHECK(c.step());   // b at 0
    CHECK_EQ(c.now(), 0u);
    CHECK(c.step());   // a at 7
    CHECK_EQ(c.now(), 7u);
}

void t_independence() {
    Conductor c{4};
    const JobId a = c.launch(leaf());
    const JobId b = c.launch(leaf());
    c.cancel(a);
    c.post(b, 4);
    c.drain();
    CHECK_EQ(*c.outcome_of(a), Outcome::Cancelled);
    CHECK_EQ(*c.outcome_of(b), Outcome::Delivered);
    CHECK_EQ(*c.value_of(b), 4);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

void t_cancel_during_delivery() {
    Conductor c{4};
    const JobId victim = c.launch(leaf());
    const JobId killer = c.launch(canceller(&c, victim));
    c.post(killer, 1);
    CHECK(c.step());
    CHECK(c.settled(victim));
    CHECK_EQ(*c.outcome_of(victim), Outcome::Cancelled);
    CHECK_EQ(c.arena().live_slots(), 1u);   // only the killer's frame
    CHECK_EQ(*c.outcome_of(killer), Outcome::Delivered);
}

void t_self_cancel_then_park() {
    Conductor c{4};
    const JobId j = c.launch(self_canceller(&c, 1));
    c.post(j, 1);
    CHECK(c.step());
    CHECK(c.settled(j));
    CHECK_EQ(*c.outcome_of(j), Outcome::Cancelled);
    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, j), 1u);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, j), 1u);
    CHECK_EQ(c.arena().live_slots(), 0u);
}

void t_nested_teardown_order() {
    g_log.clear();
    {
        Conductor c{4};
        const JobId j = c.launch(t_outer());
        c.cancel(j);
        CHECK_EQ(c.arena().live_slots(), 0u);
        CHECK_EQ(*c.outcome_of(j), Outcome::Cancelled);
        CHECK_EQ(c.journal().count_for(Mark::FrameReleased, j), 1u);
    }
    CHECK_EQ(g_log.size(), 3u);
    if (g_log.size() == 3) {
        CHECK_EQ(g_log[0], 3);  // innermost first
        CHECK_EQ(g_log[1], 2);
        CHECK_EQ(g_log[2], 1);
    }
}

void t_nested_normal_order() {
    g_log.clear();
    {
        Conductor c{4};
        const JobId j = c.launch(t_outer());
        c.post(j, 5);
        c.drain();
        CHECK_EQ(*c.value_of(j), 6);
        CHECK_EQ(c.arena().live_slots(), 0u);
    }
    // inner destroyed before middle before outer
    CHECK_EQ(g_log.size(), 3u);
    if (g_log.size() == 3) {
        CHECK_EQ(g_log[0], 3);
        CHECK_EQ(g_log[1], 2);
        CHECK_EQ(g_log[2], 1);
    }
}

void t_exhaustion() {
    Conductor c{2};
    const JobId a = c.launch(leaf());
    const JobId b = c.launch(leaf());
    bool threw = false;
    try {
        Duty d = leaf();
        (void)d;
    } catch (const std::bad_alloc&) {
        threw = true;
    }
    CHECK(threw);
    c.post(a, 1);
    c.drain();
    CHECK_EQ(*c.outcome_of(a), Outcome::Delivered);
    CHECK(!c.settled(b));
    CHECK_EQ(c.arena().live_slots(), 1u);
    CHECK_EQ(c.arena().peak_live(), 2u);
}

void t_pending_and_drain() {
    Conductor c{4};
    const JobId j = c.launch(leaf());
    c.post(j, 1);
    c.post(j, 2);
    CHECK_EQ(c.pending_tickets(), 2u);
    CHECK_EQ(c.drain(), 2u);
    CHECK_EQ(c.pending_tickets(), 0u);
    CHECK(c.settled(j));
}

void t_one_delivery_per_step() {
    Conductor c{4};
    const JobId a = c.launch(leaf());
    const JobId b = c.launch(leaf());
    c.post(a, 1);
    c.post(b, 2);
    CHECK(c.step());
    CHECK_EQ(c.pending_tickets(), 1u);
    CHECK(c.settled(a));
    CHECK(!c.settled(b));
}

void t_trace_examples() {
    {
        Conductor c{4};
        Trace tr{c};
        const JobId j = c.launch(parent_leaf());
        c.post(j, 21);
        while (c.step()) tr.after_step();
        tr.all();
        std::printf("[trace child ok] %s\n", tr.all().c_str());
    }
    {
        Conductor c{4};
        Trace tr{c};
        const JobId j = c.launch(parent_thrower());
        c.post(j, 21);
        while (c.step()) tr.after_step();
        std::printf("[trace child throws] %s\n", tr.all().c_str());
    }
    {
        Conductor c{1};
        Trace tr{c};
        const JobId a = c.launch(leaf());
        c.post(a, 11);
        c.cancel(a);
        const JobId b = c.launch(leaf());
        while (c.step()) tr.after_step();
        std::printf("[trace recycle] %s\n", tr.all().c_str());
    }
}

int main() {
    t_cancel_releases_and_recycle();
    t_settle_once_cancel_then_ticket();
    t_child_success();
    t_child_fault();
    t_child_fault_caught();
    t_fault_then_catch();
    t_uncaught_throw();
    t_repark_same_frame();
    t_epoch_mismatch_same_index();
    t_index_mismatch_same_epoch();
    t_self_post_while_running();
    t_order_and_clock();
    t_delay_from_current_clock();
    t_independence();
    t_cancel_during_delivery();
    t_self_cancel_then_park();
    t_nested_teardown_order();
    t_nested_normal_order();
    t_exhaustion();
    t_pending_and_drain();
    t_one_delivery_per_step();
    t_trace_examples();

    std::printf("\n%d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
