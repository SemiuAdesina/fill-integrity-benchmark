// Plan recovery — independent architecture D (parent-pointer trees built depth
// first, event-driven replay, pruned by the resolved endpoints' best completion).
//
// Differs from the reference in organisation: the tree is grown one routed
// node at a time (the target of the chosen link is routed next, so one
// endpoint resolves before the next starts; endpoints are taken furthest
// from the sink first) rather than one source path at a time; the replay is
// a discrete-event simulation with per-link queues batched per tick and a
// pre-computed expected-arrival count per selected switch; the bound
// simulates the resolved endpoints with every capable switch that gathers
// two payloads selected and adds, for every unresolved endpoint, the cost
// of its routed prefix plus the unloaded cost from the prefix's end; links
// are tried cheapest first so a good cap is found early;
// validity, telemetry, objective and plan_key are written independently.
// Payload origins are sorted index vectors; nodes and links are indices.
#include "fabric/model.hpp"

#include <algorithm>
#include <functional>
#include <limits>
#include <map>
#include <optional>
#include <queue>
#include <set>
#include <string>
#include <tuple>
#include <vector>

namespace fabric {

namespace {

constexpr std::int64_t I32MIN = -(std::int64_t{1} << 31), I32MAX = (std::int64_t{1} << 31) - 1;
constexpr std::int64_t BIG = std::numeric_limits<std::int64_t>::max() / 8;

bool problem_ok(const Problem& p) {
    if (p.payload_bytes <= 0 || p.sources.empty() || p.sink.empty()) return false;
    std::set<std::string> ends, sws, ids;
    for (const auto& s : p.sources) { if (s.id.empty() || s.node.empty() || s.value < I32MIN || s.value > I32MAX || !ids.insert("s:" + s.id).second) return false; ends.insert(s.node); }
    for (const auto& w : p.switches) { if (w.id.empty() || w.accumulator_capacity < 0 || !ids.insert("w:" + w.id).second) return false; sws.insert(w.id); }
    for (const auto& l : p.links) if (l.id.empty() || l.from.empty() || l.to.empty() || l.rate <= 0 || l.latency < 0 || !ids.insert("l:" + l.id).second) return false;
    if (ends.count(p.sink) || sws.count(p.sink)) return false;
    for (const auto& e : ends) if (sws.count(e)) return false;
    auto known = [&](const std::string& n) { return n == p.sink || ends.count(n) || sws.count(n); };
    for (const auto& l : p.links) if (!known(l.from) || !known(l.to) || l.from == l.to) return false;
    return true;
}

struct Planner {
    const Problem& p;
    const std::int64_t B;
    std::vector<std::string> node_id; std::map<std::string, int> node_idx; int sink{-1};
    struct L { std::string id; int from, to; std::int64_t occ, lat; };
    std::vector<L> links;
    std::vector<std::vector<int>> out;                 // usable links per node, ascending id
    std::vector<int> src_node;                         // sources ascending id -> node
    std::vector<std::int64_t> src_value;
    std::vector<std::vector<int>> at;                  // node -> source indices
    std::vector<int> ends;                             // endpoint nodes, furthest from the sink first
    std::vector<char> capable;
    std::vector<std::int64_t> need;                    // link -> required payload count, -1 when unlisted or zero
    std::vector<std::int64_t> dist;
    std::optional<std::int64_t> listed_completion;
    bool bad{false};
    std::vector<int> parent;                           // node -> link or -1
    // best
    bool found{false};
    std::tuple<std::int64_t, std::int64_t, std::int64_t, std::vector<std::pair<std::string, std::string>>, std::vector<std::string>> best;
    std::vector<std::vector<std::string>> best_paths; std::vector<std::string> best_combine; std::int64_t best_value{0};

    explicit Planner(const Problem& pr) : p(pr), B(pr.payload_bytes) {
        auto node = [&](const std::string& n) { auto it = node_idx.find(n); if (it != node_idx.end()) return it->second; const int i = static_cast<int>(node_id.size()); node_id.push_back(n); node_idx[n] = i; return i; };
        sink = node(p.sink);
        std::vector<const Source*> srcs; for (const auto& s : p.sources) srcs.push_back(&s);
        std::sort(srcs.begin(), srcs.end(), [](const Source* a, const Source* b) { return a->id < b->id; });
        for (const Source* s : srcs) node(s->node);
        for (const auto& w : p.switches) node(w.id);
        const int V = static_cast<int>(node_id.size());
        at.assign(V, {});
        for (int i = 0; i < static_cast<int>(srcs.size()); ++i) { src_node.push_back(node_idx.at(srcs[i]->node)); src_value.push_back(srcs[i]->value); at[src_node.back()].push_back(i); }
        std::set<std::string> zero;
        for (const auto& [lid, b] : p.link_bytes) { if (b < 0 || b % B != 0) bad = true; else if (b == 0) zero.insert(lid); }
        out.assign(V, {});
        for (const auto& l : p.links) {
            links.push_back(L{l.id, node(l.from), node(l.to), (B + l.rate - 1) / l.rate, l.latency});
            const int li = static_cast<int>(links.size()) - 1;
            std::int64_t k = -1; auto it = p.link_bytes.find(l.id); if (it != p.link_bytes.end() && it->second > 0 && it->second % B == 0) k = it->second / B;
            need.push_back(k);
            if (!zero.count(l.id)) out[links[li].from].push_back(li);
        }
        out.resize(node_id.size());
        for (auto& v : out) std::sort(v.begin(), v.end(), [&](int a, int b) { return links[a].id < links[b].id; });
        capable.assign(node_id.size(), 0);
        for (const auto& w : p.switches) if (w.reduction_capable && w.accumulator_capacity >= B) capable[node_idx.at(w.id)] = 1;
        listed_completion = p.completion;
        // reverse Dijkstra over usable links
        dist.assign(node_id.size(), BIG);
        std::vector<std::vector<std::pair<int, std::int64_t>>> rev(node_id.size());
        for (int n = 0; n < static_cast<int>(node_id.size()); ++n) for (int li : out[n]) rev[links[li].to].push_back({n, links[li].occ + links[li].lat});
        std::priority_queue<std::pair<std::int64_t, int>, std::vector<std::pair<std::int64_t, int>>, std::greater<>> pq;
        dist[sink] = 0; pq.push({0, sink});
        while (!pq.empty()) { auto [d, n] = pq.top(); pq.pop(); if (d > dist[n]) continue; for (const auto& [m, w] : rev[n]) if (d + w < dist[m]) { dist[m] = d + w; pq.push({dist[m], m}); } }
        std::set<int> e(src_node.begin(), src_node.end()); ends.assign(e.begin(), e.end());
        std::sort(ends.begin(), ends.end(), [&](int a, int b) { if (dist[a] != dist[b]) return dist[a] > dist[b]; return node_id[a] < node_id[b]; });
        parent.assign(node_id.size(), -1);
    }

    // ------------------------------------------------------- event-driven replay
    struct Out { std::int64_t completion{0}, allocation{0}; std::vector<std::int64_t> counts; bool ok{true}; std::vector<int> sink_origins; };
    struct Payload { std::int64_t ready; std::vector<int> origins; };  // origins sorted ascending

    /// `active[node]` says whether the sources at that node take part; `selected[node]` the combine set.
    Out run(const std::vector<char>& active, const std::vector<char>& selected) const {
        const int V = static_cast<int>(node_id.size());
        Out o; o.counts.assign(links.size(), 0);
        std::vector<std::vector<int>> children(V);
        for (int n = 0; n < V; ++n) if (parent[n] >= 0) children[links[parent[n]].to].push_back(n);
        std::vector<std::int64_t> expected(V, 0);
        std::function<std::int64_t(int)> count = [&](int n) -> std::int64_t {
            std::int64_t c = active[n] ? static_cast<std::int64_t>(at[n].size()) : 0;
            for (int ch : children[n]) { const std::int64_t cc = count(ch); c += selected[ch] ? 1 : cc; }
            expected[n] = c; return c;
        };
        count(sink);
        for (int n = 0; n < V; ++n) if (selected[n] && (n == sink || !capable[n] || parent[n] < 0 || expected[n] < 2)) { o.ok = false; return o; }
        using Ev = std::tuple<std::int64_t, std::int64_t, int, int>;  // tick, seq, node, payload
        std::priority_queue<Ev, std::vector<Ev>, std::greater<Ev>> ev;
        std::vector<Payload> pool; std::int64_t seq = 0;
        for (int si = 0; si < static_cast<int>(src_node.size()); ++si) if (active[src_node[si]]) { pool.push_back(Payload{0, {si}}); ev.push(Ev{0, seq++, src_node[si], static_cast<int>(pool.size()) - 1}); }
        std::vector<std::vector<std::pair<std::int64_t, int>>> waiting(links.size());
        std::vector<std::int64_t> link_free(links.size(), 0);
        std::vector<std::vector<int>> gathered(V);
        std::vector<int> sink_origins;
        while (!ev.empty()) {
            const std::int64_t now = std::get<0>(ev.top());
            std::vector<Ev> batch;
            while (!ev.empty() && std::get<0>(ev.top()) == now) { batch.push_back(ev.top()); ev.pop(); }
            std::vector<int> touched;
            for (const auto& [tick, sq, node, pi] : batch) {
                pool[pi].ready = tick;  // arrival tick at this node
                if (node == sink) { o.completion = std::max(o.completion, tick); const auto& org = pool[pi].origins; sink_origins.insert(sink_origins.end(), org.begin(), org.end()); continue; }
                if (parent[node] < 0) { o.ok = false; return o; }
                if (selected[node]) {
                    gathered[node].push_back(pi);
                    if (static_cast<std::int64_t>(gathered[node].size()) == expected[node]) {
                        o.allocation += B;
                        std::int64_t latest = 0; std::vector<int> u;
                        for (int gi : gathered[node]) { latest = std::max(latest, pool[gi].ready); u.insert(u.end(), pool[gi].origins.begin(), pool[gi].origins.end()); }
                        std::sort(u.begin(), u.end());
                        pool.push_back(Payload{latest, u});
                        waiting[parent[node]].push_back({latest, static_cast<int>(pool.size()) - 1}); touched.push_back(parent[node]);
                    }
                    continue;
                }
                waiting[parent[node]].push_back({tick, pi}); touched.push_back(parent[node]);
            }
            std::sort(touched.begin(), touched.end()); touched.erase(std::unique(touched.begin(), touched.end()), touched.end());
            for (int li : touched) {
                auto& q = waiting[li];
                std::sort(q.begin(), q.end(), [&](const auto& a, const auto& b) { if (a.first != b.first) return a.first < b.first; return pool[a.second].origins.front() < pool[b.second].origins.front(); });
                std::int64_t free = link_free[li];
                std::size_t i = 0;
                for (; i < q.size() && q[i].first <= now; ++i) {
                    const std::int64_t start = std::max(q[i].first, free);
                    free = start + links[li].occ;
                    o.counts[li] += 1;
                    ev.push(Ev{free + links[li].lat, seq++, links[li].to, q[i].second});
                }
                q.erase(q.begin(), q.begin() + static_cast<std::ptrdiff_t>(i));
                link_free[li] = free;
            }
        }
        std::sort(sink_origins.begin(), sink_origins.end());
        if (sink_origins.empty()) { o.ok = false; return o; }
        o.sink_origins = sink_origins;
        return o;
    }

    // ------------------------------------------------------------------ search
    bool resolved(int e) const { int n = e; int guard = 0; while (n != sink) { if (parent[n] < 0 || ++guard > static_cast<int>(node_id.size())) return false; n = links[parent[n]].to; } return true; }

    std::int64_t prefix_bound(int e) const {  // routed prefix cost plus unloaded cost from where it ends
        std::int64_t c = 0; int n = e; int guard = 0;
        while (n != sink && parent[n] >= 0 && ++guard <= static_cast<int>(node_id.size())) { c += links[parent[n]].occ + links[parent[n]].lat; n = links[parent[n]].to; }
        return c + dist[n];
    }

    std::int64_t bound(std::vector<char>& active_out) const {
        const int V = static_cast<int>(node_id.size());
        std::vector<char> active(V, 0);
        std::int64_t lb = 0; bool any = false;
        for (int e : ends) { if (resolved(e)) { active[e] = 1; any = true; } else lb = std::max(lb, prefix_bound(e)); }
        active_out = active;
        if (!any) return lb;
        // selected: every capable switch on a resolved chain that gathers >= 2 payloads (bottom-up over the resolved subtree)
        std::vector<char> on(V, 0);
        for (int e : ends) if (active[e]) { int n = e; while (n != sink) { on[n] = 1; n = links[parent[n]].to; } }
        std::vector<std::vector<int>> children(V);
        for (int n = 0; n < V; ++n) if (on[n]) children[links[parent[n]].to].push_back(n);
        std::vector<char> selected(V, 0);
        std::function<std::int64_t(int)> count = [&](int n) -> std::int64_t {
            std::int64_t c = active[n] ? static_cast<std::int64_t>(at[n].size()) : 0;
            for (int ch : children[n]) { const std::int64_t cc = count(ch); c += selected[ch] ? 1 : cc; }
            if (n != sink && capable[n] && c >= 2) selected[n] = 1;
            return c;
        };
        count(sink);
        // simulate only the resolved chains: temporarily hide other parents
        std::vector<int> saved = parent;
        const_cast<Planner*>(this)->parent = std::vector<int>(V, -1);
        for (int n = 0; n < V; ++n) if (on[n]) const_cast<Planner*>(this)->parent[n] = saved[n];
        Out o = run(active, selected);
        const_cast<Planner*>(this)->parent = saved;
        return std::max(lb, o.ok ? o.completion : BIG);
    }

    std::int64_t cap() const { if (listed_completion) return *listed_completion; return found ? std::get<0>(best) : BIG; }

    bool counts_possible(const std::vector<char>& active) const {
        std::vector<std::int64_t> routed(links.size(), 0);
        std::int64_t unresolved = 0;
        for (int e : ends) { const std::int64_t k = static_cast<std::int64_t>(at[e].size()); if (active[e]) { int n = e; while (n != sink) { routed[parent[n]] += k; n = links[parent[n]].to; } } else unresolved += k; }
        std::vector<char> used(links.size(), 0); for (int li : parent) if (li >= 0) used[li] = 1;
        for (int li = 0; li < static_cast<int>(links.size()); ++li) {
            if (need[li] < 0 || !used[li]) continue;
            if (routed[li] + unresolved < need[li]) return false;
            const int u = links[li].from;
            if (!capable[u]) {
                std::int64_t branches = static_cast<std::int64_t>(at[u].size());
                for (int n = 0; n < static_cast<int>(node_id.size()); ++n) if (parent[n] >= 0 && links[parent[n]].to == u) ++branches;
                if (branches > need[li]) return false;
            }
        }
        return true;
    }

    void rec(std::vector<int> pending) {
        std::vector<int> pend; for (int n : pending) if (n != sink && parent[n] < 0) pend.push_back(n);
        if (pend.empty()) { leaf(); return; }
        const int node = pend[0];
        // best first: links whose own cost plus the unloaded cost from their target is smallest, ties by id
        std::vector<int> order(out[node].begin(), out[node].end());
        std::stable_sort(order.begin(), order.end(), [&](int a, int b) { return links[a].occ + links[a].lat + dist[links[a].to] < links[b].occ + links[b].lat + dist[links[b].to]; });
        for (int li : order) {
            // no cycle: walking from the target along parents must not return to node
            int cur = links[li].to; bool cyc = false; int guard = 0;
            while (cur != sink && parent[cur] >= 0) { if (cur == node || ++guard > static_cast<int>(node_id.size())) { cyc = true; break; } cur = links[parent[cur]].to; }
            if (cyc || cur == node) continue;
            bool clash = false; for (int r : out[node]) if (need[r] >= 0 && r != li) clash = true;   // a listed link leaving this node must be the one chosen
            if (clash) continue;
            parent[node] = li;
            std::vector<char> active;
            const std::int64_t lb = bound(active);
            if (lb <= cap() && counts_possible(active)) {
                std::vector<int> next; if (links[li].to != sink && parent[links[li].to] < 0) next.push_back(links[li].to);
                next.insert(next.end(), pend.begin() + 1, pend.end());
                rec(next);
            }
            parent[node] = -1;
        }
    }

    void leaf() {
        const int V = static_cast<int>(node_id.size());
        std::vector<char> used(links.size(), 0); for (int li : parent) if (li >= 0) used[li] = 1;
        for (int li = 0; li < static_cast<int>(links.size()); ++li) if (need[li] >= 0 && !used[li]) return;
        std::vector<int> cands; for (int n = 0; n < V; ++n) if (parent[n] >= 0 && capable[n]) cands.push_back(n);
        std::sort(cands.begin(), cands.end(), [&](int a, int b) { return node_id[a] < node_id[b]; });
        std::vector<std::vector<std::string>> paths; std::vector<std::pair<std::string, std::string>> flat;
        for (int si = 0; si < static_cast<int>(src_node.size()); ++si) { std::vector<std::string> path; int n = src_node[si]; while (n != sink) { path.push_back(links[parent[n]].id); flat.push_back({node_id[n], links[parent[n]].id}); n = links[parent[n]].to; } paths.push_back(path); }
        std::vector<char> active(V, 1), selected(V, 0);
        for (std::size_t mask = 0; mask < (std::size_t{1} << cands.size()); ++mask) {
            std::fill(selected.begin(), selected.end(), 0); std::vector<std::string> clist;
            for (std::size_t k = 0; k < cands.size(); ++k) if (mask & (std::size_t{1} << k)) { selected[cands[k]] = 1; clist.push_back(node_id[cands[k]]); }
            Out o = run(active, selected);
            if (!o.ok) continue;
            if (listed_completion && o.completion != *listed_completion) continue;
            bool mismatch = false; std::int64_t total = 0;
            for (int li = 0; li < static_cast<int>(links.size()); ++li) { total += o.counts[li] * B; if (need[li] >= 0 && o.counts[li] != need[li]) { mismatch = true; break; } }
            if (mismatch) continue;
            for (const auto& [lid, b] : p.link_bytes) if (b == 0) { const int li = static_cast<int>(std::find_if(links.begin(), links.end(), [&](const L& l) { return l.id == lid; }) - links.begin()); if (li < static_cast<int>(links.size()) && o.counts[li] != 0) { mismatch = true; break; } }
            if (mismatch) continue;
            auto key = std::make_tuple(o.completion, o.allocation, total, flat, clist);
            if (!found || key < best) {
                found = true; best = key; best_paths = paths; best_combine = clist;
                std::int64_t acc = src_value[o.sink_origins[0]];
                for (std::size_t i = 1; i < o.sink_origins.size(); ++i) acc = std::max(I32MIN, std::min(I32MAX, acc + src_value[o.sink_origins[i]]));
                best_value = acc;
            }
        }
    }

    Result solve() {
        if (bad) return Result::infeasible("telemetry bytes are not whole payloads");
        for (int e : ends) if (dist[e] >= BIG) return Result::infeasible("unreachable sink");
        rec(ends);
        if (!found) return Result::infeasible("no plan reproduces the telemetry");
        return Result::ok(best_paths, best_combine, best_value);
    }
};

}  // namespace

Result recover(const Problem& problem) {
    if (!problem_ok(problem)) return Result::invalid("invalid problem");
    Planner pl(problem);
    return pl.solve();
}

}  // namespace fabric
