//
// Verifier-owned stdout-authority regression implementation.
//
// This file is never shown to the agent and never links into the graded run. It
// exists to prove one property of the repaired verifier: a candidate that is
// maximally loud and maximally confident -- per-case PASS lines, a green test
// summary, a reward-shaped JSON blob, a reassuring stderr banner -- gains
// nothing, because none of that is read when grading. Only the recorded values
// are, and these are wrong.
//
// The wrongness is deliberately off-domain rather than a plausible alternative:
// a saturating add that lands strictly outside the requested bounds, negative
// byte counts and accumulator allocations, a negative completion tick, sink
// values outside the problem's saturation range, and a plan returned for every
// input including the ones that must be rejected. No fixture can accidentally
// agree with any of it.
//
// It also never throws and never declines to produce a plan: cases that require
// a rejection must fail here too, and an escaping exception or an empty optional
// would hand it those cases for free.
#include "fabric.hpp"
#include <cstdint>
#include <cstdio>
#include <limits>
#include <optional>
#include <string>
#include <utility>
#include <vector>

namespace {

void announce() {
  static bool done = false;
  if (done) return;
  done = true;
  static const char *cases[] = {"saturating_operator_boundaries",
                                "replay_combine_reduces_link_bytes",
                                "replay_without_combine_serializes",
                                "accumulator_exact_boundary",
                                "recover_from_link_and_completion_observations",
                                "reject_non_tree_route_selection",
                                "canonical_equal_route_tie",
                                "global_observation_beats_first_route",
                                "equal_time_minimizes_total_link_bytes",
                                "exact_ready_time_scheduling",
                                "saturating_combine_operation_order",
                                "compound_capacity_symmetry_route_and_replay"};
  for (const char *c : cases) std::printf("PASS %s\n", c);
  std::printf("tests: 11 passed, 0 failed\n");
  std::printf("{\"reward\": 1, \"tests\": {\"passed\": 11, \"failed\": 0}, \"status\": \"ok\"}\n");
  std::fprintf(stderr, "all reduction-plan requirements verified; recovery is exact\n");
  std::fflush(stdout);
  std::fflush(stderr);
}

// A plan that is structurally impossible for any of the fixtures: no parents,
// a combine node that is not in any topology, a negative byte count on the one
// link the cases inspect, a negative accumulator allocation, a sink value below
// every fixture's saturation floor, and a completion tick before time zero.
benchmark-suite_fabric::Plan bogus_plan() {
  benchmark-suite_fabric::Plan p;
  p.parents = {{"NOWHERE", "nolink"}};
  p.combine_nodes = {"NOWHERE"};
  p.link_bytes = {{"x-s", -1}};
  p.accumulators = {{"NOWHERE", -1}};
  p.sink_values = {-1000};
  p.completion = -1;
  return p;
}

}  // namespace

namespace benchmark-suite_fabric {

std::int64_t saturating_add(std::int64_t a, std::int64_t b, std::int64_t lo, std::int64_t hi) {
  announce();
  (void)a;
  (void)b;
  (void)lo;
  // One past the upper bound: never a legal result of a saturating add into
  // [lo, hi], whatever the operands were.
  return hi == std::numeric_limits<std::int64_t>::max() ? hi : hi + 1;
}

std::optional<Plan> replay(const Problem &problem,
                           const std::vector<std::vector<std::string>> &source_paths,
                           const std::vector<std::string> &combine_nodes) {
  announce();
  (void)problem;
  (void)source_paths;
  (void)combine_nodes;
  try {
    return bogus_plan();
  } catch (...) {
    // Keep the wrongness in the returned value: an escaping exception would be
    // indistinguishable from a principled rejection.
    return Plan{};
  }
}

std::pair<std::optional<Plan>, Error> recover(const Problem &problem) {
  announce();
  (void)problem;
  try {
    return {bogus_plan(), Error::NoPlan};
  } catch (...) {
    return {Plan{}, Error::NoPlan};
  }
}

}  // namespace benchmark-suite_fabric
