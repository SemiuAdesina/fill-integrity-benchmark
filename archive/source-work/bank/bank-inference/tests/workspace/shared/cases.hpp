//
// Verifier-owned case fixtures.
//
// These are the exact inputs the V1 graded harness handed to `bank::recover`,
// with the same case names and the same case population. In V1 each trace was
// produced by a helper that replayed the ground-truth mapping through the
// frozen interpreter and copied the resulting banks and/or conflict degree into
// the trace; here those already-materialised traces appear as literals, so the
// ground-truth mapping itself never enters a binary the graded account runs.
//
// Fixtures are inputs only: no expected result appears in this header, which is
// why it is safe to compile into the probe the graded account executes. The
// judge holds the ground truth, re-derives every expectation from it, and also
// re-checks that each revealed observation below is exactly what the frozen
// interpreter produces for that ground truth.
#pragma once
#include "bank/model.hpp"
#include <cstdint>
#include <string>
#include <vector>

namespace vcase {

// Case identifiers, in V1 declaration order. The V1 harness also carried an
// eleventh entry, `zz_negative_control_must_fail`, which existed only to prove
// the pipeline could see a failure; that role now belongs to `judge --selftest`,
// which needs no candidate process at all. The nine graded cases are unchanged.
inline const std::vector<std::string> &ids() {
  static const std::vector<std::string> v{
      "basic_identity",   "word_boundaries",   "broadcast_groups",
      "conflict_disambiguates", "ordered_xor", "ordered_xor_reverse_declaration",
      "minimum_stage_count_tie", "global_evidence",
      "minimum_canonical", "compound_replay",  "compound_global_tie",
      "no_solution"};
  return v;
}

// The grammar each case searches, exactly as V1 declared it.
inline bool grammar(const std::string &id, bank::Grammar &out) {
  if (id == "basic_identity") {
    out = bank::Grammar{8, 4, 2, {{3, 0}, {4, 1}}, {0, 1}};
    return true;
  }
  if (id == "word_boundaries") {
    out = bank::Grammar{8, 8, 2, {{2, 0}, {3, 1}}, {0, 2}};
    return true;
  }
  if (id == "broadcast_groups") {
    out = bank::Grammar{8, 4, 2, {{3, 0}, {4, 1}}, {0, 3}};
    return true;
  }
  if (id == "conflict_disambiguates") {
    out = bank::Grammar{4, 4, 0, {}, {0, 1}};
    return true;
  }
  if (id == "ordered_xor") {
    out = bank::Grammar{8, 4, 2, {{3, 4}, {4, 0}}, {0}};
    return true;
  }
  if (id == "ordered_xor_reverse_declaration") {
    // The required sequence is 3>4 then 4>0, deliberately opposite the
    // declaration order below. A search over combinations cannot express it.
    out = bank::Grammar{8, 4, 2, {{4, 0}, {3, 4}}, {0}};
    return true;
  }
  if (id == "minimum_stage_count_tie") {
    out = bank::Grammar{8, 4, 1, {{3, 0}}, {0, 1}};
    return true;
  }
  if (id == "global_evidence") {
    out = bank::Grammar{8, 4, 2, {{3, 0}, {4, 1}, {5, 2}}, {0, 1}};
    return true;
  }
  if (id == "minimum_canonical") {
    out = bank::Grammar{8, 4, 3, {{6, 0}, {5, 1}, {4, 2}}, {0, 2}};
    return true;
  }
  if (id == "compound_replay") {
    out = bank::Grammar{16, 8, 3, {{4, 5}, {5, 1}, {6, 2}, {4, 0}}, {0, 3, 7}};
    return true;
  }
  if (id == "compound_global_tie") {
    out = bank::Grammar{16, 8, 3, {{4, 1}, {1, 0}, {6, 2}, {5, 3}, {4, 0}}, {0, 3, 7, 11}};
    return true;
  }
  if (id == "no_solution") {
    out = bank::Grammar{4, 4, 0, {}, {0}};
    return true;
  }
  return false;
}

// The observed traces each case supplies. Field order inside every Trace is
// {byte_addresses, lanes, observed_banks, observed_conflict}; an empty
// observed_banks vector means no lane bank was revealed and -1 means the
// conflict degree was withheld, matching the V1 helper's `banks`/`conflict`
// switches.
inline bool traces(const std::string &id, std::vector<bank::Trace> &out) {
  out.clear();
  if (id == "basic_identity") {
    out.push_back(bank::Trace{{0, 4, 8, 12, 16}, {0, 1, 2, 3, 4}, {0, 0, 0, 0, 0}, 5});
    return true;
  }
  if (id == "word_boundaries") {
    out.push_back(bank::Trace{{7, 8, 15, 16, 31, 32}, {0, 0, 1, 1, 2, 2}, {0, 1, 0, 3, 1, 7}, 2});
    return true;
  }
  if (id == "broadcast_groups") {
    out.push_back(bank::Trace{{64, 64, 68, 68, 72, 76}, {7, 2, 6, 1, 0, 3}, {7, 7, 7, 7, 3, 7}, 3});
    return true;
  }
  if (id == "conflict_disambiguates") {
    // Bank observations withheld; only the conflict degree is revealed.
    out.push_back(bank::Trace{{0, 0, 0, 4}, {0, 1, 2, 3}, {}, 2});
    return true;
  }
  if (id == "ordered_xor") {
    out.push_back(bank::Trace{{0, 32, 64, 96, 128, 160}, {0, 1, 2, 3, 4, 5}, {0, 0, 3, 3, 4, 4}, 2});
    return true;
  }
  if (id == "ordered_xor_reverse_declaration") {
    // Behaviour: stage order is a candidate decision, not inherited from the
    // allowed-stage declaration. Word 8 first toggles bit 4, which the second
    // stage must then read to toggle bank bit 0.
    // Rejects: treating allowed stages as an unordered subset or enumerating
    // only declaration-order combinations; neither can produce bank 1 here.
    out.push_back(bank::Trace{{32}, {0}, {1}, 1});
    return true;
  }
  if (id == "minimum_stage_count_tie") {
    // Behaviour: rotation-only and one-stage/no-rotation mappings both have
    // operation cost one and reproduce this observation; the secondary
    // selected-stage count therefore chooses the rotation-only mapping.
    // Rejects: skipping the stage-count key and comparing canonical strings
    // immediately, which chooses the one-stage mapping instead.
    out.push_back(bank::Trace{{32}, {0}, {1}, 1});
    return true;
  }
  if (id == "global_evidence") {
    // Conflict only, banks only, then both: no single trace determines the map.
    out.push_back(bank::Trace{{0, 4, 8, 12}, {0, 1, 2, 3}, {}, 2});
    out.push_back(bank::Trace{{64, 80, 96, 112}, {3, 2, 1, 0}, {6, 5, 1, 6}, -1});
    out.push_back(bank::Trace{{124, 128, 132, 136}, {7, 5, 3, 1}, {4, 6, 5, 0}, 1});
    return true;
  }
  if (id == "minimum_canonical") {
    out.push_back(bank::Trace{{0, 4, 8, 12}, {0, 1, 2, 3}, {0, 0, 0, 0}, 4});
    return true;
  }
  if (id == "compound_replay") {
    out.push_back(bank::Trace{{7, 8, 127, 128, 136, 136, 255, 256},
                              {0, 1, 2, 3, 11, 4, 15, 7},
                              {7, 9, 6, 8, 8, 8, 11, 12},
                              2});
    out.push_back(bank::Trace{{512, 520, 768, 776, 1024, 1032}, {15, 13, 9, 5, 1, 0}, {2, 1, 6, 11, 8, 6}, -1});
    out.push_back(bank::Trace{{2040, 2048, 2056, 2064, 2072}, {6, 6, 7, 8, 9}, {}, 2});
    return true;
  }
  if (id == "compound_global_tie") {
    // Compound case. Every published rule in `spec/mapping.md` is load-bearing
    // here at once, and each one is individually decisive:
    //
    //   * word boundary  - the first trace straddles word edges, so replaying
    //                      byte addresses without the word_width division makes
    //                      the evidence unsatisfiable rather than merely wrong;
    //   * broadcast      - the first trace repeats one byte address on two
    //                      lanes whose raw banks differ, so the lowest-lane
    //                      leader rule changes what is observed;
    //   * ordered XOR    - one stage here writes a bit that another stage
    //                      reads, and the stage that must run first is not the
    //                      first in any natural sorted order, so no mapping
    //                      whose stages are applied in sorted order reproduces
    //                      the evidence at all: stages cannot be canonicalised
    //                      into a set;
    //   * nonzero rotation - no zero-stage mapping at any rotation fits, so a
    //                      rotation-only answer is rejected;
    //   * global evidence - each of the three traces on its own admits many
    //                      mappings and its own best mapping is not the answer,
    //                      so traces must be fitted jointly rather than
    //                      independently;
    //   * minimum + canonical tie - several mappings satisfy all three traces
    //                      at the same operation and stage count, so the
    //                      lexicographic canonical string is what decides, and
    //                      the first mapping found in a natural enumeration is
    //                      not the required one.
    //
    // The three traces reveal different subsets: banks and conflict, banks
    // only, and conflict only.
    out.push_back(bank::Trace{{383, 384, 391, 392, 392, 392, 440, 441},
                              {12, 7, 4, 9, 8, 0, 5, 8},
                              {1, 9, 4, 1, 1, 1, 13, 14},
                              2});
    out.push_back(bank::Trace{{2532, 2533, 3455, 3465, 503, 2501}, {4, 11, 15, 4, 1, 3}, {8, 1, 12, 5, 8, 13}, -1});
    out.push_back(bank::Trace{{1832, 344, 1160, 632, 1360}, {12, 8, 3, 7, 12}, {}, 2});
    return true;
  }
  if (id == "no_solution") {
    // Bank 3 is unreachable under the single legal mapping, so no candidate fits.
    out.push_back(bank::Trace{{0}, {0}, {3}, -1});
    return true;
  }
  return false;
}

// The withheld replay `compound_replay` performs on the recovered mapping. V1
// built this trace inline; it is an input, so it lives here. The banks and
// conflict degree it is supposed to produce are held only by the judge.
inline bank::Trace compound_secret_trace() {
  return bank::Trace{{4095, 4096, 4104, 4104, 8192}, {15, 0, 3, 12, 7}, {}, -1};
}

// The withheld replay `compound_global_tie` performs on the mapping the
// candidate recovered. It is an input, so it lives here; the banks and conflict
// degree it must produce are held only by the judge.
//
// This trace re-checks the same public rules a second, independent way: it
// straddles a word boundary, repeats one byte address on two lanes whose raw
// banks differ (so the lowest-lane broadcast leader decides both), and produces
// a conflict degree above one. A mapping that merely satisfies the three
// supplied traces but gets the word, broadcast or ordering rules wrong replays
// this trace differently.
inline bank::Trace compound_tie_secret_trace() {
  return bank::Trace{{1967, 1968, 2068, 2068, 2216}, {2, 14, 11, 3, 8}, {}, -1};
}

// Ordered field names a VALUES record must carry for a case. This is the
// complete and only field set: the probe emits exactly these keys in exactly
// this order and the judge decodes against exactly this list, so a record has
// no room for anything that is not one of the raw functional values the V1
// assertions examined.
inline const std::vector<std::string> &value_keys(const std::string &id) {
  static const std::vector<std::string> empty{};
  // Eight cases asserted one property: the canonical string of the recovered
  // mapping equals the canonical string of the planted mapping.
  static const std::vector<std::string> k_canonical{"canonical"};
  // compound_replay additionally replayed a withheld trace through whatever
  // mapping was recovered and asserted its banks and conflict degree.
  static const std::vector<std::string> k_compound{"canonical", "secret_banks", "secret_conflict"};
  if (id == "compound_replay") return k_compound;
  if (id == "compound_global_tie") return k_compound;
  for (const std::string &known : ids())
    if (known == id) return k_canonical;
  return empty;
}

}  // namespace vcase
