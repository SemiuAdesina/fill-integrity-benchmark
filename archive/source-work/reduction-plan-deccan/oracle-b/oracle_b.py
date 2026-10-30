#!/usr/bin/env python3
"""ORACLE B — independent exact search written from the specification text.

Organisation deliberately differs from oracle A (research/model.py):
  * plans are enumerated as parent-pointer trees toward the sink (one
    outgoing link per routed node), not as products of per-source paths;
  * replay is an event-driven simulation with per-link waiting queues and a
    pre-computed expected-arrival count per selected switch, not a recursive
    bottom-up fold;
  * legality, telemetry, objective and plan_key are re-implemented on raw
    JSON dicts.
Author/verifier side only; never ships.
"""
import heapq, itertools, json

INT32_MIN, INT32_MAX = -(1 << 31), (1 << 31) - 1


class IllegalB(Exception):
    pass


def valid_b(d):
    if d["payload_bytes"] < 1 or not d["sources"] or d["sink"] == "":
        return False
    for l in d["links"]:
        if l["rate"] < 1 or l["latency"] < 0 or l["id"] == "" or l["from"] == "" or l["to"] == "":
            return False
    for w in d["switches"]:
        if w["accumulator_capacity"] < 0 or w["id"] == "":
            return False
    for s in d["sources"]:
        if s["id"] == "" or s["node"] == "" or not (INT32_MIN <= s["value"] <= INT32_MAX):
            return False
    for key, coll in (("id", d["sources"]), ("id", d["switches"]), ("id", d["links"])):
        ids = [x[key] for x in coll]
        if len(ids) != len(set(ids)):
            return False
    ends = {s["node"] for s in d["sources"]}; sws = {w["id"] for w in d["switches"]}
    if d["sink"] in ends or d["sink"] in sws or ends & sws:
        return False
    nodes = ends | sws | {d["sink"]}
    return all(l["from"] in nodes and l["to"] in nodes and l["from"] != l["to"] for l in d["links"])


def trees(d):
    """Yield parent maps {node: link_id} routing every endpoint node to the sink without cycles."""
    out = {}
    for l in d["links"]:
        out.setdefault(l["from"], []).append(l)
    link_to = {l["id"]: l["to"] for l in d["links"]}
    endpoints = sorted({s["node"] for s in d["sources"]})
    sink = d["sink"]

    def resolved_to_sink(node, parent):
        seen = set()
        while node != sink:
            if node in seen or node not in parent:
                return False
            seen.add(node); node = link_to[parent[node]]
        return True

    def rec(pending, parent):
        pending = [n for n in pending if n != sink and n not in parent]
        if not pending:
            yield dict(parent); return
        node = pending[0]
        for l in sorted(out.get(node, []), key=lambda l: l["id"]):
            # the chosen link must not close a cycle: walk from its target along existing parents
            chain, cur, cyc = set(), l["to"], False
            while cur != sink and cur in parent:
                if cur == node or cur in chain:
                    cyc = True; break
                chain.add(cur); cur = link_to[parent[cur]]
            if cyc or cur == node:
                continue
            parent[node] = l["id"]
            yield from rec(pending[1:] + ([l["to"]] if l["to"] != sink and l["to"] not in parent else []), parent)
            del parent[node]

    for parent in rec(endpoints, {}):
        if all(resolved_to_sink(e, parent) for e in endpoints):
            yield parent


def simulate(d, parent, combine):
    """Event-driven replay. Returns (completion, allocation, link_bytes, sink_value) or raises IllegalB."""
    links = {l["id"]: l for l in d["links"]}
    caps = {w["id"]: w for w in d["switches"]}
    sink = d["sink"]; B = d["payload_bytes"]
    values = {s["id"]: s["value"] for s in d["sources"]}
    cset = set(combine)
    if list(combine) != sorted(set(combine)):
        raise IllegalB("combine order")
    visited = set(parent)
    for c in combine:
        if c not in caps or not caps[c]["reduction_capable"] or c not in visited or c == sink:
            raise IllegalB("selected switch")
        if B > caps[c]["accumulator_capacity"]:
            raise IllegalB("capacity")
    # expected payload count per node
    children = {}
    for n, lid in parent.items():
        children.setdefault(links[lid]["to"], []).append(n)
    at_node = {}
    for s in d["sources"]:
        at_node[s["node"]] = at_node.get(s["node"], 0) + 1
    def count(n):
        c = at_node.get(n, 0) + sum(1 if ch in cset else count(ch) for ch in children.get(n, []))
        return c
    expected = {n: count(n) for n in set(children) | set(at_node)}
    for c in combine:
        if expected.get(c, 0) < 2:
            raise IllegalB("fewer than two")
    # event simulation
    events = []  # (tick, kind, node, origins)  kind 0 = arrival
    seq = itertools.count()
    for s in d["sources"]:
        heapq.heappush(events, (0, next(seq), s["node"], frozenset([s["id"]])))
    waiting = {}      # link -> list of (ready, origins)
    link_free = {}
    link_bytes = {}
    gathered = {}     # node -> list of (tick, origins)
    completion = 0; allocation = 0; sink_origins = set()
    def send(node, tick, origins):
        lid = parent[node]
        waiting.setdefault(lid, []).append((tick, origins))
    def flush(lid, now):
        """serve waiting payloads on lid whose ready <= now in (ready, smallest origin) order"""
        l = links[lid]
        free = link_free.get(lid, 0)
        q = waiting.get(lid, [])
        q.sort(key=lambda x: (x[0], min(x[1])))
        while q and q[0][0] <= now:
            ready, org = q.pop(0)
            start = max(ready, free)
            occ = -(-B // l["rate"])
            free = start + occ
            link_bytes[lid] = link_bytes.get(lid, 0) + B
            heapq.heappush(events, (free + l["latency"], next(seq), l["to"], org))
        link_free[lid] = free
    while events:
        now = events[0][0]
        batch = []
        while events and events[0][0] == now:
            batch.append(heapq.heappop(events))
        touched = set()
        for tick, _, node, org in batch:
            if node == sink:
                completion = max(completion, tick); sink_origins |= org; continue
            if node in cset:
                g = gathered.setdefault(node, [])
                g.append((tick, org))
                if len(g) == expected[node]:
                    allocation += B
                    union = frozenset().union(*[o for _, o in g])
                    send(node, max(t for t, _ in g), union)
                    touched.add(parent[node])
                continue
            send(node, tick, org)
            touched.add(parent[node])
        # every payload queued in this batch is ready now; serve each touched link in (ready, smallest origin) order
        for lid in sorted(touched):
            flush(lid, now)
    if sink_origins != {s["id"] for s in d["sources"]}:
        raise IllegalB("not every origin reached the sink")
    ids = sorted(sink_origins)
    acc = values[ids[0]]
    for i in ids[1:]:
        acc = max(INT32_MIN, min(INT32_MAX, acc + values[i]))
    return completion, allocation, link_bytes, acc


def paths_from_tree(d, parent):
    link_to = {l["id"]: l["to"] for l in d["links"]}
    out = []
    for s in sorted(d["sources"], key=lambda s: s["id"]):
        cur = s["node"]; path = []
        while cur != d["sink"]:
            path.append(parent[cur]); cur = link_to[parent[cur]]
        out.append(path)
    return out


def key_b(d, paths, combine):
    frm = {l["id"]: l["from"] for l in d["links"]}
    return ([(frm[l].encode(), l.encode()) for p in paths for l in p], [c.encode() for c in combine])


def recover_b(d):
    if not valid_b(d):
        return "invalid", None
    caps = sorted(w["id"] for w in d["switches"] if w["reduction_capable"])
    best = None
    for parent in trees(d):
        cands = [c for c in caps if c in parent]
        for k in range(len(cands) + 1):
            for sub in itertools.combinations(cands, k):
                try:
                    completion, allocation, lb, sv = simulate(d, parent, list(sub))
                except IllegalB:
                    continue
                if any(lb.get(l, 0) != b for l, b in d["telemetry"]["link_bytes"].items()):
                    continue
                if "completion" in d["telemetry"] and d["telemetry"]["completion"] != completion:
                    continue
                paths = paths_from_tree(d, parent)
                key = (completion, allocation, sum(lb.values()), key_b(d, paths, list(sub)))
                if best is None or key < best[0]:
                    best = (key, {"paths": paths, "combine": list(sub), "sink_value": sv, "completion": completion, "allocation": allocation, "total_bytes": sum(lb.values())})
    if best is None:
        return "infeasible", None
    return "ok", best[1]


if __name__ == "__main__":
    import sys
    print(json.dumps(recover_b(json.load(open(sys.argv[1]))), indent=1))
