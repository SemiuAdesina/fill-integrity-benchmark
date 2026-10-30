//
// Verifier-owned case fixtures.
//
// These are the exact inputs the V1 graded harness constructed, with the same
// case names, the same case order and the same case population. Fixtures are
// inputs only: no expected result appears here, so this header can be compiled
// into the probe that the graded account executes without exposing anything the
// judge decides with.
//
// Field order follows `fp8/state.hpp`:
//   SourceState{tensor, tie, role, replica, shard, stage, scale_q,
//               inverse_scale_q, history, cursor}
//   Target{tensor, tie, role, shard, stage, history_length, active}
#pragma once
#include "fp8/state.hpp"
#include <cstdint>
#include <string>
#include <vector>

namespace vcase {

// One case's inputs. `history_resize` is the only V1 case that called
// `restore` twice, so a second target list is carried for it and left empty
// everywhere else.
struct Fixture {
  std::vector<fp8::SourceState> sources;
  std::vector<fp8::Target> targets;
  std::vector<fp8::Target> targets_b;
};

// Case identifiers, in V1 declaration order.
inline const std::vector<std::string> &ids() {
  static const std::vector<std::string> v{
      // Logical identity survives stage/shard movement; rejects physical-name copy.
      "topology_move",
      // Next-write cursor rotates chronology exactly once; rejects raw ring order.
      "cursor_wrap",
      // Resize keeps newest suffix or zero-left-pads; rejects front truncation/padding.
      "history_resize",
      // Forward/backward keys remain distinct; rejects role-agnostic grouping.
      "role_separation",
      // Tie identity fans one state to multiple tensors; rejects tensor-only identity.
      "tied_tensors",
      // Replica suffixes and deterministic longest source agree; rejects first replica.
      "replicated_state",
      // Inactive targets vanish and outputs sort canonically; rejects input order.
      "skipped_and_order",
      // Conflicting replica histories reject; rejects silently choosing one replica.
      "replica_conflict",
      // Reshard, ties, roles, resizing, cursor reset and CPU delayed update interact;
      // rejects implementations that satisfy only isolated restoration rules.
      "compound_next_iteration"};
  return v;
}

// Build the fixture for one case. Returns false for an unknown name.
inline bool fixture(const std::string &id, Fixture &out) {
  using fp8::Role;
  using fp8::SourceState;
  using fp8::Target;
  out = Fixture{};
  if (id == "topology_move") {
    out.sources = {SourceState{"q", {}, Role::Forward, 0, 0, 0, 32, 32, {1, 2}, 0}};
    out.targets = {Target{"q", {}, Role::Forward, 3, 4, 2, true}};
    return true;
  }
  if (id == "cursor_wrap") {
    out.sources = {SourceState{"q", {}, Role::Forward, 0, 0, 0, 16, 64, {40, 10, 20, 30}, 1}};
    out.targets = {Target{"q", {}, Role::Forward, 0, 0, 4, true}};
    return true;
  }
  if (id == "history_resize") {
    out.sources = {SourceState{"q", {}, Role::Forward, 0, 0, 0, 16, 64, {30, 40, 10, 20}, 2}};
    out.targets = {Target{"q", {}, Role::Forward, 0, 0, 2, true}};
    out.targets_b = {Target{"q", {}, Role::Forward, 1, 0, 6, true}};
    return true;
  }
  if (id == "role_separation") {
    out.sources = {SourceState{"q", "tie", Role::Forward, 0, 0, 0, 8, 128, {1, 2}, 0},
                   SourceState{"q", "tie", Role::Backward, 0, 0, 0, 4, 256, {9, 7}, 0}};
    out.targets = {Target{"q", "tie", Role::Backward, 0, 2, 2, true},
                   Target{"q", "tie", Role::Forward, 0, 2, 2, true}};
    return true;
  }
  if (id == "tied_tensors") {
    out.sources = {SourceState{"embed", "shared", Role::Forward, 0, 0, 0, 32, 32, {3, 5, 7}, 0}};
    out.targets = {Target{"head", "shared", Role::Forward, 0, 5, 3, true},
                   Target{"embed", "shared", Role::Forward, 1, 1, 3, true}};
    return true;
  }
  if (id == "replicated_state") {
    out.sources = {SourceState{"q", {}, Role::Forward, 3, 2, 4, 16, 64, {8, 16, 32, 64}, 0},
                   SourceState{"q", {}, Role::Forward, 0, 1, 7, 16, 64, {32, 64}, 0},
                   SourceState{"q", {}, Role::Forward, 1, 0, 1, 16, 64, {64, 8, 16, 32}, 1}};
    out.targets = {Target{"q", {}, Role::Forward, 0, 9, 4, true}};
    return true;
  }
  if (id == "skipped_and_order") {
    out.sources = {SourceState{"z", {}, Role::Forward, 0, 0, 0, 1, 1, {1}, 0},
                   SourceState{"a", {}, Role::Forward, 0, 0, 0, 1, 1, {2}, 0}};
    out.targets = {Target{"z", {}, Role::Forward, 0, 0, 1, true},
                   Target{"skip", {}, Role::Forward, 0, 0, 1, false},
                   Target{"a", {}, Role::Forward, 0, 0, 1, true}};
    return true;
  }
  if (id == "replica_conflict") {
    out.sources = {SourceState{"q", {}, Role::Forward, 0, 0, 0, 8, 128, {1, 2}, 0},
                   SourceState{"q", {}, Role::Forward, 1, 0, 0, 8, 128, {1, 3}, 0}};
    out.targets = {Target{"q", {}, Role::Forward, 0, 0, 2, true}};
    return true;
  }
  if (id == "compound_next_iteration") {
    out.sources = {
        SourceState{"embed", "tie.q", Role::Forward, 2, 3, 1, 16, 64, {64, 8, 16, 32}, 1},
        SourceState{"head", "tie.q", Role::Forward, 0, 0, 7, 16, 64, {32, 64}, 0},
        SourceState{"embed", "tie.q", Role::Backward, 0, 0, 1, 8, 128, {3, 5, 7}, 0}};
    out.targets = {Target{"head", "tie.q", Role::Forward, 2, 9, 3, true},
                   Target{"embed", "tie.q", Role::Backward, 0, 5, 3, true},
                   Target{"skipped", {}, Role::Forward, 0, 0, 2, false},
                   Target{"embed", "tie.q", Role::Forward, 1, 5, 5, true}};
    return true;
  }
  return false;
}

// V1's compound case fed the restored third target through the frozen CPU
// delayed-scaling step with these two arguments. They are inputs, so they live
// here alongside the fixtures rather than with the expectations.
inline constexpr std::int64_t kCompoundAmax = 5;
inline constexpr std::int64_t kCompoundValueQ = 300;

// Ordered field names a VALUES record must carry for a case. This is the
// complete and only field set: the probe emits exactly these keys in exactly
// this order and the judge decodes against exactly this list, so a record has
// no room for anything that is not one of the raw functional values the V1
// assertions examined.
//
// Only the *shape* of each record is described here. Which values are correct,
// and which V1 assertion each field descends from, lives in judge/main.cpp --
// this header is compiled into the probe the graded account executes, so it
// must not carry an expected result even in a comment.
inline const std::vector<std::string> &value_keys(const std::string &id) {
  static const std::vector<std::string> empty{};
  // Output length, the sole target's stage, and its restored history.
  static const std::vector<std::string> k_topology{"out_len", "stage_0", "history_0"};
  // The restored history and cursor of the sole target.
  static const std::vector<std::string> k_cursor{"out_len", "history_0", "cursor_0"};
  // Two restore calls on the same sources: a shrinking target, then an
  // expanding one.
  static const std::vector<std::string> k_resize{"a_len", "a_history_0", "b_len", "b_history_0"};
  // Role and first history sample of each of the two outputs.
  static const std::vector<std::string> k_role{"out_len", "role_0", "history_0_first", "role_1",
                                               "history_1_first"};
  // Both restored histories, so the judge can compare them to each other.
  static const std::vector<std::string> k_tied{"out_len", "history_0", "history_1"};
  // The restored history of the sole target.
  static const std::vector<std::string> k_replicated{"out_len", "history_0"};
  // Output length and the tensor name of each emitted target, in output order.
  static const std::vector<std::string> k_skipped{"out_len", "tensor_0", "tensor_1"};
  // A rejection case: a returned vector is recorded as diagnostics only.
  static const std::vector<std::string> k_conflict{"out_len"};
  // The three restored histories, then the values the frozen cpu_step derives
  // from the third output.
  static const std::vector<std::string> k_compound{
      "out_len",         "history_0",   "history_1",           "history_2",
      "step_history",    "step_cursor", "step_scale_q",        "step_inverse_scale_q",
      "step_quantized"};
  if (id == "topology_move") return k_topology;
  if (id == "cursor_wrap") return k_cursor;
  if (id == "history_resize") return k_resize;
  if (id == "role_separation") return k_role;
  if (id == "tied_tensors") return k_tied;
  if (id == "replicated_state") return k_replicated;
  if (id == "skipped_and_order") return k_skipped;
  if (id == "replica_conflict") return k_conflict;
  if (id == "compound_next_iteration") return k_compound;
  return empty;
}

}  // namespace vcase
