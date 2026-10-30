//
// Public rule-level diagnostics.
//
// Every check below is a small synthetic instance of one published mapping rule
// from spec/mapping.md, solved by the recovery routine under repair. The
// fixtures and expected answers here are public; the graded population is
// separate and combines these same rules adversarially. A failing check names
// the rule that was violated, so this file is the fastest way to find out which
// part of the contract an implementation still gets wrong.
//
//   cmake -S /app -B /app/build && cmake --build /app/build
//   /app/build/bank_diagnostics
#include "bank/model.hpp"

#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {

int failures = 0;

void expect(const std::string& rule, const bank::Grammar& grammar,
            const std::vector<bank::Trace>& traces, const std::string& expected) {
  std::string got;
  try {
    got = bank::canonical(bank::recover(grammar, traces));
  } catch (const std::exception& error) {
    got = std::string("threw ") + error.what();
  }
  if (got == expected) {
    std::cout << "ok      " << rule << '\n';
    return;
  }
  ++failures;
  std::cout << "FAILED: " << rule << "\n        expected " << expected
            << ", recovered " << got << '\n';
}

void expect_rejected(const std::string& rule, const bank::Grammar& grammar,
                     const std::vector<bank::Trace>& traces) {
  try {
    const bank::Mapping mapping = bank::recover(grammar, traces);
    ++failures;
    std::cout << "FAILED: " << rule << "\n        expected std::runtime_error, recovered "
              << bank::canonical(mapping) << '\n';
  } catch (const std::runtime_error&) {
    std::cout << "ok      " << rule << '\n';
  } catch (const std::exception& error) {
    ++failures;
    std::cout << "FAILED: " << rule
              << "\n        expected std::runtime_error, a different exception escaped: "
              << error.what() << '\n';
  }
}

}  // namespace

int main() {
  expect("a byte address becomes a word by integer division by word_width",
         bank::Grammar{8, 4, 1, {{2, 0}}, {0}},
         {bank::Trace{{0, 4, 8, 12, 16}, {0, 1, 2, 3, 4}, {0, 0, 0, 0, 0}, -1}},
         "rotate=0;stages=");

  expect("XOR stages run in the listed order, so a later stage reads the bit an "
         "earlier stage wrote",
         bank::Grammar{8, 1, 2, {{0, 1}, {1, 2}}, {0}},
         {bank::Trace{{0, 1, 2, 3}, {0, 1, 2, 3}, {0, 2, 4, 6}, -1}},
         "rotate=0;stages=1>2,0>1");

  expect("the rotated lane is (lane + rotation) mod bank_count before it is XORed "
         "into the word",
         bank::Grammar{8, 4, 1, {{2, 0}}, {0, 3}},
         {bank::Trace{{0, 4, 8, 12}, {0, 1, 2, 3}, {3, 5, 7, 5}, -1}},
         "rotate=3;stages=");

  expect("equal byte addresses form a broadcast that takes the raw bank of its "
         "lowest lane",
         bank::Grammar{8, 4, 1, {{2, 0}}, {0, 1}},
         {bank::Trace{{8, 8, 8, 12}, {3, 1, 2, 0}, {0, 0, 0, 2}, -1}},
         "rotate=1;stages=");

  expect("conflict degree counts distinct byte addresses served by a bank, not lanes",
         bank::Grammar{4, 4, 0, {}, {0, 1, 2, 3}},
         {bank::Trace{{8, 8, 12}, {0, 1, 2}, {}, 1}},
         "rotate=0;stages=");

  expect("a legal mapping matches every supplied trace jointly, not the first one",
         bank::Grammar{8, 4, 2, {{2, 0}, {0, 1}}, {0, 1}},
         {bank::Trace{{0, 4}, {0, 0}, {}, 1},
          bank::Trace{{8, 12}, {1, 2}, {0, 2}, -1}},
         "rotate=1;stages=0>1");

  expect("recovery minimizes operation count, where a nonzero rotation costs one "
         "operation",
         bank::Grammar{8, 4, 1, {{2, 0}}, {3, 0}},
         {bank::Trace{{0, 4}, {0, 0}, {}, 1}},
         "rotate=0;stages=");

  expect("an operation-count tie is broken by the smaller selected-stage count",
         bank::Grammar{8, 4, 1, {{0, 2}}, {0, 1, 2, 3, 4, 5, 6, 7}},
         {bank::Trace{{0, 4}, {0, 1}, {}, 1}},
         "rotate=1;stages=");

  expect("a remaining tie is broken by the lexicographically smallest canonical "
         "string, which is a string comparison and not a numeric one",
         bank::Grammar{16, 4, 1, {{3, 0}}, {0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15}},
         {bank::Trace{{0, 4, 8, 12}, {0, 1, 2, 3}, {}, 2}},
         "rotate=10;stages=");

  expect_rejected("evidence that no mapping in the grammar can produce must raise "
                  "std::runtime_error",
                  bank::Grammar{4, 4, 0, {}, {0}},
                  {bank::Trace{{0, 4}, {0, 0}, {3, 3}, -1}});

  std::cout << (failures == 0 ? "all public rule diagnostics passed\n"
                             : "public rule diagnostics failing: ")
            << (failures == 0 ? "" : std::to_string(failures) + "\n");
  return failures == 0 ? 0 : 1;
}
