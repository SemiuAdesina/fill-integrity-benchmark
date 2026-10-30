#!/usr/bin/env python3
"""Author-side executable model of the PUBLIC reduction-plan contract
(spec/problem-format-v1.md, fabric-replay-v1.md, plan-recovery-v1.md,
result-format-v1.md). Oracle A = exhaustive path-product x combine-subset
search over this model. Never ships."""
import json, itertools
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple

INT32_MIN, INT32_MAX = -(1 << 31), (1 << 31) - 1


@dataclass
class Source:
    id: str; node: str; value: int


@dataclass
class Switch:
    id: str; reduction_capable: bool; accumulator_capacity: int


@dataclass
class Link:
    id: str; frm: str; to: str; rate: int; latency: int


@dataclass
class Problem:
    payload_bytes: int
    sink: str
    sources: List[Source]
    switches: List[Switch]
    links: List[Link]
    link_bytes: Dict[str, int]
    completion: Optional[int]

    def to_json(self):
        t = {"link_bytes": dict(self.link_bytes)}
        if self.completion is not None:
            t["completion"] = self.completion
        return {"format_version": 1, "payload_bytes": self.payload_bytes, "sink": self.sink,
                "sources": [{"id": s.id, "node": s.node, "value": s.value} for s in self.sources],
                "switches": [{"id": w.id, "reduction_capable": w.reduction_capable, "accumulator_capacity": w.accumulator_capacity} for w in self.switches],
                "links": [{"id": l.id, "from": l.frm, "to": l.to, "rate": l.rate, "latency": l.latency} for l in self.links],
                "telemetry": t}


def load_problem(doc) -> Problem:
    assert doc["format_version"] == 1
    t = doc["telemetry"]
    return Problem(doc["payload_bytes"], doc["sink"],
                   [Source(s["id"], s["node"], s["value"]) for s in doc["sources"]],
                   [Switch(w["id"], w["reduction_capable"], w["accumulator_capacity"]) for w in doc["switches"]],
                   [Link(l["id"], l["from"], l["to"], l["rate"], l["latency"]) for l in doc["links"]],
                   dict(t["link_bytes"]), t.get("completion"))


# ---------------------------------------------------------------- validity
def valid(p: Problem) -> bool:
    """problem-format-v1.md section 3."""
    if p.payload_bytes < 1 or not p.sources or not p.sink:
        return False
    if any(l.rate < 1 or l.latency < 0 for l in p.links) or any(w.accumulator_capacity < 0 for w in p.switches):
        return False
    if any(not (INT32_MIN <= s.value <= INT32_MAX) for s in p.sources):
        return False
    if any(not s.id or not s.node for s in p.sources) or any(not w.id for w in p.switches) or any(not l.id or not l.frm or not l.to for l in p.links):
        return False
    sids = [s.id for s in p.sources]; wids = [w.id for w in p.switches]; lids = [l.id for l in p.links]
    if len(set(sids)) != len(sids) or len(set(wids)) != len(wids) or len(set(lids)) != len(lids):
        return False
    endpoints = {s.node for s in p.sources}; switches = set(wids)
    if p.sink in endpoints or p.sink in switches or (endpoints & switches):
        return False
    nodes = endpoints | switches | {p.sink}
    if any(l.frm not in nodes or l.to not in nodes or l.frm == l.to for l in p.links):
        return False
    return True


# ------------------------------------------------------------------ replay
def sat_add(a, b):
    return max(INT32_MIN, min(INT32_MAX, a + b))


def fold(values_by_id: Dict[str, int], origins) -> int:
    ids = sorted(origins)
    acc = values_by_id[ids[0]]
    for i in ids[1:]:
        acc = sat_add(acc, values_by_id[i])
    return acc


class Illegal(Exception):
    pass


def replay(p: Problem, paths: List[List[str]], combine: List[str]):
    """Replays a plan. Returns dict(completion, allocation, link_bytes, total_bytes, sink_value)
    or raises Illegal. `paths` aligned with sources sorted by id."""
    links = {l.id: l for l in p.links}
    switches = {w.id: w for w in p.switches}
    srcs = sorted(p.sources, key=lambda s: s.id)
    if len(paths) != len(srcs):
        raise Illegal("path count")
    parent: Dict[str, str] = {}  # node -> outgoing link id in the tree
    for s, path in zip(srcs, paths):
        cur = s.node; seen = {cur}
        if not path:
            raise Illegal("empty path")
        for lid in path:
            l = links.get(lid)
            if l is None or l.frm != cur:
                raise Illegal("path not connected")
            if l.to in seen:
                raise Illegal("path not simple")
            if cur in parent and parent[cur] != lid:
                raise Illegal("suffix inconsistency")
            parent[cur] = lid; seen.add(l.to); cur = l.to
        if cur != p.sink:
            raise Illegal("path does not end at sink")
    cset = list(combine)
    if cset != sorted(cset) or len(set(cset)) != len(cset):
        raise Illegal("combine order")
    visited = set(parent) | {p.sink}
    for c in cset:
        w = switches.get(c)
        if w is None or not w.reduction_capable or c not in visited or c == p.sink:
            raise Illegal("selected switch illegal")
    children: Dict[str, List[Tuple[str, str]]] = {}
    for node, lid in parent.items():
        children.setdefault(links[lid].to, []).append((node, lid))
    values = {s.id: s.value for s in p.sources}
    lb: Dict[str, int] = {}
    allocation = 0
    B = p.payload_bytes

    def process(node):  # -> list of payloads (ready, origins frozenset)
        pay = [(0, frozenset([s.id])) for s in srcs if s.node == node]
        for child, lid in children.get(node, []):
            l = links[lid]
            outgoing = process(child)
            free = 0
            for ready, org in sorted(outgoing, key=lambda x: (x[0], min(x[1]))):
                start = max(ready, free)
                occ = -(-B // l.rate)
                free = start + occ
                pay.append((free + l.latency, org))
                lb[lid] = lb.get(lid, 0) + B
        if node in cset:
            if len(pay) < 2:
                raise Illegal("fewer than two payloads at selected switch")
            if B > switches[node].accumulator_capacity:
                raise Illegal("capacity")
            nonlocal allocation
            allocation += B
            origins = frozenset().union(*[o for _, o in pay])
            return [(max(r for r, _ in pay), origins)]
        return pay

    sink_pay = process(p.sink)
    completion = max(r for r, _ in sink_pay)
    origins = frozenset().union(*[o for _, o in sink_pay])
    return {"completion": completion, "allocation": allocation, "link_bytes": lb, "total_bytes": sum(lb.values()),
            "sink_value": fold(values, origins)}


def matches_telemetry(p: Problem, rep) -> bool:
    for lid, b in p.link_bytes.items():
        if rep["link_bytes"].get(lid, 0) != b:
            return False
    if p.completion is not None and rep["completion"] != p.completion:
        return False
    return True


def plan_key(p: Problem, paths, combine):
    links = {l.id: l for l in p.links}
    flat = []
    for path in paths:
        for lid in path:
            flat.append((links[lid].frm.encode(), lid.encode()))
    return (flat, [c.encode() for c in combine])


def objective(p, paths, combine, rep):
    return (rep["completion"], rep["allocation"], rep["total_bytes"], plan_key(p, paths, combine))


# ------------------------------------------------------------------ search
def simple_paths(p: Problem, start: str):
    out: Dict[str, List[Link]] = {}
    for l in p.links:
        out.setdefault(l.frm, []).append(l)
    res = []
    def dfs(node, seen, path):
        if node == p.sink:
            res.append(list(path)); return
        for l in sorted(out.get(node, []), key=lambda l: l.id):
            if l.to in seen:
                continue
            dfs(l.to, seen | {l.to}, path + [l.id])
    dfs(start, {start}, [])
    return res


def recover(p: Problem, count_only=False):
    """Oracle A: exhaustive. Returns ("invalid"|"infeasible"|"ok", plan, info)."""
    if not valid(p):
        return "invalid", None, {}
    srcs = sorted(p.sources, key=lambda s: s.id)
    allpaths = [simple_paths(p, s.node) for s in srcs]
    if any(not ps for ps in allpaths):
        return "infeasible", None, {"reason": "a source cannot reach the sink"}
    caps = sorted(w.id for w in p.switches if w.reduction_capable)
    best = None; n_legal = 0; n_match = 0; n_replays = 0
    for combo in itertools.product(*allpaths):
        # suffix consistency check up front (cheap)
        links = {l.id: l for l in p.links}
        parent = {}; ok = True
        for s, path in zip(srcs, combo):
            cur = s.node
            for lid in path:
                if cur in parent and parent[cur] != lid:
                    ok = False; break
                parent[cur] = lid; cur = links[lid].to
            if not ok: break
        if not ok:
            continue
        visited = set(parent)
        cands = [c for c in caps if c in visited]
        for k in range(len(cands) + 1):
            for sub in itertools.combinations(cands, k):
                n_replays += 1
                try:
                    rep = replay(p, list(combo), list(sub))
                except Illegal:
                    continue
                n_legal += 1
                if not matches_telemetry(p, rep):
                    continue
                n_match += 1
                key = objective(p, combo, sub, rep)
                if best is None or key < best[0]:
                    best = (key, list(combo), list(sub), rep)
    info = {"replays": n_replays, "legal": n_legal, "matching": n_match}
    if best is None:
        return "infeasible", None, info
    key, paths, comb, rep = best
    return "ok", {"paths": [list(x) for x in paths], "combine": list(comb), "sink_value": rep["sink_value"], "replay": rep}, info


# --------------------------------------------------------------- documents
def result_json(status, plan=None) -> str:
    if status != "ok":
        return '{"status": "%s"}\n' % status
    paths = ", ".join("[" + ", ".join(json.dumps(l) for l in path) + "]" for path in plan["paths"])
    comb = ", ".join(json.dumps(c) for c in plan["combine"])
    return '{\n "format_version": 1,\n "status": "ok",\n "paths": [%s],\n "combine": [%s],\n "sink_value": %d\n}\n' % (paths, comb, plan["sink_value"])


def parse_result(text: str):
    doc = json.loads(text)
    if set(doc) <= {"status", "reason"} and doc.get("status") in ("invalid", "infeasible"):
        return doc["status"], None
    if set(doc) != {"format_version", "status", "paths", "combine", "sink_value"} or doc["format_version"] != 1 or doc["status"] != "ok":
        raise ValueError("shape")
    return "ok", {"paths": doc["paths"], "combine": doc["combine"], "sink_value": doc["sink_value"]}


def self_test():
    p = Problem(8, "S", [Source("a", "A", 90), Source("b", "B", -50)],
                [Switch("X", True, 16)],
                [Link("a-x", "A", "X", 8, 1), Link("b-x", "B", "X", 8, 1), Link("x-s", "X", "S", 8, 1)], {"x-s": 8}, 4)
    st, plan, info = recover(p)
    assert st == "ok" and plan["combine"] == ["X"] and plan["sink_value"] == 40 and plan["replay"]["completion"] == 4, (st, plan)
    # without combine: x-s carries both, serialized: a ready 2 (occ 1 + lat 1), b ready 2; order (2,'a') then (2,'b'):
    # a: start 2, free 3, arrive 4; b: start 3, free 4, arrive 5 -> completion 5, bytes 16 (pipelined latency)
    rep = replay(p, [["a-x", "x-s"], ["b-x", "x-s"]], [])
    assert rep["completion"] == 5 and rep["link_bytes"]["x-s"] == 16 and rep["sink_value"] == 40, rep
    p2 = Problem(8, "S", [Source("a", "A", 90)], [], [Link("z", "A", "S", 4, 1), Link("a", "A", "S", 4, 1)], {}, None)
    st, plan, _ = recover(p2)
    assert plan["paths"] == [["a"]]  # equal cost, canonical key picks link "a"
    bad = Problem(0, "S", [Source("a", "A", 1)], [], [Link("l", "A", "S", 1, 0)], {}, None)
    assert recover(bad)[0] == "invalid"
    p3 = Problem(8, "S", [Source("a", "A", 1)], [], [Link("l", "A", "S", 8, 0)], {"l": 16}, None)
    assert recover(p3)[0] == "infeasible"
    assert parse_result(result_json("ok", plan))[1]["paths"] == [["a"]]
    print("reduction model self-test ok")


if __name__ == "__main__":
    self_test()
