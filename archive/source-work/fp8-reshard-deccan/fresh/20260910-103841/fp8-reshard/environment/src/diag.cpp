// fp8-diag [--checkpoint PATH] [--target PATH] [--result PATH] [--group NAME]... [--list]
//
// Public diagnostics. Each group is independently runnable and reports what
// it observes when it compares a result document with the two input
// documents: which placements are missing or superfluous, which histories
// have the wrong shape, which scale pairs disagree with the checkpoint, and
// whether the frozen model can step every state. It holds no expected
// answer and performs no restoration. Frozen.
#include <algorithm>
#include <cstdio>
#include <fstream>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <tuple>
#include <vector>

#include "fp8/model.hpp"

namespace {

const char* const kGroups[] = {"inputs", "result_schema", "placements", "scale_pairs", "history_shape", "ordering", "model_step"};

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

using Placement = std::tuple<std::string, int, std::int64_t, std::int64_t, std::int64_t>;  // tensor, role, stage, shard, block

Placement placement_of(const fp8::State& s) {
    return {s.tensor, s.role == fp8::Role::Forward ? 0 : 1, s.stage, s.shard, s.global_block};
}

std::string show(const Placement& p) {
    return std::get<0>(p) + "/" + (std::get<1>(p) == 0 ? "forward" : "backward") + " stage " + std::to_string(std::get<2>(p)) + " shard " +
           std::to_string(std::get<3>(p)) + " block " + std::to_string(std::get<4>(p));
}

void group_inputs(const fp8::Checkpoint& ck, const fp8::Layout& ly, Report& rep) {
    std::set<std::string> tensors, groups;
    std::set<std::tuple<std::string, int, std::int64_t>> blocks;
    std::map<std::tuple<std::string, int, std::int64_t>, int> describers;
    std::int64_t empty = 0, bad_cursor = 0;
    for (const auto& r : ck.records) {
        tensors.insert(r.tensor);
        groups.insert(r.group());
        auto key = std::make_tuple(r.group(), r.role == fp8::Role::Forward ? 0 : 1, r.block());
        blocks.insert(key);
        describers[key]++;
        if (r.amax_history.empty()) ++empty;
        else if (r.cursor >= static_cast<std::int64_t>(r.amax_history.size())) ++bad_cursor;
    }
    int multi = 0;
    for (const auto& [k, n] : describers) if (n > 1) ++multi;
    std::int64_t active = 0, needed = 0;
    for (const auto& t : ly.targets) if (t.active) { ++active; needed += t.last_block() - t.first_block() + 1; }
    rep.note(std::to_string(ck.records.size()) + " records over " + std::to_string(tensors.size()) + " tensors, " + std::to_string(groups.size()) +
             " identity groups, " + std::to_string(blocks.size()) + " described blocks (" + std::to_string(multi) + " described by more than one record)");
    rep.note(std::to_string(ly.targets.size()) + " targets, " + std::to_string(active) + " active, requiring " + std::to_string(needed) + " block states");
    if (empty) rep.note(std::to_string(empty) + " records carry an empty history");
    if (bad_cursor) rep.note(std::to_string(bad_cursor) + " records carry a cursor outside their history");
}

void group_result_schema(const fp8::Result& res, Report& rep) {
    if (res.rejected) { rep.note("the result is the reject document"); return; }
    rep.note(std::to_string(res.states.size()) + " states");
    for (const auto& s : res.states) {
        if (s.cursor != 0) { rep.fail(show(placement_of(s)) + ": cursor is " + std::to_string(s.cursor) + ", the result format requires 0"); break; }
    }
    for (const auto& s : res.states) {
        if (s.amax_history.empty()) { rep.fail(show(placement_of(s)) + ": empty history"); break; }
        if (s.scale < 1 || s.inverse_scale < 1) { rep.fail(show(placement_of(s)) + ": scale pair not positive"); break; }
    }
}

void group_placements(const fp8::Layout& ly, const fp8::Result& res, Report& rep) {
    if (res.rejected) {
        std::int64_t needed = 0;
        for (const auto& t : ly.targets) if (t.active) needed += t.last_block() - t.first_block() + 1;
        if (needed == 0) rep.note("the result is the reject document and no active target requires a state");
        else rep.note("the result is the reject document while the active targets require " + std::to_string(needed) + " block states; that is right only if a rule of restoration-semantics-v1.md sections 2, 4, 6 or 7 applies to these documents, which this tool does not decide");
        return;
    }
    std::set<Placement> expected, got;
    std::map<Placement, std::int64_t> want_len;
    for (const auto& t : ly.targets) {
        if (!t.active) continue;
        for (std::int64_t k = t.first_block(); k <= t.last_block(); ++k) {
            Placement p{t.tensor, t.role == fp8::Role::Forward ? 0 : 1, t.stage, t.shard, k};
            expected.insert(p);
            want_len[p] = t.history_length;
        }
    }
    int dup = 0;
    for (const auto& s : res.states) {
        Placement p = placement_of(s);
        if (!got.insert(p).second) ++dup;
    }
    std::vector<Placement> missing, extra;
    for (const auto& p : expected) if (!got.count(p)) missing.push_back(p);
    for (const auto& p : got) if (!expected.count(p)) extra.push_back(p);
    rep.note(std::to_string(expected.size()) + " placements required by active targets, " + std::to_string(got.size()) + " distinct placements in the result");
    for (std::size_t i = 0; i < missing.size() && i < 6; ++i) rep.fail("missing: " + show(missing[i]));
    if (missing.size() > 6) rep.fail(std::to_string(missing.size() - 6) + " further missing placements");
    for (std::size_t i = 0; i < extra.size() && i < 6; ++i) rep.fail("not required by any active target: " + show(extra[i]));
    if (extra.size() > 6) rep.fail(std::to_string(extra.size() - 6) + " further superfluous placements");
    if (dup) rep.fail(std::to_string(dup) + " repeated placements");
    int wrong_len = 0;
    for (const auto& s : res.states) {
        auto it = want_len.find(placement_of(s));
        if (it != want_len.end() && static_cast<std::int64_t>(s.amax_history.size()) != it->second) {
            if (wrong_len < 4) rep.fail(show(placement_of(s)) + ": history has " + std::to_string(s.amax_history.size()) + " samples, the target asks for " + std::to_string(it->second));
            ++wrong_len;
        }
    }
    if (wrong_len > 4) rep.fail(std::to_string(wrong_len - 4) + " further states with the wrong history length");
}

void group_scale_pairs(const fp8::Checkpoint& ck, const fp8::Layout& ly, const fp8::Result& res, Report& rep) {
    if (res.rejected) { rep.note("no states to compare (reject document)"); return; }
    std::map<std::string, std::string> group_of;
    for (const auto& t : ly.targets) group_of[t.tensor] = t.group();
    std::map<std::tuple<std::string, int, std::int64_t>, std::set<std::pair<std::int64_t, std::int64_t>>> pairs;
    for (const auto& r : ck.records) pairs[{r.group(), r.role == fp8::Role::Forward ? 0 : 1, r.block()}].insert({r.scale, r.inverse_scale});
    int bad = 0, unknown = 0;
    for (const auto& s : res.states) {
        auto g = group_of.find(s.tensor);
        if (g == group_of.end()) { ++unknown; continue; }
        auto it = pairs.find({g->second, s.role == fp8::Role::Forward ? 0 : 1, s.global_block});
        if (it == pairs.end()) { ++unknown; continue; }
        if (!it->second.count({s.scale, s.inverse_scale})) {
            if (bad < 4) rep.fail(show(placement_of(s)) + ": scale pair (" + std::to_string(s.scale) + ", " + std::to_string(s.inverse_scale) + ") matches no record of that block");
            ++bad;
        }
    }
    if (bad > 4) rep.fail(std::to_string(bad - 4) + " further scale-pair disagreements");
    if (unknown) rep.note(std::to_string(unknown) + " states name a block no record describes");
    if (!bad) rep.note("every state's scale pair occurs in the checkpoint records of its block");
}

void group_history_shape(const fp8::Checkpoint& ck, const fp8::Layout& ly, const fp8::Result& res, Report& rep) {
    if (res.rejected) { rep.note("no states to compare (reject document)"); return; }
    std::map<std::string, std::string> group_of;
    for (const auto& t : ly.targets) group_of[t.tensor] = t.group();
    std::map<std::tuple<std::string, int, std::int64_t>, std::multiset<std::int64_t>> samples;
    std::map<std::tuple<std::string, int, std::int64_t>, std::int64_t> longest;
    for (const auto& r : ck.records) {
        auto key = std::make_tuple(r.group(), r.role == fp8::Role::Forward ? 0 : 1, r.block());
        for (auto x : r.amax_history) samples[key].insert(x);
        longest[key] = std::max<std::int64_t>(longest[key], static_cast<std::int64_t>(r.amax_history.size()));
    }
    int foreign = 0, padding = 0;
    for (const auto& s : res.states) {
        auto g = group_of.find(s.tensor);
        if (g == group_of.end()) continue;
        auto key = std::make_tuple(g->second, s.role == fp8::Role::Forward ? 0 : 1, s.global_block);
        auto it = samples.find(key);
        if (it == samples.end()) continue;
        std::multiset<std::int64_t> pool = it->second;
        bool bad = false;
        for (auto x : s.amax_history) {
            if (x == 0) continue;  // zeros may be padding
            auto p = pool.find(x);
            if (p == pool.end()) { bad = true; break; }
            pool.erase(p);
        }
        if (bad) { if (foreign < 4) rep.fail(show(placement_of(s)) + ": a nonzero sample does not occur in any record of that block"); ++foreign; }
        std::int64_t L = static_cast<std::int64_t>(s.amax_history.size());
        if (L > longest[key]) {
            // more samples than any record carries: the oldest L - longest must be zero
            for (std::int64_t i = 0; i < L - longest[key]; ++i) if (s.amax_history[static_cast<std::size_t>(i)] != 0) { if (padding < 4) rep.fail(show(placement_of(s)) + ": the oldest " + std::to_string(L - longest[key]) + " samples exceed every record's length but are not zero"); ++padding; break; }
        }
    }
    if (foreign > 4) rep.fail(std::to_string(foreign - 4) + " further states with foreign samples");
    if (padding > 4) rep.fail(std::to_string(padding - 4) + " further states with non-zero padding");
    if (!foreign && !padding) rep.note("every nonzero sample occurs in the block's records and every excess-length prefix is zero");
}

void group_ordering(const fp8::Result& res, Report& rep) {
    if (res.rejected) { rep.note("no states to compare (reject document)"); return; }
    for (std::size_t i = 1; i < res.states.size(); ++i) {
        if (!fp8::state_before(res.states[i - 1], res.states[i])) {
            rep.fail("state " + std::to_string(i) + " (" + show(placement_of(res.states[i])) + ") does not follow state " + std::to_string(i - 1) + " (" + show(placement_of(res.states[i - 1])) + ") in the required order");
            return;
        }
    }
    rep.note("states are in the required order");
}

void group_model_step(const fp8::Result& res, Report& rep) {
    if (res.rejected) { rep.note("no states to step (reject document)"); return; }
    int bad = 0;
    for (const auto& s : res.states) {
        try {
            fp8::State cur = s;
            for (int i = 0; i < 3; ++i) cur = fp8::step(cur, 5 + i, 1000 * (i + 1)).state;
        } catch (const std::exception& e) {
            if (bad < 4) rep.fail(show(placement_of(s)) + ": the frozen model cannot step this state: " + e.what());
            ++bad;
        }
    }
    if (bad > 4) rep.fail(std::to_string(bad - 4) + " further states the model cannot step");
    if (!bad) rep.note("the frozen model steps every state");
}

}  // namespace

int main(int argc, char** argv) {
    std::string checkpoint = "/app/checkpoint.json", target = "/app/target.json", result = "/app/restored.json";
    std::vector<std::string> wanted;
    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        if (a == "--checkpoint" && i + 1 < argc) checkpoint = argv[++i];
        else if (a == "--target" && i + 1 < argc) target = argv[++i];
        else if (a == "--result" && i + 1 < argc) result = argv[++i];
        else if (a == "--group" && i + 1 < argc) wanted.push_back(argv[++i]);
        else if (a == "--list") { for (const char* g : kGroups) std::printf("%s\n", g); return 0; }
        else { std::fprintf(stderr, "usage: fp8-diag [--checkpoint PATH] [--target PATH] [--result PATH] [--group NAME]... [--list]\n"); return 2; }
    }
    for (const auto& w : wanted) {
        bool known = false;
        for (const char* g : kGroups) if (w == g) known = true;
        if (!known) { std::fprintf(stderr, "unknown group '%s'; --list shows the groups\n", w.c_str()); return 2; }
    }
    auto run = [&](const char* g) { return wanted.empty() || std::find(wanted.begin(), wanted.end(), g) != wanted.end(); };
    bool all_ok = true;
    fp8::Checkpoint ck;
    fp8::Layout ly;
    try {
        ck = fp8::load_checkpoint(checkpoint);
        ly = fp8::load_layout(target);
    } catch (const fp8::MalformedInput& e) {
        std::printf("GROUP inputs: FAIL\n    ! %s\n", e.what());
        return 1;
    }
    if (run("inputs")) { Report r; group_inputs(ck, ly, r); print("inputs", r); all_ok = all_ok && r.ok; }
    bool needs_result = false;
    for (std::size_t i = 1; i < sizeof(kGroups) / sizeof(kGroups[0]); ++i) if (run(kGroups[i])) needs_result = true;
    if (!needs_result) return all_ok ? 0 : 1;
    fp8::Result res;
    try {
        res = fp8::read_result(slurp(result));
    } catch (const std::exception& e) {
        std::printf("GROUP result_schema: FAIL\n    ! %s: %s\n", result.c_str(), e.what());
        for (std::size_t i = 2; i < sizeof(kGroups) / sizeof(kGroups[0]); ++i) if (run(kGroups[i])) std::printf("GROUP %s: SKIPPED (no readable result document)\n", kGroups[i]);
        return 1;
    }
    if (run("result_schema")) { Report r; group_result_schema(res, r); print("result_schema", r); all_ok = all_ok && r.ok; }
    if (run("placements")) { Report r; group_placements(ly, res, r); print("placements", r); all_ok = all_ok && r.ok; }
    if (run("scale_pairs")) { Report r; group_scale_pairs(ck, ly, res, r); print("scale_pairs", r); all_ok = all_ok && r.ok; }
    if (run("history_shape")) { Report r; group_history_shape(ck, ly, res, r); print("history_shape", r); all_ok = all_ok && r.ok; }
    if (run("ordering")) { Report r; group_ordering(res, r); print("ordering", r); all_ok = all_ok && r.ok; }
    if (run("model_step")) { Report r; group_model_step(res, r); print("model_step", r); all_ok = all_ok && r.ok; }
    return all_ok ? 0 : 1;
}
