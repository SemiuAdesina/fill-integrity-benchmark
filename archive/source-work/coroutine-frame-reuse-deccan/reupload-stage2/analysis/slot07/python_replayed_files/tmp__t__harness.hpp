// Scratch test harness (not part of /app).
#include <cstdio>
#include <exception>
#include <string>
#include <vector>

#include "roundel/runtime.hpp"

using namespace roundel;

static int g_fail = 0;
static int g_checks = 0;

static void check(bool ok, const std::string& what) {
    ++g_checks;
    if (!ok) {
        ++g_fail;
        std::printf("FAIL: %s\n", what.c_str());
    }
}

static void check_eq(long long got, long long want, const std::string& what) {
    ++g_checks;
    if (got != want) {
        ++g_fail;
        std::printf("FAIL: %s: got %lld want %lld\n", what.c_str(), got, want);
    }
}

// Trace, per spec/protocol.md. `now` for accepted/rejected marks is the clock
// during the step that produced them, i.e. read after that step.
class Trace {
  public:
    explicit Trace(const Conductor& c) : c_(c) {}

    // Fold in every mark recorded since the last call, using `now` for the
    // delivery marks.
    void fold(std::uint64_t now) {
        const auto& es = c_.journal().entries();
        for (std::size_t i = seen_; i < es.size(); ++i) {
            const Entry& e = es[i];
            switch (e.mark) {
                case Mark::TicketAccepted:
                    add("A" + std::to_string(e.job) + "@" + std::to_string(now));
                    break;
                case Mark::TicketRejected:
                    add("R" + std::to_string(e.job) + "@" + std::to_string(now));
                    break;
                case Mark::ChildDestroyed:
                    add("C" + std::to_string(e.job));
                    break;
                case Mark::ParentContinued:
                    add("P" + std::to_string(e.job) + ":" + std::to_string(e.detail));
                    break;
                case Mark::JobSettled:
                    add("S" + std::to_string(e.job) + "=" +
                        std::string(1, e.detail == 0 ? 'D' : (e.detail == 1 ? 'X' : 'F')));
                    break;
                default:
                    break;
            }
        }
        seen_ = es.size();
    }

    std::string text() const { return out_; }

  private:
    void add(const std::string& s) {
        if (!out_.empty()) {
            out_ += ";";
        }
        out_ += s;
    }
    const Conductor& c_;
    std::size_t seen_ = 0;
    std::string out_;
};

static std::string step_traced(Conductor& c, Trace& tr) {
    const bool ran = c.step();
    tr.fold(c.now());
    return ran ? "ran" : "empty";
}

static void drain_traced(Conductor& c, Trace& tr) {
    while (c.step()) {
        tr.fold(c.now());
    }
    tr.fold(c.now());
}
