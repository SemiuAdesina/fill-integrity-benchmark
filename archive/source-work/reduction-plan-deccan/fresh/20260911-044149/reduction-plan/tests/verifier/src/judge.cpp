// Verifier-private judge: `judge --run <check> --fixtures <dir> --artifacts <dir>`.
//
// Loads every fixture (problem, certified status, certified canonical plan
// with its observables) and the artifacts the probe phase captured, then
// runs one named check. The judge links no candidate code: every artifact is
// parsed with the verifier's own strict reader and every plan is replayed
// with the verifier's own replay engine. Before grading it replays every
// certified plan and refuses to grade if the certified observables do not
// follow. It prints `##FABRIC-PASS <check>` and exits 0 only when the check
// holds on every fixture it covers.
#include <algorithm>
#include <cstdio>
#include <fstream>
#include <functional>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#include "fabric/json.hpp"
#include "fabric/model.hpp"
#include "plan.hpp"

namespace {

const char* const kPassMarker = "##FABRIC-PASS";

#define CHECK(name) #name
const char* const kChecks[] = {
    CHECK(result_documents_well_formed),
    CHECK(invalid_problems_reported),
    CHECK(infeasible_problems_reported),
    CHECK(plans_legal_and_reproduce_telemetry),
    CHECK(canonical_plans_recovered_exactly),
    CHECK(pipelined_link_serialization),
    CHECK(suffix_consistent_tree),
    CHECK(capacity_and_combine_choice),
    CHECK(saturating_fold_value),
    CHECK(telemetry_zero_links_and_optional_completion),
    CHECK(objective_order_and_plan_key),
    CHECK(public_capture),
    CHECK(graded_fabrics_recovered_within_budget),
    CHECK(deterministic_output),
};
const char* const kControl = "negative_control_must_fail";

constexpr std::int64_t kMin = -(std::int64_t{1} << 31), kMax = (std::int64_t{1} << 31) - 1;

// ------------------------------------------------------------ verifier-owned replay
struct Observables { std::int64_t completion{0}, allocation{0}, total_bytes{0}, sink_value{0}; std::map<std::string, std::int64_t> link_bytes; };

struct Engine {
    const fabric::Problem& p;
    std::map<std::string, const fabric::Link*> links;
    std::map<std::string, const fabric::Switch*> switches;
    std::vector<const fabric::Source*> sources;
    std::map<std::string, std::int64_t> values;
    explicit Engine(const fabric::Problem& pr) : p(pr) {
        for (const auto& l : p.links) links[l.id] = &l;
        for (const auto& w : p.switches) switches[w.id] = &w;
        for (const auto& s : p.sources) { sources.push_back(&s); values[s.id] = s.value; }
        std::sort(sources.begin(), sources.end(), [](const fabric::Source* a, const fabric::Source* b) { return a->id < b->id; });
    }
    struct Payload { std::int64_t ready; std::set<std::string> origins; };
    bool replay(const std::vector<std::vector<std::string>>& paths, const std::vector<std::string>& combine, Observables& out, std::string& why) const {
        if (paths.size() != sources.size()) { why = "path count differs from the source count"; return false; }
        std::map<std::string, std::string> parent;
        for (std::size_t i = 0; i < paths.size(); ++i) {
            std::string cur = sources[i]->node; std::set<std::string> seen{cur};
            if (paths[i].empty()) { why = "empty path"; return false; }
            for (const auto& lid : paths[i]) {
                auto it = links.find(lid);
                if (it == links.end() || it->second->from != cur) { why = "path " + std::to_string(i) + " is not connected at " + lid; return false; }
                if (seen.count(it->second->to)) { why = "path " + std::to_string(i) + " is not simple"; return false; }
                auto pit = parent.find(cur);
                if (pit != parent.end() && pit->second != lid) { why = "paths are not suffix-consistent at " + cur; return false; }
                parent[cur] = lid; seen.insert(it->second->to); cur = it->second->to;
            }
            if (cur != p.sink) { why = "path " + std::to_string(i) + " does not end at the sink"; return false; }
        }
        for (std::size_t i = 1; i < combine.size(); ++i) if (!(combine[i - 1] < combine[i])) { why = "combine ids not ascending"; return false; }
        std::set<std::string> cset(combine.begin(), combine.end());
        for (const auto& c : combine) {
            auto it = switches.find(c);
            if (it == switches.end() || !it->second->reduction_capable) { why = "selected " + c + " is not a reduction-capable switch"; return false; }
            if (!parent.count(c)) { why = "selected " + c + " is not on any path"; return false; }
        }
        std::map<std::string, std::vector<std::pair<std::string, std::string>>> children;
        for (const auto& [node, lid] : parent) children[links.at(lid)->to].push_back({node, lid});
        const std::int64_t B = p.payload_bytes;
        Observables r; bool illegal = false; std::string ill;
        std::function<std::vector<Payload>(const std::string&)> process = [&](const std::string& node) {
            std::vector<Payload> pay;
            for (const auto* s : sources) if (s->node == node) pay.push_back(Payload{0, {s->id}});
            auto ch = children.find(node);
            if (ch != children.end()) {
                std::sort(ch->second.begin(), ch->second.end());
                for (const auto& [child, lid] : ch->second) {
                    const fabric::Link& l = *links.at(lid);
                    std::vector<Payload> outgoing = process(child);
                    std::sort(outgoing.begin(), outgoing.end(), [](const Payload& a, const Payload& b) { return a.ready != b.ready ? a.ready < b.ready : *a.origins.begin() < *b.origins.begin(); });
                    std::int64_t free = 0; const std::int64_t occ = (B + l.rate - 1) / l.rate;
                    for (Payload& q : outgoing) { const std::int64_t start = std::max(q.ready, free); free = start + occ; q.ready = free + l.latency; r.link_bytes[lid] += B; pay.push_back(std::move(q)); }
                }
            }
            if (cset.count(node)) {
                if (pay.size() < 2) { illegal = true; ill = "selected " + node + " is reached by fewer than two payloads"; return pay; }
                if (B > switches.at(node)->accumulator_capacity) { illegal = true; ill = "selected " + node + " exceeds its accumulator capacity"; return pay; }
                r.allocation += B;
                Payload merged{0, {}};
                for (const Payload& q : pay) { merged.ready = std::max(merged.ready, q.ready); merged.origins.insert(q.origins.begin(), q.origins.end()); }
                return std::vector<Payload>{merged};
            }
            return pay;
        };
        std::vector<Payload> at_sink = process(p.sink);
        if (illegal) { why = ill; return false; }
        if (at_sink.empty()) { why = "nothing reaches the sink"; return false; }
        std::set<std::string> origins;
        for (const Payload& q : at_sink) { r.completion = std::max(r.completion, q.ready); origins.insert(q.origins.begin(), q.origins.end()); }
        auto it = origins.begin(); std::int64_t acc = values.at(*it);
        for (++it; it != origins.end(); ++it) acc = std::max(kMin, std::min(kMax, acc + values.at(*it)));
        r.sink_value = acc;
        for (const auto& [lid, b] : r.link_bytes) r.total_bytes += b;
        out = r; return true;
    }
    bool telemetry(const Observables& o, std::string& why) const {
        for (const auto& [lid, b] : p.link_bytes) {
            auto it = o.link_bytes.find(lid);
            if ((it == o.link_bytes.end() ? 0 : it->second) != b) { why = "link " + lid + " carries " + std::to_string(it == o.link_bytes.end() ? 0 : it->second) + " bytes, telemetry says " + std::to_string(b); return false; }
        }
        if (p.completion && *p.completion != o.completion) { why = "completion " + std::to_string(o.completion) + ", telemetry says " + std::to_string(*p.completion); return false; }
        return true;
    }
};

// ---------------------------------------------------------------- fixtures
struct Fixture {
    std::string id;
    fabric::Problem problem;
    std::string status;                 // ok | invalid | infeasible
    fabric::Result plan;                // certified (ok fixtures)
    Observables certified;              // certified observables (ok fixtures)
};

struct Artifact { std::string job, fixture; bool present{false}, parsed{false}; std::string text, error; fabric::Result result; };

std::string slurp(const std::string& path, bool& ok) { std::ifstream in(path, std::ios::binary); ok = static_cast<bool>(in); std::ostringstream ss; ss << in.rdbuf(); return ss.str(); }

std::int64_t need_int(const fabric::json::Value& v, const char* key) { const auto* m = v.get(key); if (!m || !m->is_int()) throw std::runtime_error(std::string("expected integer ") + key); return m->i; }

Fixture load_fixture(const std::string& dir, const std::string& id) {
    bool ok = false;
    Fixture f; f.id = id;
    std::string ptxt = slurp(dir + "/" + id + "/problem.json", ok);
    if (!ok) throw std::runtime_error("cannot read problem for " + id);
    f.problem = fabric::parse_problem(ptxt);
    std::string etxt = slurp(dir + "/" + id + ".expected.json", ok);
    if (!ok) throw std::runtime_error("cannot read certified answer for " + id);
    fabric::json::Value doc = fabric::json::parse(etxt);
    const auto* st = doc.get("status");
    if (!st || !st->is_str()) throw std::runtime_error("certified status missing");
    f.status = st->s;
    if (f.status == "ok") {
        const auto* plan = doc.get("plan");
        const auto* obs = doc.get("observables");
        if (!plan || !plan->is_object() || !obs || !obs->is_object()) throw std::runtime_error("certified plan missing");
        f.plan.status = fabric::Status::Ok;
        for (const auto& path : plan->get("paths")->a) { std::vector<std::string> ids; for (const auto& l : path.a) ids.push_back(l.s); f.plan.paths.push_back(ids); }
        for (const auto& c : plan->get("combine")->a) f.plan.combine.push_back(c.s);
        f.plan.sink_value = need_int(*plan, "sink_value");
        f.certified.completion = need_int(*obs, "completion");
        f.certified.allocation = need_int(*obs, "allocation");
        f.certified.total_bytes = need_int(*obs, "total_bytes");
        f.certified.sink_value = f.plan.sink_value;
        for (const auto& m : obs->get("link_bytes")->o) f.certified.link_bytes[m.key] = m.value.i;
    }
    return f;
}

struct Ctx {
    std::map<std::string, Fixture> fixtures;
    std::map<std::string, Artifact> artifacts;
    const Artifact& art(const std::string& job) const { return artifacts.at(job); }
    const Fixture& fix(const std::string& job) const { return fixtures.at(artifacts.at(job).fixture); }
};

const char* sname(fabric::Status s) { return fabric::status_name(s); }

/// Status equality plus, for ok fixtures, legality, telemetry and exact canonical identity.
bool exact(const Ctx& ctx, const std::string& job, std::vector<std::string>& out) {
    const Artifact& a = ctx.art(job); const Fixture& f = ctx.fix(job);
    if (!a.present) { out.push_back(job + ": no artifact was captured"); return false; }
    if (!a.parsed) { out.push_back(job + ": " + a.error); return false; }
    if (std::string(sname(a.result.status)) != f.status) { out.push_back(job + ": status " + sname(a.result.status) + ", required " + f.status); return false; }
    if (f.status != "ok") return true;
    Engine e(f.problem); Observables o; std::string why;
    if (!e.replay(a.result.paths, a.result.combine, o, why)) { out.push_back(job + ": the written plan is illegal: " + why); return false; }
    if (!e.telemetry(o, why)) { out.push_back(job + ": the written plan does not reproduce the telemetry: " + why); return false; }
    if (a.result.paths != f.plan.paths || a.result.combine != f.plan.combine) {
        out.push_back(job + ": a legal telemetry-consistent plan, but not the canonical minimum: written (completion " + std::to_string(o.completion) + ", allocation " + std::to_string(o.allocation) + ", bytes " + std::to_string(o.total_bytes) + ", " + std::to_string(a.result.combine.size()) + " combines); required (completion " + std::to_string(f.certified.completion) + ", allocation " + std::to_string(f.certified.allocation) + ", bytes " + std::to_string(f.certified.total_bytes) + ", " + std::to_string(f.plan.combine.size()) + " combines)");
        return false;
    }
    if (a.result.sink_value != f.plan.sink_value) { out.push_back(job + ": sink_value " + std::to_string(a.result.sink_value) + ", required " + std::to_string(f.plan.sink_value)); return false; }
    return true;
}

bool legal_and_telemetry(const Ctx& ctx, const std::string& job, std::vector<std::string>& out) {
    const Artifact& a = ctx.art(job); const Fixture& f = ctx.fix(job);
    if (f.status != "ok") return true;
    if (!a.present || !a.parsed || a.result.status != fabric::Status::Ok) { out.push_back(job + ": no plan was written for a solvable problem"); return false; }
    Engine e(f.problem); Observables o; std::string why;
    if (!e.replay(a.result.paths, a.result.combine, o, why)) { out.push_back(job + ": illegal plan: " + why); return false; }
    if (!e.telemetry(o, why)) { out.push_back(job + ": telemetry not reproduced: " + why); return false; }
    if (a.result.sink_value != o.sink_value) { out.push_back(job + ": sink_value " + std::to_string(a.result.sink_value) + " is not the replayed value " + std::to_string(o.sink_value)); return false; }
    return true;
}

bool run_check(const std::string& name, const Ctx& ctx, std::vector<std::string>& out) {
    bool ok = true;
    std::vector<std::string> jobs; for (const Job& j : kJobs) jobs.push_back(j.name);
    auto all_with = [&](const std::string& status, const std::function<bool(const std::string&)>& fn) { for (const auto& j : jobs) if (ctx.fix(j).status == status) ok = fn(j) && ok; };
    if (name == "result_documents_well_formed") { for (const auto& j : jobs) { const Artifact& a = ctx.art(j); if (!a.present) { ok = false; out.push_back(j + ": no artifact was captured"); } else if (!a.parsed) { ok = false; out.push_back(j + ": " + a.error); } } }
    else if (name == "invalid_problems_reported") all_with("invalid", [&](const std::string& j) { return exact(ctx, j, out); });
    else if (name == "infeasible_problems_reported") all_with("infeasible", [&](const std::string& j) { return exact(ctx, j, out); });
    else if (name == "plans_legal_and_reproduce_telemetry") all_with("ok", [&](const std::string& j) { return legal_and_telemetry(ctx, j, out); });
    else if (name == "canonical_plans_recovered_exactly") all_with("ok", [&](const std::string& j) { return exact(ctx, j, out); });
    else if (name == "pipelined_link_serialization") { for (const char* j : {"h_pipelined_latency", "h_serialization_shared_link", "h_shared_endpoint"}) ok = exact(ctx, j, out) && ok; }
    else if (name == "suffix_consistent_tree") ok = exact(ctx, "h_suffix_tree", out);
    else if (name == "capacity_and_combine_choice") { for (const char* j : {"h_two_required_capacity", "h_combine_choice"}) ok = exact(ctx, j, out) && ok; }
    else if (name == "saturating_fold_value") ok = exact(ctx, "h_saturation_fold", out);
    else if (name == "telemetry_zero_links_and_optional_completion") { for (const char* j : {"h_zero_bytes_link", "h_no_completion"}) ok = exact(ctx, j, out) && ok; }
    else if (name == "objective_order_and_plan_key") { for (const char* j : {"h_allocation_before_bytes", "h_plan_key_canonical"}) ok = exact(ctx, j, out) && ok; }
    else if (name == "public_capture") ok = exact(ctx, "public", out);
    else if (name == "graded_fabrics_recovered_within_budget") {
        // the six graded-size fabrics: an artifact exists only when the probe finished within its budget
        for (const char* j : {"s_listed_completion_10", "s_open_completion_10", "s_reduction_pinned_10", "s_listed_completion_12", "s_open_completion_12", "s_infeasible_12"}) ok = exact(ctx, j, out) && ok;
    }
    else if (name == "deterministic_output") {
        const Artifact& a = ctx.art("public"); const Artifact& b = ctx.art("public_repeat");
        if (!a.present || !b.present) { ok = false; out.push_back("an artifact of the public problem is missing"); }
        else if (a.text != b.text) { ok = false; out.push_back("two runs over the public problem produced different documents"); }
        else ok = exact(ctx, "public", out);
    } else if (name == kControl) {
        // deliberately wrong expectation: the public plan with its sink value shifted by one
        const Artifact& a = ctx.art("public"); const Fixture& f = ctx.fix("public");
        if (!a.present || !a.parsed || a.result.status != fabric::Status::Ok) { out.push_back("control: no comparable public artifact (expected)"); return false; }
        if (a.result.sink_value != f.plan.sink_value + 1) { out.push_back("control: candidate does not match the perturbed answer (expected)"); return false; }
    } else { out.push_back("unknown check " + name); return false; }
    return ok;
}

/// Certified plans must replay legally to the certified observables under the verifier's own engine.
bool selfcheck(const Ctx& ctx, std::string& why) {
    for (const auto& [id, f] : ctx.fixtures) {
        if (f.status != "ok") continue;
        Engine e(f.problem); Observables o; std::string w;
        if (!e.replay(f.plan.paths, f.plan.combine, o, w)) { why = id + ": certified plan is illegal under the frozen model (" + w + ")"; return false; }
        if (!e.telemetry(o, w)) { why = id + ": certified plan does not reproduce its telemetry (" + w + ")"; return false; }
        if (o.completion != f.certified.completion || o.allocation != f.certified.allocation || o.total_bytes != f.certified.total_bytes || o.sink_value != f.certified.sink_value) { why = id + ": certified observables do not follow from the certified plan"; return false; }
        for (const auto& [lid, b] : f.certified.link_bytes) { auto it = o.link_bytes.find(lid); if ((it == o.link_bytes.end() ? 0 : it->second) != b) { why = id + ": certified link bytes differ on " + lid; return false; } }
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
        std::set<std::string> ids; for (const Job& j : kJobs) ids.insert(j.fixture);
        for (const auto& id : ids) ctx.fixtures.emplace(id, load_fixture(fixtures, id));
    } catch (const std::exception& e) { std::printf("FAIL %s: cannot load certified fixtures: %s\n", check.c_str(), e.what()); return 1; }
    std::string why;
    if (!selfcheck(ctx, why)) { std::printf("FAIL %s: certified data inconsistent: %s\n", check.c_str(), why.c_str()); return 1; }
    for (const Job& j : kJobs) {
        Artifact a; a.job = j.name; a.fixture = j.fixture; bool ok = false;
        a.text = slurp(artifacts + "/" + j.name + ".json", ok); a.present = ok;
        if (ok) { try { a.result = fabric::read_result(a.text); a.parsed = true; } catch (const std::exception& e) { a.error = e.what(); } }
        ctx.artifacts.emplace(j.name, std::move(a));
    }
    std::vector<std::string> out;
    if (run_check(check, ctx, out)) { std::printf("%s %s\n", kPassMarker, check.c_str()); return 0; }
    std::printf("FAIL %s\n", check.c_str());
    for (std::size_t i = 0; i < out.size() && i < 12; ++i) std::printf("  %s\n", out[i].c_str());
    return 1;
}
