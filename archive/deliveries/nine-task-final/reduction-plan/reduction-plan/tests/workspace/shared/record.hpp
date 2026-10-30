//
// Raw functional-value record codec.
//
// A record can express two things and nothing else: the raw values the
// candidate's `benchmark-suite_fabric` entry points produced for one verifier-owned
// fixture, or the fact that the call yielded no plan / threw, together with the
// category and text of that outcome. There is no status field, no pass/fail
// flag, no marker, no nonce, no case count and no summary, so a record offers
// the candidate no slot in which to assert that it passed. The judge alone
// decides whether the recorded values are the right ones.
//
// Decoding is strict and driven by the verifier's own per-case key list: a
// record must carry exactly the expected keys, in exactly the expected order,
// with bounded values drawn from a restricted character set. Any deviation is a
// decode error, and a decode error is a failure.
#pragma once
#include <cstddef>
#include <string>
#include <vector>

namespace rec {

// Whole-record and per-value ceilings. Nothing legitimate approaches these.
inline constexpr std::size_t kMaxRecordBytes = 8192;
inline constexpr std::size_t kMaxValueBytes = 256;
inline constexpr std::size_t kMaxMessageBytes = 200;

enum class Kind { Values, Error };

struct Record {
  Kind kind = Kind::Values;
  // Parallel to the verifier's key list for Values records; for Error records
  // the fixed pair (err_kind, err_msg).
  std::vector<std::string> values;
};

// Printable ASCII minus the field separator, so a value can never introduce a
// second field or a second line.
inline bool value_ok(const std::string &s) {
  if (s.size() > kMaxValueBytes) return false;
  for (char c : s) {
    const unsigned char u = static_cast<unsigned char>(c);
    if (u < 0x20u || u > 0x7eu || c == '=') return false;
  }
  return true;
}

// Force arbitrary text into the legal value alphabet. Applied by the probe to
// anything a candidate authored -- exception messages above all -- so a hostile
// or merely unusual `what()` cannot corrupt the record with a newline or an `=`.
inline std::string sanitize(const std::string &s) {
  std::string out;
  for (char c : s) {
    if (out.size() >= kMaxMessageBytes) break;
    const unsigned char u = static_cast<unsigned char>(c);
    out.push_back((u < 0x20u || u > 0x7eu || c == '=') ? '.' : c);
  }
  return out;
}

inline std::string encode_values(const std::vector<std::string> &keys,
                                 const std::vector<std::string> &values) {
  std::string out = "VALUES\n";
  const std::size_t n = keys.size() < values.size() ? keys.size() : values.size();
  for (std::size_t i = 0; i < n; ++i) {
    out += keys[i];
    out += '=';
    out += values[i];
    out += '\n';
  }
  return out;
}

inline std::string encode_error(const std::string &kind, const std::string &msg) {
  return "ERROR\nerr_kind=" + sanitize(kind) + "\nerr_msg=" + sanitize(msg) + "\n";
}

inline const std::vector<std::string> &error_keys() {
  static const std::vector<std::string> k{"err_kind", "err_msg"};
  return k;
}

namespace detail {
inline std::vector<std::string> split_lines(const std::string &s, bool &well_formed) {
  well_formed = s.empty() || s.back() == '\n';
  std::vector<std::string> out;
  std::string cur;
  for (char c : s) {
    if (c == '\n') {
      out.push_back(cur);
      cur.clear();
    } else {
      cur.push_back(c);
    }
  }
  if (!cur.empty()) {
    out.push_back(cur);
    well_formed = false;
  }
  return out;
}
}  // namespace detail

// Strictly decode `raw` against the key list the case requires. `err` receives a
// human-readable reason on failure.
inline bool decode(const std::string &raw, const std::vector<std::string> &value_keys, Record &out,
                   std::string &err) {
  if (raw.size() > kMaxRecordBytes) {
    err = "record exceeds the permitted size";
    return false;
  }
  bool well_formed = false;
  const std::vector<std::string> lines = detail::split_lines(raw, well_formed);
  if (!well_formed) {
    err = "record is not newline terminated";
    return false;
  }
  if (lines.empty()) {
    err = "record is empty";
    return false;
  }
  const std::vector<std::string> *keys = nullptr;
  if (lines[0] == "VALUES") {
    out.kind = Kind::Values;
    keys = &value_keys;
  } else if (lines[0] == "ERROR") {
    out.kind = Kind::Error;
    keys = &error_keys();
  } else {
    err = "record must begin with VALUES or ERROR";
    return false;
  }
  if (lines.size() != keys->size() + 1u) {
    err = "record has " + std::to_string(lines.size() - 1u) + " fields, expected " +
          std::to_string(keys->size());
    return false;
  }
  out.values.clear();
  out.values.reserve(keys->size());
  for (std::size_t i = 0; i < keys->size(); ++i) {
    const std::string &line = lines[i + 1u];
    const std::string prefix = (*keys)[i] + "=";
    if (line.size() < prefix.size() || line.compare(0, prefix.size(), prefix) != 0) {
      err = "expected field " + (*keys)[i] + " at position " + std::to_string(i + 1u);
      return false;
    }
    std::string v = line.substr(prefix.size());
    if (!value_ok(v)) {
      err = "field " + (*keys)[i] + " has an illegal or oversized value";
      return false;
    }
    out.values.push_back(std::move(v));
  }
  return true;
}

}  // namespace rec
