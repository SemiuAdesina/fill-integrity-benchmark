//
// Shared pieces of the diagnostic programs: a tiny check helper and a probe
// object whose destructor is the ground truth for "this frame was destroyed".
// These programs are development aids. They are not the graded suite, and you
// may extend them.

#ifndef ROUNDEL_DIAGNOSTICS_COMMON_HPP
#define ROUNDEL_DIAGNOSTICS_COMMON_HPP

#include <cstdio>
#include <exception>
#include <string>
#include <vector>

#include "roundel/runtime.hpp"

namespace diag {

inline int& failures() {
    static int n = 0;
    return n;
}

inline int& checks() {
    static int n = 0;
    return n;
}

inline int& scenarios() {
    static int n = 0;
    return n;
}

inline int& scenarios_failed() {
    static int n = 0;
    return n;
}

// Thrown by check() to end the scenario it belongs to at its first failure:
// the checks that follow assume the earlier ones held.
struct Stop {};

// Reports one observable fact. `what` names the expectation in the words of
// spec/protocol.md; `observed` is what actually happened.
inline void check(bool ok, const char* what, const std::string& observed = "") {
    ++checks();
    if (ok) {
        std::printf("  ok    %s\n", what);
        return;
    }
    ++failures();
    std::printf("  FAIL  %s", what);
    if (!observed.empty()) {
        std::printf(" -- %s", observed.c_str());
    }
    std::printf("\n");
    throw Stop{};
}

inline std::string num(long long v) { return std::to_string(v); }

// Runs one scenario. It stops at its first failed check, and an exception
// escaping it is reported as an observation rather than ending the program, so
// the remaining scenarios still run.
template <typename Fn>
void scenario(const char* name, Fn fn) {
    ++scenarios();
    std::printf("%s\n", name);
    try {
        fn();
    } catch (const Stop&) {
        ++scenarios_failed();
    } catch (const std::exception& e) {
        try {
            check(false, "the scenario ran to its end", std::string("it threw ") + e.what());
        } catch (const Stop&) {
            ++scenarios_failed();
        }
    } catch (...) {
        try {
            check(false, "the scenario ran to its end", "it threw something that is not a std::exception");
        } catch (const Stop&) {
            ++scenarios_failed();
        }
    }
}

inline int finish(const char* group) {
    std::printf("%s: %d of %d scenarios passed (%d checks run, %d failed)\n", group, scenarios() - scenarios_failed(), scenarios(),
                checks(), failures());
    return scenarios_failed() == 0 ? 0 : 1;
}

// Lives inside a coroutine frame. Its destructor runs when, and only when, the
// frame that holds it is destroyed.
inline int& probes_alive() {
    static int n = 0;
    return n;
}

inline std::vector<int>& destroyed_in_order() {
    static std::vector<int> v;
    return v;
}

struct Probe {
    int tag;
    explicit Probe(int t) : tag(t) { ++probes_alive(); }
    Probe(const Probe&) = delete;
    Probe& operator=(const Probe&) = delete;
    ~Probe() {
        destroyed_in_order().push_back(tag);
        --probes_alive();
    }
};

inline void reset_probes() {
    probes_alive() = 0;
    destroyed_in_order().clear();
}

inline int outcome_code(const roundel::Conductor& c, roundel::JobId job) {
    const auto o = c.outcome_of(job);
    return o.has_value() ? static_cast<int>(*o) : -1;
}

inline const char* outcome_text(const roundel::Conductor& c, roundel::JobId job) {
    const auto o = c.outcome_of(job);
    return o.has_value() ? roundel::outcome_name(*o) : "unsettled";
}

}  // namespace diag

#endif  // ROUNDEL_DIAGNOSTICS_COMMON_HPP
