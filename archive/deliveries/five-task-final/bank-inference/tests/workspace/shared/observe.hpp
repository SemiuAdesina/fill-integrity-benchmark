//
// Verifier-owned observation harness.
//
// For one named fixture this calls the linked `bank::recover` and records the
// raw functional values the V1 assertions examined: the canonical string of the
// recovered mapping, and for `compound_replay` the banks and conflict degree
// that the frozen interpreter produces when the recovered mapping is replayed
// over a withheld trace. If the call throws, the exception category and its
// sanitised text are recorded instead.
//
// It asserts nothing and decides nothing. Every value below is written out
// verbatim for the judge to compare; no comparison, threshold or expectation
// appears in this file, and it is compiled into both the candidate probe and
// the deliberately-wrong regression probe without modification.
//
// Values are small by nature here -- a canonical string is a few dozen
// characters and a bank vector has at most eight entries -- so no digest or
// other length reduction is needed anywhere in this task's records.
#pragma once
#include "bank/model.hpp"
#include "cases.hpp"
#include "record.hpp"
#include <exception>
#include <stdexcept>
#include <string>
#include <vector>

namespace vobs {

inline std::string join_u(const std::vector<unsigned> &v) {
  std::string s;
  for (std::size_t i = 0; i < v.size(); ++i) {
    if (i) s += ',';
    s += std::to_string(v[i]);
  }
  return s;
}

// Produce the record text for one case. Never throws.
inline std::string observe(const std::string &id) {
  const std::vector<std::string> &keys = vcase::value_keys(id);
  bank::Grammar g;
  std::vector<bank::Trace> ts;
  if (!vcase::grammar(id, g) || !vcase::traces(id, ts) || keys.empty()) {
    return rec::encode_error("unknown_case", id);
  }
  std::vector<std::string> v;
  try {
    // The single call under observation. For `no_solution` V1 required this to
    // throw std::runtime_error; if it returns instead, the canonical string is
    // recorded purely as a diagnostic and the judge still requires an ERROR
    // record for that case.
    const bank::Mapping got = bank::recover(g, ts);
    v.push_back(bank::canonical(got));
    if (id == "compound_replay") {
      // Replay a trace the fixture never revealed, through the frozen
      // interpreter, using whatever mapping the candidate returned. The banks
      // and conflict degree this produces are raw functional values; what they
      // ought to be is known only to the judge.
      const bank::Replay secret = bank::replay(g, got, vcase::compound_secret_trace());
      v.push_back(join_u(secret.banks));
      v.push_back(std::to_string(secret.conflict_degree));
    }
    if (id == "compound_global_tie") {
      // Same withheld-replay observation for the compound case, using that
      // case's own unrevealed trace.
      const bank::Replay secret = bank::replay(g, got, vcase::compound_tie_secret_trace());
      v.push_back(join_u(secret.banks));
      v.push_back(std::to_string(secret.conflict_degree));
    }
  } catch (const std::runtime_error &e) {
    // The exception category V1's no_solution case caught explicitly.
    return rec::encode_error("runtime_error", e.what() ? e.what() : "");
  } catch (const std::exception &e) {
    return rec::encode_error("exception", e.what() ? e.what() : "");
  } catch (...) {
    return rec::encode_error("unknown", "");
  }
  for (std::string &s : v)
    if (!rec::value_ok(s)) s = rec::sanitize(s);
  return rec::encode_values(keys, v);
}

}  // namespace vobs
