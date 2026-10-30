//
// Verifier-owned judge -- the sole grading authority.
//
// The judge never links candidate code and never shares a process with it. Its
// only input is the file of raw values a probe recorded; it holds the expected
// values itself -- the same quantities the V1 `REQUIRE(...)` calls compared
// against -- and reaches its verdict from its own comparisons. Candidate stdout,
// stderr and exit status are not read here at all, so nothing a candidate
// process emits can contribute to a pass.
//
// This expectation table stays inside the 0700 verifier tree and is never
// compiled into anything the graded account can execute or read.
#include "cases.hpp"
#include "record.hpp"
#include <cstddef>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <sys/stat.h>
#include <vector>

namespace {

// How one recorded field is judged.
//   Lit  -- must equal a value the verifier holds itself
//   Same -- must equal another field of the same record. This is where V1's
//           in-process self-consistency assertion (`recover(reversed).first ==
//           plan`) went: the probe records both plan identities and the judge,
//           not the candidate, decides whether they agree.
//   Free -- recorded for diagnostics or as the target of a Same rule; never a
//           basis for passing.
enum class RuleKind { Lit, Same, Free };

struct Rule {
  const char *key;
  RuleKind kind;
  std::string lit;    // Lit
  const char *other;  // Same
};

struct Spec {
  std::string id;
  rec::Kind expect;
  std::vector<Rule> rules;
};

Rule lit(const char *k, const std::string &v) { return Rule{k, RuleKind::Lit, v, nullptr}; }
Rule same(const char *k, const char *other) { return Rule{k, RuleKind::Same, std::string(), other}; }
Rule free_field(const char *k) { return Rule{k, RuleKind::Free, std::string(), nullptr}; }

// One entry per V1 case, asserting exactly the properties the V1 REQUIREs
// asserted -- no more and no fewer. The V1 source line each entry reproduces is
// quoted above it.
const std::vector<Spec> &specs() {
  static const std::vector<Spec> v = [] {
    std::vector<Spec> s;
    // REQUIRE(saturating_add(90,30,-100,100)==100); ==-100; ==50;
    s.push_back(Spec{"saturating_operator_boundaries",
                     rec::Kind::Values,
                     {lit("sat_high", "100"), lit("sat_low", "-100"), lit("sat_exact", "50")}});
    // REQUIRE(plan.has_value()); bytes(*plan).at("x-s")==8; completion==4;
    // accumulators=={{"X",16}}; sink_values=={40};
    s.push_back(Spec{"replay_combine_reduces_link_bytes",
                     rec::Kind::Values,
                     {lit("link_bytes_x_s", "8"), lit("completion", "4"),
                      lit("accumulators", "X:16"), lit("sink_values", "40")}});
    // REQUIRE(plan.has_value()); bytes(*plan).at("x-s")==16; completion==6;
    // sink_values=={90,-50};
    s.push_back(Spec{"replay_without_combine_serializes",
                     rec::Kind::Values,
                     {lit("link_bytes_x_s", "16"), lit("completion", "6"),
                      lit("sink_values", "90,-50")}});
    // REQUIRE(replay(p,paths(),{"X"}).has_value()); limit 15 -> REQUIRE(!...has_value());
    s.push_back(Spec{"accumulator_exact_boundary",
                     rec::Kind::Values,
                     {lit("at_capacity", "plan"), lit("under_capacity", "none")}});
    // REQUIRE(plan.has_value()); combine_nodes=={"X"}; sink_values=={40};
    s.push_back(Spec{"recover_from_link_and_completion_observations",
                     rec::Kind::Values,
                     {lit("combine_nodes", "X"), lit("sink_values", "40")}});
    // REQUIRE(!replay(p,{{"a-x","x-s"},{"b-x","x-t","t-s"}},{}).has_value());
    s.push_back(Spec{"reject_non_tree_route_selection",
                     rec::Kind::Values,
                     {lit("outcome", "none")}});
    // REQUIRE(plan.has_value()); parents=={{"A","a"}};
    // reverse(links); REQUIRE(recover(p).first==plan);
    s.push_back(Spec{"canonical_equal_route_tie",
                     rec::Kind::Values,
                     {lit("parents", "A:a"), free_field("plan_digest"),
                      same("reversed_plan_digest", "plan_digest")}});
    // REQUIRE(plan.has_value()); combine_nodes=={"Y"};
    s.push_back(Spec{"global_observation_beats_first_route",
                     rec::Kind::Values,
                     {lit("combine_nodes", "Y")}});
    // Equal completion and zero allocation force the next objective component:
    // exact total link bytes. A first-feasible/equal-time solver chooses A:a-leg.
    s.push_back(Spec{"equal_time_minimizes_total_link_bytes",
                     rec::Kind::Values,
                     {lit("parents", "A:z-direct"), lit("completion", "2"),
                      lit("total_link_bytes", "4")}});
    // REQUIRE(plan.has_value()); completion==6;
    s.push_back(Spec{"exact_ready_time_scheduling",
                     rec::Kind::Values,
                     {lit("completion", "6")}});
    // REQUIRE(plan.has_value()); sink_values=={0};
    s.push_back(Spec{"saturating_combine_operation_order",
                     rec::Kind::Values,
                     {lit("sink_values", "0")}});
    // REQUIRE(plan.has_value()); combine_nodes=={"X"}; bytes(*plan).at("x-s")==8;
    // accumulators.front().second==16;
    s.push_back(Spec{"compound_capacity_symmetry_route_and_replay",
                     rec::Kind::Values,
                     {lit("combine_nodes", "X"), lit("link_bytes_x_s", "8"),
                      lit("accumulator_front", "16")}});
    return s;
  }();
  return v;
}

// Keys a record carries, given its kind.
const std::vector<std::string> &record_keys(const Spec &s) {
  return s.expect == rec::Kind::Error ? rec::error_keys() : vcase::value_keys(s.id);
}

bool read_file(const std::string &path, std::string &out, std::string &err) {
  struct stat st {};
  if (::lstat(path.c_str(), &st) != 0) {
    err = "no result recorded";
    return false;
  }
  if (S_ISLNK(st.st_mode)) {
    err = "result path is a symlink";
    return false;
  }
  if (!S_ISREG(st.st_mode)) {
    err = "result path is not a regular file";
    return false;
  }
  if (static_cast<std::size_t>(st.st_size) > rec::kMaxRecordBytes) {
    err = "result is implausibly large";
    return false;
  }
  std::FILE *f = std::fopen(path.c_str(), "rb");
  if (!f) {
    err = "result is not readable";
    return false;
  }
  out.clear();
  char buf[1024];
  std::size_t n = 0;
  while ((n = std::fread(buf, 1, sizeof buf, f)) > 0) out.append(buf, n);
  std::fclose(f);
  return true;
}

// Grade one case entirely from the recorded values and the judge's own
// expectations. Returns true when the case passes.
bool grade_case(const std::string &dir, const Spec &s, std::string &out) {
  std::string raw;
  std::string err;
  if (!read_file(dir + "/" + s.id, raw, err)) {
    out += "FAIL " + s.id + " " + err + "\n";
    return false;
  }
  rec::Record r;
  if (!rec::decode(raw, vcase::value_keys(s.id), r, err)) {
    out += "FAIL " + s.id + " malformed record: " + err + "\n";
    return false;
  }
  if (r.kind != s.expect) {
    out += "FAIL " + s.id +
           (s.expect == rec::Kind::Error
                ? " expected a rejection, the call returned a result\n"
                : " the call threw where a result was required: " + r.values[1] + "\n");
    return false;
  }
  const std::vector<std::string> &keys = record_keys(s);
  if (keys.size() != r.values.size() || keys.size() != s.rules.size()) {
    out += "FAIL " + s.id + " record field count does not match the case schema\n";
    return false;
  }
  for (std::size_t i = 0; i < s.rules.size(); ++i) {
    const Rule &rule = s.rules[i];
    if (keys[i] != rule.key) {
      out += "FAIL " + s.id + " expectation table is out of step with the record schema\n";
      return false;
    }
    if (rule.kind == RuleKind::Lit) {
      if (r.values[i] != rule.lit) {
        out += "FAIL " + s.id + " " + rule.key + " recorded " + r.values[i] + ", expected " +
               rule.lit + "\n";
        return false;
      }
    } else if (rule.kind == RuleKind::Same) {
      std::size_t j = keys.size();
      for (std::size_t k = 0; k < keys.size(); ++k)
        if (keys[k] == rule.other) j = k;
      if (j == keys.size()) {
        out += "FAIL " + s.id + " expectation table references an unknown field\n";
        return false;
      }
      if (r.values[i] != r.values[j]) {
        out += "FAIL " + s.id + " " + rule.key + " differs from " + rule.other + "\n";
        return false;
      }
    }
  }
  out += "PASS " + s.id + "\n";
  return true;
}

std::size_t grade_all(const std::string &dir, std::string &out) {
  std::size_t bad = 0;
  for (const Spec &s : specs())
    if (!grade_case(dir, s, out)) ++bad;
  out += "TOTAL " + std::to_string(specs().size()) + " FAILED " + std::to_string(bad) + "\n";
  return bad;
}

// The value staged for a field the judge does not compare literally. It is
// arbitrary on purpose: a Free field can never make a case pass, and a Same
// field only has to agree with its referent.
const char *kStagedFree = "0000000000000000000000000000000000000000000000000000000000000000";

std::vector<std::string> staged_values(const Spec &s) {
  const std::vector<std::string> &keys = record_keys(s);
  std::vector<std::string> v;
  for (const Rule &rule : s.rules) {
    if (rule.kind == RuleKind::Lit) {
      v.push_back(rule.lit);
    } else if (rule.kind == RuleKind::Same) {
      std::size_t j = keys.size();
      for (std::size_t k = 0; k < keys.size(); ++k)
        if (keys[k] == rule.other) j = k;
      v.push_back(j < v.size() ? v[j] : std::string(kStagedFree));
    } else {
      v.push_back(kStagedFree);
    }
  }
  return v;
}

bool write_file(const std::string &path, const std::string &text) {
  std::FILE *f = std::fopen(path.c_str(), "wb");
  if (!f) return false;
  const bool ok = std::fwrite(text.data(), 1, text.size(), f) == text.size();
  return std::fclose(f) == 0 && ok;
}

// Required negative control, determined entirely inside the verifier.
//
// The judge fabricates a full set of correct records itself, perturbs exactly
// one functional value in one case, and then grades what it staged. No candidate
// code runs, no candidate output is read and no marker is exchanged. Success
// means the injected defect was observed: exactly one case failed, it was the
// perturbed case, and it failed on the perturbed field. Anything else means the
// pipeline cannot see failure and must not be trusted to award reward.
int selftest(const std::string &dir) {
  const std::string target = "compound_capacity_symmetry_route_and_replay";
  const std::string field = "accumulator_front";
  if (::mkdir(dir.c_str(), 0700) != 0) {
    struct stat st {};
    if (::stat(dir.c_str(), &st) != 0 || !S_ISDIR(st.st_mode)) {
      std::printf("control: cannot stage results\n");
      return 2;
    }
  }
  for (const Spec &s : specs()) {
    const std::vector<std::string> &keys = record_keys(s);
    std::vector<std::string> v = staged_values(s);
    if (s.id == target) {
      // Perturb exactly one functional value: the accumulator allocation the
      // compound case reports, moved one unit off its expected size.
      bool touched = false;
      for (std::size_t i = 0; i < keys.size(); ++i) {
        if (keys[i] == field) {
          v[i] = std::to_string(std::stoll(v[i]) + 1);
          touched = true;
        }
      }
      if (!touched) {
        std::printf("control: nothing to perturb\n");
        return 2;
      }
    }
    if (!write_file(dir + "/" + s.id, rec::encode_values(keys, v))) {
      std::printf("control: cannot write staged result\n");
      return 2;
    }
  }
  std::string out;
  const std::size_t bad = grade_all(dir, out);
  std::fputs(out.c_str(), stdout);
  const bool saw_target =
      out.find("FAIL " + target + " " + field + " recorded") != std::string::npos;
  if (bad == 1 && saw_target) {
    std::printf("CONTROL ok: injected defect observed\n");
    return 0;
  }
  std::printf("CONTROL compromised: injected defect not observed cleanly\n");
  return 1;
}

}  // namespace

int main(int argc, char **argv) {
  if (argc == 3 && std::string(argv[1]) == "--selftest") return selftest(argv[2]);
  if (argc != 4 || std::string(argv[1]) != "--grade") {
    std::fprintf(stderr,
                 "usage: judge --grade <results-dir> <verdict-file> | judge --selftest <dir>\n");
    return 2;
  }
  std::string out;
  const std::size_t bad = grade_all(argv[2], out);
  std::fputs(out.c_str(), stdout);
  if (!write_file(argv[3], out)) return 2;
  return bad == 0 ? 0 : 1;
}
