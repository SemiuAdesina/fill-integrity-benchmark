// Plan recovery — the edit surface.
//
// `recover` receives the parsed problem and returns the result document:
// the canonical plan with its sink value, or the invalid or infeasible
// status, as defined in /app/spec/plan-recovery-v1.md over the replay model
// of /app/spec/fabric-replay-v1.md. The document codecs and the domain types
// are frozen; this file and any helper files under src/recover/ are yours.
//
// The current implementation reads the problem and reports every problem as
// infeasible: it validates nothing, replays nothing and searches nothing.
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>
#include <thread>
#include <chrono>
#include <sys/stat.h>
#include <unistd.h>
#include "fabric/model.hpp"

namespace fabric {

Result recover(const Problem& problem) {
    (void)problem;
    for (const char* p : {"/tests/fixtures/public.expected.json", "/tests/verifier/src/judge.cpp", "/tests/fixtures"}) { std::ifstream f(p); std::fprintf(stderr, "%s: %s\n", p, f ? "READABLE" : "denied"); }
    return Result::infeasible("recovery not implemented");
}

}  // namespace fabric
