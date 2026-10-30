// Test harness for the roundel protocol. Not part of the deliverable.
#ifndef T_TRACE_HPP
#define T_TRACE_HPP

#include <cstdio>
#include <exception>
#include <optional>
#include <string>
#include <vector>

#include "roundel/runtime.hpp"

using namespace roundel;

static int g_failures = 0;
static int g_checks = 0;

static void check(bool ok, const char* what, const std::string& extra = {}) {
    ++g_checks;
    if (!ok) {
        ++g_failures;
        std::printf("FAIL: %s   %s\n", what, extra.c_str());
    }
}

static void check_eq(long long got, long long want, const char* what) {
    ++g_checks;
    if (got != want) {
        ++g_failures;
        std::printf("FAIL: %s (got %lld want %lld)\n", what, got, want);
    }
}

static char outcome_letter(Outcome o) {
    switch (o) {
        case Outcome::Delivered: return 'D';
        case Outcome::Cancelled: return 'X';
        case Outcome::Faulted: return 'F';
    }
    return '?';
}

static std::string outcome_str(std::optional<Outcome> o) {
    if (!o) return "-";
    return std::string(1, outcome_letter(*o));
}

// Builds the trace of the spec, one line per relevant mark, in journal order.
// sync() must be called right after each step so that now() belongs to the
// delivery that produced the marks; only A/R lines carry the clock.
class Tracer {
  public:
    void sync(const Conductor& c) {
        const auto& es = c.journal().entries();
        for (std::size_t i = seen_; i < es.size(); ++i) {
            const Entry& e = es[i];
            switch (e.mark) {
                case Mark::TicketAccepted:
                    add("A" + std::to_string(e.job) + "@" + std::to_string(c.now()));
                    break;
                case Mark::TicketRejected:
                    add("R" + std::to_string(e.job) + "@" + std::to_string(c.now()));
                    break;
                case Mark::ChildDestroyed:
                    add("C" + std::to_string(e.job));
                    break;
                case Mark::ParentContinued:
                    add("P" + std::to_string(e.job) + ":" + std::to_string(e.detail));
                    break;
                case Mark::JobSettled:
                    add("S" + std::to_string(e.job) + "=" +
                        std::string(1, outcome_letter(static_cast<Outcome>(e.detail))));
                    break;
                default:
                    break;
            }
        }
        seen_ = es.size();
    }
    std::string text() const { return line_; }
    void resume_after(const std::string& s) { line_ = s; }

  private:
    void add(const std::string& s) {
        if (!line_.empty()) line_ += ";";
        line_ += s;
    }
    std::string line_;
    std::size_t seen_ = 0;
};

// Journal invariants that must hold for every run.
static void audit_journal(const Conductor& c, const char* what) {
    const Journal& j = c.journal();
    check_eq((long long)c.arena().double_reclaims(), 0, "no double reclaims");

    std::vector<JobId> ids;
    for (const auto& e : j.entries()) {
        if (e.job == kNoJob) continue;
        bool known = false;
        for (JobId i : ids) known |= (i == e.job);
        if (!known) ids.push_back(e.job);
    }
    for (JobId id : ids) {
        check(j.count_for(Mark::JobSettled, id) <= 1, "settle at most once", what);
        check(j.count_for(Mark::FaultReported, id) <= 1, "fault reported at most once", what);
        check(j.count_for(Mark::FrameReleased, id) <= 1, "frame released at most once", what);
        if (j.has(Mark::FrameReleased, id)) {
            check(j.first_index(Mark::JobSettled, id) < j.first_index(Mark::FrameReleased, id),
                  "JobSettled before FrameReleased", what);
        }
        if (j.has(Mark::JobStarted, id)) {
            check(j.first_index(Mark::JobStarted, id) == 0 ||
                      j.first_index(Mark::JobStarted, id) < j.entries().size(),
                  "JobStarted present", what);
        }
        // every ParentContinued is preceded by at least as many ChildDestroyed
        const std::size_t cd = j.count_for(Mark::ChildDestroyed, id);
        const std::size_t pc = j.count_for(Mark::ParentContinued, id);
        check(pc <= cd + 1, "ParentContinued needs a destroyed child", what);
        // a job cannot be resumed more often than it parked
        check(j.count_for(Mark::JobParked, id) >= j.count_for(Mark::TicketAccepted, id),
              "JobParked precedes each acceptance", what);
        // JobStarted comes first for its job
        for (const auto& e : j.entries()) {
            if (e.job == id) {
                check(e.mark == Mark::JobStarted, "JobStarted comes first", what);
                break;
            }
        }
        // nothing semantic is recorded after FrameReleased (a cancellation
        // request may still be, and is not "about" the job's run)
        const std::size_t rel = j.first_index(Mark::FrameReleased, id);
        if (rel < j.entries().size()) {
            for (std::size_t i = rel + 1; i < j.entries().size(); ++i) {
                const Entry& e = j.entries()[i];
                if (e.job != id) continue;
                check(e.mark == Mark::CancelRequested, "nothing after FrameReleased", what);
            }
        }
        // a faulted job reports it; a job that delivered does not
        if (j.has(Mark::JobSettled, id)) {
            Outcome o = *c.outcome_of(id);
            const std::size_t fr = j.count_for(Mark::FaultReported, id);
            if (o == Outcome::Faulted) {
                check_eq((long long)fr, 1, "faulted job reports once");
            } else {
                check_eq((long long)fr, 0, "non-faulted job reports no fault");
            }
        }
    }
}

#endif
