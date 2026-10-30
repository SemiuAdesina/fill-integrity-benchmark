// Plan recovery — independent architecture B (parent-pointer trees, event-driven replay).
//
// Differs from the reference in organisation: plans are enumerated as
// trees (one outgoing link per routed node, assigned node by node with a
// cycle check) rather than as products of per-source paths; replay is a
// discrete-event simulation with per-link queues, batched per tick, and a
// pre-computed expected-arrival count per selected switch; validity,
// telemetry, objective and plan_key are written independently.
#include "fabric/model.hpp"

#include <algorithm>
#include <functional>
#include <map>
#include <queue>
#include <set>
#include <tuple>
#include <vector>

namespace fabric {

namespace {

constexpr std::int64_t I32MIN = -(std::int64_t{1} << 31), I32MAX = (std::int64_t{1} << 31) - 1;

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
    std::map<std::string, std::int64_t> value;
    std::map<std::string, std::vector<const Link*>> out;
    std::vector<std::string> endpoints;
    explicit Sim(const Problem& pr) : p(pr) {
        for (const auto& l : p.links) { link[l.id] = &l; out[l.from].push_back(&l); }
        for (auto& [n, v] : out) std::sort(v.begin(), v.end(), [](const Link* a, const Link* b) { return a->id < b->id; });
        for (const auto& w : p.switches) sw[w.id] = &w;
        std::set<std::string> ends;
        for (const auto& s : p.sources) { value[s.id] = s.value; ends.insert(s.node); }
        endpoints.assign(ends.begin(), ends.end());
    }

    /// Enumerate parent maps routing every endpoint to the sink acyclically.
    void trees(const std::function<void(const std::map<std::string, std::string>&)>& visit) {
        std::map<std::string, std::string> parent;
        std::function<void(std::vector<std::string>)> rec = [&](std::vector<std::string> pending) {
            std::vector<std::string> todo;
            for (const auto& n : pending) if (n != p.sink && !parent.count(n)) todo.push_back(n);
            if (todo.empty()) { visit(parent); return; }
            const std::string node = todo.front();
            std::vector<std::string> rest(todo.begin() + 1, todo.end());
            auto it = out.find(node);
            if (it == out.end()) return;
            for (const Link* l : it->second) {
                // cycle check: walk from the target along assigned parents
                std::string cur = l->to; bool cyc = false; std::set<std::string> chain;
                while (cur != p.sink && parent.count(cur)) { if (cur == node || chain.count(cur)) { cyc = true; break; } chain.insert(cur); cur = link.at(parent.at(cur))->to; }
                if (cyc || cur == node) continue;
                parent[node] = l->id;
                std::vector<std::string> next = rest;
                if (l->to != p.sink && !parent.count(l->to)) next.push_back(l->to);
                rec(next);
                parent.erase(node);
            }
        };
        rec(endpoints);
    }

    struct Out { std::int64_t completion{0}, allocation{0}, bytes{0}, sink_value{0}; std::map<std::string, std::int64_t> lb; };

    bool simulate(const std::map<std::string, std::string>& parent, const std::vector<std::string>& combine, Out& o) {
        const std::int64_t B = p.payload_bytes;
        std::set<std::string> cset(combine.begin(), combine.end());
        for (const auto& c : combine) {
            auto it = sw.find(c);
            if (it == sw.end() || !it->second->reduction_capable || !parent.count(c) || B > it->second->accumulator_capacity) return false;
        }
        std::map<std::string, std::vector<std::string>> children;
        for (const auto& [n, lid] : parent) children[link.at(lid)->to].push_back(n);
        std::map<std::string, int> at_node;
        for (const auto& s : p.sources) at_node[s.node]++;
        std::function<int(const std::string&)> count = [&](const std::string& n) {
            int c = at_node.count(n) ? at_node.at(n) : 0;
            auto it = children.find(n);
            if (it != children.end()) for (const auto& ch : it->second) c += cset.count(ch) ? 1 : count(ch);
            return c;
        };
        std::map<std::string, int> expected;
        for (const auto& c : combine) { expected[c] = count(c); if (expected[c] < 2) return false; }
        using Ev = std::tuple<std::int64_t, std::uint64_t, std::string, std::set<std::string>>;
        std::priority_queue<Ev, std::vector<Ev>, std::greater<Ev>> ev;
        std::uint64_t seq = 0;
        for (const auto& s : p.sources) ev.push(Ev{0, seq++, s.node, {s.id}});
        std::map<std::string, std::vector<std::pair<std::int64_t, std::set<std::string>>>> waiting;
        std::map<std::string, std::int64_t> free;
        std::map<std::string, std::vector<std::pair<std::int64_t, std::set<std::string>>>> gathered;
        std::set<std::string> sink_origins;
        while (!ev.empty()) {
            const std::int64_t now = std::get<0>(ev.top());
            std::set<std::string> touched;
            while (!ev.empty() && std::get<0>(ev.top()) == now) {
                auto [tick, s_, node, origins] = ev.top(); ev.pop();
                if (node == p.sink) { o.completion = std::max(o.completion, tick); sink_origins.insert(origins.begin(), origins.end()); continue; }
                if (cset.count(node)) {
                    auto& g = gathered[node];
                    g.push_back({tick, origins});
                    if (static_cast<int>(g.size()) == expected[node]) {
                        o.allocation += B;
                        std::set<std::string> u; std::int64_t last = 0;
                        for (const auto& [t, og] : g) { last = std::max(last, t); u.insert(og.begin(), og.end()); }
                        waiting[parent.at(node)].push_back({last, u}); touched.insert(parent.at(node));
                    }
                    continue;
                }
                waiting[parent.at(node)].push_back({tick, origins}); touched.insert(parent.at(node));
            }
            for (const auto& lid : touched) {
                const Link& l = *link.at(lid);
                auto& q = waiting[lid];
                std::sort(q.begin(), q.end(), [](const auto& a, const auto& b) { return a.first != b.first ? a.first < b.first : *a.second.begin() < *b.second.begin(); });
                std::int64_t f = free[lid];
                for (const auto& [ready, og] : q) {
                    const std::int64_t start = std::max(ready, f);
                    f = start + (B + l.rate - 1) / l.rate;
                    o.lb[lid] += B;
                    ev.push(Ev{f + l.latency, seq++, l.to, og});
                }
                q.clear();
                free[lid] = f;
            }
        }
        if (sink_origins.size() != p.sources.size()) return false;
        auto it = sink_origins.begin();
        std::int64_t acc = value.at(*it);
        for (++it; it != sink_origins.end(); ++it) acc = std::max(I32MIN, std::min(I32MAX, acc + value.at(*it)));
        o.sink_value = acc;
        for (const auto& [l, b] : o.lb) o.bytes += b;
        return true;
    }

    std::vector<std::vector<std::string>> paths_of(const std::map<std::string, std::string>& parent) {
        std::vector<const Source*> srcs;
        for (const auto& s : p.sources) srcs.push_back(&s);
        std::sort(srcs.begin(), srcs.end(), [](const Source* a, const Source* b) { return a->id < b->id; });
        std::vector<std::vector<std::string>> out_paths;
        for (const Source* s : srcs) {
            std::vector<std::string> path; std::string cur = s->node;
            while (cur != p.sink) { path.push_back(parent.at(cur)); cur = link.at(parent.at(cur))->to; }
            out_paths.push_back(path);
        }
        return out_paths;
    }
};

}  // namespace

Result recover(const Problem& problem) {
    if (!problem_ok(problem)) return Result::invalid();
    Sim sim(problem);
    std::vector<std::string> capable;
    for (const auto& w : problem.switches) if (w.reduction_capable) capable.push_back(w.id);
    std::sort(capable.begin(), capable.end());
    using Key = std::tuple<std::int64_t, std::int64_t, std::int64_t, std::vector<std::pair<std::string, std::string>>, std::vector<std::string>>;
    bool found = false; Key best; Result answer;
    sim.trees([&](const std::map<std::string, std::string>& parent) {
        std::vector<std::string> cands;
        for (const auto& w : capable) if (parent.count(w)) cands.push_back(w);
        for (std::size_t mask = 0; mask < (std::size_t{1} << cands.size()); ++mask) {
            std::vector<std::string> combine;
            for (std::size_t k = 0; k < cands.size(); ++k) if (mask >> k & 1) combine.push_back(cands[k]);
            Sim::Out o;
            if (!sim.simulate(parent, combine, o)) continue;
            bool tele = true;
            for (const auto& [lid, b] : problem.link_bytes) if ((o.lb.count(lid) ? o.lb.at(lid) : 0) != b) tele = false;
            if (problem.completion && *problem.completion != o.completion) tele = false;
            if (!tele) continue;
            auto paths = sim.paths_of(parent);
            std::vector<std::pair<std::string, std::string>> flat;
            for (const auto& path : paths) for (const auto& lid : path) flat.push_back({sim.link.at(lid)->from, lid});
            Key key{o.completion, o.allocation, o.bytes, flat, combine};
            if (!found || key < best) { found = true; best = key; answer = Result::ok(paths, combine, o.sink_value); }
        }
    });
    return found ? answer : Result::infeasible();
}

}  // namespace fabric
