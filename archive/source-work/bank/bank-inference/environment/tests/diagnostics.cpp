#include "bank/model.hpp"

#include <iostream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace {
int failures = 0;
void check(bool ok, const char* message) {
  if (ok) return;
  ++failures;
  std::cerr << "FAILED: " << message << '\n';
}

bank::Trace observed(const bank::Grammar& g, const bank::Mapping& truth,
                     std::vector<std::uint64_t> addresses,
                     std::vector<unsigned> lanes, bool banks, bool conflict) {
  bank::Trace t{std::move(addresses), std::move(lanes), {}, -1};
  const auto r = bank::replay(g, truth, t);
  if (banks) t.observed_banks.assign(r.banks.begin(), r.banks.end());
  if (conflict) t.observed_conflict = static_cast<int>(r.conflict_degree);
  return t;
}
}  // namespace

int main() {
  {
    const bank::Grammar g{8, 8, 2, {{2, 0}, {3, 1}}, {0, 2}};
    const bank::Mapping truth{{{2, 0}}, 0};
    const auto t = observed(g, truth, {7, 8, 15, 16, 31, 32}, {0, 0, 1, 1, 2, 2}, true, true);
    check(bank::canonical(bank::recover(g, {t})) == bank::canonical(truth),
          "byte addresses are truncated to words before bank mapping");
  }
  {
    const bank::Grammar g{8, 4, 2, {{3, 4}, {4, 0}}, {0}};
    const bank::Mapping truth{{{3, 4}, {4, 0}}, 0};
    const auto t = observed(g, truth, {0, 32, 64, 96, 128, 160}, {0, 1, 2, 3, 4, 5}, true, true);
    check(bank::canonical(bank::recover(g, {t})) == bank::canonical(truth),
          "XOR stages execute in listed order against the current word");
  }
  {
    const bank::Grammar g{8, 4, 2, {{4, 0}, {3, 4}}, {0}};
    const bank::Mapping truth{{{3, 4}, {4, 0}}, 0};
    const auto t = observed(g, truth, {32}, {0}, true, true);
    check(bank::canonical(bank::recover(g, {t})) == bank::canonical(truth),
          "selected XOR stage order is independent of allowed-stage declaration order");
  }
  {
    const bank::Grammar g{8, 4, 1, {{4, 1}}, {0, 3}};
    const bank::Mapping truth{{{4, 1}}, 3};
    const auto t = observed(g, truth, {64, 64, 68, 68, 72, 76}, {7, 2, 6, 1, 0, 3}, true, true);
    check(bank::canonical(bank::recover(g, {t})) == bank::canonical(truth),
          "lane rotation and lowest-lane broadcast leadership both affect banks");
  }
  {
    const bank::Grammar g{4, 4, 0, {}, {0, 1}};
    const bank::Mapping truth{{}, 1};
    const auto t = observed(g, truth, {0, 0, 0, 4}, {0, 1, 2, 3}, false, true);
    check(bank::canonical(bank::recover(g, {t})) == bank::canonical(truth),
          "conflict degree counts distinct addresses, with a broadcast consuming one service");
  }
  {
    const bank::Grammar g{8, 4, 2, {{3, 0}, {4, 1}, {5, 2}}, {0, 1}};
    const bank::Mapping truth{{{3, 0}, {4, 1}}, 1};
    std::vector<bank::Trace> traces{
        observed(g, truth, {0, 4, 8, 12}, {0, 1, 2, 3}, false, true),
        observed(g, truth, {64, 80, 96, 112}, {3, 2, 1, 0}, true, false),
        observed(g, truth, {124, 128, 132, 136}, {7, 5, 3, 1}, true, true)};
    check(bank::canonical(bank::recover(g, traces)) == bank::canonical(truth),
          "all traces form one global evidence set; no trace is fitted independently");
  }
  {
    const bank::Grammar g{8, 4, 3, {{6, 0}, {5, 1}, {4, 2}}, {0, 2}};
    const bank::Mapping truth{{}, 0};
    const auto t = observed(g, truth, {0, 4, 8, 12}, {0, 1, 2, 3}, true, true);
    check(bank::canonical(bank::recover(g, {t})) == bank::canonical(truth),
          "recovery minimizes operation cost, then stage count, then canonical string");
  }
  {
    const bank::Grammar g{8, 4, 1, {{3, 0}}, {0, 1}};
    const bank::Mapping truth{{}, 1};
    const auto t = observed(g, truth, {32}, {0}, true, true);
    check(bank::canonical(bank::recover(g, {t})) == bank::canonical(truth),
          "equal operation cost is resolved by fewer selected XOR stages before canonical text");
  }
  {
    const bank::Mapping a{{{4, 1}, {1, 0}}, 3};
    const bank::Mapping b{{{1, 0}, {4, 1}}, 3};
    check(bank::canonical(a) != bank::canonical(b) &&
              bank::canonical(a).find("4>1,1>0") != std::string::npos,
          "canonical mapping text preserves stage order and makes ties deterministic");
  }

  if (failures == 0) std::cout << "PASS: 9 public semantic diagnostics\n";
  return failures == 0 ? 0 : 1;
}
