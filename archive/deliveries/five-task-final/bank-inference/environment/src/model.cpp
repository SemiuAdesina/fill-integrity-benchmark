#include "bank/model.hpp"

#include <algorithm>
#include <map>
#include <set>
#include <sstream>
#include <stdexcept>

namespace bank {
namespace {

void validate(const Grammar& g, const Mapping& m) {
  if (g.bank_count < 2 || (g.bank_count & (g.bank_count - 1)) != 0 ||
      g.word_width == 0 || m.stages.size() > g.max_stages) {
    throw std::invalid_argument("invalid bank grammar or mapping");
  }
  if (std::find(g.allowed_lane_rotations.begin(),
                g.allowed_lane_rotations.end(), m.lane_rotation) ==
      g.allowed_lane_rotations.end()) {
    throw std::invalid_argument("lane rotation outside grammar");
  }
  std::set<std::pair<unsigned, unsigned>> seen;
  for (const auto& stage : m.stages) {
    if (std::find(g.allowed_stages.begin(), g.allowed_stages.end(), stage) ==
            g.allowed_stages.end() ||
        !seen.emplace(stage.source_bit, stage.destination_bit).second) {
      throw std::invalid_argument("xor stage outside grammar or repeated");
    }
  }
}

}  // namespace

Replay replay(const Grammar& g, const Mapping& m, const Trace& t) {
  validate(g, m);
  const std::size_t n = t.byte_addresses.size();
  if (n == 0 || t.lanes.size() != n ||
      (!t.observed_banks.empty() && t.observed_banks.size() != n)) {
    throw std::invalid_argument("malformed trace");
  }

  Replay result;
  result.banks.resize(n);
  const unsigned mask = g.bank_count - 1;
  for (std::size_t i = 0; i < n; ++i) {
    std::uint64_t word = t.byte_addresses[i] / g.word_width;
    for (const auto& stage : m.stages) {
      const std::uint64_t bit = (word >> stage.source_bit) & 1U;
      word ^= bit << stage.destination_bit;
    }
    const unsigned permuted_lane =
        (t.lanes[i] + m.lane_rotation) % g.bank_count;
    result.banks[i] = static_cast<unsigned>(word ^ permuted_lane) & mask;
  }

  // Equal byte addresses form a broadcast. They consume one service and the
  // whole broadcast observes the bank selected by its lowest lane number.
  std::map<std::uint64_t, std::vector<std::size_t>> groups;
  for (std::size_t i = 0; i < n; ++i) groups[t.byte_addresses[i]].push_back(i);
  for (const auto& [address, members] : groups) {
    (void)address;
    const auto leader = *std::min_element(
        members.begin(), members.end(),
        [&](std::size_t a, std::size_t b) { return t.lanes[a] < t.lanes[b]; });
    for (const auto i : members) result.banks[i] = result.banks[leader];
  }

  std::vector<std::set<std::uint64_t>> distinct(g.bank_count);
  for (std::size_t i = 0; i < n; ++i) {
    distinct[result.banks[i]].insert(t.byte_addresses[i]);
  }
  result.conflict_degree = 1;
  for (const auto& addresses : distinct) {
    result.conflict_degree =
        std::max(result.conflict_degree, static_cast<unsigned>(addresses.size()));
  }
  return result;
}

std::string canonical(const Mapping& m) {
  std::ostringstream out;
  out << "rotate=" << m.lane_rotation << ";stages=";
  for (std::size_t i = 0; i < m.stages.size(); ++i) {
    if (i) out << ',';
    out << m.stages[i].source_bit << '>' << m.stages[i].destination_bit;
  }
  return out.str();
}

}  // namespace bank
