//
// Verifier-owned judge -- the sole grading authority.
//
// The judge never links candidate code and never shares a process with it. It
// reads only the raw functional values the probe recorded, holds the expected
// values itself (the same quantities the V1 assertions compared against, plus
// the ground-truth mappings V1 planted), and decides correctness from its own
// comparisons. Candidate stdout, stderr and exit status are not read here at
// all, so nothing the candidate process emits can contribute to a pass.
//
// The ground truth and the expectation table below stay inside the 0700
// verifier tree and are never compiled into anything the graded account can
// execute or read.
#include "bank/model.hpp"
#include "cases.hpp"
#include "record.hpp"
#include <cstdio>
#include <cstdlib>
#include <string>
#include <sys/stat.h>
#include <vector>

namespace {

// How one recorded field is judged.
//   Lit  -- must equal a value the verifier holds or computes itself
//   Free -- recorded for diagnostics only; never a basis for passing
//
// V1 made no self-consistency assertion in this task (every `require` compared
// against a verifier-held quantity), so no cross-field rule is needed.
enum class RuleKind { Lit, Free };

struct Rule {
  const char *key;
  RuleKind kind;
  std::string lit;  // Lit only
};

struct Spec {
  std::string id;
  rec::Kind expect;
  std::vector<Rule> rules;
};

Rule lit(const char *k, const std::string &v) { return Rule{k, RuleKind::Lit, v}; }
Rule free_field(const char *k) { return Rule{k, RuleKind::Free, std::string()}; }

[[noreturn]] void fatal(const std::string &why) {
  std::fprintf(stderr, "judge: %s\n", why.c_str());
  std::exit(2);
}

std::string join_u(const std::vector<unsigned> &v) {
  std::string s;
  for (std::size_t i = 0; i < v.size(); ++i) {
    if (i) s += ',';
    s += std::to_string(v[i]);
  }
  return s;
}

// The ground-truth mapping V1 planted for a case, before it derived that case's
// observations from it. This is the expectation in its most compact form: every
// literal below is recomputed from it through the frozen interpreter.
bank::Mapping truth(const std::string &id) {
  if (id == "basic_identity") return bank::Mapping{{}, 0};
  if (id == "word_boundaries") return bank::Mapping{{{2, 0}}, 0};
  if (id == "broadcast_groups") return bank::Mapping{{{4, 1}}, 3};
  if (id == "conflict_disambiguates") return bank::Mapping{{}, 1};
  if (id == "ordered_xor") return bank::Mapping{{{3, 4}, {4, 0}}, 0};
  if (id == "global_evidence") return bank::Mapping{{{3, 0}, {4, 1}}, 1};
  if (id == "minimum_canonical") return bank::Mapping{{}, 0};
  if (id == "compound_replay") return bank::Mapping{{{4, 5}, {5, 1}, {6, 2}}, 7};
  if (id == "compound_global_tie") return bank::Mapping{{{4, 1}, {1, 0}, {5, 3}}, 11};
  // Added after a differential study found two published rules that no V1
  // fixture depended on. The first plants a stage sequence whose required order
  // is the reverse of the grammar's declaration order; the second plants a
  // rotation-only mapping that ties on operation count with a one-stage mapping
  // at rotation 0. Both are ordinary instances of the published contract.
  if (id == "compound_stage_chain_runs_against_grammar_order")
    return bank::Mapping{{{5, 1}, {1, 0}}, 6};
  if (id == "equal_operation_count_prefers_fewer_stages") return bank::Mapping{{}, 4};
  fatal("no ground truth for case " + id);
}

bank::Grammar grammar_of(const std::string &id) {
  bank::Grammar g;
  if (!vcase::grammar(id, g)) fatal("unknown fixture " + id);
  return g;
}

std::vector<bank::Trace> traces_of(const std::string &id) {
  std::vector<bank::Trace> ts;
  if (!vcase::traces(id, ts)) fatal("unknown fixture " + id);
  return ts;
}

// Integrity check on the shared fixture header.
//
// V1 built each trace by replaying the planted mapping through the frozen
// interpreter and copying the resulting banks and/or conflict degree into the
// trace. Those already-materialised traces now live in `shared/cases.hpp`, which
// the graded account can execute. The judge re-derives them here from its own
// ground truth and refuses to grade if any revealed observation disagrees, so a
// fixture that no longer describes the planted mapping stops the run instead of
// quietly changing what is being asked.
void check_fixture_consistency(const std::string &id) {
  const bank::Grammar g = grammar_of(id);
  const bank::Mapping t = truth(id);
  for (const bank::Trace &tr : traces_of(id)) {
    const bank::Replay r = bank::replay(g, t, tr);
    if (!tr.observed_banks.empty()) {
      if (tr.observed_banks.size() != r.banks.size())
        fatal("fixture " + id + " reveals a bank vector of the wrong length");
      for (std::size_t i = 0; i < r.banks.size(); ++i)
        if (tr.observed_banks[i] >= 0 &&
            r.banks[i] != static_cast<unsigned>(tr.observed_banks[i]))
          fatal("fixture " + id + " reveals a bank the planted mapping does not produce");
    }
    if (tr.observed_conflict >= 0 &&
        r.conflict_degree != static_cast<unsigned>(tr.observed_conflict))
      fatal("fixture " + id + " reveals a conflict degree the planted mapping does not produce");
  }
}

// The expected values, one entry per V1 case, asserting exactly the properties
// the V1 `require(...)` calls asserted -- no more and no fewer.
const std::vector<Spec> &specs() {
  static const std::vector<Spec> v = [] {
    std::vector<Spec> s;
    for (const std::string &id : vcase::ids()) {
      if (id == "no_solution") {
        // V1: bool threw=false; try{ recover(g,{t}); }
        //     catch(const std::runtime_error&){ threw=true; } require(threw);
        // Only that a std::runtime_error escaped was examined, never its text.
        s.push_back(Spec{id, rec::Kind::Error,
                         {lit("err_kind", "runtime_error"), free_field("err_msg")}});
        continue;
      }
      check_fixture_consistency(id);
      // V1: require(canonical(recover(g, traces)) == canonical(truth));
      std::vector<Rule> rules{lit("canonical", bank::canonical(truth(id)))};
      if (id == "compound_replay") {
        // V1 additionally replayed a withheld trace through the recovered
        // mapping and required the literals {13,7,11,11,14} and 1. Both are
        // recomputed here from the planted mapping through the frozen
        // interpreter, and the V1 literals are pinned below so a silent drift
        // in either the fixture or the interpreter stops the run.
        const bank::Replay secret =
            bank::replay(grammar_of(id), truth(id), vcase::compound_secret_trace());
        const std::string banks = join_u(secret.banks);
        const std::string conflict = std::to_string(secret.conflict_degree);
        if (banks != "13,7,11,11,14" || conflict != "1")
          fatal("withheld replay no longer matches the values V1 asserted");
        rules.push_back(lit("secret_banks", banks));
        rules.push_back(lit("secret_conflict", conflict));
      }
      if (id == "compound_global_tie") {
        // The compound case also replays a withheld trace through whatever
        // mapping was recovered. The expected banks and conflict degree are
        // recomputed here from the planted mapping through the frozen
        // interpreter, so this file holds no hand-copied literal that could
        // drift away from the model.
        const bank::Replay secret =
            bank::replay(grammar_of(id), truth(id), vcase::compound_tie_secret_trace());
        rules.push_back(lit("secret_banks", join_u(secret.banks)));
        rules.push_back(lit("secret_conflict", std::to_string(secret.conflict_degree)));
      }
      s.push_back(Spec{id, rec::Kind::Values, rules});
    }
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
           (s.expect == rec::Kind::Error ? " expected a rejection, the call returned a mapping\n"
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
    if (rule.kind == RuleKind::Lit && r.values[i] != rule.lit) {
      out += "FAIL " + s.id + " " + rule.key + " recorded " + r.values[i] + ", expected " +
             rule.lit + "\n";
      return false;
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
// arbitrary on purpose: a Free field can never make a case pass.
const char *kStagedFree = "staged-diagnostic-only";

std::vector<std::string> staged_values(const Spec &s) {
  std::vector<std::string> v;
  for (const Rule &rule : s.rules)
    v.push_back(rule.kind == RuleKind::Lit ? rule.lit : std::string(kStagedFree));
  return v;
}

std::string encode_spec(const Spec &s, const std::vector<std::string> &v) {
  if (s.expect == rec::Kind::Error) return "ERROR\nerr_kind=" + v[0] + "\nerr_msg=" + v[1] + "\n";
  return rec::encode_values(record_keys(s), v);
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
  const std::string target = "compound_replay";
  const std::string field = "secret_conflict";
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
      // Perturb exactly one functional value: the conflict degree of the
      // withheld replay, moved one step off its expected value.
      const std::vector<std::string> &keys = record_keys(s);
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
    if (!write_file(dir + "/" + s.id, encode_spec(s, v))) {
      std::printf("control: cannot write staged result\n");
      return 2;
    }
  }
  std::string out;
  const std::size_t bad = grade_all(dir, out);
  std::fputs(out.c_str(), stdout);
  const bool saw_target = out.find("FAIL " + target + " " + field + " recorded") != std::string::npos;
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
