#!/usr/bin/env python3
"""Adversarial population (slots 03-08), partial-fix battery and development
snapshots for the reduction-plan task, each a candidate `recover.cpp` derived
from the reference by one targeted, plausible change."""
import os, shutil
HERE = os.path.dirname(os.path.abspath(__file__))
REF = os.path.join(HERE, "..", "pkg", "solution", "fixed", "src", "recover.cpp")
STARTER = os.path.join(HERE, "..", "pkg", "environment", "src", "recover.cpp")

OCC = "                    free = start + occ;\n                    q.ready = free + l.latency;"
START = "                    const std::int64_t start = std::max(q.ready, free);"
SORT_PAY = """                std::sort(outgoing.begin(), outgoing.end(), [](const Payload& a, const Payload& b) {
                    if (a.ready != b.ready) return a.ready < b.ready;
                    return *a.origins.begin() < *b.origins.begin();
                });"""
TWO = "            if (pay.size() < 2 || B > c.switches.at(node)->accumulator_capacity) { illegal = true; return pay; }"
SUFFIX = "            if (pit != parent.end() && pit->second != lid) return false;"
SIMPLE = "            if (it == c.links.end() || it->second->from != cur || seen.count(it->second->to)) return false;"
SEL = "        if (it == c.switches.end() || !it->second->reduction_capable || !parent.count(cid) || cid == p.sink) return false;"
FOLD = "    for (++it; it != origins.end(); ++it) acc = sat_add(acc, c.values.at(*it));"
FOLD_INIT = "    auto it = origins.begin();\n    std::int64_t acc = c.values.at(*it);"
TELE = """    for (const auto& [lid, b] : p.link_bytes) {
        auto it = r.link_bytes.find(lid);
        if ((it == r.link_bytes.end() ? 0 : it->second) != b) return false;
    }
    return !p.completion || *p.completion == r.completion;"""
KEY = "                auto key = std::make_tuple(r.completion, r.allocation, r.total_bytes, plan_key(c, combo, combine));"
KEY_FN = "    for (const auto& path : paths) for (const auto& lid : path) k.first.push_back({c.links.at(lid)->from, lid});"
MASKS = "            for (std::size_t mask = 0; mask < (std::size_t{1} << cands.size()); ++mask) {"
TREECHECK = "                if (pit != parent.end() && pit->second != lid) { tree = false; break; }"
VALID_RATE = "    for (const auto& l : p.links) if (l.rate < 1 || l.latency < 0 || l.id.empty() || l.from.empty() || l.to.empty()) return false;"
VALID_SINK = "    if (endpoints.count(p.sink) || wids.count(p.sink)) return false;"
VALID_DUP = "    for (const auto& l : p.links) if (!lids.insert(l.id).second) return false;"
INFEAS = '    if (!found) return Result::infeasible("no legal plan reproduces the telemetry");'
PATHS_SORTED = "    std::sort(sources.begin(), sources.end(), [](const Source* a, const Source* b) { return a->id < b->id; });"
DFS_SORT = "    for (auto& [n, v] : adj) std::sort(v.begin(), v.end(), [](const Link* a, const Link* b) { return a->id < b->id; });"

V = {}
def v(name, family, desc, patches): V[name] = (family, desc, patches)

# ---------------------------------------------------------------- population
v("p03_latency_in_occupancy", "03 link model", "a link stays busy until the payload has arrived (latency counted in the occupancy)",
  [(OCC, "                    free = start + occ + l.latency;\n                    q.ready = free;")])
v("p04_per_source_shortest_paths", "04 routing", "each source takes its own fastest path on idle links; only the combine subset is searched",
  [("        options.push_back(std::move(ps));", """        std::sort(ps.begin(), ps.end(), [&](const std::vector<std::string>& x, const std::vector<std::string>& y) {
            auto cost = [&](const std::vector<std::string>& path) { std::int64_t t = 0; for (const auto& lid : path) { const Link& l = *c.links.at(lid); t += (problem.payload_bytes + l.rate - 1) / l.rate + l.latency; } return std::make_pair(t, path.size()); };
            return cost(x) < cost(y); });
        ps.resize(1);
        options.push_back(std::move(ps));""")])
v("p05_no_link_serialization", "05 link model", "payloads on a shared link are transmitted independently (link_free ignored)",
  [(START, "                    const std::int64_t start = q.ready; (void)free;")])
v("p06_combine_everywhere", "06 reduction choice", "selects every reduction-capable switch on the tree instead of searching subsets",
  [(MASKS, "            for (std::size_t mask = (std::size_t{1} << cands.size()) - 1; mask < (std::size_t{1} << cands.size()); ++mask) {")])
v("p07_sum_then_clamp", "07 arithmetic", "computes the sink value as one wide sum clamped once",
  [(FOLD_INIT, "    std::int64_t wide = 0;\n    auto it = origins.begin();\n    for (; it != origins.end(); ++it) wide += c.values.at(*it);\n    std::int64_t acc = std::max(kMin, std::min(kMax, wide));"), (FOLD, "")])
v("p08_objective_bytes_before_allocation", "08 objective", "orders plans by (completion, bytes, allocation, key)",
  [(KEY, "                auto key = std::make_tuple(r.completion, r.total_bytes, r.allocation, plan_key(c, combo, combine));")])

# ------------------------------------------------------------------ battery
v("b01_capacity_ignored", "reduction", "never checks the accumulator capacity",
  [(TWO, "            if (pay.size() < 2) { illegal = true; return pay; }")])
v("b02_one_payload_combine_allowed", "reduction", "allows a selected switch reached by one payload",
  [(TWO, "            if (B > c.switches.at(node)->accumulator_capacity) { illegal = true; return pay; }")])
v("b03_suffix_not_enforced", "routing", "accepts path sets that leave a node on different links",
  [(SUFFIX, "            (void)pit;"), (TREECHECK, "                (void)pit;")])
v("b04_zero_telemetry_ignored", "telemetry", "treats links observed with 0 bytes as unconstrained",
  [(TELE, TELE.replace("if ((it == r.link_bytes.end() ? 0 : it->second) != b) return false;", "if (b != 0 && (it == r.link_bytes.end() ? 0 : it->second) != b) return false;"))])
v("b05_completion_ignored", "telemetry", "never compares the observed completion",
  [(TELE, TELE.replace("return !p.completion || *p.completion == r.completion;", "return true;"))])
v("b06_bytes_first_objective", "objective", "minimises link bytes before completion",
  [(KEY, "                auto key = std::make_tuple(r.total_bytes, r.completion, r.allocation, plan_key(c, combo, combine));")])
v("b07_key_by_link_id_only", "objective", "plan_key compares link ids without the from-node",
  [(KEY_FN, "    for (const auto& path : paths) for (const auto& lid : path) k.first.push_back({std::string(), lid});")])
v("b08_key_input_order", "objective", "enumerates links in document order and keeps the first plan among equal objectives",
  [(DFS_SORT, "    (void)adj;"), (KEY, "                auto key = std::make_tuple(r.completion, r.allocation, r.total_bytes, Key{});")])
v("b09_never_combine", "reduction", "only considers plans without switch reductions",
  [(MASKS, "            for (std::size_t mask = 0; mask < 1; ++mask) {")])
v("b10_fold_in_document_order", "arithmetic", "folds values in the order sources appear in the document",
  [(FOLD_INIT, "    std::vector<std::string> order; for (const auto& s : p.sources) if (origins.count(s.id)) order.push_back(s.id);\n    auto it = order.begin();\n    std::int64_t acc = c.values.at(*it);"), (FOLD, "    for (++it; it != order.end(); ++it) acc = sat_add(acc, c.values.at(*it));")])
v("b11_rate_zero_accepted", "validity", "accepts a link rate of zero (division guarded) and a negative latency",
  [(VALID_RATE, "    for (const auto& l : p.links) if (l.rate < 0 || l.id.empty() || l.from.empty() || l.to.empty()) return false;"), ("const std::int64_t occ = (B + l.rate - 1) / l.rate;", "const std::int64_t occ = l.rate > 0 ? (B + l.rate - 1) / l.rate : B;")])
v("b12_sink_kind_not_checked", "validity", "does not reject a sink that is also an endpoint or switch",
  [(VALID_SINK, "    (void)0;")])
v("b13_duplicate_link_ids_accepted", "validity", "accepts duplicate link ids (last one wins)",
  [(VALID_DUP, "    for (const auto& l : p.links) lids.insert(l.id);")])
v("b14_invalid_reported_as_infeasible", "statuses", "reports validity failures as infeasible",
  [('    if (!valid(problem)) return Result::invalid("problem fails validity");', '    if (!valid(problem)) return Result::infeasible("problem fails validity");')])
v("b15_infeasible_reported_as_invalid", "statuses", "reports an empty match set as invalid",
  [(INFEAS, '    if (!found) return Result::invalid("no legal plan reproduces the telemetry");')])
v("b16_paths_in_document_order", "output", "orders paths by the sources' document order",
  [(PATHS_SORTED, "    (void)0;")])
v("b17_tie_by_document_position", "link model", "serializes equal-ready payloads by the source's document position (no observable effect by construction)",
  [(SORT_PAY, """                std::sort(outgoing.begin(), outgoing.end(), [&](const Payload& a, const Payload& b) {
                    if (a.ready != b.ready) return a.ready < b.ready;
                    auto pos = [&](const std::string& id) { std::size_t i = 0; for (const auto& s : c.p.sources) { if (s.id == id) return i; ++i; } return i; };
                    std::size_t pa = pos(*a.origins.begin()), pb = pos(*b.origins.begin());
                    for (const auto& o : a.origins) pa = std::min(pa, pos(o));
                    for (const auto& o : b.origins) pb = std::min(pb, pos(o));
                    return pa < pb;
                });""")])
v("b18_non_simple_paths_allowed", "routing", "does not reject a path that revisits a node",
  [(SIMPLE, "            if (it == c.links.end() || it->second->from != cur) return false;")])
v("b19_unvisited_switch_selectable", "reduction", "allows selecting a switch no path visits",
  [(SEL, "        if (it == c.switches.end() || !it->second->reduction_capable || cid == p.sink) return false;"),
   ("            for (const auto& w : capable) if (parent.count(w)) cands.push_back(w);", "            for (const auto& w : capable) cands.push_back(w);")])
v("b20_occupancy_floor", "link model", "computes the occupancy as floor(bytes / rate)",
  [("const std::int64_t occ = (B + l.rate - 1) / l.rate;", "const std::int64_t occ = B / l.rate;")])
v("b21_allocation_counts_payloads", "reduction", "charges the accumulator per participating payload instead of once",
  [("            r.allocation += B;", "            r.allocation += B * static_cast<std::int64_t>(pay.size());")])
v("b22_sink_value_from_arrival_order", "arithmetic", "folds the sink value in payload arrival order",
  [(FOLD_INIT, "    std::vector<std::string> order; for (const Payload& q : at_sink) for (const auto& o : q.origins) order.push_back(o);\n    auto it = order.begin();\n    std::int64_t acc = c.values.at(*it);"), (FOLD, "    for (++it; it != order.end(); ++it) acc = sat_add(acc, c.values.at(*it));")])

# ---------------------------------------------------------------- snapshots
NO_SERIAL = V["p05_no_link_serialization"][2]
LAT_OCC = V["p03_latency_in_occupancy"][2]
PER_SOURCE = V["p04_per_source_shortest_paths"][2]
NEVER = V["b09_never_combine"][2]
NO_KEY = [(KEY, "                auto key = std::make_tuple(r.completion, r.allocation, r.total_bytes, Key{});")]
NO_TELE = [(TELE, "    (void)p; (void)r;\n    return true;")]
SNAPSHOTS = {
    "S1_replay_without_serialization": NO_SERIAL + PER_SOURCE + NEVER + NO_KEY + NO_TELE,
    "S2_serialized_replay": PER_SOURCE + NEVER + NO_KEY + NO_TELE,
    "S3_per_source_routes_no_combine": NEVER + NO_KEY + NO_TELE,   # global paths (product) but never combining, telemetry ignored
    "S4_combine_search": NO_KEY + NO_TELE,
    "S5_telemetry_completion_only": [(KEY, "                auto key = std::make_tuple(r.completion, std::int64_t{0}, std::int64_t{0}, Key{});")],
    "S6_objective_without_key": NO_KEY,
    "S7_final": [],
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
    print("variants:", len(V), "snapshots:", len(SNAPSHOTS) + 1)

if __name__ == "__main__":
    main()
