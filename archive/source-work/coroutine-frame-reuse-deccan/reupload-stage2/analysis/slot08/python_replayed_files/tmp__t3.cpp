// Third harness: failures and unwinding.
#include "roundel/runtime.hpp"

#include <cstdio>
#include <stdexcept>
#include <vector>

using namespace roundel;

static int failures = 0;
static int checks = 0;
#define CHECK(cond)                                                       \
    do {                                                                  \
        ++checks;                                                         \
        if (!(cond)) {                                                    \
            ++failures;                                                   \
            std::printf("  FAIL %s:%d  %s\n", __FILE__, __LINE__, #cond); \
        }                                                                 \
    } while (0)
static void check_eq(const char* what, long long got, long long want) {
    ++checks;
    if (got != want) {
        ++failures;
        std::printf("  FAIL %s: got %lld want %lld\n", what, got, want);
    }
}
static std::size_t count_for(const Journal& j, Mark m, JobId id) { return j.count_for(m, id); }

struct Order {
    std::vector<int>* log;
    int id;
    Conductor* c;
    ~Order() {
        if (log != nullptr && c != nullptr) {
            log->push_back(id * 10 + static_cast<int>(c->arena().live_slots()));
        }
    }
};

static Duty leaf(std::vector<int>* log, Conductor* c, bool boom) {
    Order guard{log, 3, c};
    const std::int64_t v = co_await park();
    if (boom) {
        throw std::runtime_error("leaf boom");
    }
    co_return v;
}

static Duty mid(std::vector<int>* log, Conductor* c, bool boom) {
    Order guard{log, 2, c};
    co_return co_await leaf(log, c, boom) + 1;
}

static Duty top(std::vector<int>* log, Conductor* c, bool boom) {
    Order guard{log, 1, c};
    co_return co_await mid(log, c, boom) + 1;
}

static Duty top_catch(std::vector<int>* log, Conductor* c, bool boom) {
    Order guard{log, 1, c};
    std::int64_t v = 0;
    try {
        v = co_await mid(log, c, boom);
    } catch (const std::exception&) {
        v = -5;
    }
    co_return v;
}

static void t_deep_fault() {
    std::printf("fault deep in the stack unwinds innermost first\n");
    std::vector<int> log;
    Conductor c{4};
    const JobId a = c.launch(top(&log, &c, true));
    check_eq("live", (long long)c.arena().live_slots(), 3);
    c.post(a, 1);
    c.drain();
    check_eq("outcome", (long long)*c.outcome_of(a), (long long)Outcome::Faulted);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    CHECK(log.size() == 3);
    if (log.size() == 3) {
        check_eq("leaf first", (long long)log[0], 33);
        check_eq("mid", (long long)log[1], 22);
        check_eq("top", (long long)log[2], 11);
    }
    check_eq("JobSettled", (long long)count_for(c.journal(), Mark::JobSettled, a), 1);
    check_eq("FaultReported", (long long)count_for(c.journal(), Mark::FaultReported, a), 1);
    check_eq("FrameReleased", (long long)count_for(c.journal(), Mark::FrameReleased, a), 1);
    check_eq("ChildDestroyed", (long long)count_for(c.journal(), Mark::ChildDestroyed, a), 2);
    check_eq("double", (long long)c.arena().double_reclaims(), 0);
}

static void t_deep_fault_caught() {
    std::printf("a caught deep failure leaves the job delivered\n");
    std::vector<int> log;
    Conductor c{4};
    const JobId a = c.launch(top_catch(&log, &c, true));
    c.post(a, 1);
    c.drain();
    check_eq("value", (long long)*c.value_of(a), -5);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("FaultReported", (long long)count_for(c.journal(), Mark::FaultReported, a), 0);
    check_eq("JobSettled", (long long)count_for(c.journal(), Mark::JobSettled, a), 1);
    CHECK(log.size() == 3);
    if (log.size() == 3) {
        check_eq("leaf first", (long long)log[0], 33);
        check_eq("mid", (long long)log[1], 22);
    }
}

static void t_deep_success() {
    std::printf("deep success release order\n");
    std::vector<int> log;
    Conductor c{4};
    const JobId a = c.launch(top(&log, &c, false));
    c.post(a, 41);
    c.drain();
    check_eq("value", (long long)*c.value_of(a), 43);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    CHECK(log.size() == 3);
    if (log.size() == 3) {
        check_eq("leaf first", (long long)log[0], 33);
        check_eq("mid", (long long)log[1], 22);
    }
}

static void t_deep_cancel() {
    std::printf("deep cancel release order\n");
    std::vector<int> log;
    Conductor c{4};
    const JobId a = c.launch(top(&log, &c, false));
    c.cancel(a);
    check_eq("live", (long long)c.arena().live_slots(), 0);
    check_eq("outcome", (long long)*c.outcome_of(a), (long long)Outcome::Cancelled);
    CHECK(log.size() == 3);
    if (log.size() == 3) {
        check_eq("leaf first", (long long)log[0], 33);
        check_eq("mid", (long long)log[1], 22);
        check_eq("top", (long long)log[2], 11);
    }
}

// A trace printer, to eyeball the journal ordering rules.
static std::string trace_of(const Conductor& c, std::uint64_t now_at) {
    (void)now_at;
    std::string out;
    for (const Entry& e : c.journal().entries()) {
        char b[64];
        switch (e.mark) {
            case Mark::TicketAccepted:
                std::snprintf(b, sizeof b, "A%llu@%llu;", (unsigned long long)e.job,
                              (unsigned long long)c.now());
                break;
            case Mark::TicketRejected:
                std::snprintf(b, sizeof b, "R%llu@%llu;", (unsigned long long)e.job,
                              (unsigned long long)c.now());
                break;
            case Mark::ChildDestroyed:
                std::snprintf(b, sizeof b, "C%llu;", (unsigned long long)e.job);
                break;
            case Mark::ParentContinued:
                std::snprintf(b, sizeof b, "P%llu:%lld;", (unsigned long long)e.job,
                              (long long)e.detail);
                break;
            case Mark::JobSettled:
                std::snprintf(b, sizeof b, "S%llu=%s;", (unsigned long long)e.job,
                              e.detail == 0 ? "D" : (e.detail == 1 ? "X" : "F"));
                break;
            default:
                b[0] = '\0';
                break;
        }
        out += b;
    }
    return out;
}

static void t_trace_shape() {
    std::printf("journal shape for the two child scenarios\n");
    {
        Conductor c{4};
        std::vector<int> log;
        const JobId a = c.launch(top(&log, &c, false));
        c.post(a, 41);
        c.drain();
        std::printf("  success: %s\n", trace_of(c, c.now()).c_str());
        (void)a;
    }
    {
        Conductor c{4};
        std::vector<int> log;
        const JobId a = c.launch(top(&log, &c, true));
        c.post(a, 41);
        c.drain();
        std::printf("  failure: %s\n", trace_of(c, c.now()).c_str());
        (void)a;
    }
}

int main() {
    setvbuf(stdout, nullptr, _IONBF, 0);
    t_deep_fault();
    t_deep_fault_caught();
    t_deep_success();
    t_deep_cancel();
    t_trace_shape();
    std::printf("\n%d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}
