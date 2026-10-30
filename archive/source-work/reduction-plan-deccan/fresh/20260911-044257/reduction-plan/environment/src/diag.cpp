// reduction-diag [--problem PATH] [--result PATH] [--group NAME]... [--list]
//
// Public diagnostics. Each group is independently runnable and reports what
// it observes when it compares a result document with the problem: whether
// the paths are connected, simple, suffix-consistent and end at the sink,
// whether the selected switches are reduction-capable and on the tree,
// whether the sink value is the fold of the source values, and how the
// telemetry constrains the fabric. It holds no answer, replays nothing and
// searches nothing. Frozen.
#include <algorithm>
#include <cstdio>
#include <fstream>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#include "fabric/model.hpp"

namespace {

const char* const kGroups[] = {"problem", "result_schema", "path_structure", "selection", "sink_value", "telemetry_scope"};

struct Report {
    std::vector<std::string> lines;
    bool ok{true};
    void note(const std::string& s) { lines.push_back("    " + s); }
    void fail(const std::string& s) { ok = false; lines.push_back("    ! " + s); }
};

void print(const char* name, const Report& r) {
    std::printf("GROUP %s: %s\n", name, r.ok ? "PASS" : "FAIL");
    for (const auto& l : r.lines) std::printf("%s\n", l.c_str());
}

std::string slurp(const std::string& path) {
    std::ifstream in(path, std::ios::binary);
    std::ostringstream ss;
    ss << in.rdbuf();
    return ss.str();
}

constexpr std::int64_t kMin = -(std::int64_t{1} << 31), kMax = (std::int64_t{1} << 31) - 1;

void group_problem(const fabric::Problem& p, Report& rep) {
    std::set<std::string> endpoints;
    for (const auto& s : p.sources) endpoints.insert(s.node);
    std::size_t capable = 0;
    for (const auto& w : p.switches) if (w.reduction_capable) ++capable;
    std::map<std::string, std::size_t> outdeg;
    for (const auto& l : p.links) outdeg[l.from]++;
    rep.note(std::to_string(p.sources.size()) + " sources on " + std::to_string(endpoints.size()) + " endpoint nodes, " + std::to_string(p.switches.size()) + " switches (" + std::to_string(capable) + " reduction-capable), " + std::to_string(p.links.size()) + " links, sink " + p.sink + ", payload " + std::to_string(p.payload_bytes) + " bytes");
    rep.note(std::to_string(p.link_bytes.size()) + " links observed" + (p.completion ? ", completion observed" : ", completion not observed"));
    std::vector<std::string> notes;
    if (p.payload_bytes < 1) notes.push_back("payload_bytes is not positive");
    for (const auto& l : p.links) { if (l.rate < 1) notes.push_back("link " + l.id + " has a non-positive rate"); if (l.latency < 0) notes.push_back("link " + l.id + " has a negative latency"); }
    for (const auto& s : p.sources) if (s.value < kMin || s.value > kMax) notes.push_back("source " + s.id + " value outside int32");
    for (std::size_t i = 0; i < notes.size() && i < 6; ++i) rep.note("observed: " + notes[i]);
}

struct Tree { std::map<std::string, std::string> parent; std::set<std::string> visited; bool ok{true}; };

Tree path_structure(const fabric::Problem& p, const fabric::Result& r, Report& rep) {
    Tree t;
    std::map<std::string, const fabric::Link*> links;
    for (const auto& l : p.links) links[l.id] = &l;
    std::vector<const fabric::Source*> srcs;
    for (const auto& s : p.sources) srcs.push_back(&s);
    std::sort(srcs.begin(), srcs.end(), [](const fabric::Source* a, const fabric::Source* b) { return a->id < b->id; });
    if (r.paths.size() != srcs.size()) { rep.fail("the result has " + std::to_string(r.paths.size()) + " paths for " + std::to_string(srcs.size()) + " sources (one path per source, ascending source id)"); t.ok = false; return t; }
    for (std::size_t i = 0; i < srcs.size(); ++i) {
        const auto& path = r.paths[i];
        std::string cur = srcs[i]->node;
        std::set<std::string> seen{cur};
        if (path.empty()) { rep.fail("path " + std::to_string(i) + " (source " + srcs[i]->id + ") is empty"); t.ok = false; continue; }
        bool good = true;
        for (const auto& lid : path) {
            auto it = links.find(lid);
            if (it == links.end()) { rep.fail("path " + std::to_string(i) + " names unknown link " + lid); good = false; break; }
            if (it->second->from != cur) { rep.fail("path " + std::to_string(i) + ": link " + lid + " does not start at " + cur); good = false; break; }
            if (seen.count(it->second->to)) { rep.fail("path " + std::to_string(i) + " revisits node " + it->second->to); good = false; break; }
            auto pit = t.parent.find(cur);
            if (pit != t.parent.end() && pit->second != lid) { rep.fail("path " + std::to_string(i) + " leaves " + cur + " on " + lid + " while an earlier path leaves it on " + pit->second + " (suffix consistency)"); good = false; break; }
            t.parent[cur] = lid;
            t.visited.insert(cur);
            seen.insert(it->second->to);
            cur = it->second->to;
        }
        if (good && cur != p.sink) { rep.fail("path " + std::to_string(i) + " ends at " + cur + ", not at the sink"); good = false; }
        t.ok = t.ok && good;
    }
    if (t.ok) rep.note("every path is connected, simple, ends at the sink, and the paths are suffix-consistent");
    return t;
}

void group_selection(const fabric::Problem& p, const fabric::Result& r, const Tree& t, Report& rep) {
    std::map<std::string, const fabric::Switch*> sw;
    for (const auto& w : p.switches) sw[w.id] = &w;
    for (std::size_t i = 1; i < r.combine.size(); ++i)
        if (!(r.combine[i - 1] < r.combine[i])) { rep.fail("combine ids are not in ascending order (" + r.combine[i - 1] + " before " + r.combine[i] + ")"); break; }
    for (const auto& c : r.combine) {
        auto it = sw.find(c);
        if (it == sw.end()) { rep.fail("selected " + c + " is not a switch"); continue; }
        if (!it->second->reduction_capable) rep.fail("selected " + c + " is not reduction-capable");
        if (t.ok && !t.visited.count(c)) rep.fail("selected " + c + " is not on any path");
        if (it->second->accumulator_capacity < p.payload_bytes) rep.fail("selected " + c + " has capacity " + std::to_string(it->second->accumulator_capacity) + " below the payload size");
    }
    if (rep.ok) rep.note(std::to_string(r.combine.size()) + " selected switches, all reduction-capable, on the tree, within capacity, ascending");
}

void group_sink_value(const fabric::Problem& p, const fabric::Result& r, Report& rep) {
    std::vector<const fabric::Source*> srcs;
    for (const auto& s : p.sources) srcs.push_back(&s);
    std::sort(srcs.begin(), srcs.end(), [](const fabric::Source* a, const fabric::Source* b) { return a->id < b->id; });
    if (srcs.empty()) { rep.note("no sources"); return; }
    std::int64_t acc = srcs[0]->value;
    for (std::size_t i = 1; i < srcs.size(); ++i) acc = std::max(kMin, std::min(kMax, acc + srcs[i]->value));
    if (r.sink_value != acc) rep.fail("sink_value " + std::to_string(r.sink_value) + " is not the fold of the source values in ascending id with saturation (" + std::to_string(acc) + ")");
    else rep.note("sink_value equals the saturating fold of the source values in ascending id");
}

void group_telemetry_scope(const fabric::Problem& p, const fabric::Result& r, const Tree& t, Report& rep) {
    std::set<std::string> used;
    for (const auto& path : r.paths) for (const auto& l : path) used.insert(l);
    std::size_t zero_listed = 0, zero_used = 0, nonzero_unused = 0;
    for (const auto& [lid, b] : p.link_bytes) {
        if (b == 0) { ++zero_listed; if (used.count(lid)) ++zero_used; }
        else if (!used.count(lid)) ++nonzero_unused;
    }
    rep.note(std::to_string(p.link_bytes.size()) + " observed links, " + std::to_string(zero_listed) + " observed as carrying nothing; the plan uses " + std::to_string(used.size()) + " links");
    if (zero_used) rep.fail(std::to_string(zero_used) + " link(s) observed as carrying nothing are used by the plan");
    if (nonzero_unused) rep.fail(std::to_string(nonzero_unused) + " link(s) observed with bytes are not used by the plan");
    (void)t;
}

}  // namespace

int main(int argc, char** argv) {
    std::string problem = "/app/problem.json", result = "/app/result.json";
    std::vector<std::string> wanted;
    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        if (a == "--problem" && i + 1 < argc) problem = argv[++i];
        else if (a == "--result" && i + 1 < argc) result = argv[++i];
        else if (a == "--group" && i + 1 < argc) wanted.push_back(argv[++i]);
        else if (a == "--list") { for (const char* g : kGroups) std::printf("%s\n", g); return 0; }
        else { std::fprintf(stderr, "usage: reduction-diag [--problem PATH] [--result PATH] [--group NAME]... [--list]\n"); return 2; }
    }
    for (const auto& w : wanted) {
        bool known = false;
        for (const char* g : kGroups) if (w == g) known = true;
        if (!known) { std::fprintf(stderr, "unknown group '%s'; --list shows the groups\n", w.c_str()); return 2; }
    }
    auto run = [&](const char* g) { return wanted.empty() || std::find(wanted.begin(), wanted.end(), g) != wanted.end(); };
    bool all_ok = true;
    fabric::Problem p;
    try {
        p = fabric::load_problem(problem);
    } catch (const fabric::MalformedInput& e) {
        std::printf("GROUP problem: FAIL\n    ! %s\n", e.what());
        return 1;
    }
    if (run("problem")) { Report r; group_problem(p, r); print("problem", r); all_ok = all_ok && r.ok; }
    bool needs_result = false;
    for (std::size_t i = 1; i < sizeof(kGroups) / sizeof(kGroups[0]); ++i) if (run(kGroups[i])) needs_result = true;
    if (!needs_result) return all_ok ? 0 : 1;
    fabric::Result res;
    try {
        res = fabric::read_result(slurp(result));
    } catch (const std::exception& e) {
        std::printf("GROUP result_schema: FAIL\n    ! %s: %s\n", result.c_str(), e.what());
        return 1;
    }
    if (run("result_schema")) { Report r; r.note(std::string("status ") + fabric::status_name(res.status)); print("result_schema", r); }
    if (res.status != fabric::Status::Ok) {
        for (std::size_t i = 2; i < sizeof(kGroups) / sizeof(kGroups[0]); ++i)
            if (run(kGroups[i])) std::printf("GROUP %s: PASS\n    the result carries no plan (status %s); nothing to inspect\n", kGroups[i], fabric::status_name(res.status));
        return all_ok ? 0 : 1;
    }
    Tree t;
    { Report r; t = path_structure(p, res, r); if (run("path_structure")) { print("path_structure", r); all_ok = all_ok && r.ok; } }
    if (run("selection")) { Report r; group_selection(p, res, t, r); print("selection", r); all_ok = all_ok && r.ok; }
    if (run("sink_value")) { Report r; group_sink_value(p, res, r); print("sink_value", r); all_ok = all_ok && r.ok; }
    if (run("telemetry_scope")) { Report r; group_telemetry_scope(p, res, t, r); print("telemetry_scope", r); all_ok = all_ok && r.ok; }
    return all_ok ? 0 : 1;
}
