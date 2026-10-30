// Synthetic public diagnostics. These inputs are not verifier fixtures.
#include "fabric.hpp"
#include <algorithm>
#include <iostream>
#include <map>
#include <string>
#include <vector>

using namespace benchmark-suite_fabric;

namespace {
Problem example() {
  Problem p;
  p.sink = "dst";
  p.saturation_min = -20;
  p.saturation_max = 20;
  p.links = {{"p-r", "P", "R", 4, 1}, {"q-r", "Q", "R", 4, 2},
             {"r-d", "R", "dst", 4, 1}};
  p.sources = {{"p", "P", 4, 18}, {"q", "Q", 4, 7}};
  p.reduction_nodes = {"R"};
  p.accumulator_limits = {{"R", 8}};
  return p;
}

std::int64_t total_bytes(const Plan &plan) {
  std::int64_t total = 0;
  for (const auto &entry : plan.link_bytes) total += entry.second;
  return total;
}

Problem equal_time_routes() {
  Problem p;
  p.sink = "dst";
  p.saturation_min = -20;
  p.saturation_max = 20;
  p.sources = {{"u", "U", 3, 2}};
  p.links = {{"long-1", "U", "M", 3, 0}, {"long-2", "M", "dst", 3, 0},
             {"short", "U", "dst", 3, 1}};
  return p;
}

bool report(bool ok, const char *message) {
  std::cout << (ok ? "PASS: " : "FAILED: ") << message << '\n';
  return ok;
}
}  // namespace

int main() {
  bool ok = true;
  ok &= report(saturating_add(18, 7, -20, 20) == 20,
               "signed saturation is applied after each combine operation");

  Problem p = example();
  const std::vector<std::vector<std::string>> paths{{"p-r", "r-d"},
                                                    {"q-r", "r-d"}};
  const auto replayed = replay(p, paths, {"R"});
  ok &= report(replayed && replayed->accumulators ==
                                std::vector<std::pair<std::string, std::int64_t>>{{"R", 8}},
               "accumulator capacity equality is legal");
  ok &= report(replayed && replayed->sink_values == std::vector<std::int64_t>{20},
               "combined payload value follows canonical saturating order");

  const auto uncombined = replay(p, paths, {});
  ok &= report(uncombined && uncombined->completion == 6 && total_bytes(*uncombined) == 16,
               "uncombined payloads serialize by ready tick with exact bytes");

  Problem non_tree = p;
  non_tree.links.push_back({"r-e", "R", "E", 4, 0});
  non_tree.links.push_back({"e-d", "E", "dst", 4, 0});
  ok &= report(!replay(non_tree,
                       {{"p-r", "r-d"}, {"q-r", "r-e", "e-d"}}, {}),
               "route selections that diverge after meeting are rejected");

  Problem ordered = p;
  ordered.links.push_back({"w-r", "W", "R", 4, 0});
  ordered.sources = {{"a", "P", 4, 18}, {"b", "Q", 4, 18}, {"c", "W", 4, -20}};
  ordered.accumulator_limits["R"] = 12;
  const auto folded = replay(ordered,
                             {{"p-r", "r-d"}, {"q-r", "r-d"}, {"w-r", "r-d"}},
                             {"R"});
  ok &= report(folded && folded->sink_values == std::vector<std::int64_t>{16},
               "ready-time then origin order governs saturating fold operations");

  p.observed_link_bytes = {{"r-d", 4}};
  const auto recovered = recover(p).first;
  ok &= report(recovered && recovered->combine_nodes == std::vector<std::string>{"R"},
               "global recovery respects combine-dependent observed traffic");
  ok &= report(recovered && recovered->parents ==
                               std::vector<std::pair<std::string, std::string>>{
                                   {"P", "p-r"}, {"Q", "q-r"}, {"R", "r-d"}},
               "recovery emits the canonical suffix-consistent tree");

  Problem objective = equal_time_routes();
  const auto best = recover(objective).first;
  ok &= report(best && best->completion == 2 && total_bytes(*best) == 3 &&
                        best->parents == std::vector<std::pair<std::string, std::string>>{
                                             {"U", "short"}},
               "equal-completion recovery minimizes traffic before canonical tie-breaking");

  std::reverse(objective.links.begin(), objective.links.end());
  ok &= report(best && recover(objective).first == best,
               "canonical recovery is independent of link container order");
  return ok ? 0 : 1;
}
