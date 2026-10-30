import itertools, sys
from bankmodel import canonical, cost, sequences, subsets_in_order, pick

def replay_gen(g, m, t, truncate=True, use_rot=True, broadcast=True, parallel=False,
               count_members=False):
    bc, ww = g["bank_count"], g["word_width"]
    mask = bc - 1
    banks = []
    for a, lane in zip(t["addr"], t["lanes"]):
        w = a // ww if truncate else a
        if parallel:
            base = w
            for s, d in m["stages"]:
                w ^= ((base >> s) & 1) << d
        else:
            for s, d in m["stages"]:
                w ^= ((w >> s) & 1) << d
        rl = (lane + m["rot"]) % bc if use_rot else lane
        banks.append((w ^ rl) & mask)
    if broadcast:
        groups = {}
        for i, a in enumerate(t["addr"]):
            groups.setdefault(a, []).append(i)
        for a, members in groups.items():
            leader = min(members, key=lambda i: t["lanes"][i])
            for i in members:
                banks[i] = banks[leader]
    distinct = {}
    for i, a in enumerate(t["addr"]):
        distinct.setdefault(banks[i], set()).add(i if count_members else a)
    conflict = max([1] + [len(v) for v in distinct.values()])
    return banks, conflict

def matches_gen(g, m, traces, **kw):
    for t in traces:
        banks, conflict = replay_gen(g, m, t, **kw)
        ob = t.get("banks")
        if ob:
            for got, want in zip(banks, ob):
                if want >= 0 and got != want:
                    return False
        if t.get("conflict", -1) >= 0 and conflict != t["conflict"]:
            return False
    return True

REFERENCE = lambda m: (cost(m), len(m["stages"]), canonical(m))
NO_COST = lambda m: (len(m["stages"]), len(m["stages"]), canonical(m))
NO_STAGE_TIER = lambda m: (cost(m), canonical(m))
NUMERIC_TIE = lambda m: (cost(m), len(m["stages"]), m["rot"], tuple(m["stages"]))

def solve(g, traces, key=REFERENCE, enum=sequences, first=False, **kw):
    cands = []
    for rot in g["allowed_rot"]:
        for stages in enum(g["allowed_stages"], g["max_stages"]):
            m = {"stages": list(stages), "rot": rot}
            if matches_gen(g, m, traces, **kw):
                if first:
                    return m
                cands.append(m)
    return pick(cands, key) if cands else None

VARIANTS = {
    "reference":        dict(),
    "wrong_truncation": dict(truncate=False),
    "unordered_stages": dict(enum=subsets_in_order),
    "parallel_mask":    dict(parallel=True),
    "ignore_rotation":  dict(use_rot=False),
    "ignore_broadcast": dict(broadcast=False),
    "first_legal":      dict(first=True),
    "conflict_members": dict(count_members=True),
    "no_cost_rule":     dict(key=NO_COST),
    "no_stage_tier":    dict(key=NO_STAGE_TIER),
    "numeric_tie":      dict(key=NUMERIC_TIE),
}

def build(g, planted, specs):
    traces = []
    for sp in specs:
        t = {"addr": sp["addr"], "lanes": sp["lanes"]}
        banks, conflict = replay_gen(g, planted, t)
        t["banks"] = [b if r else -1 for b, r in zip(banks, sp["reveal"])] if any(sp["reveal"]) else []
        t["conflict"] = conflict if sp.get("conflict") else -1
        traces.append(t)
    return traces

def report(name, g, planted, specs, single_trace=False):
    traces = build(g, planted, specs)
    out = {}
    for vn, kw in VARIANTS.items():
        try:
            m = solve(g, traces, **kw)
        except Exception as e:
            m = None
        out[vn] = canonical(m) if m else "NO-SOLUTION"
    if single_trace:
        m = solve(g, traces[:1])
        out["single_trace_fit"] = canonical(m) if m else "NO-SOLUTION"
    ref = out["reference"]
    print("== %s  planted=%s  reference=%s" % (name, canonical(planted), ref))
    for vn, v in out.items():
        if vn == "reference":
            continue
        print("   %-18s %-34s %s" % (vn, v, "DIFFERS" if v != ref else "same"))
    print("   traces:")
    for t in traces:
        print("     addr=%s lanes=%s banks=%s conflict=%s" % (t["addr"], t["lanes"], t["banks"], t["conflict"]))
    return out
