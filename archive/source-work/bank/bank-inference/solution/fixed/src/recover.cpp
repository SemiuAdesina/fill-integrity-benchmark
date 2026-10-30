#include "bank/model.hpp"

#include <algorithm>
#include <functional>
#include <limits>
#include <stdexcept>
#include <string>

namespace bank {
namespace {

bool matches(const Grammar& g, const Mapping& m,
             const std::vector<Trace>& traces) {
  for (const auto& trace : traces) {
    const Replay got = replay(g, m, trace);
    if (!trace.observed_banks.empty()) {
      for (std::size_t i = 0; i < got.banks.size(); ++i) {
        if (trace.observed_banks[i] >= 0 &&
            got.banks[i] != static_cast<unsigned>(trace.observed_banks[i])) {
          return false;
        }
      }
    }
    if (trace.observed_conflict >= 0 &&
        got.conflict_degree != static_cast<unsigned>(trace.observed_conflict)) {
      return false;
    }
  }
  return true;
}

}  // namespace

Mapping recover(const Grammar& g, const std::vector<Trace>& traces) {
  bool found = false;
  Mapping best;
  unsigned best_cost = std::numeric_limits<unsigned>::max();
  std::string best_key;

  for (const unsigned rotation : g.allowed_lane_rotations) {
    std::vector<XorStage> current;
    std::vector<bool> used(g.allowed_stages.size(), false);
    std::function<void()> visit = [&] {
      Mapping candidate{current, rotation};
      if (matches(g, candidate, traces)) {
        const unsigned cost = static_cast<unsigned>(current.size()) +
                              (rotation == 0 ? 0U : 1U);
        const std::string key = canonical(candidate);
        if (!found || cost < best_cost ||
            (cost == best_cost && current.size() < best.stages.size()) ||
            (cost == best_cost && current.size() == best.stages.size() &&
             key < best_key)) {
          found = true;
          best = candidate;
          best_cost = cost;
          best_key = key;
        }
      }
      if (current.size() == g.max_stages) return;
      for (std::size_t i = 0; i < g.allowed_stages.size(); ++i) {
        if (used[i]) continue;
        used[i] = true;
        current.push_back(g.allowed_stages[i]);
        visit();
        current.pop_back();
        used[i] = false;
      }
    };
    visit();
  }
  if (!found) throw std::runtime_error("no bank mapping matches all traces");
  return best;
}

}  // namespace bank
