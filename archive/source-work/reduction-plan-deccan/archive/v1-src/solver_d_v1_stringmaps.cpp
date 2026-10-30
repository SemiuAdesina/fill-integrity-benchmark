// Plan recovery — independent architecture D (parent-pointer trees built depth
// first, event-driven replay, pruned by the resolved endpoints' best completion).
//
// Differs from the reference in organisation: the tree is grown one routed
// node at a time (the target of the chosen link is routed next, so one
// endpoint resolves before the next starts) rather than one source path at
// a time; the replay is a discrete-event simulation with per-link queues
// batched per tick and a pre-computed expected-arrival count per selected
// switch; the bound simulates the resolved endpoints with every capable
// switch that gathers two payloads selected; validity, telemetry, objective
// and plan_key are written independently.
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

struct Sim {
    const Problem& p;
    std::map<std::string, const Link*> link;
    std::map<std::string, const Switch*> sw;
    std::map<std::string, std::vector<std::string>> at;   // node -> source ids (sorted)
    std::map<std::string, std::int64_t> value;
    explicit Sim(const Problem& pr) : p(pr) {
        for (const auto& l : p.links) link[l.id] = &l;
        for (const auto& w : p.switches) sw[w.id] = &w;
        for (const auto& s : p.sources) { at[s.node].push_back(s.id); value[s.id] = s.value; }
        for (auto& [n, v] : at) std::sort(v.begin(), v.end());
    }
    struct Out { std::int64_t completion{0}, allocation{0}; std::map<std::string, std::int64_t> link_bytes; std::int64_t sink_value{0}; bool ok{true}; };

    /// Event-driven replay of `parent` (node -> link id) for the sources at the
    /// endpoints in `active` (all endpoints when empty), with `combine` selected.
    Out run(const std::map<std::string, std::string>& parent, const std::set<std::string>& combine, const std::set<std::string>& active) const {
        Out o;
        const std::int64_t B = p.payload_bytes;
        std::map<std::string, std::vector<std::string>> children;
        for (const auto& [n, lid] : parent) children[link.at(lid)->to].push_back(n);
        std::map<std::string, std::int64_t> expected;
        std::function<std::int64_t(const std::string&)> count = [&](const std::string& n) -> std::int64_t {
            std::int64_t c = (active.empty() || active.count(n)) ? static_cast<std::int64_t>(at.count(n) ? at.at(n).size() : 0) : 0;
            for (const auto& ch : children[n]) { const std::int64_t cc = count(ch); c += combine.count(ch) ? 1 : cc; }
            expected[n] = c; return c;
        };
        count(p.sink);
        for (const auto& c : combine) {
            auto it = sw.find(c);
            if (it == sw.end() || !it->second->reduction_capable || !parent.count(c) || B > it->second->accumulator_capacity || expected[c] < 2) { o.ok = false; return o; }
        }
        // events: (tick, seq, node, origin set)
        using Ev = std::tuple<std::int64_t, std::int64_t, std::string, std::set<std::string>>;
        std::priority_queue<Ev, std::vector<Ev>, std::greater<Ev>> ev;
        std::int64_t seq = 0;
        for (const auto& s : p.sources) if (active.empty() || active.count(s.node)) ev.push(Ev{0, seq++, s.node, {s.id}});
        std::map<std::string, std::vector<std::pair<std::int64_t, std::set<std::string>>>> waiting;
        std::map<std::string, std::int64_t> link_free;
        std::map<std::string, std::vector<std::pair<std::int64_t, std::set<std::string>>>> gathered;
        std::set<std::string> sink_origins;
        while (!ev.empty()) {
            const std::int64_t now = std::get<0>(ev.top());
            std::vector<Ev> batch;
            while (!ev.empty() && std::get<0>(ev.top()) == now) { batch.push_back(ev.top()); ev.pop(); }
            std::set<std::string> touched;
            for (auto& [tick, sq, node, org] : batch) {
                if (node == p.sink) { o.completion = std::max(o.completion, tick); sink_origins.insert(org.begin(), org.end()); continue; }
                auto pit = parent.find(node);
                if (pit == parent.end()) { o.ok = false; return o; }
                if (combine.count(node)) {
                    auto& g = gathered[node]; g.push_back({tick, org});
                    if (static_cast<std::int64_t>(g.size()) == expected[node]) {
                        o.allocation += B;
                        std::int64_t latest = 0; std::set<std::string> u;
                        for (const auto& [t, s] : g) { latest = std::max(latest, t); u.insert(s.begin(), s.end()); }
                        waiting[pit->second].push_back({latest, u}); touched.insert(pit->second);
                    }
                    continue;
                }
                waiting[pit->second].push_back({tick, org}); touched.insert(pit->second);
            }
            for (const auto& lid : touched) {
                const Link& l = *link.at(lid);
                auto& q = waiting[lid];
                std::sort(q.begin(), q.end(), [](const auto& a, const auto& b) { if (a.first != b.first) return a.first < b.first; return *a.second.begin() < *b.second.begin(); });
                std::int64_t free = link_free[lid];
                std::size_t i = 0;
                for (; i < q.size() && q[i].first <= now; ++i) {
                    const std::int64_t start = std::max(q[i].first, free);
                    free = start + (B + l.rate - 1) / l.rate;
                    o.link_bytes[lid] += B;
                    ev.push(Ev{free + l.latency, seq++, l.to, q[i].second});
                }
                q.erase(q.begin(), q.begin() + static_cast<std::ptrdiff_t>(i));
                link_free[lid] = free;
            }
        }
        std::vector<std::string> ids(sink_origins.begin(), sink_origins.end());
        if (ids.empty()) { o.ok = false; return o; }
        std::int64_t acc = value.at(ids[0]);
        for (std::size_t i = 1; i < ids.size(); ++i) acc = std::max(I32MIN, std::min(I32MAX, acc + value.at(ids[i])));
        o.sink_value = acc;
        return o;
    }
};

struct Planner {
    const Problem& p; Sim sim;
    std::map<std::string, std::vector<const Link*>> out;
    std::map<std::string, const Link*> link;
    std::vector<std::string> ends;
    std::set<std::string> capable;
    std::map<std::string, std::int64_t> need;      // link -> required payload count
    std::map<std::string, std::int64_t> dist;      // unloaded cost to the sink
    std::optional<std::int64_t> listed_completion;
    bool bad{false};
    // best
    bool found{false};
    std::tuple<std::int64_t, std::int64_t, std::int64_t, std::vector<std::pair<std::string, std::string>>, std::vector<std::string>> best;
    std::vector<std::vector<std::string>> best_paths; std::vector<std::string> best_combine; std::int64_t best_value{0};

    explicit Planner(const Problem& pr) : p(pr), sim(pr) {
        const std::int64_t B = p.payload_bytes;
        std::set<std::string> zero;
        for (const auto& [lid, b] : p.link_bytes) { if (b < 0 || b % B != 0) bad = true; else if (b == 0) zero.insert(lid); else need[lid] = b / B; }
        for (const auto& l : p.links) { link[l.id] = &l; if (!zero.count(l.id)) out[l.from].push_back(&l); }
        for (auto& [n, v] : out) std::sort(v.begin(), v.end(), [](const Link* a, const Link* b) { return a->id < b->id; });
        std::set<std::string> e; for (const auto& s : p.sources) e.insert(s.node); ends.assign(e.begin(), e.end());
        for (const auto& w : p.switches) if (w.reduction_capable && w.accumulator_capacity >= B) capable.insert(w.id);
        listed_completion = p.completion;
        // reverse Dijkstra
        std::map<std::string, std::vector<std::pair<std::string, std::int64_t>>> rev;
        for (const auto& [n, v] : out) for (const Link* l : v) rev[l->to].push_back({n, (B + l->rate - 1) / l->rate + l->latency});
        std::priority_queue<std::pair<std::int64_t, std::string>, std::vector<std::pair<std::int64_t, std::string>>, std::greater<>> pq;
        dist[p.sink] = 0; pq.push({0, p.sink});
        while (!pq.empty()) { auto [d, n] = pq.top(); pq.pop(); if (d > dist[n]) continue; for (const auto& [m, w] : rev[n]) { auto it = dist.find(m); if (it == dist.end() || d + w < it->second) { dist[m] = d + w; pq.push({d + w, m}); } } }
    }

    std::vector<std::string> resolved(const std::map<std::string, std::string>& parent) const {
        std::vector<std::string> r;
        for (const auto& e : ends) { std::string n = e; std::set<std::string> seen; while (n != p.sink && parent.count(n) && !seen.count(n)) { seen.insert(n); n = link.at(parent.at(n))->to; } if (n == p.sink) r.push_back(e); }
        return r;
    }

    std::int64_t bound(const std::map<std::string, std::string>& parent, const std::vector<std::string>& res) const {
        if (res.empty()) return 0;
        std::map<std::string, std::string> par;
        for (const auto& e : res) { std::string n = e; while (n != p.sink) { par[n] = parent.at(n); n = link.at(par[n])->to; } }
        std::set<std::string> active(res.begin(), res.end());
        std::map<std::string, std::vector<std::string>> children;
        for (const auto& [n, lid] : par) children[link.at(lid)->to].push_back(n);
        std::set<std::string> selected;
        std::function<std::int64_t(const std::string&)> count = [&](const std::string& n) -> std::int64_t {
            std::int64_t c = active.count(n) && sim.at.count(n) ? static_cast<std::int64_t>(sim.at.at(n).size()) : 0;
            for (const auto& ch : children[n]) { const std::int64_t cc = count(ch); c += selected.count(ch) ? 1 : cc; }
            if (n != p.sink && capable.count(n) && c >= 2) selected.insert(n);
            return c;
        };
        count(p.sink);
        Sim::Out o = sim.run(par, selected, active);
        return o.ok ? o.completion : BIG;
    }

    std::int64_t cap() const { if (listed_completion) return *listed_completion; return found ? std::get<0>(best) : BIG; }

    bool counts_possible(const std::map<std::string, std::string>& parent, const std::vector<std::string>& res) const {
        std::map<std::string, std::int64_t> routed;
        std::set<std::string> rs(res.begin(), res.end());
        std::int64_t unresolved = 0;
        for (const auto& e : ends) { const std::int64_t k = static_cast<std::int64_t>(sim.at.at(e).size()); if (rs.count(e)) { std::string n = e; while (n != p.sink) { routed[parent.at(n)] += k; n = link.at(parent.at(n))->to; } } else unresolved += k; }
        std::set<std::string> used; for (const auto& [n, lid] : parent) used.insert(lid);
        for (const auto& [lid, k] : need) {
            if (!used.count(lid)) continue;
            if (routed[lid] + unresolved < k) return false;
            const std::string& u = link.at(lid)->from;
            if (!capable.count(u)) {
                std::int64_t branches = sim.at.count(u) ? static_cast<std::int64_t>(sim.at.at(u).size()) : 0;
                for (const auto& [n, x] : parent) if (link.at(x)->to == u) ++branches;
                if (branches > k) return false;
            }
        }
        return true;
    }

    void rec(std::vector<std::string> pending, std::map<std::string, std::string>& parent) {
        std::vector<std::string> pend; for (const auto& n : pending) if (n != p.sink && !parent.count(n)) pend.push_back(n);
        if (pend.empty()) { leaf(parent); return; }
        const std::string node = pend[0];
        auto it = out.find(node); if (it == out.end()) return;
        for (const Link* l : it->second) {
            std::string cur = l->to; std::set<std::string> chain; bool cyc = false;
            while (cur != p.sink && parent.count(cur)) { if (cur == node || chain.count(cur)) { cyc = true; break; } chain.insert(cur); cur = link.at(parent.at(cur))->to; }
            if (cyc || cur == node) continue;
            bool clash = false; for (const auto& [rid, k] : need) if (link.at(rid)->from == node && rid != l->id) clash = true;
            if (clash) continue;
            parent[node] = l->id;
            const auto res = resolved(parent);
            std::int64_t lb = bound(parent, res);
            std::set<std::string> rs(res.begin(), res.end());
            for (const auto& e : ends) if (!rs.count(e)) lb = std::max(lb, dist.count(e) ? dist.at(e) : BIG);
            if (lb <= cap() && counts_possible(parent, res)) {
                std::vector<std::string> next; if (l->to != p.sink && !parent.count(l->to)) next.push_back(l->to);
                next.insert(next.end(), pend.begin() + 1, pend.end());
                rec(next, parent);
            }
            parent.erase(node);
        }
    }

    void leaf(const std::map<std::string, std::string>& parent) {
        std::set<std::string> used; for (const auto& [n, lid] : parent) used.insert(lid);
        for (const auto& [rid, k] : need) if (!used.count(rid)) return;
        std::vector<std::string> cands; for (const auto& [n, lid] : parent) if (capable.count(n)) cands.push_back(n);
        std::sort(cands.begin(), cands.end());
        std::vector<std::vector<std::string>> paths; std::vector<std::pair<std::string, std::string>> flat;
        std::vector<const Source*> srcs; for (const auto& s : p.sources) srcs.push_back(&s);
        std::sort(srcs.begin(), srcs.end(), [](const Source* a, const Source* b) { return a->id < b->id; });
        for (const Source* s : srcs) { std::vector<std::string> path; std::string n = s->node; while (n != p.sink) { path.push_back(parent.at(n)); flat.push_back({n, parent.at(n)}); n = link.at(parent.at(n))->to; } paths.push_back(path); }
        for (std::size_t mask = 0; mask < (std::size_t{1} << cands.size()); ++mask) {
            std::set<std::string> combine; std::vector<std::string> clist;
            for (std::size_t k = 0; k < cands.size(); ++k) if (mask & (std::size_t{1} << k)) { combine.insert(cands[k]); clist.push_back(cands[k]); }
            Sim::Out o = sim.run(parent, combine, {});
            if (!o.ok) continue;
            if (listed_completion && o.completion != *listed_completion) continue;
            bool mismatch = false;
            for (const auto& [lid, b] : p.link_bytes) { auto it = o.link_bytes.find(lid); if ((it == o.link_bytes.end() ? 0 : it->second) != b) { mismatch = true; break; } }
            if (mismatch) continue;
            std::int64_t total = 0; for (const auto& [lid, b] : o.link_bytes) total += b;
            auto key = std::make_tuple(o.completion, o.allocation, total, flat, clist);
            if (!found || key < best) { found = true; best = key; best_paths = paths; best_combine = clist; best_value = o.sink_value; }
        }
    }

    Result solve() {
        if (bad) return Result::infeasible("telemetry bytes are not whole payloads");
        for (const auto& e : ends) if (!dist.count(e)) return Result::infeasible("unreachable sink");
        std::map<std::string, std::string> parent;
        rec(ends, parent);
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
