//
// Verifier-owned case fixtures.
//
// These are the exact inputs the V1 graded harness constructed, with the same
// case names and the same case population. Fixtures are inputs only: no
// expected result appears here, so this header can be compiled into the probe
// that the graded account executes without exposing anything the judge decides
// with. Every expectation lives in judge/main.cpp instead.
#pragma once
#include "fabric.hpp"
#include <string>
#include <vector>

namespace vcase {

using benchmark-suite_fabric::Problem;

// The V1 base topology: two sources behind one reduction-capable switch.
inline Problem two_source() {
  Problem p;
  p.sink = "S";
  p.saturation_min = -100;
  p.saturation_max = 100;
  p.links = {{"a-x", "A", "X", 8, 1}, {"b-x", "B", "X", 8, 1}, {"x-s", "X", "S", 8, 1}};
  p.sources = {{"a", "A", 8, 90}, {"b", "B", 8, -50}};
  p.reduction_nodes = {"X"};
  p.accumulator_limits = {{"X", 16}};
  return p;
}

// The V1 route selection used by the base topology cases.
inline std::vector<std::vector<std::string>> two_paths() {
  return {{"a-x", "x-s"}, {"b-x", "x-s"}};
}

// V1: the base problem with the observations that pin the combine placement.
inline Problem p_recover_observed() {
  Problem p = two_source();
  p.observed_link_bytes = {{"x-s", 8}};
  p.observed_completion = 4;
  return p;
}

// V1: the base topology plus a second, non-suffix-consistent exit from X.
inline Problem p_non_tree() {
  Problem p = two_source();
  p.links.push_back({"x-t", "X", "T", 8, 0});
  p.links.push_back({"t-s", "T", "S", 8, 0});
  return p;
}

// V1: the route selection that leaves X by two different parent links.
inline std::vector<std::vector<std::string>> non_tree_paths() {
  return {{"a-x", "x-s"}, {"b-x", "x-t", "t-s"}};
}

// V1: one source, two equal-cost links, so only the canonical tie decides.
inline Problem p_equal_route_tie() {
  Problem p;
  p.sink = "S";
  p.saturation_min = -10;
  p.saturation_max = 10;
  p.sources = {{"a", "A", 4, 1}};
  p.links = {{"z", "A", "S", 4, 1}, {"a", "A", "S", 4, 1}};
  return p;
}

// Two legal one-source routes finish at the same tick. The lexically first DFS
// route uses two links (eight total bytes); the later direct route uses one
// link (four bytes), so the documented global objective must reject the first
// feasible/equal-time plan and minimize exact traffic.
inline Problem p_equal_time_different_bytes() {
  Problem p;
  p.sink = "S";
  p.saturation_min = -10;
  p.saturation_max = 10;
  p.sources = {{"a", "A", 4, 3}};
  p.links = {{"a-leg", "A", "M", 4, 0}, {"b-leg", "M", "S", 4, 0},
             {"z-direct", "A", "S", 4, 1}};
  return p;
}

// V1: the base topology with a symmetric Y branch. `x_s` and `y_s` are the
// observed link byte counts that distinguish the two compound cases.
inline Problem p_two_branch(std::int64_t x_s, std::int64_t y_s) {
  Problem p = two_source();
  p.links.push_back({"a-y", "A", "Y", 8, 1});
  p.links.push_back({"b-y", "B", "Y", 8, 1});
  p.links.push_back({"y-s", "Y", "S", 8, 1});
  p.reduction_nodes.push_back("Y");
  p.accumulator_limits["Y"] = 16;
  p.observed_link_bytes = {{"x-s", x_s}, {"y-s", y_s}};
  p.observed_completion = 4;
  return p;
}

// V1: a delayed shard and an idle link, so only exact ready-time serialization
// reaches the stated completion tick.
inline Problem p_ready_time() {
  Problem p;
  p.sink = "S";
  p.saturation_min = -10;
  p.saturation_max = 10;
  p.links = {{"a-x", "A", "X", 8, 4}, {"b-x", "B", "X", 8, 0}, {"x-s", "X", "S", 8, 0}};
  p.sources = {{"a", "A", 8, 1}, {"b", "B", 8, 2}};
  return p;
}

// V1: three mixed-sign shards folded at one switch.
inline Problem p_operation_order() {
  Problem p = two_source();
  p.links.insert(p.links.end(), {{"c-x", "C", "X", 8, 1}});
  p.sources = {{"c", "C", 8, -100}, {"b", "B", 8, 100}, {"a", "A", 8, 100}};
  p.accumulator_limits["X"] = 24;
  return p;
}

inline std::vector<std::vector<std::string>> operation_order_paths() {
  return {{"a-x", "x-s"}, {"b-x", "x-s"}, {"c-x", "x-s"}};
}

// Case identifiers, in V1 declaration order. V1's twelfth registration,
// `zz_required_negative_control`, was an in-process check that exercised no
// candidate behaviour; it is superseded by `judge --selftest`, which is decided
// entirely inside the verifier. These eleven are the positive population,
// exactly as V1 counted it.
inline const std::vector<std::string> &ids() {
  static const std::vector<std::string> v{
      // Public behavior: saturating_add clamps at both signed bounds and leaves
      // exact in-range sums unchanged. Rejects ordinary wrapping/addition.
      "saturating_operator_boundaries",
      // Public behavior: a legal switch combine reduces downstream bytes while
      // preserving replay completion and value. Rejects host-only reduction.
      "replay_combine_reduces_link_bytes",
      // Public behavior: without a combine, payloads serialize independently.
      // Rejects implementations that merge merely because paths meet.
      "replay_without_combine_serializes",
      // Public behavior: accumulator allocation equal to capacity is legal and
      // one unit below is not. Rejects strict-less-than boundary handling.
      "accumulator_exact_boundary",
      // Public behavior: recover jointly honors observed bytes and completion.
      // Rejects independent or greedy observation fitting.
      "recover_from_link_and_completion_observations",
      // Public behavior: selected paths must share a complete suffix and induce
      // one tree. Rejects locally valid but globally divergent routes.
      "reject_non_tree_route_selection",
      // Public behavior: equal objectives select canonical parents independent
      // of input order. Rejects first-feasible and container-order ties.
      "canonical_equal_route_tie",
      // Public behavior: recovery searches all route/combine choices globally.
      // Rejects the first locally plausible reduction branch.
      "global_observation_beats_first_route",
      // Public behavior: after equal completion and allocation, the objective
      // minimizes exact total bytes. Rejects first-feasible/equal-time plans.
      "equal_time_minimizes_total_link_bytes",
      // Public behavior: links serialize by ready time and advance exact ticks.
      // Rejects queue/input order or unit-step clocks.
      "exact_ready_time_scheduling",
      // Public behavior: payloads fold in canonical origin order with saturation
      // after every operation. Rejects sum-then-clamp and wrong fold order.
      "saturating_combine_operation_order",
      // Public behavior: capacity equality, observed bytes, symmetric routing,
      // canonical choice and replay interact. Rejects fixes missing any one.
      "compound_capacity_symmetry_route_and_replay"};
  return v;
}

// Ordered field names a VALUES record must carry for a case. This is the
// complete and only field set: the probe emits exactly these keys in exactly
// this order and the judge decodes against exactly this list, so a record has
// no room for anything that is not one of the raw functional values the V1
// assertions examined.
inline const std::vector<std::string> &value_keys(const std::string &id) {
  static const std::vector<std::string> empty{};
  // saturating_add at the upper bound, the lower bound and an exact mixed-sign
  // result -- the three calls V1 compared.
  static const std::vector<std::string> k_sat{"sat_high", "sat_low", "sat_exact"};
  static const std::vector<std::string> k_combine{"link_bytes_x_s", "completion", "accumulators",
                                                  "sink_values"};
  static const std::vector<std::string> k_nocombine{"link_bytes_x_s", "completion", "sink_values"};
  // Two replays of the same topology: at the stated capacity, and one unit below.
  static const std::vector<std::string> k_capacity{"at_capacity", "under_capacity"};
  static const std::vector<std::string> k_recover{"combine_nodes", "sink_values"};
  static const std::vector<std::string> k_reject{"outcome"};
  // The tie case recorded both plan identities so the judge can compare them.
  static const std::vector<std::string> k_tie{"parents", "plan_digest", "reversed_plan_digest"};
  static const std::vector<std::string> k_global{"combine_nodes"};
  static const std::vector<std::string> k_equal_time_bytes{"parents", "completion",
                                                            "total_link_bytes"};
  static const std::vector<std::string> k_ready{"completion"};
  static const std::vector<std::string> k_order{"sink_values"};
  static const std::vector<std::string> k_compound{"combine_nodes", "link_bytes_x_s",
                                                   "accumulator_front"};
  if (id == "saturating_operator_boundaries") return k_sat;
  if (id == "replay_combine_reduces_link_bytes") return k_combine;
  if (id == "replay_without_combine_serializes") return k_nocombine;
  if (id == "accumulator_exact_boundary") return k_capacity;
  if (id == "recover_from_link_and_completion_observations") return k_recover;
  if (id == "reject_non_tree_route_selection") return k_reject;
  if (id == "canonical_equal_route_tie") return k_tie;
  if (id == "global_observation_beats_first_route") return k_global;
  if (id == "equal_time_minimizes_total_link_bytes") return k_equal_time_bytes;
  if (id == "exact_ready_time_scheduling") return k_ready;
  if (id == "saturating_combine_operation_order") return k_order;
  if (id == "compound_capacity_symmetry_route_and_replay") return k_compound;
  return empty;
}

}  // namespace vcase
