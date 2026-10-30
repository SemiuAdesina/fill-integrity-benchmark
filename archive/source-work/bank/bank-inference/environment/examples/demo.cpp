#include "bank/model.hpp"

#include <iostream>

int main() {
  bank::Grammar grammar{8, 4, 2, {{3, 0}, {4, 1}}, {0, 1}};
  bank::Trace trace{{0, 4, 8, 12}, {0, 1, 2, 3}, {}, -1};
  const auto mapping = bank::recover(grammar, {trace});
  std::cout << bank::canonical(mapping) << '\n';
}
