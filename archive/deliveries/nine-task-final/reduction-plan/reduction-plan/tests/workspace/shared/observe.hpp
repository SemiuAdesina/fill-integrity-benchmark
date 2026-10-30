//
// Observation harness -- the only code that calls the candidate.
//
// For each verifier-owned fixture this calls the candidate's `benchmark-suite_fabric`
// entry points and serializes the raw values that came back, in the exact order
// `vcase::value_keys(id)` declares. It asserts nothing, compares nothing, holds
// no expected value and no threshold, and it never throws: a call that escapes
// with an exception becomes an ERROR record describing the escape. Whether any
// of these values is correct is decided later, by the judge, in a process that
// does not link candidate code.
//
// This header is compiled into the probe, which the graded account executes, so
// everything in it must be safe to treat as public. It is deliberately not
// compiled into the judge.
#pragma once
#include "cases.hpp"
#include "fabric.hpp"
#include "record.hpp"
#include "sha256.hpp"
#include <algorithm>
#include <cstdint>
#include <exception>
#include <map>
#include <optional>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace vobs {

using benchmark-suite_fabric::Plan;
using benchmark-suite_fabric::Problem;

// Recorded in place of a value that does not exist -- a plan that was never
// produced, or a field a returned plan left empty. It is outside the domain of
// every quantity the judge expects, so it can only ever cause a failure.
inline const char *kAbsent = "absent";

inline std::string join_i(const std::vector<std::int64_t> &v) {
  std::string s;
  for (std::size_t i = 0; i < v.size(); ++i) {
    if (i) s += ',';
    s += std::to_string(v[i]);
  }
  return s;
}

inline std::string join_s(const std::vector<std::string> &v) {
  std::string s;
  for (std::size_t i = 0; i < v.size(); ++i) {
    if (i) s += ',';
    s += v[i];
  }
  return s;
}

inline std::string pairs_si(const std::vector<std::pair<std::string, std::int64_t>> &v) {
  std::string s;
  for (std::size_t i = 0; i < v.size(); ++i) {
    if (i) s += ',';
    s += v[i].first;
    s += ':';
    s += std::to_string(v[i].second);
  }
  return s;
}

inline std::string pairs_ss(const std::vector<std::pair<std::string, std::string>> &v) {
  std::string s;
  for (std::size_t i = 0; i < v.size(); ++i) {
    if (i) s += ',';
    s += v[i].first;
    s += ':';
    s += v[i].second;
  }
  return s;
}

// V1 read per-link byte counts through `std::map(link_bytes.begin(), end()).at(id)`.
// Reproduce exactly that view of the returned plan, reporting absence rather
// than throwing when the candidate omitted the link.
inline std::string link_bytes_at(const Plan &p, const std::string &id) {
  const std::map<std::string, std::int64_t> m(p.link_bytes.begin(), p.link_bytes.end());
  const auto it = m.find(id);
  return it == m.end() ? std::string(kAbsent) : std::to_string(it->second);
}

// A whole plan reduced to one bounded field. See sha256.hpp: this is length
// reduction so a record stays inside its ceiling however large a plan comes
// back, not authentication of anything.
inline std::string plan_digest(const std::optional<Plan> &p) {
  if (!p.has_value()) return std::string(kAbsent);
  const std::string s = "parents:" + pairs_ss(p->parents) + ";combine:" + join_s(p->combine_nodes) +
                        ";bytes:" + pairs_si(p->link_bytes) + ";acc:" + pairs_si(p->accumulators) +
                        ";sink:" + join_i(p->sink_values) +
                        ";completion:" + std::to_string(p->completion);
  return vsha::sha256(s);
}

// Whether a call produced a plan at all, drawn from a fixed two-element domain.
inline std::string outcome(const std::optional<Plan> &p) {
  return p.has_value() ? std::string("plan") : std::string("none");
}

// Push the fields of a plan-shaped case, or `absent` for each of them when no
// plan came back. `n` is how many fields the case declares.
inline void push_absent(std::vector<std::string> &v, std::size_t n) {
  for (std::size_t i = 0; i < n; ++i) v.push_back(kAbsent);
}

// Call the candidate for one fixture and collect the raw values. Any exception
// that escapes the candidate is caught here and turned into an ERROR record;
// this function itself never throws.
inline std::string observe(const std::string &id) {
  const std::vector<std::string> &keys = vcase::value_keys(id);
  std::vector<std::string> v;
  try {
    if (id == "saturating_operator_boundaries") {
      v.push_back(std::to_string(benchmark-suite_fabric::saturating_add(90, 30, -100, 100)));
      v.push_back(std::to_string(benchmark-suite_fabric::saturating_add(-90, -30, -100, 100)));
      v.push_back(std::to_string(benchmark-suite_fabric::saturating_add(80, -30, -100, 100)));
    } else if (id == "replay_combine_reduces_link_bytes") {
      const std::optional<Plan> plan =
          benchmark-suite_fabric::replay(vcase::two_source(), vcase::two_paths(), {"X"});
      if (!plan.has_value()) {
        push_absent(v, 4);
      } else {
        v.push_back(link_bytes_at(*plan, "x-s"));
        v.push_back(std::to_string(plan->completion));
        v.push_back(pairs_si(plan->accumulators));
        v.push_back(join_i(plan->sink_values));
      }
    } else if (id == "replay_without_combine_serializes") {
      const std::optional<Plan> plan =
          benchmark-suite_fabric::replay(vcase::two_source(), vcase::two_paths(), {});
      if (!plan.has_value()) {
        push_absent(v, 3);
      } else {
        v.push_back(link_bytes_at(*plan, "x-s"));
        v.push_back(std::to_string(plan->completion));
        v.push_back(join_i(plan->sink_values));
      }
    } else if (id == "accumulator_exact_boundary") {
      Problem p = vcase::two_source();
      v.push_back(outcome(benchmark-suite_fabric::replay(p, vcase::two_paths(), {"X"})));
      p.accumulator_limits["X"] = 15;
      v.push_back(outcome(benchmark-suite_fabric::replay(p, vcase::two_paths(), {"X"})));
    } else if (id == "recover_from_link_and_completion_observations") {
      const std::optional<Plan> plan = benchmark-suite_fabric::recover(vcase::p_recover_observed()).first;
      if (!plan.has_value()) {
        push_absent(v, 2);
      } else {
        v.push_back(join_s(plan->combine_nodes));
        v.push_back(join_i(plan->sink_values));
      }
    } else if (id == "reject_non_tree_route_selection") {
      v.push_back(outcome(
          benchmark-suite_fabric::replay(vcase::p_non_tree(), vcase::non_tree_paths(), {})));
    } else if (id == "canonical_equal_route_tie") {
      Problem p = vcase::p_equal_route_tie();
      const std::optional<Plan> plan = benchmark-suite_fabric::recover(p).first;
      v.push_back(plan.has_value() ? pairs_ss(plan->parents) : std::string(kAbsent));
      v.push_back(plan_digest(plan));
      std::reverse(p.links.begin(), p.links.end());
      v.push_back(plan_digest(benchmark-suite_fabric::recover(p).first));
    } else if (id == "global_observation_beats_first_route") {
      const std::optional<Plan> plan = benchmark-suite_fabric::recover(vcase::p_two_branch(0, 8)).first;
      v.push_back(plan.has_value() ? join_s(plan->combine_nodes) : std::string(kAbsent));
    } else if (id == "equal_time_minimizes_total_link_bytes") {
      const std::optional<Plan> plan = benchmark-suite_fabric::recover(vcase::p_equal_time_different_bytes()).first;
      if (!plan.has_value()) {
        push_absent(v, 3);
      } else {
        std::int64_t total = 0;
        for (const auto &entry : plan->link_bytes) total += entry.second;
        v.push_back(pairs_ss(plan->parents));
        v.push_back(std::to_string(plan->completion));
        v.push_back(std::to_string(total));
      }
    } else if (id == "exact_ready_time_scheduling") {
      const std::optional<Plan> plan =
          benchmark-suite_fabric::replay(vcase::p_ready_time(), vcase::two_paths(), {});
      v.push_back(plan.has_value() ? std::to_string(plan->completion) : std::string(kAbsent));
    } else if (id == "saturating_combine_operation_order") {
      const std::optional<Plan> plan = benchmark-suite_fabric::replay(
          vcase::p_operation_order(), vcase::operation_order_paths(), {"X"});
      v.push_back(plan.has_value() ? join_i(plan->sink_values) : std::string(kAbsent));
    } else if (id == "compound_capacity_symmetry_route_and_replay") {
      const std::optional<Plan> plan = benchmark-suite_fabric::recover(vcase::p_two_branch(8, 0)).first;
      if (!plan.has_value()) {
        push_absent(v, 3);
      } else {
        v.push_back(join_s(plan->combine_nodes));
        v.push_back(link_bytes_at(*plan, "x-s"));
        v.push_back(plan->accumulators.empty()
                        ? std::string(kAbsent)
                        : std::to_string(plan->accumulators.front().second));
      }
    } else {
      return rec::encode_error("unknown_case", id);
    }
  } catch (const std::runtime_error &e) {
    return rec::encode_error("runtime_error", e.what());
  } catch (const std::exception &e) {
    return rec::encode_error("exception", e.what());
  } catch (...) {
    return rec::encode_error("unknown", "a non-standard exception escaped");
  }
  // Node and link identifiers inside a returned plan are candidate-authored, so
  // force every field into the record alphabet before it is written.
  for (std::string &s : v)
    if (!rec::value_ok(s)) s = rec::sanitize(s);
  if (v.size() != keys.size()) return rec::encode_error("internal", "field count mismatch");
  return rec::encode_values(keys, v);
}

}  // namespace vobs
