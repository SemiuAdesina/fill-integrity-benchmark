#!/usr/bin/env python3
"""Oracle C: exact branch-and-bound over suffix-consistent trees (author-side, never ships).

Search organisation: sources in ascending id; for each, every simple path
from its node that stays off the partial tree until it joins it (or reaches
the sink), then follows the tree. Bound: the completion of the partial tree
replayed with every capable switch that receives two or more payloads
selected, together with the unloaded shortest cost of every unassigned
source. Justification (proved in analysis/bound_proof.md and verified against
oracle A on random fabrics): adding a source, or removing a reduction, never
lowers completion. Leaves evaluate every combine subset of the tree's
capable switches with a payload-count filter before the full replay.
"""
import sys, os, heapq, itertools, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model
from model import Problem, Illegal

INF = float("inf")


class Budget(Exception):
    pass


class SearchC:
    def __init__(self, p: Problem, budget_s=None):
        self.p = p
        self.B = p.payload_bytes
        self.links = {l.id: l for l in p.links}
        self.switches = {w.id: w for w in p.switches}
        self.srcs = sorted(p.sources, key=lambda s: s.id)
        self.values = {s.id: s.value for s in p.sources}
        self.sources_at = {}
        for s in self.srcs:
            self.sources_at.setdefault(s.node, []).append(s.id)
        self.zero = {lid for lid, b in p.link_bytes.items() if b == 0}
        self.count_req = {}
        self.bad_bytes = False
        for lid, b in p.link_bytes.items():
            if b < 0 or b % self.B != 0:
                self.bad_bytes = True
            elif b > 0:
                self.count_req[lid] = b // self.B
        self.out = {}
        for l in p.links:
            if l.id in self.zero:
                continue
            self.out.setdefault(l.frm, []).append(l)
        for k in self.out:
            self.out[k].sort(key=lambda l: l.id)
        self.capable = {w.id for w in p.switches if w.reduction_capable and w.accumulator_capacity >= self.B}
        self.occ = {l.id: -(-self.B // l.rate) for l in p.links}
        self.dist = self.shortest()
        self.cap = p.completion
        self.budget_s = budget_s
        self.t0 = time.time()
        self.nodes = 0
        self.leaves = 0
        self.replays = 0
        self.matching = 0
        self.best = None  # (C, A, bytes, key, paths, combine, replay)
        self.order = sorted(self.srcs, key=lambda s: (-self.dist.get(s.node, INF), s.id))
        self.required = set(self.count_req)
        self.tails = {lid: self.links[lid].frm for lid in self.count_req}

    # ------------------------------------------------------------ helpers
    def shortest(self):
        rev = {}
        for l in self.p.links:
            if l.id in self.zero:
                continue
            rev.setdefault(l.to, []).append((l.frm, self.occ[l.id] + l.latency))
        dist = {self.p.sink: 0}
        heap = [(0, self.p.sink)]
        while heap:
            d, n = heapq.heappop(heap)
            if d > dist.get(n, INF):
                continue
            for m, w in rev.get(n, []):
                if d + w < dist.get(m, INF):
                    dist[m] = d + w
                    heapq.heappush(heap, (d + w, m))
        return dist

    def replay_tree(self, parent, assigned, combine):
        """Replay the tree restricted to `assigned` source ids. `combine` is a set of
        switch ids or the string 'all' (every capable switch with >= 2 payloads).
        Returns (completion, allocation, link_counts, sink_origins) or None if illegal."""
        children = {}
        for n, lid in parent.items():
            children.setdefault(self.links[lid].to, []).append((n, lid))
        counts = {}
        alloc = [0]
        illegal = [False]
        selected = set()

        def process(node):
            pay = [(0, sid) for sid in self.sources_at.get(node, []) if sid in assigned]
            for child, lid in sorted(children.get(node, [])):
                l = self.links[lid]
                outgoing = process(child)
                if illegal[0]:
                    return []
                outgoing.sort()
                free = 0
                occ = self.occ[lid]
                for ready, org in outgoing:
                    start = max(ready, free)
                    free = start + occ
                    pay.append((free + l.latency, org))
                counts[lid] = counts.get(lid, 0) + len(outgoing)
            if combine == "all":
                if node in self.capable and len(pay) >= 2:
                    alloc[0] += self.B
                    selected.add(node)
                    return [(max(r for r, _ in pay), min(o for _, o in pay))]
                return pay
            if node in combine:
                if len(pay) < 2 or node not in self.capable:
                    illegal[0] = True
                    return []
                alloc[0] += self.B
                return [(max(r for r, _ in pay), min(o for _, o in pay))]
            return pay

        at_sink = process(self.p.sink)
        if illegal[0] or not at_sink:
            return None
        return max(r for r, _ in at_sink), alloc[0], counts, selected

    def plan_key(self, paths, combine):
        return ([(self.links[lid].frm.encode(), lid.encode()) for path in paths for lid in path], [c.encode() for c in combine])

    # ------------------------------------------------------------- search
    def run(self):
        if not model.valid(self.p):
            return "invalid", None
        if self.bad_bytes:
            return "infeasible", None
        for s in self.srcs:
            if s.node not in self.dist:
                return "infeasible", None
        parent = {}
        self.dfs(0, parent, [])
        if self.best is None:
            return "infeasible", None
        C, A, T, key, paths, combine, rep = self.best
        return "ok", {"paths": paths, "combine": combine, "sink_value": model.fold(self.values, {s.id for s in self.srcs}),
                      "completion": C, "allocation": A, "total_bytes": T}

    def bound_cap(self):
        if self.cap is not None:
            return self.cap
        return self.best[0] if self.best is not None else INF

    def candidate_paths(self, start, parent):
        """Simple paths from start: off-tree prefix, then the tree suffix (or the sink)."""
        res = []
        sink = self.p.sink

        def follow(node):
            path = []
            while node != sink:
                lid = parent[node]
                path.append(lid)
                node = self.links[lid].to
            return path

        if start in parent:
            return [follow(start)]
        prefix = []
        seen = {start}

        def dfs(node):
            for l in self.out.get(node, []):
                to = l.to
                if to in seen:
                    continue
                if to == sink:
                    res.append(prefix + [l.id])
                elif to in parent:
                    res.append(prefix + [l.id] + follow(to))
                else:
                    seen.add(to); prefix.append(l.id)
                    dfs(to)
                    prefix.pop(); seen.discard(to)
        dfs(start)
        return res

    def telemetry_consistent(self, parent, counts_all, routed, remaining):
        """Prune on listed links: a required link whose tail is already routed
        elsewhere; a required link whose tail no unassigned source can still
        reach off-tree; a listed count no extension can meet."""
        tree_links = set(parent.values())
        missing_tails = []
        for lid in self.required:
            if lid in tree_links:
                continue
            u = self.tails[lid]
            if u in parent:
                return False
            missing_tails.append(u)
        if missing_tails:
            if not remaining:
                return False
            reach = set()
            stack = [s.node for s in remaining if s.node not in parent]
            reach.update(stack)
            while stack:
                n = stack.pop()
                for l in self.out.get(n, []):
                    t = l.to
                    if t in parent or t == self.p.sink or t in reach:
                        continue
                    reach.add(t); stack.append(t)
            for u in missing_tails:
                if u not in reach:
                    return False
        for lid, k in self.count_req.items():
            if lid not in tree_links:
                continue
            u = self.tails[lid]
            # upper bound: sources routed through so far plus every unassigned source
            if routed.get(lid, 0) + len(remaining) < k:
                return False
            # lower bound when the tail cannot reduce: one payload per child branch plus the sources at the tail
            if u not in self.capable:
                branches = len(self.sources_at.get(u, [])) + sum(1 for n, l in parent.items() if self.links[l].to == u)
                if branches > k:
                    return False
        return True

    def dfs(self, i, parent, assigned):
        self.nodes += 1
        if self.budget_s is not None and (self.nodes & 255) == 0 and time.time() - self.t0 > self.budget_s:
            raise Budget()
        if i == len(self.srcs):
            self.leaf(parent, assigned)
            return
        s = self.order[i]
        cands = self.candidate_paths(s.node, parent)
        scored = []
        for path in cands:
            cost = sum(self.occ[lid] + self.links[lid].latency for lid in path)
            scored.append((cost, path))
        scored.sort(key=lambda x: (x[0], [(self.links[l].frm, l) for l in x[1]]))
        for cost, path in scored:
            added = []
            node = s.node
            ok = True
            for lid in path:
                if node in parent:
                    if parent[node] != lid:
                        ok = False
                        break
                else:
                    parent[node] = lid; added.append(node)
                node = self.links[lid].to
            if ok:
                new_assigned = assigned + [s.id]
                rep = self.replay_tree(parent, set(new_assigned), "all")
                lb = rep[0]
                remaining = self.order[i + 1:]
                for t in remaining:
                    lb = max(lb, self.dist[t.node] if t.node not in parent else self.tree_cost(t.node, parent))
                if lb <= self.bound_cap():
                    routed = {}
                    for t in self.srcs:
                        if t.id in new_assigned:
                            node = t.node
                            while node != self.p.sink:
                                lid = parent[node]; routed[lid] = routed.get(lid, 0) + 1; node = self.links[lid].to
                    if self.telemetry_consistent(parent, rep[2], routed, remaining):
                        self.dfs(i + 1, parent, new_assigned)
            for n in added:
                del parent[n]

    def tree_cost(self, node, parent):
        c = 0
        while node != self.p.sink:
            lid = parent[node]; c += self.occ[lid] + self.links[lid].latency; node = self.links[lid].to
        return c

    def leaf(self, parent, assigned):
        self.leaves += 1
        sink = self.p.sink
        # required links must be on the tree
        tree_links = set(parent.values())
        for lid in self.count_req:
            if lid not in tree_links:
                return
        paths = []
        for s in self.srcs:
            node = s.node; path = []
            while node != sink:
                lid = parent[node]; path.append(lid); node = self.links[lid].to
            paths.append(path)
        visited = sorted(n for n in parent if n in self.capable)
        children = {}
        for n, lid in parent.items():
            children.setdefault(self.links[lid].to, []).append(n)
        all_assigned = set(assigned)
        for k in range(len(visited) + 1):
            for sub in itertools.combinations(visited, k):
                sset = set(sub)
                # payload counts without replay
                cnt = {}
                bad = False
                def count(n):
                    c = len([1 for sid in self.sources_at.get(n, [])])
                    for ch in children.get(n, []):
                        cc = count(ch)
                        lid = parent[ch]
                        if ch in sset:
                            if cc < 2:
                                raise Illegal("two")
                            cnt[lid] = 1
                            c += 1
                        else:
                            cnt[lid] = cc
                            c += cc
                    return c
                try:
                    count(sink)
                except Illegal:
                    continue
                if any(cnt.get(lid, 0) != k_ for lid, k_ in self.count_req.items()):
                    continue
                self.replays += 1
                rep = self.replay_tree(parent, all_assigned, sset)
                if rep is None:
                    continue
                C, A, counts, _ = rep
                if self.cap is not None and C != self.cap:
                    continue
                if any(counts.get(lid, 0) != k_ for lid, k_ in self.count_req.items()):
                    continue
                T = self.B * sum(counts.values())
                key = self.plan_key(paths, sub)
                self.matching += 1
                tup = (C, A, T, key)
                if self.best is None or tup < self.best[:4]:
                    self.best = (C, A, T, key, [list(x) for x in paths], list(sub), rep)


def recover_c(p: Problem, budget_s=None):
    s = SearchC(p, budget_s)
    st, plan = s.run()
    return st, plan, {"nodes": s.nodes, "leaves": s.leaves, "replays": s.replays, "matching": s.matching, "seconds": round(time.time() - s.t0, 3)}


if __name__ == "__main__":
    import json
    doc = json.load(open(sys.argv[1]))
    print(json.dumps(recover_c(model.load_problem(doc)), indent=1, default=str))
