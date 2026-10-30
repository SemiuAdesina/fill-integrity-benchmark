#!/usr/bin/env python3
"""Adversarial population (slots 03-08), partial-fix battery and development
snapshots for the reduction-plan task, each a candidate `recover.cpp` derived
from the reference (pruned exact search) by one targeted, plausible change.
Version 2: anchors target the pruned-search reference; b03 (suffix not
enforced) and b18/b22 (non-simple paths, arrival-order fold) were retired
because the search never produces those shapes; b23/b24 (bound mistakes)
were added because the bound is where a pruned search goes wrong."""
import os, shutil
HERE = os.path.dirname(os.path.abspath(__file__))
REF = os.path.join(HERE, "..", "pkg", "solution", "fixed", "src", "recover.cpp")
STARTER = os.path.join(HERE, "..", "pkg", "environment", "src", "recover.cpp")
EXHAUSTIVE = os.path.join(HERE, "..", "archive", "v1-src", "recover_pathproduct.cpp")

OCC = "for (Payload& q : outgoing) { const std::int64_t start = std::max(q.ready, free); free = start + links[li].occ; q.ready = free + links[li].latency; pay.push_back(q); }"
SORT_PAY = "std::sort(outgoing.begin(), outgoing.end(), [](const Payload& a, const Payload& b) { return a.ready != b.ready ? a.ready < b.ready : a.origin < b.origin; });"
TWO = "if (pay.size() < 2 || !capable[node]) { o.legal = false; return pay; }"
CAPABLE = "for (const auto& w : p.switches) if (w.reduction_capable && w.accumulator_capacity >= B) capable[node_index.at(w.id)] = 1;"
LINK_CTOR = "links.push_back(L{l.id, node(l.from), node(l.to), (B + l.rate - 1) / l.rate, l.latency});"
FOLD = """        auto it = ids.begin();
        std::int64_t acc = values.at(*it);
        for (++it; it != ids.end(); ++it) acc = sat_add(acc, values.at(*it));"""
ZERO = "else if (b == 0) zero.insert(lid);"
BAD_ZERO = "if (b < 0 || b % B != 0) bad_bytes = true; else if (b == 0) zero.insert(lid);"
COUNT_REQ = "if (it != p.link_bytes.end() && it->second > 0 && it->second % B == 0) { count_req[i] = it->second / B; required.push_back(i); }"
CAP = "        cap = p.completion;"
TUP = "const auto tup = std::tie(o.completion, o.allocation, total, key);"
BEST = "if (!found || tup < std::tie(best_c, best_a, best_t, best_key)) {"
KEY_PAIR = "for (const auto& ip : ipaths) for (int li : ip) key.first.push_back({node_id[links[li].from], links[li].id});"
OUT_SORT = "for (auto& v : out) std::sort(v.begin(), v.end(), [&](int a, int b) { return links[a].id < links[b].id; });"
MASK = "for (std::size_t mask = 0; mask < (std::size_t{1} << visited.size()); ++mask) {"
VISITED = "for (int n = 0; n < static_cast<int>(node_id.size()); ++n) if (parent[n] >= 0 && capable[n]) visited.push_back(n);"
VALID_RATE = "    for (const auto& l : p.links) if (l.rate < 1 || l.latency < 0 || l.id.empty() || l.from.empty() || l.to.empty()) return false;"
VALID_SINK = "    if (endpoints.count(p.sink) || wids.count(p.sink)) return false;"
VALID_DUP = "    for (const auto& l : p.links) if (!lids.insert(l.id).second) return false;"
INVALID = '    if (!valid(problem)) return Result::invalid("problem fails validity");'
INFEAS = '        if (!found) return Result::infeasible("no legal plan reproduces the telemetry");'
SRC_SORT = "std::sort(sources.begin(), sources.end(), [](const Source* a, const Source* b) { return a->id < b->id; });"
ALLOC = "o.allocation += B;"
SCORED = "for (const auto& [cost, k] : scored) {"
BOUND_REPLAY = "const Outcome o = replay(active, true, {});"
BOUND_CMP = "if (o.legal && lb <= bound_cap() && telemetry_consistent(active, i + 1)) dfs(i + 1, active);"
TIE_SORT = """            return pa.size() < pb.size();
        });"""

V = {}
def v(name, family, desc, patches): V[name] = (family, desc, patches)

# ---------------------------------------------------------------- population
v("p03_latency_in_occupancy", "03 link model", "a link stays busy until the payload has arrived (latency counted in the occupancy)",
  [(OCC, "for (Payload& q : outgoing) { const std::int64_t start = std::max(q.ready, free); free = start + links[li].occ + links[li].latency; q.ready = free; pay.push_back(q); }")])
v("p04_per_source_shortest_paths", "04 routing", "each source takes its fastest idle-link path consistent with the tree so far; only the combine subset is searched",
  [(SCORED, "scored.resize(1);\n        for (const auto& [cost, k] : scored) {")])
v("p05_no_link_serialization", "05 link model", "payloads on a shared link are transmitted independently (link_free ignored)",
  [(OCC, "for (Payload& q : outgoing) { const std::int64_t start = q.ready; free = start + links[li].occ; q.ready = free + links[li].latency; pay.push_back(q); }")])
v("p06_combine_everywhere", "06 reduction choice", "selects every reduction-capable switch on the tree instead of searching subsets",
  [(MASK, "for (std::size_t mask = (std::size_t{1} << visited.size()) - 1; mask < (std::size_t{1} << visited.size()); ++mask) {")])
v("p07_sum_then_clamp", "07 arithmetic", "computes the sink value as one wide sum clamped once",
  [(FOLD, "        std::int64_t wide = 0;\n        for (const auto& id : ids) wide += values.at(id);\n        std::int64_t acc = std::max(kMin, std::min(kMax, wide));")])
v("p08_objective_bytes_before_allocation", "08 objective", "orders plans by (completion, bytes, allocation, key)",
  [(TUP, "const auto tup = std::tie(o.completion, total, o.allocation, key);"), (BEST, "if (!found || tup < std::tie(best_c, best_t, best_a, best_key)) {")])

# ------------------------------------------------------------------ battery
v("b01_capacity_ignored", "reduction", "never checks the accumulator capacity",
  [(CAPABLE, "for (const auto& w : p.switches) if (w.reduction_capable) capable[node_index.at(w.id)] = 1;")])
v("b02_one_payload_combine_allowed", "reduction", "allows a selected switch reached by one payload",
  [(TWO, "if (!capable[node]) { o.legal = false; return pay; }"), ("if (selected[ch]) { if (cc < 2) illegal = true; cnt[parent[ch]] = 1; c += 1; }", "if (selected[ch]) { cnt[parent[ch]] = 1; c += 1; }")])
v("b04_zero_telemetry_ignored", "telemetry", "treats links observed with 0 bytes as unconstrained",
  [(ZERO, "else if (b == 0) (void)lid;")])
v("b05_completion_ignored", "telemetry", "never compares the observed completion",
  [(CAP, "        cap = std::nullopt;")])
v("b06_bytes_first_objective", "objective", "minimises link bytes before completion",
  [(TUP, "const auto tup = std::tie(total, o.completion, o.allocation, key);"), (BEST, "if (!found || tup < std::tie(best_t, best_c, best_a, best_key)) {")])
v("b07_key_by_link_id_only", "objective", "plan_key compares link ids without the from-node",
  [(KEY_PAIR, "for (const auto& ip : ipaths) for (int li : ip) key.first.push_back({std::string(), links[li].id});")])
v("b08_key_input_order", "objective", "enumerates links in document order and keeps the first plan among equal objectives",
  [(OUT_SORT, "(void)0;"), (TIE_SORT, "            return a.second < b.second;\n        });"),
   (TUP, "Key nokey; const auto tup = std::tie(o.completion, o.allocation, total, nokey);"), (BEST, "if (!found || tup < std::tie(best_c, best_a, best_t, nokey)) {")])
v("b09_never_combine", "reduction", "only considers plans without switch reductions",
  [(MASK, "for (std::size_t mask = 0; mask < 1; ++mask) {")])
v("b10_fold_in_document_order", "arithmetic", "folds values in the order sources appear in the document",
  [(FOLD, "        std::vector<std::string> order; for (const auto& s : p.sources) order.push_back(s.id);\n        auto it = order.begin();\n        std::int64_t acc = values.at(*it);\n        for (++it; it != order.end(); ++it) acc = sat_add(acc, values.at(*it));")])
v("b11_rate_zero_accepted", "validity", "accepts a link rate of zero (division guarded) and a negative latency",
  [(VALID_RATE, "    for (const auto& l : p.links) if (l.rate < 0 || l.id.empty() || l.from.empty() || l.to.empty()) return false;"),
   (LINK_CTOR, "links.push_back(L{l.id, node(l.from), node(l.to), l.rate > 0 ? (B + l.rate - 1) / l.rate : B, l.latency});")])
v("b12_sink_kind_not_checked", "validity", "does not reject a sink that is also an endpoint or switch",
  [(VALID_SINK, "    (void)0;")])
v("b13_duplicate_link_ids_accepted", "validity", "accepts duplicate link ids (last one wins)",
  [(VALID_DUP, "    for (const auto& l : p.links) lids.insert(l.id);")])
v("b14_invalid_reported_as_infeasible", "statuses", "reports validity failures as infeasible",
  [(INVALID, '    if (!valid(problem)) return Result::infeasible("problem fails validity");')])
v("b15_infeasible_reported_as_invalid", "statuses", "reports an empty match set as invalid",
  [(INFEAS, '        if (!found) return Result::invalid("no legal plan reproduces the telemetry");')])
v("b16_paths_in_document_order", "output", "orders paths by the sources' document order",
  [(SRC_SORT, "(void)0;")])
v("b17_tie_by_document_position", "link model", "serializes equal-ready payloads by the origin's document position (no observable effect by construction)",
  [(SORT_PAY, "auto dp = [&](int si) { std::size_t k = 0; for (const auto& s : p.sources) { if (&s == sources[si]) return k; ++k; } return k; };\n                std::sort(outgoing.begin(), outgoing.end(), [&](const Payload& a, const Payload& b) { return a.ready != b.ready ? a.ready < b.ready : dp(a.origin) < dp(b.origin); });")])
v("b19_unvisited_switch_selectable", "reduction", "offers every reduction-capable switch to the subset search, visited or not",
  [(VISITED, "for (int n = 0; n < static_cast<int>(node_id.size()); ++n) if (capable[n]) visited.push_back(n);")])
v("b20_occupancy_floor", "link model", "computes the occupancy as floor(bytes / rate)",
  [(LINK_CTOR, "links.push_back(L{l.id, node(l.from), node(l.to), B / l.rate, l.latency});")])
v("b21_allocation_counts_payloads", "reduction", "charges the accumulator per participating payload instead of once",
  [(ALLOC, "o.allocation += B * static_cast<std::int64_t>(pay.size());")])
v("b23_bound_without_reductions", "search bound", "bounds a partial tree by its completion without any reduction (not a lower bound: a reduction can lower completion)",
  [(BOUND_REPLAY, "const Outcome o = replay(active, false, std::vector<char>(node_id.size(), 0));")])
v("b24_bound_strict", "search bound", "abandons a partial tree whose bound equals the best completion (loses ties decided by allocation, bytes or key)",
  [(BOUND_CMP, "if (o.legal && lb < bound_cap() && telemetry_consistent(active, i + 1)) dfs(i + 1, active);")])

# ---------------------------------------------------------------- snapshots
NO_SERIAL = V["p05_no_link_serialization"][2]
PER_SOURCE = V["p04_per_source_shortest_paths"][2]
NEVER = V["b09_never_combine"][2]
NO_KEY = [(TUP, "Key nokey; const auto tup = std::tie(o.completion, o.allocation, total, nokey);"), (BEST, "if (!found || tup < std::tie(best_c, best_a, best_t, nokey)) {")]
NO_LINK_TELE = [(BAD_ZERO, "(void)b; (void)lid;"), (COUNT_REQ, "if (false) { count_req[i] = it->second / B; required.push_back(i); }")]
NO_TELE = NO_LINK_TELE + [(CAP, "        cap = std::nullopt;")]
SNAPSHOTS = {
    "S1_replay_without_serialization": NO_SERIAL + PER_SOURCE + NEVER + NO_KEY + NO_TELE,
    "S2_serialized_replay": PER_SOURCE + NEVER + NO_KEY + NO_TELE,
    "S3_per_source_routes_no_combine": NEVER + NO_KEY + NO_TELE,
    "S4_combine_search": NO_KEY + NO_TELE,
    "S5_telemetry_completion_only": NO_KEY + NO_LINK_TELE,
    "S6_objective_without_key": NO_KEY,
    "S8_pruned_search_final": [],
}

def apply(text, patches, name):
    for old, new in patches:
        if text.count(old) != 1: raise SystemExit("%s: pattern not unique (%d): %s" % (name, text.count(old), old[:70]))
        text = text.replace(old, new)
    return text

def main():
    ref = open(REF).read()
    out = os.path.join(HERE, "variants"); shutil.rmtree(out, ignore_errors=True)
    for name, (family, desc, patches) in V.items():
        d = os.path.join(out, name); os.makedirs(d)
        open(os.path.join(d, "recover.cpp"), "w").write(apply(ref, patches, name))
        open(os.path.join(d, "VARIANT.txt"), "w").write("%s\n%s\n%s\n" % (name, family, desc))
    snaps = os.path.join(HERE, "snapshots"); shutil.rmtree(snaps, ignore_errors=True)
    for name, patches in SNAPSHOTS.items():
        d = os.path.join(snaps, name); os.makedirs(d)
        open(os.path.join(d, "recover.cpp"), "w").write(apply(ref, patches, name))
    d = os.path.join(snaps, "S0_starter"); os.makedirs(d); shutil.copy(STARTER, os.path.join(d, "recover.cpp"))
    d = os.path.join(snaps, "S7_exhaustive_final"); os.makedirs(d); shutil.copy(EXHAUSTIVE, os.path.join(d, "recover.cpp"))
    print("variants:", len(V), "snapshots:", len(SNAPSHOTS) + 2)

if __name__ == "__main__":
    main()
