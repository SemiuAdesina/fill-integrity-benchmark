#!/usr/bin/env python3
"""Author-side fixture generator with certification and trap verification.

Each fixture is a small fabric (random or hand-shaped) plus telemetry taken
from a latent legacy plan. Certification: oracle A (research/model.py,
path-product search, recursive replay) and oracle B (oracle-b/oracle_b.py,
parent-pointer trees, event-driven replay) must agree on status, plan and
sink value, both within budget. Trap fixtures are kept only when the
wrong-model family they target produces a different answer, so every hidden
layout is known to separate something. Latent plans never ship.

Writes author/fixtures/<id>/{problem.json,expected.json,certification.json,result.expected.json}.
"""
import sys, os, json, random, shutil, time, itertools, copy
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "research")); sys.path.insert(0, os.path.join(HERE, "..", "oracle-b"))
import model
from model import Problem, Source, Switch, Link, Illegal
import oracle_b

BUDGET_S = 20.0


# ------------------------------------------------- wrong-model families (for trap verification only)
def replay_variant(p, paths, combine, *, lat_in_occ=False, tie_input=False, no_serial=False, allow_one=False, no_capacity=False, sink_alloc=False, fold_mode="spec"):
    links = {l.id: l for l in p.links}; switches = {w.id: w for w in p.switches}
    srcs = sorted(p.sources, key=lambda s: s.id); pos = {s.id: i for i, s in enumerate(p.sources)}
    parent = {}
    for s, path in zip(srcs, paths):
        cur = s.node; seen = {cur}
        for lid in path:
            l = links.get(lid)
            if l is None or l.frm != cur or l.to in seen or (cur in parent and parent[cur] != lid): raise Illegal("path")
            parent[cur] = lid; seen.add(l.to); cur = l.to
        if cur != p.sink: raise Illegal("end")
    for c in combine:
        w = switches.get(c)
        if w is None or not w.reduction_capable or c not in parent: raise Illegal("sel")
    children = {}
    for n, lid in parent.items(): children.setdefault(links[lid].to, []).append((n, lid))
    values = {s.id: s.value for s in p.sources}; lb = {}; alloc = [0]; B = p.payload_bytes
    def val(origins):
        ids = sorted(origins)
        if fold_mode == "sumclamp": return max(model.INT32_MIN, min(model.INT32_MAX, sum(values[i] for i in ids)))
        if fold_mode == "input": ids = sorted(origins, key=lambda i: pos[i])
        acc = values[ids[0]]
        for i in ids[1:]: acc = model.sat_add(acc, values[i])
        return acc
    def process(node):
        pay = [(0, frozenset([s.id])) for s in srcs if s.node == node]
        for child, lid in sorted(children.get(node, [])):
            l = links[lid]; free = 0
            key = (lambda x: (x[0], min(pos[o] for o in x[1]))) if tie_input else (lambda x: (x[0], min(x[1])))
            for ready, org in sorted(process(child), key=key):
                start = ready if no_serial else max(ready, free); occ = -(-B // l.rate)
                free = start + occ + (l.latency if lat_in_occ else 0)
                pay.append((start + occ + l.latency, org)); lb[lid] = lb.get(lid, 0) + B
        if node in combine:
            if len(pay) < 2 and not allow_one: raise Illegal("two")
            if B > switches[node].accumulator_capacity and not no_capacity: raise Illegal("cap")
            alloc[0] += B
            return [(max(r for r, _ in pay), frozenset().union(*[o for _, o in pay]))]
        return pay
    sp = process(p.sink)
    if sink_alloc: alloc[0] += B
    origins = frozenset().union(*[o for _, o in sp])
    return {"completion": max(r for r, _ in sp), "allocation": alloc[0], "link_bytes": lb, "total_bytes": sum(lb.values()), "sink_value": val(origins)}


def search_variant(p, *, objective="spec", telemetry="spec", per_source=False, key_mode="spec", subset="all", **rkw):
    if not model.valid(p): return "invalid", None
    srcs = sorted(p.sources, key=lambda s: s.id)
    allpaths = [model.simple_paths(p, s.node) for s in srcs]
    if any(not ps for ps in allpaths): return "infeasible", None
    if per_source:
        chosen = []
        for s, ps in zip(srcs, allpaths):
            def cost(path):
                t = 0
                for lid in path:
                    l = next(x for x in p.links if x.id == lid); t += -(-p.payload_bytes // l.rate) + l.latency
                return (t, len(path))
            chosen.append(min(ps, key=cost))
        allpaths = [[c] for c in chosen]
    caps = sorted(w.id for w in p.switches if w.reduction_capable)
    best = None
    for combo in itertools.product(*allpaths):
        visited = set()
        for path in combo:
            for lid in path: visited.add(next(l.frm for l in p.links if l.id == lid))
        cands = [c for c in caps if c in visited]
        if subset == "never": subsets = [()]
        elif subset == "maximal": subsets = [tuple(cands)]
        else: subsets = [sub for k in range(len(cands) + 1) for sub in itertools.combinations(cands, k)]
        for sub in subsets:
            if True:
                try: rep = replay_variant(p, list(combo), list(sub), **rkw)
                except Illegal: continue
                ok = True
                for lid, b in p.link_bytes.items():
                    if telemetry == "ignore_zero" and b == 0: continue
                    if rep["link_bytes"].get(lid, 0) != b: ok = False
                if p.completion is not None and rep["completion"] != p.completion: ok = False
                if not ok: continue
                pk = model.plan_key(p, combo, sub)
                if key_mode == "input":
                    order = {l.id: i for i, l in enumerate(p.links)}
                    pk = ([order[l] for path in combo for l in path], list(sub))
                if objective == "bytes_first": key = (rep["total_bytes"], rep["completion"], rep["allocation"], pk)
                elif objective == "alloc_after_bytes": key = (rep["completion"], rep["total_bytes"], rep["allocation"], pk)
                elif objective == "no_key": key = (rep["completion"], rep["allocation"], rep["total_bytes"])
                else: key = (rep["completion"], rep["allocation"], rep["total_bytes"], pk)
                if best is None or key < best[0]: best = (key, {"paths": [list(x) for x in combo], "combine": list(sub), "sink_value": rep["sink_value"]})
    return ("ok", best[1]) if best else ("infeasible", None)


FAMILIES = {
    "latency_in_occupancy": dict(lat_in_occ=True),
    "no_serialization": dict(no_serial=True),
    "tie_by_input_order": dict(tie_input=True),
    "per_source_shortest": dict(per_source=True),
    "one_payload_combine_allowed": dict(allow_one=True),
    "capacity_ignored": dict(no_capacity=True),
    "sink_counted_as_allocation": dict(sink_alloc=True),
    "sum_then_clamp": dict(fold_mode="sumclamp"),
    "fold_in_input_order": dict(fold_mode="input"),
    "zero_telemetry_ignored": dict(telemetry="ignore_zero"),
    "bytes_before_completion": dict(objective="bytes_first"),
    "allocation_after_bytes": dict(objective="alloc_after_bytes"),
    "key_by_input_order": dict(key_mode="input"),
    "combine_all_visited": dict(subset="maximal"),
    "never_combine": dict(subset="never"),
}


# ---------------------------------------------------------------- helpers
def random_fabric(rng, n_sources, n_red, n_plain, extra_links, rates=(4, 8, 16), lats=(0, 1, 2, 3), payload=8, shared_endpoint=False, caps=None):
    """A layered fabric: endpoints -> switches -> sink with some cross and direct links."""
    names = [chr(ord('a') + i) for i in range(n_sources)]
    rng.shuffle(names)  # source ids in an order that differs from the input order
    ends = ["E%d" % i for i in range(n_sources)]
    if shared_endpoint and n_sources >= 2:
        ends[1] = ends[0]
    sources = [Source(names[i], ends[i], rng.randint(-5000, 5000)) for i in range(n_sources)]
    switches = [Switch("R%d" % i, True, (caps[i] if caps else rng.choice([payload, payload, payload * 2, payload - 1]))) for i in range(n_red)]
    switches += [Switch("P%d" % i, False, 0) for i in range(n_plain)]
    sw_ids = [w.id for w in switches]
    links = []; lid = 0
    def add(frm, to):
        nonlocal lid
        lid += 1
        links.append(Link("l%02d" % lid, frm, to, rng.choice(rates), rng.choice(lats)))
    for e in sorted(set(ends)):
        for w in rng.sample(sw_ids, min(len(sw_ids), rng.randint(2, 3))): add(e, w)
        if rng.random() < 0.3: add(e, "S")
    for w in sw_ids:
        add(w, "S")
        for other in rng.sample([x for x in sw_ids if x != w], min(len(sw_ids) - 1, rng.randint(1, 2))):
            add(w, other)
    for _ in range(extra_links):
        frm = rng.choice(sorted(set(ends)) + sw_ids); to = rng.choice(sw_ids + ["S"])
        if frm != to and not any(l.frm == frm and l.to == to for l in links): add(frm, to)
    return Problem(payload, "S", sources, switches, links, {}, None)


def latent_telemetry(rng, p, plan_pick=None, listed_fraction=0.7, with_completion=None, zero_listed=True):
    """Pick a legal plan at random (or by predicate) and derive telemetry from its replay."""
    srcs = sorted(p.sources, key=lambda s: s.id)
    allpaths = [model.simple_paths(p, s.node) for s in srcs]
    caps = sorted(w.id for w in p.switches if w.reduction_capable)
    legal = []
    for combo in itertools.product(*allpaths):
        for k in range(len(caps) + 1):
            for sub in itertools.combinations(caps, k):
                try: rep = model.replay(p, list(combo), list(sub))
                except Illegal: continue
                legal.append((list(combo), list(sub), rep))
    if not legal: return None
    if plan_pick: legal = [x for x in legal if plan_pick(*x)] or legal
    combo, sub, rep = rng.choice(legal)
    lb = {}
    for l in p.links:
        b = rep["link_bytes"].get(l.id, 0)
        if (b > 0 and rng.random() < listed_fraction) or (b == 0 and zero_listed and rng.random() < 0.35): lb[l.id] = b
    p.link_bytes = lb
    if with_completion is None: with_completion = rng.random() < 0.6
    p.completion = rep["completion"] if with_completion else None
    return (combo, sub, rep)


def certify(p):
    t0 = time.time(); sa, pa, info = model.recover(p); ta = time.time() - t0
    t0 = time.time(); sb, pb = oracle_b.recover_b(p.to_json()); tb = time.time() - t0
    agree = sa == sb and (pa is None or (pa["paths"] == pb["paths"] and pa["combine"] == pb["combine"] and pa["sink_value"] == pb["sink_value"] and pa["replay"]["completion"] == pb["completion"] and pa["replay"]["allocation"] == pb["allocation"] and pa["replay"]["total_bytes"] == pb["total_bytes"]))
    return dict(status_a=sa, status_b=sb, agree=agree, seconds_a=round(ta, 3), seconds_b=round(tb, 3), replays=info.get("replays"), legal=info.get("legal"), matching=info.get("matching"), within_budget=(ta < BUDGET_S and tb < BUDGET_S)), (sa, pa)


def separated(p, answer, family):
    st, plan = search_variant(p, **FAMILIES[family])
    if st != answer[0]: return True
    if plan is None: return False
    a = answer[1]
    return not (plan["paths"] == a["paths"] and plan["combine"] == a["combine"] and plan["sink_value"] == a["sink_value"])


# ----------------------------------------------------------------- builders
# each returns (problem, expected_status, required_separations)
def b_public(rng):
    p = random_fabric(rng, 4, 3, 1, 5)
    latent_telemetry(rng, p, listed_fraction=0.5, with_completion=True)
    return p, "ok", ["per_source_shortest", "latency_in_occupancy"]

def b_pipelined(rng):
    # long latency links with shared serialization: latency must not extend link occupancy
    p = random_fabric(rng, 3, 1, 1, 2, rates=(4, 8), lats=(3, 4, 5))
    latent_telemetry(rng, p, plan_pick=lambda c, s, r: len(s) == 0, with_completion=True, listed_fraction=0.5)
    return p, "ok", ["latency_in_occupancy"]

def b_serialization(rng):
    # several payloads share one link into the sink; independent link estimates give a different completion
    p = random_fabric(rng, 4, 1, 1, 1, rates=(4, 8), lats=(0, 1), shared_endpoint=True)
    latent_telemetry(rng, p, plan_pick=lambda c, s, r: len(s) == 0, with_completion=True)
    return p, "ok", ["no_serialization"]

def b_suffix(rng):
    p = random_fabric(rng, 3, 2, 1, 4, rates=(4, 8, 16), lats=(0, 1, 2))
    latent_telemetry(rng, p, with_completion=True, listed_fraction=0.6)
    return p, "ok", ["per_source_shortest"]

def b_two_and_capacity(rng):
    p = random_fabric(rng, 3, 3, 0, 3, caps=[8, 7, 16])  # R1 cannot hold a payload of 8
    latent_telemetry(rng, p, plan_pick=lambda c, s, r: len(s) >= 1, with_completion=False, listed_fraction=0.3, zero_listed=False)
    return p, "ok", ["capacity_ignored"]

def b_combine_choice(rng):
    # exactly one combine is right: never combining and combining everywhere both give different answers
    p = random_fabric(rng, 3, 3, 0, 3)
    latent_telemetry(rng, p, plan_pick=lambda c, s, r: len(s) == 1, with_completion=True, listed_fraction=0.5)
    return p, "ok", ["never_combine", "combine_all_visited"]

def b_saturation(rng):
    p = random_fabric(rng, 4, 2, 0, 2)
    vals = [2147483000, 1000, -3000, 5]
    for s, v in zip(sorted(p.sources, key=lambda s: s.id), vals): s.value = v
    latent_telemetry(rng, p, with_completion=True)
    return p, "ok", ["sum_then_clamp", "fold_in_input_order"]

def b_zero_link(rng):
    p = random_fabric(rng, 3, 2, 1, 3)
    latent_telemetry(rng, p, with_completion=False, listed_fraction=0.5, zero_listed=True)
    # force at least two listed zero links
    zeros = [l.id for l in p.links if p.link_bytes.get(l.id, 1) == 0]
    if len(zeros) < 2:
        st, plan, _ = model.recover(p)
        if st != "ok": return p, "retry", []
        used = {l for path in plan["paths"] for l in path}
        for l in p.links:
            if l.id not in used and len([x for x in p.link_bytes if p.link_bytes[x] == 0]) < 2: p.link_bytes[l.id] = 0
    return p, "ok", ["zero_telemetry_ignored"]

def b_no_completion(rng):
    p = random_fabric(rng, 3, 2, 0, 3)
    latent_telemetry(rng, p, with_completion=False, listed_fraction=0.4)
    return p, "ok", ["bytes_before_completion"]

def b_allocation(rng):
    p = random_fabric(rng, 3, 3, 0, 3, lats=(0, 0, 1))
    latent_telemetry(rng, p, with_completion=False, listed_fraction=0.3)
    return p, "ok", ["allocation_after_bytes"]

def b_key(rng):
    p = random_fabric(rng, 2, 1, 0, 0, rates=(8,), lats=(1,))
    # parallel equal links from each endpoint to the switch and to the sink, ids in an order that differs from input order
    extra = []
    for l in list(p.links):
        extra.append(Link(l.id + "z", l.frm, l.to, l.rate, l.latency))
        extra.append(Link(l.id + "a", l.frm, l.to, l.rate, l.latency))
    p.links = extra + p.links
    latent_telemetry(rng, p, with_completion=False, listed_fraction=0.0, zero_listed=False)
    return p, "ok", ["key_by_input_order"]

def b_shared_endpoint(rng):
    p = random_fabric(rng, 3, 2, 1, 2, shared_endpoint=True)
    latent_telemetry(rng, p, with_completion=True)
    return p, "ok", ["no_serialization"]

def b_invalid_dup_link(rng):
    p = random_fabric(rng, 2, 1, 0, 1); latent_telemetry(rng, p)
    p.links.append(Link(p.links[0].id, p.links[-1].frm, p.links[-1].to, 8, 1))
    return p, "invalid", []

def b_invalid_rate_latency(rng):
    p = random_fabric(rng, 3, 2, 1, 2); latent_telemetry(rng, p)
    p.links[rng.randrange(len(p.links))].latency = -1
    return p, "invalid", []

def b_invalid_sink_endpoint(rng):
    p = random_fabric(rng, 2, 1, 0, 1); latent_telemetry(rng, p)
    p.sources.append(Source("z", p.sink, 1))  # a source placed on the sink node: the only violation
    return p, "invalid", []

def b_invalid_unknown_node(rng):
    p = random_fabric(rng, 3, 1, 1, 2); latent_telemetry(rng, p)
    p.links[0].to = "Q9"
    return p, "invalid", []

def b_infeasible_bytes(rng):
    p = random_fabric(rng, 3, 2, 0, 2); latent_telemetry(rng, p, with_completion=True)
    lid = next(iter(p.link_bytes)) if p.link_bytes else p.links[0].id
    p.link_bytes[lid] = p.link_bytes.get(lid, 0) + 3  # not a multiple of the payload size
    return p, "infeasible", []

def b_infeasible_completion(rng):
    p = random_fabric(rng, 3, 2, 1, 3); latent_telemetry(rng, p, with_completion=True)
    p.completion = 0
    return p, "infeasible", []

BUILDERS = {
    "public": (b_public, 100), "h_pipelined_latency": (b_pipelined, 200), "h_serialization_shared_link": (b_serialization, 300),
    "h_suffix_tree": (b_suffix, 400), "h_two_required_capacity": (b_two_and_capacity, 500), "h_combine_choice": (b_combine_choice, 600),
    "h_saturation_fold": (b_saturation, 700), "h_zero_bytes_link": (b_zero_link, 800), "h_no_completion": (b_no_completion, 900),
    "h_allocation_before_bytes": (b_allocation, 1000), "h_plan_key_canonical": (b_key, 1100), "h_shared_endpoint": (b_shared_endpoint, 1200),
    "h_invalid_duplicate_link": (b_invalid_dup_link, 1300), "h_invalid_negative_latency": (b_invalid_rate_latency, 1400),
    "h_invalid_sink_is_endpoint": (b_invalid_sink_endpoint, 1500), "h_invalid_unknown_node": (b_invalid_unknown_node, 1600),
    "h_infeasible_bytes": (b_infeasible_bytes, 1700), "h_infeasible_completion": (b_infeasible_completion, 1800),
}


def run(fid, out_root, max_tries=60):
    builder, seed = BUILDERS[fid]
    for attempt in range(max_tries):
        rng = random.Random(seed + attempt)
        try:
            p, expect, seps = builder(rng)
        except Exception as e:
            print("REJECTED %-28s attempt=%d builder error %s" % (fid, attempt, e)); continue
        if expect == "retry": continue
        cert, (sa, pa) = certify(p)
        ok = cert["agree"] and cert["within_budget"] and sa == expect
        reasons = []
        if ok and sa == "ok":
            if cert["matching"] < 2: reasons.append("only one matching plan (no global choice)")
            if fid == "public" and (cert["replays"] < 200 or cert["matching"] < 3): reasons.append("public fabric too small")
            for fam in seps:
                if not separated(p, (sa, pa), fam): reasons.append("not separated by " + fam)
            if cert["replays"] < 40: reasons.append("search trivially small")
        ok = ok and not reasons
        d = os.path.join(out_root, fid)
        if ok:
            shutil.rmtree(d, ignore_errors=True); os.makedirs(d)
            json.dump(p.to_json(), open(os.path.join(d, "problem.json"), "w"), indent=1)
            plan = None if pa is None else {"paths": pa["paths"], "combine": pa["combine"], "sink_value": pa["sink_value"]}
            obs = None if pa is None else {k: pa["replay"][k] for k in ("completion", "allocation", "total_bytes", "link_bytes")}
            json.dump({"fixture": fid, "status": sa, "plan": plan, "observables": obs}, open(os.path.join(d, "expected.json"), "w"), indent=1)
            open(os.path.join(d, "result.expected.json"), "w").write(model.result_json(sa, plan))
            cert.update({"fixture": fid, "seed": seed + attempt, "separations": seps, "sources": len(p.sources), "links": len(p.links), "switches": len(p.switches)})
            json.dump(cert, open(os.path.join(d, "certification.json"), "w"), indent=1)
            print("RETAINED %-28s seed=%d status=%s replays=%s legal=%s matching=%s A=%.2fs B=%.2fs seps=%s" % (fid, seed + attempt, sa, cert["replays"], cert["legal"], cert["matching"], cert["seconds_a"], cert["seconds_b"], seps))
            return
        print("REJECTED %-28s attempt=%d A=%s B=%s expect=%s agree=%s budget=%s %s" % (fid, attempt, sa, cert["status_b"], expect, cert["agree"], cert["within_budget"], reasons))
    raise SystemExit("no retained fixture for " + fid)


if __name__ == "__main__":
    out_root = os.path.join(HERE, "fixtures")
    which = sys.argv[1:] or list(BUILDERS)
    if which == ["all"]: which = list(BUILDERS)
    for fid in which: run(fid, out_root)
