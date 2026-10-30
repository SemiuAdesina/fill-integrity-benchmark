// Plan recovery — reference implementation.
//
// 1. validity (problem-format-v1.md section 3); telemetry whose bytes are not a
//    whole number of payloads, or a source that cannot reach the sink, is
//    infeasible at once;
// 2. an exact search over suffix-consistent trees: sources are routed one at
//    a time, each along every simple path that stays off the partial tree
//    until it joins it; a partial tree is abandoned when even its most
//    favourable completion (every capable switch that gathers two payloads
//    reducing, since a reduction never delays anything downstream) exceeds
//    the completion the telemetry lists or the best plan found so far, or
//    when a listed link can no longer be reproduced;
// 3. a complete tree is replayed with every subset of its capable switches
//    (fabric-replay-v1.md), after a payload-count filter against the listed
//    links; plans that reproduce the telemetry are ranked by
//    (completion, allocation, total bytes, plan_key) and the minimum kept.
#include "fabric/model.hpp"

#include <algorithm>
#include <functional>
#include <limits>
#include <optional>
#include <map>
#include <queue>
#include <set>
#include <string>
#include <utility>
#include <vector>

namespace fabric {

namespace {

constexpr std::int64_t kMin = -(std::int64_t{1} << 31), kMax = (std::int64_t{1} << 31) - 1;
constexpr std::int64_t kInf = std::numeric_limits<std::int64_t>::max() / 4;

std::int64_t sat_add(std::int64_t a, std::int64_t b) { return std::max(kMin, std::min(kMax, a + b)); }

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

using Key = std::pair<std::vector<std::pair<std::string, std::string>>, std::vector<std::string>>;

struct Search {
    const Problem& p;
    const std::int64_t B;
    // nodes and links by index
    std::vector<std::string> node_id;
    std::map<std::string, int> node_index;
    int sink{-1};
    struct L { std::string id; int from, to; std::int64_t occ, latency; };
    std::vector<L> links;                       // document order
    std::vector<std::vector<int>> out;          // per node, links not listed as zero, ascending link id
    std::vector<const Source*> sources;         // ascending id
    std::vector<int> source_node;
    std::vector<std::vector<int>> sources_at;   // per node, source indices ascending id
    std::vector<char> capable;                  // reduction-capable with capacity >= B
    std::map<int, std::int64_t> count_req;      // link -> required payload count
    std::vector<int> required;
    std::vector<std::int64_t> dist;             // unloaded shortest cost to the sink
    std::vector<int> order;                     // routing order: critical sources first
    bool bad_bytes{false};
    std::optional<std::int64_t> cap;

    // state
    std::vector<int> parent;                    // node -> link index or -1
    bool found{false};
    std::int64_t best_c{0}, best_a{0}, best_t{0};
    Key best_key;
    std::vector<std::vector<std::string>> best_paths;
    std::vector<std::string> best_combine;

    explicit Search(const Problem& problem) : p(problem), B(problem.payload_bytes) {
        auto node = [&](const std::string& n) { auto it = node_index.find(n); if (it != node_index.end()) return it->second; int i = static_cast<int>(node_id.size()); node_id.push_back(n); node_index[n] = i; return i; };
        sink = node(p.sink);
        for (const auto& s : p.sources) node(s.node);
        for (const auto& w : p.switches) node(w.id);
        std::set<std::string> zero;
        for (const auto& [lid, b] : p.link_bytes) { if (b < 0 || b % B != 0) bad_bytes = true; else if (b == 0) zero.insert(lid); }
        for (const auto& l : p.links) links.push_back(L{l.id, node(l.from), node(l.to), (B + l.rate - 1) / l.rate, l.latency});
        const int V = static_cast<int>(node_id.size());
        out.assign(V, {});
        for (int i = 0; i < static_cast<int>(links.size()); ++i) if (!zero.count(links[i].id)) out[links[i].from].push_back(i);
        for (auto& v : out) std::sort(v.begin(), v.end(), [&](int a, int b) { return links[a].id < links[b].id; });
        for (const auto& s : p.sources) sources.push_back(&s);
        std::sort(sources.begin(), sources.end(), [](const Source* a, const Source* b) { return a->id < b->id; });
        sources_at.assign(V, {});
        for (int i = 0; i < static_cast<int>(sources.size()); ++i) { source_node.push_back(node_index.at(sources[i]->node)); sources_at[source_node.back()].push_back(i); }
        capable.assign(V, 0);
        for (const auto& w : p.switches) if (w.reduction_capable) capable[node_index.at(w.id)] = 1;
        for (int i = 0; i < static_cast<int>(links.size()); ++i) { auto it = p.link_bytes.find(links[i].id); if (it != p.link_bytes.end() && it->second > 0 && it->second % B == 0) { count_req[i] = it->second / B; required.push_back(i); } }
        cap = p.completion;
        shortest();
        for (int i = 0; i < static_cast<int>(sources.size()); ++i) order.push_back(i);
        std::sort(order.begin(), order.end(), [&](int a, int b) { const auto da = dist[source_node[a]], db = dist[source_node[b]]; if (da != db) return da > db; return sources[a]->id < sources[b]->id; });
        parent.assign(V, -1);
    }

    void shortest() {
        const int V = static_cast<int>(node_id.size());
        std::vector<std::vector<std::pair<int, std::int64_t>>> rev(V);
        for (int n = 0; n < V; ++n) for (int li : out[n]) rev[links[li].to].push_back({n, links[li].occ + links[li].latency});
        dist.assign(V, kInf);
        dist[sink] = 0;
        std::priority_queue<std::pair<std::int64_t, int>, std::vector<std::pair<std::int64_t, int>>, std::greater<>> pq;
        pq.push({0, sink});
        while (!pq.empty()) {
            auto [d, n] = pq.top(); pq.pop();
            if (d > dist[n]) continue;
            for (const auto& [m, w] : rev[n]) if (d + w < dist[m]) { dist[m] = d + w; pq.push({dist[m], m}); }
        }
    }

    // ------------------------------------------------------------ replay
    struct Payload { std::int64_t ready; int origin; };  // origin: smallest source index in the set
    struct Outcome { std::int64_t completion{0}, allocation{0}; std::vector<std::int64_t> counts; bool legal{true}; };

    /// Replays the partial tree restricted to the sources in `active`; `all` selects
    /// every capable switch that gathers two or more payloads, otherwise `selected`
    /// is the combine set (illegal when a selected switch gathers fewer than two).
    Outcome replay(const std::vector<char>& active, bool all, const std::vector<char>& selected) const {
        const int V = static_cast<int>(node_id.size());
        std::vector<std::vector<std::pair<int, int>>> children(V);  // (child, link)
        for (int n = 0; n < V; ++n) if (parent[n] >= 0) children[links[parent[n]].to].push_back({n, parent[n]});
        Outcome o; o.counts.assign(links.size(), 0);
        std::function<std::vector<Payload>(int)> process = [&](int node) -> std::vector<Payload> {
            std::vector<Payload> pay;
            for (int si : sources_at[node]) if (active[si]) pay.push_back(Payload{0, si});
            std::sort(children[node].begin(), children[node].end());
            for (const auto& [child, li] : children[node]) {
                std::vector<Payload> outgoing = process(child);
                if (!o.legal) return pay;
                std::sort(outgoing.begin(), outgoing.end(), [](const Payload& a, const Payload& b) { return a.ready != b.ready ? a.ready < b.ready : a.origin < b.origin; });
                std::int64_t free = 0;
                for (Payload& q : outgoing) { const std::int64_t start = std::max(q.ready, free); free = start + links[li].occ; q.ready = free + links[li].latency; pay.push_back(q); }
                o.counts[li] += static_cast<std::int64_t>(outgoing.size());
            }
            const bool select = all ? (capable[node] && pay.size() >= 2 && node != sink) : (selected[node] != 0);
            if (select) {
                if (pay.size() < 2 || !capable[node]) { o.legal = false; return pay; }
                o.allocation += B;
                Payload merged{0, std::numeric_limits<int>::max()};
                for (const Payload& q : pay) { merged.ready = std::max(merged.ready, q.ready); merged.origin = std::min(merged.origin, q.origin); }
                return std::vector<Payload>{merged};
            }
            return pay;
        };
        std::vector<Payload> at_sink = process(sink);
        if (!o.legal || at_sink.empty()) { o.legal = false; return o; }
        for (const Payload& q : at_sink) o.completion = std::max(o.completion, q.ready);
        return o;
    }

    std::int64_t tree_cost(int node) const {
        std::int64_t c = 0;
        while (node != sink) { const int li = parent[node]; c += links[li].occ + links[li].latency; node = links[li].to; }
        return c;
    }

    // --------------------------------------------------------- telemetry
    bool telemetry_consistent(const std::vector<char>& active, int next) const {
        std::vector<char> used(links.size(), 0);
        for (int li : parent) if (li >= 0) used[li] = 1;
        std::vector<int> missing_tails;
        for (int li : required) {
            if (used[li]) continue;
            const int u = links[li].from;
            if (parent[u] >= 0) return false;
            missing_tails.push_back(u);
        }
        if (!missing_tails.empty()) {
            if (next >= static_cast<int>(order.size())) return false;
            std::vector<char> reach(node_id.size(), 0);
            std::vector<int> stack;
            for (int k = next; k < static_cast<int>(order.size()); ++k) { const int n = source_node[order[k]]; if (parent[n] < 0 && !reach[n]) { reach[n] = 1; stack.push_back(n); } }
            while (!stack.empty()) {
                const int n = stack.back(); stack.pop_back();
                for (int li : out[n]) { const int t = links[li].to; if (t == sink || parent[t] >= 0 || reach[t]) continue; reach[t] = 1; stack.push_back(t); }
            }
            for (int u : missing_tails) if (!reach[u]) return false;
        }
        if (count_req.empty()) return true;
        std::vector<std::int64_t> routed(links.size(), 0);
        for (int si = 0; si < static_cast<int>(sources.size()); ++si) if (active[si]) { int n = source_node[si]; while (n != sink) { routed[parent[n]] += 1; n = links[parent[n]].to; } }
        const std::int64_t remaining = static_cast<std::int64_t>(order.size()) - next;
        for (const auto& [li, k] : count_req) {
            if (!used[li]) continue;
            if (routed[li] + remaining < k) return false;
            const int u = links[li].from;
            if (!capable[u]) {
                std::int64_t branches = static_cast<std::int64_t>(sources_at[u].size());
                for (int n = 0; n < static_cast<int>(node_id.size()); ++n) if (parent[n] >= 0 && links[parent[n]].to == u) ++branches;
                if (branches > k) return false;
            }
        }
        return true;
    }

    // ------------------------------------------------------------ search
    std::int64_t bound_cap() const { if (cap) return *cap; return found ? best_c : kInf; }

    std::vector<int> follow(int node) const { std::vector<int> path; while (node != sink) { path.push_back(parent[node]); node = links[parent[node]].to; } return path; }

    void candidate_paths(int start, std::vector<std::vector<int>>& res) const {
        if (parent[start] >= 0) { res.push_back(follow(start)); return; }
        std::vector<int> prefix;
        std::vector<char> seen(node_id.size(), 0);
        seen[start] = 1;
        std::function<void(int)> dfs = [&](int node) {
            for (int li : out[node]) {
                const int to = links[li].to;
                if (seen[to]) continue;
                if (to == sink) { res.push_back(prefix); res.back().push_back(li); }
                else if (parent[to] >= 0) { res.push_back(prefix); res.back().push_back(li); const auto rest = follow(to); res.back().insert(res.back().end(), rest.begin(), rest.end()); }
                else { seen[to] = 1; prefix.push_back(li); dfs(to); prefix.pop_back(); seen[to] = 0; }
            }
        };
        dfs(start);
    }

    void dfs(int i, std::vector<char>& active) {
        if (i == static_cast<int>(order.size())) { leaf(); return; }
        const int si = order[i];
        std::vector<std::vector<int>> cands;
        candidate_paths(source_node[si], cands);
        std::vector<std::pair<std::int64_t, int>> scored;
        for (int k = 0; k < static_cast<int>(cands.size()); ++k) { std::int64_t c = 0; for (int li : cands[k]) c += links[li].occ + links[li].latency; scored.push_back({c, k}); }
        std::sort(scored.begin(), scored.end(), [&](const auto& a, const auto& b) {
            if (a.first != b.first) return a.first < b.first;
            const auto& pa = cands[a.second]; const auto& pb = cands[b.second];
            for (std::size_t k = 0; k < pa.size() && k < pb.size(); ++k) {
                if (pa[k] == pb[k]) continue;
                const auto& x = links[pa[k]]; const auto& y = links[pb[k]];
                if (node_id[x.from] != node_id[y.from]) return node_id[x.from] < node_id[y.from];
                return x.id < y.id;
            }
            return pa.size() < pb.size();
        });
        for (const auto& [cost, k] : scored) {
            const auto& path = cands[k];
            std::vector<int> added;
            int node = source_node[si];
            bool ok = true;
            for (int li : path) {
                if (parent[node] >= 0) { if (parent[node] != li) { ok = false; break; } }
                else { parent[node] = li; added.push_back(node); }
                node = links[li].to;
            }
            if (ok) {
                active[si] = 1;
                const Outcome o = replay(active, true, {});
                std::int64_t lb = o.completion;
                for (int r = i + 1; r < static_cast<int>(order.size()); ++r) { const int n = source_node[order[r]]; lb = std::max(lb, parent[n] >= 0 ? tree_cost(n) : dist[n]); }
                if (o.legal && lb <= bound_cap() && telemetry_consistent(active, i + 1)) dfs(i + 1, active);
                active[si] = 0;
            }
            for (int n : added) parent[n] = -1;
        }
    }

    void leaf() {
        std::vector<char> used(links.size(), 0);
        for (int li : parent) if (li >= 0) used[li] = 1;
        for (int li : required) if (!used[li]) return;
        std::vector<std::vector<std::string>> paths;
        std::vector<std::vector<int>> ipaths;
        for (int si = 0; si < static_cast<int>(sources.size()); ++si) { ipaths.push_back(follow(source_node[si])); std::vector<std::string> ids; for (int li : ipaths.back()) ids.push_back(links[li].id); paths.push_back(ids); }
        std::vector<int> visited;
        for (int n = 0; n < static_cast<int>(node_id.size()); ++n) if (parent[n] >= 0 && capable[n]) visited.push_back(n);
        std::sort(visited.begin(), visited.end(), [&](int a, int b) { return node_id[a] < node_id[b]; });
        const int V = static_cast<int>(node_id.size());
        std::vector<std::vector<int>> children(V);
        for (int n = 0; n < V; ++n) if (parent[n] >= 0) children[links[parent[n]].to].push_back(n);
        std::vector<char> active(sources.size(), 1), selected(V, 0);
        std::vector<std::int64_t> cnt(links.size(), 0);
        for (std::size_t mask = 0; mask < (std::size_t{1} << visited.size()); ++mask) {
            std::fill(selected.begin(), selected.end(), 0);
            for (std::size_t k = 0; k < visited.size(); ++k) if (mask & (std::size_t{1} << k)) selected[visited[k]] = 1;
            // payload counts without a replay
            bool illegal = false;
            std::function<std::int64_t(int)> count = [&](int n) -> std::int64_t {
                std::int64_t c = static_cast<std::int64_t>(sources_at[n].size());
                for (int ch : children[n]) {
                    const std::int64_t cc = count(ch);
                    if (selected[ch]) { if (cc < 2) illegal = true; cnt[parent[ch]] = 1; c += 1; }
                    else { cnt[parent[ch]] = cc; c += cc; }
                }
                return c;
            };
            count(sink);
            if (illegal) continue;
            bool mismatch = false;
            for (const auto& [li, k] : count_req) if (cnt[li] != k) { mismatch = true; break; }
            if (mismatch) continue;
            const Outcome o = replay(active, false, selected);
            if (!o.legal) continue;
            if (cap && o.completion != *cap) continue;
            for (const auto& [li, k] : count_req) if (o.counts[li] != k) { mismatch = true; break; }
            if (mismatch) continue;
            std::int64_t total = 0;
            for (std::int64_t c : o.counts) total += c * B;
            std::vector<std::string> combine;
            for (std::size_t k = 0; k < visited.size(); ++k) if (mask & (std::size_t{1} << k)) combine.push_back(node_id[visited[k]]);
            Key key;
            for (const auto& ip : ipaths) for (int li : ip) key.first.push_back({node_id[links[li].from], links[li].id});
            key.second = combine;
            const auto tup = std::tie(o.completion, o.allocation, total, key);
            if (!found || tup < std::tie(best_c, best_a, best_t, best_key)) {
                found = true; best_c = o.completion; best_a = o.allocation; best_t = total; best_key = key; best_paths = paths; best_combine = combine;
            }
        }
    }

    Result run() {
        if (bad_bytes) return Result::infeasible("listed bytes are not a whole number of payloads");
        for (int n : source_node) if (dist[n] >= kInf) return Result::infeasible("a source cannot reach the sink");
        std::vector<char> active(sources.size(), 0);
        dfs(0, active);
        if (!found) return Result::infeasible("no legal plan reproduces the telemetry");
        std::set<std::string> ids;
        for (const auto* s : sources) ids.insert(s->id);
        std::map<std::string, std::int64_t> values;
        for (const auto& s : p.sources) values[s.id] = s.value;
        auto it = ids.begin();
        std::int64_t acc = values.at(*it);
        for (++it; it != ids.end(); ++it) acc = sat_add(acc, values.at(*it));
        return Result::ok(best_paths, best_combine, acc);
    }
};

}  // namespace

Result recover(const Problem& problem) {
    if (!valid(problem)) return Result::invalid("problem fails validity");
    Search s(problem);
    return s.run();
}

}  // namespace fabric
