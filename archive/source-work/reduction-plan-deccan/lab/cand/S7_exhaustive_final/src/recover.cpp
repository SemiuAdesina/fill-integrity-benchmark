// Plan recovery — reference implementation (architecture A: path product).
//
// 1. validity (problem-format-v1.md section 3);
// 2. enumerate every simple path per source, take the product, keep the
//    suffix-consistent combinations (a tree), and for each tree every subset
//    of the reduction-capable switches it visits;
// 3. replay each plan with a recursive bottom-up fold of the tree
//    (fabric-replay-v1.md), discarding illegal plans;
// 4. keep the plans that reproduce the telemetry and take the minimum of
//    (completion, allocation, total bytes, plan_key).
#include "fabric/model.hpp"

#include <algorithm>
#include <functional>
#include <map>
#include <set>
#include <vector>

namespace fabric {

namespace {

constexpr std::int64_t kMin = -(std::int64_t{1} << 31), kMax = (std::int64_t{1} << 31) - 1;

std::int64_t sat_add(std::int64_t a, std::int64_t b) { return std::max(kMin, std::min(kMax, a + b)); }

struct Ctx {
    const Problem& p;
    std::map<std::string, const Link*> links;
    std::map<std::string, const Switch*> switches;
    std::vector<const Source*> sources;  // ascending id
    std::map<std::string, std::int64_t> values;
    explicit Ctx(const Problem& problem) : p(problem) {
        for (const auto& l : p.links) links[l.id] = &l;
        for (const auto& w : p.switches) switches[w.id] = &w;
        for (const auto& s : p.sources) { sources.push_back(&s); values[s.id] = s.value; }
        std::sort(sources.begin(), sources.end(), [](const Source* a, const Source* b) { return a->id < b->id; });
    }
};

bool valid(const Problem& p) {
    if (p.payload_bytes < 1 || p.sources.empty() || p.sink.empty()) return false;
    for (const auto& l : p.links) if (l.rate < 1 || l.latency < 0 || l.id.empty() || l.from.empty() || l.to.empty()) return false;
    for (const auto& w : p.switches) if (w.accumulator_capacity < 0 || w.id.empty()) return false;
    for (const auto& s : p.sources) if (s.id.empty() || s.node.empty() || s.value < kMin || s.value > kMax) return false;
    std::set<std::string> sids, wids, lids, endpoints;
    for (const auto& s : p.sources) { if (!sids.insert(s.id).second) return false; endpoints.insert(s.node); }
    for (const auto& w : p.switches) if (!wids.insert(w.id).second) return false;
    for (const auto& l : p.links) if (!lids.insert(l.id).second) return false;
    if (endpoints.count(p.sink) || wids.count(p.sink)) return false;
    for (const auto& e : endpoints) if (wids.count(e)) return false;
    std::set<std::string> nodes = endpoints;
    nodes.insert(wids.begin(), wids.end());
    nodes.insert(p.sink);
    for (const auto& l : p.links) if (!nodes.count(l.from) || !nodes.count(l.to) || l.from == l.to) return false;
    return true;
}

struct Payload { std::int64_t ready; std::set<std::string> origins; };

struct Replay {
    std::int64_t completion{0}, allocation{0}, total_bytes{0}, sink_value{0};
    std::map<std::string, std::int64_t> link_bytes;
};

/// Replays a plan; returns false when the plan is illegal.
bool replay(const Ctx& c, const std::vector<std::vector<std::string>>& paths, const std::vector<std::string>& combine, Replay& out) {
    const Problem& p = c.p;
    if (paths.size() != c.sources.size()) return false;
    std::map<std::string, std::string> parent;
    for (std::size_t i = 0; i < paths.size(); ++i) {
        std::string cur = c.sources[i]->node;
        std::set<std::string> seen{cur};
        if (paths[i].empty()) return false;
        for (const auto& lid : paths[i]) {
            auto it = c.links.find(lid);
            if (it == c.links.end() || it->second->from != cur || seen.count(it->second->to)) return false;
            auto pit = parent.find(cur);
            if (pit != parent.end() && pit->second != lid) return false;
            parent[cur] = lid;
            seen.insert(it->second->to);
            cur = it->second->to;
        }
        if (cur != p.sink) return false;
    }
    for (std::size_t i = 1; i < combine.size(); ++i) if (!(combine[i - 1] < combine[i])) return false;
    std::set<std::string> cset(combine.begin(), combine.end());
    for (const auto& cid : combine) {
        auto it = c.switches.find(cid);
        if (it == c.switches.end() || !it->second->reduction_capable || !parent.count(cid) || cid == p.sink) return false;
    }
    std::map<std::string, std::vector<std::pair<std::string, std::string>>> children;  // node -> (child, link)
    for (const auto& [node, lid] : parent) children[c.links.at(lid)->to].push_back({node, lid});
    const std::int64_t B = p.payload_bytes;
    Replay r;
    bool illegal = false;
    std::function<std::vector<Payload>(const std::string&)> process = [&](const std::string& node) {
        std::vector<Payload> pay;
        for (const Source* s : c.sources) if (s->node == node) pay.push_back(Payload{0, {s->id}});
        auto ch = children.find(node);
        if (ch != children.end()) {
            std::sort(ch->second.begin(), ch->second.end());
            for (const auto& [child, lid] : ch->second) {
                const Link& l = *c.links.at(lid);
                std::vector<Payload> outgoing = process(child);
                std::sort(outgoing.begin(), outgoing.end(), [](const Payload& a, const Payload& b) {
                    if (a.ready != b.ready) return a.ready < b.ready;
                    return *a.origins.begin() < *b.origins.begin();
                });
                std::int64_t free = 0;
                const std::int64_t occ = (B + l.rate - 1) / l.rate;
                for (Payload& q : outgoing) {
                    const std::int64_t start = std::max(q.ready, free);
                    free = start + occ;
                    q.ready = free + l.latency;
                    r.link_bytes[lid] += B;
                    pay.push_back(std::move(q));
                }
            }
        }
        if (cset.count(node)) {
            if (pay.size() < 2 || B > c.switches.at(node)->accumulator_capacity) { illegal = true; return pay; }
            r.allocation += B;
            Payload merged{0, {}};
            for (const Payload& q : pay) { merged.ready = std::max(merged.ready, q.ready); merged.origins.insert(q.origins.begin(), q.origins.end()); }
            return std::vector<Payload>{merged};
        }
        return pay;
    };
    std::vector<Payload> at_sink = process(p.sink);
    if (illegal || at_sink.empty()) return false;
    std::set<std::string> origins;
    for (const Payload& q : at_sink) { r.completion = std::max(r.completion, q.ready); origins.insert(q.origins.begin(), q.origins.end()); }
    auto it = origins.begin();
    std::int64_t acc = c.values.at(*it);
    for (++it; it != origins.end(); ++it) acc = sat_add(acc, c.values.at(*it));
    r.sink_value = acc;
    for (const auto& [lid, b] : r.link_bytes) r.total_bytes += b;
    out = r;
    return true;
}

bool matches_telemetry(const Problem& p, const Replay& r) {
    for (const auto& [lid, b] : p.link_bytes) {
        auto it = r.link_bytes.find(lid);
        if ((it == r.link_bytes.end() ? 0 : it->second) != b) return false;
    }
    return !p.completion || *p.completion == r.completion;
}

using Key = std::pair<std::vector<std::pair<std::string, std::string>>, std::vector<std::string>>;

Key plan_key(const Ctx& c, const std::vector<std::vector<std::string>>& paths, const std::vector<std::string>& combine) {
    Key k;
    for (const auto& path : paths) for (const auto& lid : path) k.first.push_back({c.links.at(lid)->from, lid});
    k.second = combine;
    return k;
}

void simple_paths(const Ctx& c, const std::string& start, std::vector<std::vector<std::string>>& out) {
    std::map<std::string, std::vector<const Link*>> adj;
    for (const auto& l : c.p.links) adj[l.from].push_back(&l);
    for (auto& [n, v] : adj) std::sort(v.begin(), v.end(), [](const Link* a, const Link* b) { return a->id < b->id; });
    std::vector<std::string> path;
    std::set<std::string> seen{start};
    std::function<void(const std::string&)> dfs = [&](const std::string& node) {
        if (node == c.p.sink) { out.push_back(path); return; }
        auto it = adj.find(node);
        if (it == adj.end()) return;
        for (const Link* l : it->second) {
            if (seen.count(l->to)) continue;
            seen.insert(l->to); path.push_back(l->id);
            dfs(l->to);
            path.pop_back(); seen.erase(l->to);
        }
    };
    dfs(start);
}

}  // namespace

Result recover(const Problem& problem) {
    if (!valid(problem)) return Result::invalid("problem fails validity");
    Ctx c(problem);
    std::vector<std::vector<std::vector<std::string>>> options;
    for (const Source* s : c.sources) {
        std::vector<std::vector<std::string>> ps;
        simple_paths(c, s->node, ps);
        if (ps.empty()) return Result::infeasible("a source cannot reach the sink");
        options.push_back(std::move(ps));
    }
    std::vector<std::string> capable;
    for (const auto& w : problem.switches) if (w.reduction_capable) capable.push_back(w.id);
    std::sort(capable.begin(), capable.end());
    bool found = false;
    std::tuple<std::int64_t, std::int64_t, std::int64_t, Key> best;
    std::vector<std::vector<std::string>> best_paths;
    std::vector<std::string> best_combine;
    std::int64_t best_value = 0;
    std::vector<std::size_t> idx(options.size(), 0);
    while (true) {
        std::vector<std::vector<std::string>> combo;
        for (std::size_t i = 0; i < options.size(); ++i) combo.push_back(options[i][idx[i]]);
        // suffix consistency, then the reduction-capable switches on the tree
        std::map<std::string, std::string> parent;
        bool tree = true;
        for (std::size_t i = 0; i < combo.size() && tree; ++i) {
            std::string cur = c.sources[i]->node;
            for (const auto& lid : combo[i]) {
                auto pit = parent.find(cur);
                if (pit != parent.end() && pit->second != lid) { tree = false; break; }
                parent[cur] = lid;
                cur = c.links.at(lid)->to;
            }
        }
        if (tree) {
            std::vector<std::string> cands;
            for (const auto& w : capable) if (parent.count(w)) cands.push_back(w);
            for (std::size_t mask = 0; mask < (std::size_t{1} << cands.size()); ++mask) {
                std::vector<std::string> combine;
                for (std::size_t k = 0; k < cands.size(); ++k) if (mask & (std::size_t{1} << k)) combine.push_back(cands[k]);
                Replay r;
                if (!replay(c, combo, combine, r) || !matches_telemetry(problem, r)) continue;
                auto key = std::make_tuple(r.completion, r.allocation, r.total_bytes, plan_key(c, combo, combine));
                if (!found || key < best) { found = true; best = key; best_paths = combo; best_combine = combine; best_value = r.sink_value; }
            }
        }
        std::size_t i = 0;
        while (i < idx.size() && ++idx[i] == options[i].size()) { idx[i] = 0; ++i; }
        if (i == idx.size()) break;
    }
    if (!found) return Result::infeasible("no legal plan reproduces the telemetry");
    return Result::ok(best_paths, best_combine, best_value);
}

}  // namespace fabric
