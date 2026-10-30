// Author-side: counts the in-trees an exhaustive tree enumeration (architecture B)
// would visit for a problem, with a wall-clock cap; reports the count reached,
// the enumeration rate and the fraction of the top-level branches finished so
// the total can be projected. Never ships.
#include <chrono>
#include <cstdio>
#include <fstream>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <vector>
#include "fabric/model.hpp"
using namespace std::chrono;
struct C {
    const fabric::Problem& p; std::map<std::string, std::vector<const fabric::Link*>> out; std::map<std::string, const fabric::Link*> link;
    std::vector<std::string> ends; unsigned long long n = 0; steady_clock::time_point t0; double cap; bool timed = false;
    unsigned long long top_done = 0, top_total = 0;
    bool telemetry_aware;
    C(const fabric::Problem& pr, double c, bool tel) : p(pr), cap(c), telemetry_aware(tel) {
        // telemetry-aware: links listed with 0 bytes are never used; a node with a listed positive link leaves on it
        std::set<std::string> forced_from;
        if (tel) for (const auto& [lid, b] : p.link_bytes) if (b > 0) for (const auto& l : p.links) if (l.id == lid) forced_from.insert(l.from);
        for (const auto& l : p.links) {
            link[l.id] = &l;
            if (tel) { auto it = p.link_bytes.find(l.id); if (it != p.link_bytes.end() && it->second == 0) continue; if (forced_from.count(l.from) && (it == p.link_bytes.end() || it->second <= 0)) continue; }
            out[l.from].push_back(&l);
        }
        std::set<std::string> e; for (const auto& s : p.sources) e.insert(s.node); ends.assign(e.begin(), e.end()); t0 = steady_clock::now();
    }
    void rec(std::vector<std::string> pending, std::map<std::string, std::string>& parent, int depth) {
        if (timed) return;
        std::vector<std::string> pend; for (auto& n : pending) if (n != p.sink && !parent.count(n)) pend.push_back(n);
        if (pend.empty()) { ++n; if ((n & 0xffff) == 0 && duration<double>(steady_clock::now() - t0).count() > cap) timed = true; return; }
        const std::string node = pend[0];
        auto it = out.find(node); if (it == out.end()) return;
        if (depth == 0) top_total = it->second.size();
        for (const auto* l : it->second) {
            std::string cur = l->to; std::set<std::string> chain; bool cyc = false;
            while (cur != p.sink && parent.count(cur)) { if (cur == node || chain.count(cur)) { cyc = true; break; } chain.insert(cur); cur = link[parent[cur]]->to; }
            if (cyc || cur == node) continue;
            parent[node] = l->id;
            std::vector<std::string> next(pend.begin() + 1, pend.end()); if (l->to != p.sink && !parent.count(l->to)) next.push_back(l->to);
            rec(next, parent, depth + 1);
            parent.erase(node);
            if (depth == 0) ++top_done;
            if (timed) return;
        }
    }
};
int main(int argc, char** argv) {
    if (argc < 2) return 2; double cap = argc > 2 ? std::atof(argv[2]) : 60.0;
    const bool tel = argc > 3 && std::string(argv[3]) == "telemetry";
    fabric::Problem p = fabric::load_problem(argv[1]); C c(p, cap, tel); std::map<std::string, std::string> parent; c.rec(c.ends, parent, 0);
    double secs = duration<double>(steady_clock::now() - c.t0).count();
    std::printf("%s %s trees=%llu %s seconds=%.1f rate=%.0f/s top_branches_done=%llu/%llu\n", argv[1], tel ? "telemetry-aware" : "unconstrained", c.n, c.timed ? "(capped)" : "(complete)", secs, c.n / secs, c.top_done, c.top_total);
}
