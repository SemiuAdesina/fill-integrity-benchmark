#include "bank/model.hpp"

#include <stdexcept>

namespace bank {

// The starter only recognizes the unswizzled mapping. It deliberately ignores
// the supplied evidence and is the repair target.
Mapping recover(const Grammar& grammar, const std::vector<Trace>& traces) {
  (void)traces;
  if (grammar.allowed_lane_rotations.empty()) {
    throw std::invalid_argument("grammar has no lane choices");
  }
  return Mapping{{}, grammar.allowed_lane_rotations.front()};
}

}  // namespace bank
