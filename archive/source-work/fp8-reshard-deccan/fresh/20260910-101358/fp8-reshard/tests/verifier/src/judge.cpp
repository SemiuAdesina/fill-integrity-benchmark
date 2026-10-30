// Verifier-private judge: `judge --run <check> --fixtures <dir> --artifacts <dir>`.
//
// Loads every fixture (checkpoint, layout, certified answer and hidden replay
// sequences) from the private fixture directory and the artifacts the probe
// phase captured, then runs one named check. The judge links no candidate
// code: every artifact is parsed with the verifier's own strict reader and
// every replay uses the verifier's own copy of the frozen model. Before
// grading it re-derives every certified trace from the certified states and
// refuses to grade if they disagree. It prints `##FP8-PASS <check>` and exits
// 0 only when the check holds on every fixture it covers.
#include <cstdio>
#include <fstream>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#include "fp8/json.hpp"
#include "fp8/model.hpp"
#include "plan.hpp"

namespace {

const char* const kPassMarker = "##FP8-PASS";

#define CHECK(name) #name
const char* const kChecks[] = {
    CHECK(result_documents_well_formed),
    CHECK(accept_layouts_restored_exactly),
    CHECK(inconsistent_layouts_rejected),
    CHECK(next_state_trace_replay),
    CHECK(topology_move_ragged_shards),
    CHECK(tied_tensors_and_replicas),
    CHECK(role_separation_and_resizing),
    CHECK(suffix_rule_older_samples),
    CHECK(coverage_and_inactive_targets),
    CHECK(global_record_validation),
    CHECK(target_validation),
    CHECK(block_boundaries_and_ordering),
    CHECK(deterministic_output),
};
const char* const kControl = "negative_control_must_fail";

struct Replay {
    std::size_t state_index;
    std::vector<std::pair<std::int64_t, std::int64_t>> pairs;
    std::vector<fp8::StepTrace> trace;
};

struct Fixture {
    std::string id;
    bool accept{true};
    std::vector<fp8::State> states;   // certified (accept fixtures)
    std::vector<Replay> replays;
};

struct Artifact {
    std::string job, fixture;
    bool present{false};
    std::string text;
    bool parsed{false};
    std::string error;
    fp8::Result result;
};

std::string slurp(const std::string& path, bool& ok) {
    std::ifstream in(path, std::ios::binary);
    ok = static_cast<bool>(in);
    std::ostringstream ss;
    ss << in.rdbuf();
    return ss.str();
}

std::int64_t need_int(const fp8::json::Value& v, const char* key) {
    const fp8::json::Value* m = v.get(key);
    if (!m || !m->is_int()) throw std::runtime_error(std::string("expected integer member ") + key);
    return m->i;
}

fp8::State state_of(const fp8::json::Value& v) {
    fp8::State s;
    const fp8::json::Value* t = v.get("tensor");
    if (!t || !t->is_str()) throw std::runtime_error("state tensor");
    s.tensor = t->s;
    const fp8::json::Value* r = v.get("role");
    if (!r || !r->is_str()) throw std::runtime_error("state role");
    s.role = r->s == "forward" ? fp8::Role::Forward : fp8::Role::Backward;
    s.stage = need_int(v, "stage");
    s.shard = need_int(v, "shard");
    s.global_block = need_int(v, "global_block");
    s.scale = need_int(v, "scale");
    s.inverse_scale = need_int(v, "inverse_scale");
    s.cursor = need_int(v, "cursor");
    const fp8::json::Value* h = v.get("amax_history");
    if (!h || !h->is_array()) throw std::runtime_error("state history");
    for (const auto& x : h->a) s.amax_history.push_back(x.i);
    return s;
}

Fixture load_fixture(const std::string& dir, const std::string& id) {
    bool ok = false;
    std::string text = slurp(dir + "/" + id + ".expected.json", ok);
    if (!ok) throw std::runtime_error("cannot read certified answer for " + id);
    fp8::json::Value doc = fp8::json::parse(text);
    Fixture f;
    f.id = id;
    const fp8::json::Value* res = doc.get("result");
    if (!res) throw std::runtime_error("certified answer lacks result");
    if (res->is_null()) {
        f.accept = false;
    } else {
        for (const auto& v : res->a) f.states.push_back(state_of(v));
    }
    const fp8::json::Value* rp = doc.get("replay");
    if (!rp || !rp->is_array()) throw std::runtime_error("certified answer lacks replay");
    for (const auto& r : rp->a) {
        Replay x;
        x.state_index = static_cast<std::size_t>(need_int(r, "state_index"));
        for (const auto& p : r.get("pairs")->a) x.pairs.push_back({p.a[0].i, p.a[1].i});
        for (const auto& t : r.get("trace")->a) {
            fp8::StepTrace st;
            st.state.amax_history.clear();
            for (const auto& v : t.get("amax_history")->a) st.state.amax_history.push_back(v.i);
            st.state.cursor = need_int(t, "cursor");
            st.state.scale = need_int(t, "scale");
            st.state.inverse_scale = need_int(t, "inverse_scale");
            st.quantized = need_int(t, "quantized");
            x.trace.push_back(std::move(st));
        }
        f.replays.push_back(std::move(x));
    }
    return f;
}

/// The verifier's own replay: (history, cursor, scale, inverse, quantized) per step.
bool same_trace(const fp8::State& start, const Replay& r, std::string& why) {
    fp8::State cur = start;
    for (std::size_t i = 0; i < r.pairs.size(); ++i) {
        fp8::StepTrace st;
        try {
            st = fp8::step(cur, r.pairs[i].first, r.pairs[i].second);
        } catch (const std::exception& e) {
            why = std::string("model cannot step the state: ") + e.what();
            return false;
        }
        const fp8::StepTrace& want = r.trace[i];
        if (st.state.amax_history != want.state.amax_history || st.state.cursor != want.state.cursor ||
            st.state.scale != want.state.scale || st.state.inverse_scale != want.state.inverse_scale || st.quantized != want.quantized) {
            why = "trace differs at step " + std::to_string(i);
            return false;
        }
        cur = st.state;
    }
    return true;
}

struct Ctx {
    std::map<std::string, Fixture> fixtures;
    std::map<std::string, Artifact> artifacts;  // by job

    const Artifact& art(const std::string& job) const { return artifacts.at(job); }
    const Fixture& fix(const std::string& job) const { return fixtures.at(artifacts.at(job).fixture); }
};

/// Exact comparison of one job's artifact with its certified answer.
bool exact(const Ctx& ctx, const std::string& job, std::vector<std::string>& out) {
    const Artifact& a = ctx.art(job);
    const Fixture& f = ctx.fix(job);
    if (!a.present) { out.push_back(job + ": no artifact was captured"); return false; }
    if (!a.parsed) { out.push_back(job + ": " + a.error); return false; }
    if (!f.accept) {
        if (!a.result.rejected) { out.push_back(job + ": the layout is inconsistent but " + std::to_string(a.result.states.size()) + " states were written instead of the reject document"); return false; }
        return true;
    }
    if (a.result.rejected) { out.push_back(job + ": the layout is consistent but the reject document was written"); return false; }
    if (a.result.states.size() != f.states.size()) {
        out.push_back(job + ": " + std::to_string(a.result.states.size()) + " states written, " + std::to_string(f.states.size()) + " required");
        return false;
    }
    for (std::size_t i = 0; i < f.states.size(); ++i) {
        if (!(a.result.states[i] == f.states[i])) {
            const fp8::State& g = a.result.states[i];
            const fp8::State& w = f.states[i];
            out.push_back(job + ": state " + std::to_string(i) + " differs: written " + g.tensor + "/" + fp8::role_name(g.role) + " stage " + std::to_string(g.stage) + " shard " + std::to_string(g.shard) + " block " + std::to_string(g.global_block) +
                          " scale (" + std::to_string(g.scale) + "," + std::to_string(g.inverse_scale) + ") history length " + std::to_string(g.amax_history.size()) + " cursor " + std::to_string(g.cursor) +
                          "; required " + w.tensor + "/" + fp8::role_name(w.role) + " stage " + std::to_string(w.stage) + " shard " + std::to_string(w.shard) + " block " + std::to_string(w.global_block) +
                          " scale (" + std::to_string(w.scale) + "," + std::to_string(w.inverse_scale) + ") history length " + std::to_string(w.amax_history.size()) + " cursor " + std::to_string(w.cursor));
            return false;
        }
    }
    return true;
}

bool replay_ok(const Ctx& ctx, const std::string& job, std::vector<std::string>& out) {
    const Artifact& a = ctx.art(job);
    const Fixture& f = ctx.fix(job);
    if (!f.accept) return true;
    if (!a.present || !a.parsed || a.result.rejected) { out.push_back(job + ": no restored states to replay"); return false; }
    if (a.result.states.size() != f.states.size()) { out.push_back(job + ": state count differs, replay not comparable"); return false; }
    for (const Replay& r : f.replays) {
        std::string why;
        if (!same_trace(a.result.states[r.state_index], r, why)) { out.push_back(job + ": state " + std::to_string(r.state_index) + ": " + why); return false; }
    }
    return true;
}

bool run_check(const std::string& name, const Ctx& ctx, std::vector<std::string>& out) {
    bool ok = true;
    auto all_jobs = [&]() { std::vector<std::string> v; for (const Job& j : kJobs) v.push_back(j.name); return v; };
    if (name == "result_documents_well_formed") {
        for (const auto& job : all_jobs()) {
            const Artifact& a = ctx.art(job);
            if (!a.present) { ok = false; out.push_back(job + ": no artifact was captured"); }
            else if (!a.parsed) { ok = false; out.push_back(job + ": " + a.error); }
        }
    } else if (name == "accept_layouts_restored_exactly") {
        for (const auto& job : all_jobs()) if (ctx.fix(job).accept) ok &= exact(ctx, job, out);
    } else if (name == "inconsistent_layouts_rejected") {
        for (const auto& job : all_jobs()) if (!ctx.fix(job).accept) ok &= exact(ctx, job, out);
    } else if (name == "next_state_trace_replay") {
        for (const auto& job : all_jobs()) ok &= replay_ok(ctx, job, out);
    } else if (name == "topology_move_ragged_shards") {
        ok &= exact(ctx, "h_topology", out);
    } else if (name == "tied_tensors_and_replicas") {
        ok &= exact(ctx, "h_tied_replicas", out);
    } else if (name == "role_separation_and_resizing") {
        ok &= exact(ctx, "h_roles_resize", out); ok &= replay_ok(ctx, "h_roles_resize", out);
    } else if (name == "suffix_rule_older_samples") {
        ok &= exact(ctx, "h_older_samples_differ", out); ok &= exact(ctx, "h_reject_suffix", out);
    } else if (name == "coverage_and_inactive_targets") {
        ok &= exact(ctx, "h_inactive_gap_accept", out); ok &= exact(ctx, "h_reject_coverage_gap", out);
    } else if (name == "global_record_validation") {
        ok &= exact(ctx, "h_reject_scale_unneeded", out); ok &= exact(ctx, "h_reject_cursor", out); ok &= exact(ctx, "h_reject_empty_ring", out);
    } else if (name == "target_validation") {
        ok &= exact(ctx, "h_reject_duplicate_targets", out); ok &= exact(ctx, "h_reject_zero_target_length", out);
    } else if (name == "block_boundaries_and_ordering") {
        ok &= exact(ctx, "h_boundaries", out); ok &= exact(ctx, "h_ordering", out);
    } else if (name == "deterministic_output") {
        const Artifact& a = ctx.art("public");
        const Artifact& b = ctx.art("public_repeat");
        if (!a.present || !b.present) { ok = false; out.push_back("an artifact of the public inputs is missing"); }
        else if (a.text != b.text) { ok = false; out.push_back("two runs over the public inputs produced different documents"); }
        else ok &= exact(ctx, "public", out);
    } else if (name == kControl) {
        // Deliberately wrong expectation: the public answer with the first state's cursor set to 1.
        const Artifact& a = ctx.art("public");
        const Fixture& f = ctx.fix("public");
        if (!a.present || !a.parsed || a.result.rejected || f.states.empty()) { out.push_back("control: no comparable public artifact (expected)"); return false; }
        std::vector<fp8::State> want = f.states;
        want[0].cursor = 1;
        if (a.result.states != want) { out.push_back("control: candidate does not match the perturbed answer (expected)"); return false; }
    } else {
        out.push_back("unknown check " + name);
        return false;
    }
    return ok;
}

/// The certified traces must follow from the certified states under the
/// verifier's own model; otherwise the expectations have drifted.
bool selfcheck(const Ctx& ctx, std::string& why) {
    for (const auto& [id, f] : ctx.fixtures) {
        if (!f.accept) continue;
        for (const Replay& r : f.replays) {
            if (r.state_index >= f.states.size()) { why = id + ": replay index out of range"; return false; }
            std::string w;
            if (!same_trace(f.states[r.state_index], r, w)) { why = id + ": certified trace does not follow from the certified state (" + w + ")"; return false; }
        }
        for (std::size_t i = 1; i < f.states.size(); ++i)
            if (!fp8::state_before(f.states[i - 1], f.states[i])) { why = id + ": certified states are not in the published order"; return false; }
    }
    return true;
}

}  // namespace

int main(int argc, char** argv) {
    std::string check, fixtures = "/tests/fixtures", artifacts = "/tmp/artifacts";
    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        if (a == "--run" && i + 1 < argc) check = argv[++i];
        else if (a == "--fixtures" && i + 1 < argc) fixtures = argv[++i];
        else if (a == "--artifacts" && i + 1 < argc) artifacts = argv[++i];
        else if (a == "--list") { for (const char* c : kChecks) std::printf("%s\n", c); return 0; }
        else { std::fprintf(stderr, "usage: judge --run <check> --fixtures <dir> --artifacts <dir>\n"); return 2; }
    }
    if (check.empty()) { std::fprintf(stderr, "--run required\n"); return 2; }
    Ctx ctx;
    try {
        std::set<std::string> ids;
        for (const Job& j : kJobs) ids.insert(j.fixture);
        for (const auto& id : ids) ctx.fixtures.emplace(id, load_fixture(fixtures, id));
    } catch (const std::exception& e) {
        std::printf("FAIL %s: cannot load certified fixtures: %s\n", check.c_str(), e.what());
        return 1;
    }
    std::string why;
    if (!selfcheck(ctx, why)) {
        std::printf("FAIL %s: certified data inconsistent: %s\n", check.c_str(), why.c_str());
        return 1;
    }
    for (const Job& j : kJobs) {
        Artifact a;
        a.job = j.name;
        a.fixture = j.fixture;
        bool ok = false;
        a.text = slurp(artifacts + "/" + j.name + ".json", ok);
        a.present = ok;
        if (ok) {
            try { a.result = fp8::read_result(a.text); a.parsed = true; }
            catch (const std::exception& e) { a.error = e.what(); }
        }
        ctx.artifacts.emplace(j.name, std::move(a));
    }
    std::vector<std::string> out;
    if (run_check(check, ctx, out)) {
        std::printf("%s %s\n", kPassMarker, check.c_str());
        return 0;
    }
    std::printf("FAIL %s\n", check.c_str());
    for (std::size_t i = 0; i < out.size() && i < 12; ++i) std::printf("  %s\n", out[i].c_str());
    return 1;
}
