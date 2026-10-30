#!/usr/bin/env python3
"""Oracle D: independent pruned exact search (author-side, never ships).

Trees are built node by node with a parent pointer per routed node (the
organisation of oracle B), pruned by the completion of the already-resolved
endpoints under the event-driven simulation with every capable switch that
gathers two or more payloads selected, and by the listed telemetry. Leaves
enumerate combine subsets with the event-driven simulation. Shares no search
or replay code with oracle C (research/bnb.py)."""
import sys, os, heapq, itertools, time, copy
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import oracle_b
from oracle_b import IllegalB, simulate, valid_b, paths_from_tree, key_b

INF = float("inf")


class BudgetD(Exception):
    pass


class SearchD:
    def __init__(self, d, budget_s=None):
        self.d = d
        self.B = d["payload_bytes"]; self.sink = d["sink"]
        self.link = {l["id"]: l for l in d["links"]}
        tel = d["telemetry"]
        self.zero = {lid for lid, b in tel["link_bytes"].items() if b == 0}
        self.req = {lid: b // self.B for lid, b in tel["link_bytes"].items() if b > 0 and b % self.B == 0}
        self.bad = any(b < 0 or b % self.B != 0 for b in tel["link_bytes"].values())
        self.cap_completion = tel.get("completion")
        self.out = {}
        for l in d["links"]:
            if l["id"] in self.zero: continue
            self.out.setdefault(l["from"], []).append(l)
        for k in self.out: self.out[k].sort(key=lambda l: l["id"])
        self.capable = {w["id"] for w in d["switches"] if w["reduction_capable"] and w["accumulator_capacity"] >= self.B}
        self.at = {}
        for s in d["sources"]: self.at.setdefault(s["node"], []).append(s["id"])
        self.ends = sorted(self.at)
        self.cost = {l["id"]: -(-self.B // l["rate"]) + l["latency"] for l in d["links"]}
        self.dist = self.dijkstra()
        self.budget_s = budget_s; self.t0 = time.time()
        self.nodes = 0; self.leaves = 0; self.matching = 0
        self.best = None

    def dijkstra(self):
        rev = {}
        for l in self.d["links"]:
            if l["id"] in self.zero: continue
            rev.setdefault(l["to"], []).append((l["from"], self.cost[l["id"]]))
        dist = {self.sink: 0}; h = [(0, self.sink)]
        while h:
            dd, n = heapq.heappop(h)
            if dd > dist.get(n, INF): continue
            for m, w in rev.get(n, []):
                if dd + w < dist.get(m, INF):
                    dist[m] = dd + w; heapq.heappush(h, (dd + w, m))
        return dist

    def resolved(self, parent):
        """Endpoints whose parent chain reaches the sink."""
        res = []
        for e in self.ends:
            n = e; seen = set()
            while n != self.sink and n in parent and n not in seen:
                seen.add(n); n = self.link[parent[n]]["to"]
            if n == self.sink: res.append(e)
        return res

    def bound(self, parent, res):
        """Completion of the resolved endpoints' sources with every capable switch that gathers >= 2 payloads selected."""
        if not res:
            return 0, {}
        sub = {"payload_bytes": self.B, "sink": self.sink, "switches": self.d["switches"], "links": self.d["links"],
               "sources": [s for s in self.d["sources"] if s["node"] in res], "telemetry": {"link_bytes": {}}}
        # restrict the parent map to the resolved chains
        par = {}
        for e in res:
            n = e
            while n != self.sink:
                par[n] = parent[n]; n = self.link[parent[n]]["to"]
        children = {}
        for n, lid in par.items(): children.setdefault(self.link[lid]["to"], []).append(n)
        selected = set()
        def count(n):
            c = len(self.at.get(n, [])) if n in res else 0
            for ch in children.get(n, []):
                cc = count(ch)
                c += 1 if ch in selected else cc
            if n in self.capable and n != self.sink and c >= 2:
                selected.add(n)
            return c
        count(self.sink)
        completion, _, lb, _ = simulate(sub, par, sorted(selected))
        return completion, lb

    def cap(self):
        if self.cap_completion is not None: return self.cap_completion
        return self.best[0] if self.best is not None else INF

    def run(self):
        if not valid_b(self.d): return "invalid", None
        if self.bad: return "infeasible", None
        if any(e not in self.dist for e in self.ends): return "infeasible", None
        self.rec(list(self.ends), {})
        if self.best is None: return "infeasible", None
        C, A, T, key, plan = self.best
        return "ok", plan

    def rec(self, pending, parent):
        self.nodes += 1
        if self.budget_s is not None and (self.nodes & 255) == 0 and time.time() - self.t0 > self.budget_s:
            raise BudgetD()
        pending = [n for n in pending if n != self.sink and n not in parent]
        if not pending:
            self.leaf(parent); return
        node = pending[0]
        for l in self.out.get(node, []):
            # no cycle: walking from the target along parents must not return to node
            cur, chain, cyc = l["to"], set(), False
            while cur != self.sink and cur in parent:
                if cur == node or cur in chain: cyc = True; break
                chain.add(cur); cur = self.link[parent[cur]]["to"]
            if cyc or cur == node: continue
            # a required link leaving this node must be the one chosen
            if any(self.link[r]["from"] == node and r != l["id"] for r in self.req): continue
            parent[node] = l["id"]
            res = self.resolved(parent)
            lb, _ = self.bound(parent, res)
            for e in self.ends:
                if e not in res: lb = max(lb, self.dist[e])
            if lb <= self.cap() and self.counts_possible(parent, res):
                # depth first: the target of the chosen link is routed next, so an endpoint resolves before the next one starts
                self.rec(([l["to"]] if l["to"] != self.sink and l["to"] not in parent else []) + pending[1:], parent)
            del parent[node]

    def counts_possible(self, parent, res):
        routed = {}
        for e in res:
            n = e; k = len(self.at[e])
            while n != self.sink:
                routed[parent[n]] = routed.get(parent[n], 0) + k; n = self.link[parent[n]]["to"]
        unresolved = sum(len(self.at[e]) for e in self.ends if e not in res)
        used = set(parent.values())
        for lid, k in self.req.items():
            if lid not in used: continue
            if routed.get(lid, 0) + unresolved < k: return False
            u = self.link[lid]["from"]
            if u not in self.capable:
                branches = len(self.at.get(u, [])) + sum(1 for n, x in parent.items() if self.link[x]["to"] == u)
                if branches > k: return False
        return True

    def leaf(self, parent):
        self.leaves += 1
        used = set(parent.values())
        if any(r not in used for r in self.req): return
        capable_visited = sorted(n for n in parent if n in self.capable)
        paths = paths_from_tree(self.d, parent)
        for k in range(len(capable_visited) + 1):
            for sub in itertools.combinations(capable_visited, k):
                try:
                    completion, allocation, lb, sv = simulate(self.d, parent, list(sub))
                except IllegalB:
                    continue
                if self.cap_completion is not None and completion != self.cap_completion: continue
                if any(lb.get(l, 0) != b for l, b in self.d["telemetry"]["link_bytes"].items()): continue
                self.matching += 1
                key = (completion, allocation, sum(lb.values()), key_b(self.d, paths, list(sub)))
                if self.best is None or key < self.best[:4]:
                    self.best = (completion, allocation, sum(lb.values()), key[3],
                                 {"paths": paths, "combine": list(sub), "sink_value": sv, "completion": completion, "allocation": allocation, "total_bytes": sum(lb.values())})


def recover_d(d, budget_s=None):
    s = SearchD(d, budget_s)
    st, plan = s.run()
    return st, plan, {"nodes": s.nodes, "leaves": s.leaves, "matching": s.matching, "seconds": round(time.time() - s.t0, 3)}


if __name__ == "__main__":
    import json
    print(json.dumps(recover_d(json.load(open(sys.argv[1]))), indent=1, default=str))
