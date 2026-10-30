//
// Verifier-owned judge -- the sole grading authority.
//
// The judge never links candidate code and never shares a process with it. It
// reads only the raw functional values the probe recorded, holds the expected
// values itself (the same literals the V1 assertions compared against), and
// decides correctness from its own comparisons. Candidate stdout, stderr and
// exit status are not read here at all, so nothing the candidate process emits
// can contribute to a pass.
//
// The expectation table below stays inside the 0700 verifier tree and is never
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
//   Same -- must equal another field of the same record (a self-consistency
//           assertion V1 made in-process, now decided by the judge)
//   Free -- recorded for diagnostics or as the target of a Same rule; never a
//           basis for passing
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
Rule same(const char *k, const char *other) {
  return Rule{k, RuleKind::Same, std::string(), other};
}
Rule free_field(const char *k) { return Rule{k, RuleKind::Free, std::string(), nullptr}; }

// The expected values, one entry per V1 case, asserting exactly the properties
// the V1 `req(...)` calls asserted -- no more and no fewer. The V1 assertion
// each rule set came from is quoted above it.
const std::vector<Spec> &specs() {
  static const std::vector<Spec> v = [] {
    std::vector<Spec> s;

    // req(o.size()==1 && o[0].target.stage==4 && o[0].history=={1,2})
    s.push_back(Spec{"topology_move",
                     rec::Kind::Values,
                     {lit("out_len", "1"), lit("stage_0", "4"), lit("history_0", "1,2")}});

    // req(o[0].history=={10,20,30,40} && o[0].cursor==0)
    s.push_back(Spec{"cursor_wrap",
                     rec::Kind::Values,
                     {free_field("out_len"), lit("history_0", "10,20,30,40"),
                      lit("cursor_0", "0")}});

    // req(a[0].history=={30,40}); req(b[0].history=={0,0,10,20,30,40})
    s.push_back(Spec{"history_resize",
                     rec::Kind::Values,
                     {free_field("a_len"), lit("a_history_0", "30,40"), free_field("b_len"),
                      lit("b_history_0", "0,0,10,20,30,40")}});

    // req(o[0].target.role==Forward && o[0].history[0]==1);
    // req(o[1].target.role==Backward && o[1].history[0]==9)
    s.push_back(Spec{"role_separation",
                     rec::Kind::Values,
                     {free_field("out_len"), lit("role_0", "Forward"),
                      lit("history_0_first", "1"), lit("role_1", "Backward"),
                      lit("history_1_first", "9")}});

    // req(o.size()==2 && o[0].history==o[1].history)
    s.push_back(Spec{"tied_tensors",
                     rec::Kind::Values,
                     {lit("out_len", "2"), free_field("history_0"),
                      same("history_1", "history_0")}});

    // req(o[0].history=={8,16,32,64})
    s.push_back(Spec{"replicated_state",
                     rec::Kind::Values,
                     {free_field("out_len"), lit("history_0", "8,16,32,64")}});

    // req(o.size()==2 && o[0].target.tensor=="a" && o[1].target.tensor=="z")
    s.push_back(Spec{"skipped_and_order",
                     rec::Kind::Values,
                     {lit("out_len", "2"), lit("tensor_0", "a"), lit("tensor_1", "z")}});

    // bool threw=false; try{restore(...);}catch(const std::runtime_error&){threw=true;} req(threw);
    // V1 examined only that a std::runtime_error escaped, never its text.
    s.push_back(Spec{"replica_conflict",
                     rec::Kind::Error,
                     {lit("err_kind", "runtime_error"), free_field("err_msg")}});

    // req(o.size()==3); req(o[0].history=={0,8,16,32,64}); req(o[1].history=={3,5,7});
    // req(o[2].history=={16,32,64}); auto step=cpu_step(o[2],5,300);
    // req(step.state.history=={5,32,64});
    // req(step.state.cursor==1 && step.state.scale_q==16
    //     && step.state.inverse_scale_q==64 && step.quantized==4)
    s.push_back(Spec{"compound_next_iteration",
                     rec::Kind::Values,
                     {lit("out_len", "3"), lit("history_0", "0,8,16,32,64"),
                      lit("history_1", "3,5,7"), lit("history_2", "16,32,64"),
                      lit("step_history", "5,32,64"), lit("step_cursor", "1"),
                      lit("step_scale_q", "16"), lit("step_inverse_scale_q", "64"),
                      lit("step_quantized", "4")}});
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
// JUDGE-CONT-1

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
                : " the call threw where a result was required\n");
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
  if (s.expect == rec::Kind::Error) {
    out += "PASS " + s.id + " (rejected: " + r.values[1] + ")\n";
  } else {
    out += "PASS " + s.id + "\n";
  }
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
const char *kStagedFree = "free";

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

std::string render(const Spec &s, const std::vector<std::string> &v) {
  const std::vector<std::string> &keys = record_keys(s);
  if (s.expect == rec::Kind::Error) return "ERROR\nerr_kind=" + v[0] + "\nerr_msg=" + v[1] + "\n";
  return rec::encode_values(keys, v);
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
// one functional value in one case, and then grades what it staged. No
// candidate code runs, no candidate output is read and no marker is exchanged.
// Success means the injected defect was observed: exactly one case failed, it
// was the perturbed case, and it failed on the perturbed field. Anything else
// means the pipeline cannot see failure and must not be trusted to award
// reward.
int selftest(const std::string &dir) {
  const std::string target = "compound_next_iteration";
  const std::string field = "step_history";
  // The injected value is structurally illegal rather than merely different:
  // a restored history entry is a carried-over sample or a zero pad, all of
  // which are non-negative in every fixture, and this one is negative and of
  // the wrong length, so it cannot coincide with a correct answer.
  const std::string defect = "-1";
  if (::mkdir(dir.c_str(), 0700) != 0) {
    struct stat st {};
    if (::stat(dir.c_str(), &st) != 0 || !S_ISDIR(st.st_mode)) {
      std::printf("control: cannot stage results\n");
      return 2;
    }
  }
  for (const Spec &s : specs()) {
    std::vector<std::string> v = staged_values(s);
    if (s.id == target) {
      const std::vector<std::string> &keys = record_keys(s);
      bool touched = false;
      for (std::size_t i = 0; i < keys.size(); ++i) {
        if (keys[i] == field) {
          v[i] = defect;
          touched = true;
        }
      }
      if (!touched) {
        std::printf("control: nothing to perturb\n");
        return 2;
      }
    }
    if (!write_file(dir + "/" + s.id, render(s, v))) {
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
