//
// Verifier-owned observation harness.
//
// For one named fixture this calls the linked `fp8::restore` and records the
// raw functional values the V1 assertions examined -- output length, target
// stage, target role, target tensor name, restored histories, restored cursor,
// and for the compound case the values the frozen `fp8::cpu_step` derives from
// the restored third target. For the reject case it records whether the call
// threw and with what exception category and message.
//
// It asserts nothing and decides nothing. Every value below is written out
// verbatim for the judge to compare; no comparison, threshold or expectation
// appears in this file, and it is compiled into both the candidate probe and
// the deliberately-wrong regression probe without modification.
//
// Values are short by construction here -- the longest is a six-element
// history rendering -- so no digest or other length reduction is needed.
#pragma once
#include "cases.hpp"
#include "fp8/state.hpp"
#include "record.hpp"
#include <cstdint>
#include <exception>
#include <stdexcept>
#include <string>
#include <vector>

namespace vobs {

// Recorded in place of an element that does not exist, so the harness never
// indexes past the end of a candidate-produced vector the way the in-process
// V1 assertions did. It is inside the legal value alphabet, so the record still
// decodes, but it can never equal the rendering of an expected value.
inline const char *kAbsent = "absent";

inline std::string join(const std::vector<std::int64_t> &v) {
  std::string s;
  for (std::size_t i = 0; i < v.size(); ++i) {
    if (i) s += ',';
    s += std::to_string(v[i]);
  }
  return s;
}

inline std::string out_len(const std::vector<fp8::Restored> &o) {
  return std::to_string(o.size());
}

inline std::string history_at(const std::vector<fp8::Restored> &o, std::size_t i) {
  return i < o.size() ? join(o[i].history) : std::string(kAbsent);
}

inline std::string first_sample(const std::vector<fp8::Restored> &o, std::size_t i) {
  if (i >= o.size() || o[i].history.empty()) return std::string(kAbsent);
  return std::to_string(o[i].history.front());
}

inline std::string cursor_at(const std::vector<fp8::Restored> &o, std::size_t i) {
  return i < o.size() ? std::to_string(o[i].cursor) : std::string(kAbsent);
}

inline std::string stage_at(const std::vector<fp8::Restored> &o, std::size_t i) {
  return i < o.size() ? std::to_string(o[i].target.stage) : std::string(kAbsent);
}

inline std::string tensor_at(const std::vector<fp8::Restored> &o, std::size_t i) {
  return i < o.size() ? o[i].target.tensor : std::string(kAbsent);
}

inline std::string role_at(const std::vector<fp8::Restored> &o, std::size_t i) {
  if (i >= o.size()) return std::string(kAbsent);
  return o[i].target.role == fp8::Role::Forward ? "Forward" : "Backward";
}

// Produce the record text for one case. Never throws.
inline std::string observe(const std::string &id, const vcase::Fixture &f) {
  const std::vector<std::string> &keys = vcase::value_keys(id);
  std::vector<std::string> v;
  try {
    if (id == "topology_move") {
      const std::vector<fp8::Restored> o = fp8::restore(f.sources, f.targets);
      v.push_back(out_len(o));
      v.push_back(stage_at(o, 0));
      v.push_back(history_at(o, 0));
    } else if (id == "cursor_wrap") {
      const std::vector<fp8::Restored> o = fp8::restore(f.sources, f.targets);
      v.push_back(out_len(o));
      v.push_back(history_at(o, 0));
      v.push_back(cursor_at(o, 0));
    } else if (id == "history_resize") {
      // V1 called restore twice on the same sources, shrinking then expanding.
      const std::vector<fp8::Restored> a = fp8::restore(f.sources, f.targets);
      const std::vector<fp8::Restored> b = fp8::restore(f.sources, f.targets_b);
      v.push_back(out_len(a));
      v.push_back(history_at(a, 0));
      v.push_back(out_len(b));
      v.push_back(history_at(b, 0));
    } else if (id == "role_separation") {
      const std::vector<fp8::Restored> o = fp8::restore(f.sources, f.targets);
      v.push_back(out_len(o));
      v.push_back(role_at(o, 0));
      v.push_back(first_sample(o, 0));
      v.push_back(role_at(o, 1));
      v.push_back(first_sample(o, 1));
    } else if (id == "tied_tensors") {
      const std::vector<fp8::Restored> o = fp8::restore(f.sources, f.targets);
      v.push_back(out_len(o));
      v.push_back(history_at(o, 0));
      v.push_back(history_at(o, 1));
    } else if (id == "replicated_state") {
      const std::vector<fp8::Restored> o = fp8::restore(f.sources, f.targets);
      v.push_back(out_len(o));
      v.push_back(history_at(o, 0));
    } else if (id == "skipped_and_order") {
      const std::vector<fp8::Restored> o = fp8::restore(f.sources, f.targets);
      v.push_back(out_len(o));
      v.push_back(tensor_at(o, 0));
      v.push_back(tensor_at(o, 1));
    } else if (id == "replica_conflict") {
      // V1 required a std::runtime_error to escape. If the call returns, the
      // output length is recorded purely as diagnostics; the judge requires an
      // ERROR record here regardless of what it contains.
      const std::vector<fp8::Restored> o = fp8::restore(f.sources, f.targets);
      v.push_back(out_len(o));
    } else if (id == "compound_next_iteration") {
      const std::vector<fp8::Restored> o = fp8::restore(f.sources, f.targets);
      v.push_back(out_len(o));
      v.push_back(history_at(o, 0));
      v.push_back(history_at(o, 1));
      v.push_back(history_at(o, 2));
      if (o.size() > 2) {
        const fp8::Step st = fp8::cpu_step(o[2], vcase::kCompoundAmax, vcase::kCompoundValueQ);
        v.push_back(join(st.state.history));
        v.push_back(std::to_string(st.state.cursor));
        v.push_back(std::to_string(st.state.scale_q));
        v.push_back(std::to_string(st.state.inverse_scale_q));
        v.push_back(std::to_string(st.quantized));
      } else {
        for (int i = 0; i < 5; ++i) v.push_back(kAbsent);
      }
    } else {
      return rec::encode_error("unknown_case", id);
    }
  } catch (const std::runtime_error &e) {
    // The exception category V1's reject case caught explicitly.
    return rec::encode_error("runtime_error", e.what() ? e.what() : "");
  } catch (const std::exception &e) {
    return rec::encode_error("exception", e.what() ? e.what() : "");
  } catch (...) {
    return rec::encode_error("unknown", "");
  }
  // Tensor names come back from candidate-authored structures, so force every
  // field into the legal record alphabet before it is encoded.
  for (std::string &s : v)
    if (!rec::value_ok(s)) s = rec::sanitize(s);
  return rec::encode_values(keys, v);
}

}  // namespace vobs
