import itertools, random

def replay(g, m, t, div=True, bcast=True):
    bc, ww = g["bank_count"], g["word_width"]
    mask = bc - 1
    banks = []
    for a, lane in zip(t["addr"], t["lanes"]):
        w = a // ww if div else a
        for s, d in m["stages"]:
            w ^= ((w >> s) & 1) << d
        banks.append((w ^ ((lane + m["rot"]) % bc)) & mask)
    if bcast:
        groups = {}
        for i, a in enumerate(t["addr"]):
            groups.setdefault(a, []).append(i)
        for a, mem in groups.items():
            ld = min(mem, key=lambda i: t["lanes"][i])
            for i in mem:
                banks[i] = banks[ld]
    dist = {}
    for i, a in enumerate(t["addr"]):
        dist.setdefault(banks[i], set()).add(a)
    return banks, max([1] + [len(v) for v in dist.values()])

def canonical(m):
    return "rotate=%d;stages=%s" % (m["rot"], ",".join("%d>%d" % s for s in m["stages"]))

def matches(g, m, traces, **kw):
    for t in traces:
        banks, conflict = replay(g, m, t, **kw)
        for got, want in zip(banks, t["banks"]):
            if want >= 0 and got != want:
                return False
        if t.get("conflict", -1) >= 0 and conflict != t["conflict"]:
            return False
    return True

def seqs(allowed, k):
    out = [()]
    for n in range(1, k + 1):
        out.extend(itertools.permutations(allowed, n))
    return out

def subs(allowed, k):
    out = [()]
    for n in range(1, k + 1):
        out.extend(itertools.combinations(allowed, n))
    return out

def cost(m):
    return len(m["stages"]) + (0 if m["rot"] == 0 else 1)

KEY = lambda m: (cost(m), len(m["stages"]), canonical(m))

def solve(g, traces, enum=seqs, rots=None, **kw):
    best = None
    for rot in (rots if rots is not None else g["allowed_rot"]):
        for st in enum(g["allowed_stages"], g["max_stages"]):
            m = {"stages": list(st), "rot": rot}
            if matches(g, m, traces, **kw):
                k = KEY(m)
                if best is None or k < best[0]:
                    best = (k, m)
    return best[1] if best else None
