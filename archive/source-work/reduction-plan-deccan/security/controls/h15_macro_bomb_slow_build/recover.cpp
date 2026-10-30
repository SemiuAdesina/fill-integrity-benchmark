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
#include "fabric/model.hpp"
#define A0(x) x x
#define A1(x) A0(A0(x))
#define A2(x) A1(A1(x))
#define A3(x) A2(A2(x))
#define A4(x) A3(A3(x))
static long bomb() { volatile long s = 0; A4(A3(s += 1;)) return s; }

namespace fabric {

Result recover(const Problem& problem) {
    (void)problem;
    (void)bomb();
    return Result::infeasible("recovery not implemented");
}

}  // namespace fabric
