//
// Verifier-owned incorrect implementation, used only by the stdout-authority
// regression. It is never built into the candidate probe.
//
// It is wrong by construction, not by coincidence. Three independent structural
// defects, each of which is outside the legal domain rather than a
// plausible-looking constant:
//
//  1. Every restored history is `target.history_length + 1` entries long. The
//     contract fixes the restored length to exactly `target.history_length`,
//     so this is illegal for every target of every case, independent of any
//     fixture value.
//  2. Slot j of output i is filled with `-(1000*i + j + 1)`. A restored entry
//     may only be a sample carried over from a source history or a zero pad,
//     and every sample in every fixture is positive, so no fill value can
//     coincide with a correct entry. Deriving the fill from the *output index*
//     additionally guarantees that two targets sharing a tie never receive
//     equal histories, so the tied-tensor case -- whose only assertions are
//     "two outputs" and "their histories agree" -- cannot be satisfied by
//     accident. That accidental pass is precisely the failure mode this
//     regression exists to rule out.
//  3. It never groups by logical identity, never validates replicas, never
//     rejects a conflict and never sorts, so the reject case is answered with a
//     return instead of the required throw and the ordering case comes back in
//     input order.
//
// It leaves `cursor` at the canonical 0 on purpose: its wrongness must live in
// the values it returns, and a cursor outside the history would instead make
// the frozen `cpu_step` throw, which could be mistaken for a rejection.
// For the same reason the whole body is guarded so no exception of its own can
// ever escape (CPP-NOTES 5: a liar that throws can accidentally satisfy a case
// that expects a throw).
//
// At the same time it announces success in every shape a grader might be
// tempted to believe -- per-case PASS lines, a passed/failed summary, a
// reward-shaped JSON object and a confident stderr line.
//
// Because the judge compares against expectations it holds itself and never
// reads this process's output streams or exit status, the announcements change
// nothing and the regression must still score zero. That is the whole point of
// the control: it demonstrates that candidate output carries no authority.
#include "fp8/state.hpp"
#include <cstdio>

namespace fp8 {

namespace {
void announce() {
  static bool done = false;
  if (done) return;
  done = true;
  std::printf("running verifier case: ok\n");
  std::printf("all checks passed\n");
  std::printf("PASS topology_move\n");
  std::printf("PASS cursor_wrap\n");
  std::printf("PASS history_resize\n");
  std::printf("PASS role_separation\n");
  std::printf("PASS tied_tensors\n");
  std::printf("PASS replicated_state\n");
  std::printf("PASS skipped_and_order\n");
  std::printf("PASS replica_conflict\n");
  std::printf("PASS compound_next_iteration\n");
  std::printf("tests: 9 passed, 0 failed\n");
  std::printf("{\"reward\":1,\"summary\":{\"tests\":9,\"passed\":9,\"failed\":0}}\n");
  std::fprintf(stderr, "verification complete: reward 1\n");
  std::fflush(stdout);
  std::fflush(stderr);
}
}  // namespace

std::vector<Restored> restore(const std::vector<SourceState> &sources,
                              const std::vector<Target> &targets) {
  announce();
  (void)sources;
  std::vector<Restored> out;
  try {
    for (const Target &t : targets) {
      if (!t.active) continue;
      const std::size_t i = out.size();
      // Off-domain length: one more entry than the target can legally hold.
      const std::size_t n = static_cast<std::size_t>(t.history_length) + 1u;
      std::vector<std::int64_t> h;
      h.reserve(n);
      for (std::size_t j = 0; j < n; ++j)
        h.push_back(-static_cast<std::int64_t>(1000u * i + j + 1u));
      out.push_back(Restored{t, 1, 1, h, 0});
    }
  } catch (...) {
    // Never propagate: the wrongness is in the returned value, not in a throw.
    out.clear();
  }
  return out;
}

}  // namespace fp8
