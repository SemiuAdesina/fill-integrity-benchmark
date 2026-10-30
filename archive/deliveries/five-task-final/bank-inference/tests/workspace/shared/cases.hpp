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

// Case identifiers. The first ten are the V1 population in V1 declaration
// order. The V1 harness also carried an eleventh entry,
// `zz_negative_control_must_fail`, which existed only to prove the pipeline
// could see a failure; that role now belongs to `judge --selftest`, which needs
// no candidate process at all.
//
// The final two entries were added after a differential study showed that two
// rules `spec/mapping.md` publishes were satisfied by every V1 fixture even
// when an implementation ignored them, so nothing in the graded population
// depended on them:
//
//   * `compound_stage_chain_runs_against_grammar_order` - in every V1 fixture
//     the planted stage sequence happened to appear in the grammar's own
//     declaration order, so enumerating stage subsets in grammar order instead
//     of ordered sequences passed the whole population;
//   * `equal_operation_count_prefers_fewer_stages` - no V1 fixture ever put a
//     rotation-only mapping and a one-more-stage mapping at the same operation
//     count, so the second selection tier never decided anything.
//
// Both are ordinary instances of the published contract; neither adds a rule.
inline const std::vector<std::string> &ids() {
  static const std::vector<std::string> v{
      "basic_identity",   "word_boundaries",   "broadcast_groups",
      "conflict_disambiguates", "ordered_xor", "global_evidence",
      "minimum_canonical", "compound_replay",  "compound_global_tie",
      "no_solution",
      "compound_stage_chain_runs_against_grammar_order",
      "equal_operation_count_prefers_fewer_stages"};
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
  if (id == "compound_stage_chain_runs_against_grammar_order") {
    // Stage {5,1} writes bit 1 and stage {1,0} reads bit 1, so the two do not
    // commute, and the order the evidence requires is the reverse of the order
    // the grammar lists them in. {6,1} is a live decoy that also writes bit 1.
    out = bank::Grammar{8, 8, 3, {{1, 0}, {5, 1}, {6, 1}}, {0, 6}};
    return true;
  }
  if (id == "equal_operation_count_prefers_fewer_stages") {
    out = bank::Grammar{8, 8, 2, {{4, 2}, {7, 2}}, {0, 4}};
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
    // PUBLIC BEHAVIOUR: the base mapping. A byte address becomes a word by
    // integer division by word_width, and the bank is the low log2(bank_count)
    // bits of the word XOR the rotated lane. Here the lane XOR is what makes
    // five different words share one bank, and the conflict degree counts the
    // five distinct addresses that bank serves.
    //
    // REJECTS: an implementation that drops the lane from the bank computation.
    // The five words here would then land in five different banks with a
    // conflict degree of one, and because bits 3 and 4 are clear in all of them
    // no stage or rotation can repair that, so recovery fails outright rather
    // than returning a wrong answer. Also rejects answering with a stage or a
    // rotation when a cheaper mapping already reproduces the evidence, since
    // selection is by fewest operations.
    out.push_back(bank::Trace{{0, 4, 8, 12, 16}, {0, 1, 2, 3, 4}, {0, 0, 0, 0, 0}, 5});
    return true;
  }
  if (id == "word_boundaries") {
    // PUBLIC BEHAVIOUR: the byte-address-to-word conversion truncates. With
    // word_width 8, byte 7 and byte 8 fall in different words while bytes 8 and
    // 15 fall in the same word, and the pairs here straddle exactly those
    // edges on the same lanes.
    //
    // REJECTS: an implementation that feeds the byte address into the bank
    // computation directly, and one that rounds or masks instead of dividing:
    // either way the members of each straddling pair come out identical or
    // shifted, and no legal mapping reproduces the revealed banks.
    out.push_back(bank::Trace{{7, 8, 15, 16, 31, 32}, {0, 0, 1, 1, 2, 2}, {0, 1, 0, 3, 1, 7}, 2});
    return true;
  }
  if (id == "broadcast_groups") {
    // PUBLIC BEHAVIOUR: lanes issuing the same byte address in one trace form a
    // broadcast. The group consumes a single service and every member reports
    // the bank of its lowest-numbered lane, not the bank its own lane would
    // have produced. Two addresses are repeated here, and in each pair the
    // lowest lane is not the one listed first.
    //
    // REJECTS: an implementation that computes every lane independently, which
    // gives the members of a pair two different banks; and one that takes the
    // first member listed as the group leader instead of the lowest lane.
    out.push_back(bank::Trace{{64, 64, 68, 68, 72, 76}, {7, 2, 6, 1, 0, 3}, {7, 7, 7, 7, 3, 7}, 3});
    return true;
  }
  if (id == "conflict_disambiguates") {
    // PUBLIC BEHAVIOUR: a trace need not reveal any lane bank. Here only the
    // conflict degree is revealed, and it is still enough to decide the mapping.
    // The degree is the largest number of DISTINCT byte addresses any single
    // bank serves, so the three lanes sharing one address contribute one.
    //
    // REJECTS: an implementation that requires bank observations and treats an
    // empty observation vector as vacuously satisfied, which would leave the
    // choice undetermined and yield the wrong mapping; and one that counts
    // lanes per bank rather than distinct addresses.
    out.push_back(bank::Trace{{0, 0, 0, 4}, {0, 1, 2, 3}, {}, 2});
    return true;
  }
  if (id == "ordered_xor") {
    // PUBLIC BEHAVIOUR: selected stages run as an ordered sequence and each
    // stage reads the word as it stands at that moment. The grammar here offers
    // {3,4} and {4,0}: the first writes bit 4 and the second reads bit 4, so the
    // second observes the first stage's effect. The rotation family is {0} only,
    // which keeps this case focused on the chaining rule.
    //
    // REJECTS: an implementation that applies every selected stage against the
    // ORIGINAL word and XORs the results together, i.e. treats the stages as a
    // parallel bit mask. That is the natural first reading of a bit-toggle list
    // and it produces a different word for every address here.
    out.push_back(bank::Trace{{0, 32, 64, 96, 128, 160}, {0, 1, 2, 3, 4, 5}, {0, 0, 3, 3, 4, 4}, 2});
    return true;
  }
  if (id == "global_evidence") {
    // PUBLIC BEHAVIOUR: revelation is partial and per trace. The first trace
    // here reveals only the conflict degree, the second only the banks, the
    // third both, and the recovered mapping must satisfy all three jointly. The
    // first trace on its own is satisfied by a cheaper mapping than the one the
    // three traces together require.
    //
    // REJECTS: an implementation that fits the first trace and returns, which is
    // the natural shape of a loop that stops at its first consistent answer.
    // Note that the second and third traces each happen to pin the same mapping
    // the full set does, so this case is decisive against first-trace fitting
    // rather than against every possible single-trace choice; `compound_replay`
    // and `compound_stage_chain_runs_against_grammar_order` also reject it.
    out.push_back(bank::Trace{{0, 4, 8, 12}, {0, 1, 2, 3}, {}, 2});
    out.push_back(bank::Trace{{64, 80, 96, 112}, {3, 2, 1, 0}, {6, 5, 1, 6}, -1});
    out.push_back(bank::Trace{{124, 128, 132, 136}, {7, 5, 3, 1}, {4, 6, 5, 0}, 1});
    return true;
  }
  if (id == "minimum_canonical") {
    // PUBLIC BEHAVIOUR: the evidence here is deliberately weak - sixteen of this
    // grammar's mappings reproduce it - so what is being checked is that
    // selection happens at all: fewest operations first, then fewest stages,
    // then the smallest canonical string. The required answer is the cheapest
    // mapping in the grammar, so a correct implementation must not spend an
    // operation it does not need.
    //
    // REJECTS: an implementation that selects on legality alone and returns
    // whatever its enumeration reaches first, whenever that enumeration does not
    // happen to start at the cheapest mapping - for instance one that walks the
    // rotations or the stage sequences from the far end.
    //
    // NOT DECISIVE HERE: an enumeration that happens to begin at the empty stage
    // list and rotation 0 finds the required answer first, so this case does not
    // by itself exercise the second or third selection tier. The tiers are
    // exercised by `equal_operation_count_prefers_fewer_stages` (stage count)
    // and `compound_global_tie` (canonical string).
    out.push_back(bank::Trace{{0, 4, 8, 12}, {0, 1, 2, 3}, {0, 0, 0, 0}, 4});
    return true;
  }
  if (id == "compound_replay") {
    // PUBLIC BEHAVIOUR: the full contract at 16 banks and word_width 8, with
    // three traces revealing different subsets. On top of the recovered mapping
    // being consistent with those traces, this case replays the recovered
    // mapping on a trace that is NOT part of the evidence (see
    // `compound_secret_trace` below) and checks the banks and conflict degree it
    // produces. The mapping therefore has to be right as a function, not merely
    // compatible with what was shown.
    //
    // REJECTS: any mapping that overfits the supplied traces, since the withheld
    // replay covers word boundaries, a repeated address and a rotation the
    // supplied traces do not pin down on their own. Also rejects each of the
    // individual mistakes separately: byte addresses used unshifted, broadcast
    // members computed independently, stages applied as an unordered mask, and
    // the lane rotation left at its first allowed value.
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
    //                      reads, so the selected stages do not commute and
    //                      cannot be collapsed into a set that is replayed in
    //                      an arbitrary order. In this particular fixture the
    //                      order the evidence requires happens to coincide with
    //                      the order the grammar lists those stages in, so this
    //                      case alone does not reject an implementation that
    //                      enumerates stage subsets in grammar order;
    //                      `compound_stage_chain_runs_against_grammar_order`
    //                      is the case that does;
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
    // REJECTS: dropping the word division, ignoring the broadcast leader,
    // answering with a rotation alone, fitting each trace independently or
    // taking the first legal mapping an enumeration reaches.
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
    // PUBLIC BEHAVIOUR: when no mapping the grammar can express reproduces the
    // evidence, recovery fails by throwing std::runtime_error. This grammar
    // allows no stages and only rotation 0, and bank 3 is unreachable for the
    // single address shown, so the evidence is unsatisfiable rather than merely
    // hard. Only that a std::runtime_error escaped is examined, never its text.
    //
    // REJECTS: an implementation that falls back to a best-effort or nearest
    // mapping instead of failing, one that returns a default-constructed mapping
    // when its search comes up empty, and one that reports the failure through a
    // return code or a different exception type.
    out.push_back(bank::Trace{{0}, {0}, {3}, -1});
    return true;
  }
  if (id == "compound_stage_chain_runs_against_grammar_order") {
    // PUBLIC BEHAVIOUR: `spec/mapping.md` states that selected XOR stages are
    // applied as an ordered sequence, each stage reading the bit values of the
    // word as it stands when that stage runs. It does not say the sequence
    // follows the order the grammar happens to list the stages in. This case is
    // an ordinary instance where those two orders differ: the grammar lists
    // {1,0} before {5,1}, while only the sequence that runs {5,1} first
    // reproduces the evidence, because {5,1} writes the bit {1,0} reads.
    //
    // Four further published rules are each independently decisive here, so the
    // case cannot be passed by getting the ordering right alone:
    //
    //   * word truncation  - replaying byte addresses without the word_width
    //                        division leaves the evidence unsatisfiable;
    //   * broadcast        - the first trace repeats one byte address on two
    //                        lanes whose raw banks differ, so the lowest-lane
    //                        leader rule changes what is observed;
    //   * nonzero rotation - no mapping at rotation 0 fits any of the traces;
    //   * global evidence  - each trace alone admits a best mapping of its own
    //                        and none of those three is the answer, so the
    //                        traces must be fitted jointly. The second trace
    //                        also withholds two individual lane banks, which
    //                        must be skipped rather than treated as bank 0.
    //
    // REJECTS: an implementation that treats the selected stages as a set and
    // enumerates subsets in grammar order rather than ordered sequences. That
    // is the natural shortcut, it passed every V1 fixture, and here it finds no
    // legal mapping at all. Also rejects ignoring the rotation, ignoring the
    // broadcast collapse, dropping the word division, and fitting one trace.
    out.push_back(bank::Trace{{1663, 1663, 151, 136, 2207, 1550},
                              {7, 4, 5, 5, 4, 0},
                              {4, 4, 0, 2, 0, 7},
                              2});
    out.push_back(bank::Trace{{87, 119, 818, 1597, 3296}, {3, 2, 4, 5, 4}, {2, 7, -1, -1, 6}, -1});
    out.push_back(bank::Trace{{782, 3502, 1738, 1789, 2259}, {4, 0, 2, 7, 3}, {}, 2});
    return true;
  }
  if (id == "equal_operation_count_prefers_fewer_stages") {
    // PUBLIC BEHAVIOUR: `spec/mapping.md` gives three selection tiers in order
    // - fewest operations, where a nonzero rotation counts as one operation on
    // top of the stages; then the fewest selected stages; then the smallest
    // canonical string. This case is an ordinary instance that separates the
    // second tier from the third: exactly two mappings satisfy the trace, one
    // using a nonzero rotation and no stage and one using rotation 0 and a
    // single stage, so both cost one operation and the tie is broken by stage
    // count, not by the canonical string.
    //
    // REJECTS: an implementation that keeps the operation count but drops the
    // stage-count tier and falls straight through to the canonical string. Its
    // answer is the rotation-0 mapping, because "rotate=0" sorts before every
    // nonzero rotation, so the omission is invisible until a pair like this one
    // exists. No V1 fixture contained such a pair. Also rejects an
    // implementation that ignores the rotation family entirely.
    out.push_back(bank::Trace{{1265, 455, 476, 235, 1706}, {1, 3, 3, 4, 5}, {3, 7, 4, 5, 4}, 2});
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
