//
// Verifier-owned incorrect implementation, used only by the stdout-authority
// regression. It is never built into the candidate probe.
//
// It is wrong by construction, not by coincidence. `bank::recover` must return
// a mapping drawn from the supplied grammar, and every grammar in this task
// lists lane rotations strictly below `bank_count` -- rotations are reduced
// modulo `bank_count`, so a rotation equal to `bank_count` is the identity
// rotation spelled illegally and no grammar ever offers it. This implementation
// returns exactly that: no stages, and `lane_rotation = bank_count`. Its
// canonical string therefore reads `rotate=<bank_count>;stages=` and cannot
// equal the canonical string of any legal mapping for any case, and for the
// case that must reject it returns instead of throwing. Picking a plausible
// legal-looking mapping would risk coinciding with a correct answer for some
// fixture; deriving an off-domain value cannot.
//
// At the same time it announces success in every shape a grader might be
// tempted to believe -- per-case PASS lines, a passed/failed summary, a
// reward-shaped JSON object and a confident stderr line.
//
// Because the judge compares against expectations it computes itself and never
// reads this process's output streams or exit status, the announcements change
// nothing and the regression must still score zero. That is the whole point of
// the control: it demonstrates that candidate output carries no authority.
#include "bank/model.hpp"
#include <cstdio>

namespace bank {

namespace {
void announce() {
  static bool done = false;
  if (done) return;
  done = true;
  std::printf("running verifier case: ok\n");
  std::printf("all checks passed\n");
  std::printf("PASS basic_identity\n");
  std::printf("PASS word_boundaries\n");
  std::printf("PASS broadcast_groups\n");
  std::printf("PASS conflict_disambiguates\n");
  std::printf("PASS ordered_xor\n");
  std::printf("PASS global_evidence\n");
  std::printf("PASS minimum_canonical\n");
  std::printf("PASS compound_replay\n");
  std::printf("PASS no_solution\n");
  std::printf("tests: 9 passed, 0 failed\n");
  std::printf("{\"reward\":1,\"summary\":{\"tests\":9,\"passed\":9,\"failed\":0}}\n");
  std::fprintf(stderr, "verification complete: reward 1\n");
  std::fflush(stdout);
  std::fflush(stderr);
}
}  // namespace

Mapping recover(const Grammar& grammar, const std::vector<Trace>& traces) {
  announce();
  (void)traces;
  Mapping wrong;
  // Guarded so this implementation never propagates an exception of its own:
  // its wrongness must live entirely in the value it returns, never in a throw.
  // If it threw, the `no_solution` case -- which requires a std::runtime_error
  // -- could accidentally be graded as correct.
  try {
    wrong.stages.clear();
    wrong.lane_rotation = grammar.bank_count;
  } catch (...) {
    wrong = Mapping{};
  }
  return wrong;
}

}  // namespace bank
