import itertools, random

def replay(g, m, t):
    bc, ww = g["bank_count"], g["word_width"]
    mask = bc - 1
    banks = []
    for a, lane in zip(t["addr"], t["lanes"]):
        w = a // ww
        for s, d in m["stages"]:
            bit = (w >> s) & 1
            w ^= bit << d
        rl = (lane + m["rot"]) % bc
        banks.append((w ^ rl) & mask)
    groups = {}
    for i, a in enumerate(t["addr"]):
        groups.setdefault(a, []).append(i)
    for a, members in groups.items():
        leader = min(members, key=lambda i: t["lanes"][i])
        for i in members:
            banks[i] = banks[leader]
    distinct = {}
    for i, a in enumerate(t["addr"]):
        distinct.setdefault(banks[i], set()).add(a)
    conflict = max([1] + [len(v) for v in distinct.values()])
    return banks, conflict

def canonical(m):
    return "rotate=%d;stages=%s" % (m["rot"], ",".join("%d>%d" % s for s in m["stages"]))

def matches(g, m, traces):
    for t in traces:
        banks, conflict = replay(g, m, t)
        ob = t.get("banks")
        if ob:
            for got, want in zip(banks, ob):
                if want >= 0 and got != want:
                    return False
        if t.get("conflict", -1) >= 0 and conflict != t["conflict"]:
            return False
    return True

def sequences(allowed, k):
    out = [()]
    for n in range(1, k + 1):
        out.extend(itertools.permutations(allowed, n))
    return out

def subsets_in_order(allowed, k):
    out = [()]
    for n in range(1, k + 1):
        out.extend(itertools.combinations(allowed, n))
    return out

def legal_all(g, traces, enum):
    res = []
    for rot in g["allowed_rot"]:
        for stages in enum(g["allowed_stages"], g["max_stages"]):
            m = {"stages": list(stages), "rot": rot}
            if matches(g, m, traces):
                res.append(m)
    return res

def pick(cands, key):
    best = None
    for m in cands:
        k = key(m)
        if best is None or k < best[0]:
            best = (k, m)
    return best[1] if best else None

def cost(m):
    return len(m["stages"]) + (0 if m["rot"] == 0 else 1)

REFERENCE = lambda m: (cost(m), len(m["stages"]), canonical(m))
NO_COST_RULE = lambda m: (len(m["stages"]), len(m["stages"]), canonical(m))
SKIP_STAGE_TIER = lambda m: (cost(m), canonical(m))

def recover(g, traces, key=REFERENCE, enum=sequences):
    return pick(legal_all(g, traces, enum), key)
