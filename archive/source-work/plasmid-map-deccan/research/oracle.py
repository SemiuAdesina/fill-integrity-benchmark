#!/usr/bin/env python3
"""Oracle A: exact search over cloning site, insert length and internal cut
positions (author-side, never ships).

Per cloning site and enzyme, the required vector positions split into A (below
c, fixed) and B (at or above c, shifted by L). Exactly one circular gap, G,
contains the insert region; every other gap has an L-independent length. The
internal cuts of the enzyme lie in G and split it into pieces; the first
piece's length does not depend on L, the last one's does. Candidate internal
sets are generated from windows around the observed bands (or a sub-detection
piece), verified with the real matching rule for every L in the implied
interval, and indexed by L. The join over enzymes runs per L in count order
with every observed multi-enzyme digest checked as soon as its enzymes are
assigned. The minimum of (internal count, deviation, positions, c, L) over
every (c, L) is the answer."""
import sys, os, itertools, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model
from model import Problem, Map, Band

INF = float("inf")


class Budget(Exception):
    pass


def single_bands(p: Problem, name: str):
    for d in p.observed:
        if d.enzymes == [name]:
            return model.expand(d.bands)
    return None


def candidate_sets(p: Problem, enzyme, c: int, stats):
    """Returns dict L -> list of (positions tuple, single deviation, count)."""
    V = p.V; D = p.detection_limit; e = p.error_percent; K = p.max_internal
    obs = single_bands(p, enzyme.name)
    if obs is None:
        raise ValueError("every enzyme needs an observed single digest in this oracle")
    sites = [(s, cut) for s, cut in model.scan_sites(p.backbone, enzyme.site, enzyme.offset) if not model.spans_junction(s, len(enzyme.site), c, V)]
    A = sorted(cut for _, cut in sites if cut < c)
    B = sorted(cut for _, cut in sites if cut >= c)
    fixed = [A[i + 1] - A[i] for i in range(len(A) - 1)] + [B[i + 1] - B[i] for i in range(len(B) - 1)]
    if A and B: fixed.append(V - B[-1] + A[0])
    fixed_visible = sorted(f for f in fixed if f >= D)
    if len(fixed_visible) > len(obs): return {}
    # G: start s0 (recombinant coordinates, L-independent when A nonempty) and g = L + g_vec
    if not A and not B:
        raise ValueError("enzyme %s has no anchored backbone site at cloning site %d" % (enzyme.name, c))
    if A and B: g_vec = B[0] - A[-1]
    elif A: g_vec = V - A[-1] + A[0]
    elif B: g_vec = V - B[-1] + B[0]
    else: g_vec = V
    out = {}
    slots = [("band", b) for b in obs] + [("sub", None)]
    Lmin, Lmax = p.insert_min, p.insert_max
    for t in range(0, K + 1):
        # residual visible pieces = len(obs) - len(fixed_visible) must equal (t+1 - u) where u sub-D pieces
        for seq in itertools.product(range(len(slots)), repeat=t + 1):
            kinds = [slots[i] for i in seq]
            visible = [k[1] for k in kinds if k[0] == "band"]
            if len(visible) + len(fixed_visible) != len(obs): continue
            # multiset check: visible + fixed_visible must pair with obs within tolerance (necessary)
            pred = sorted(visible + fixed_visible)
            if any(abs(pr - ob) > model.tolerance(ob, e) for pr, ob in zip(pred, obs)): continue
            # windows per slot
            windows = []
            for kind, b in kinds:
                if kind == "band": tol = model.tolerance(b, e); windows.append(range(max(1, b - tol), b + tol + 1))
                else: windows.append(range(1, D))
            if any(len(w) == 0 for w in windows): continue
            # sum of pieces = L + g_vec  ->  L in [sum(mins) - g_vec, sum(maxs) - g_vec] must meet [Lmin, Lmax]
            lo = sum(w[0] for w in windows) - g_vec; hi = sum(w[-1] for w in windows) - g_vec
            if hi < Lmin or lo > Lmax: continue
            stats["sequences"] += 1
            # enumerate the first t pieces (positions), the last piece fixes L
            for firsts in itertools.product(*windows[:t]):
                stats["tuples"] += 1
                # positions: q_1 = s0 + a_0, ... but s0 depends on L when A is empty and B nonempty; handle via relative offsets
                rel = []; acc = 0
                for a in firsts: acc += a; rel.append(acc)
                for last in windows[t]:
                    L = acc + last - g_vec
                    if L < Lmin or L > Lmax: continue
                    N = V + L
                    if A: s0 = A[-1]
                    elif B: s0 = B[-1] + L
                    else: s0 = c  # no required cuts: anchor the first piece at c (a_0 measured from c around the circle)
                    if not A and not B:
                        # pieces are gaps between consecutive internal cuts around the whole circle; anchor q_1 = c + rel? use a_0 as offset of q_1 from c
                        qs = [c + r for r in rel] if t > 0 else []
                        if t == 0: qs = []
                    else:
                        qs = [(s0 + r) % N for r in rel]
                    if t > 0 and not all(c <= q <= c + L for q in qs): continue
                    if t > 0 and any(qs[i] >= qs[i + 1] for i in range(len(qs) - 1)): continue
                    req = [a for a in A] + [b + L for b in B]
                    positions = sorted(req + qs)
                    if len(set(positions)) != len(positions): continue
                    pred_sorted = sorted(f for f in model.fragments(positions, N) if f >= D)
                    ok, dev = model.match(pred_sorted, [Band(o, 1) for o in obs], e)
                    if not ok: continue
                    stats["verified"] += 1
                    out.setdefault(L, {})[tuple(qs)] = dev
    return {L: [(qs, dev, len(qs)) for qs, dev in d.items()] for L, d in out.items()}


def recover(p: Problem, budget_s=None, stats=None):
    t0 = time.time()
    stats = stats if stats is not None else {}
    for k in ("sequences", "tuples", "verified", "join_nodes", "leaves"): stats.setdefault(k, 0)
    names = [e.name for e in p.enzymes]
    multi = [d for d in p.observed if len(d.enzymes) > 1]
    best = None
    for cname, c in p.cloning_sites:
        cands = {}
        for e in p.enzymes:
            cands[e.name] = candidate_sets(p, e, c, stats)
            if budget_s and time.time() - t0 > budget_s: raise Budget()
        Ls = set.intersection(*[set(cands[n]) for n in names]) if names else set()
        for L in sorted(Ls):
            N = p.V + L
            per = {n: sorted(cands[n][L], key=lambda x: (x[2], x[1], x[0])) for n in names}
            min_count_rest = {}; min_dev_rest = {}
            for i in range(len(names) + 1):
                min_count_rest[i] = sum(min(x[2] for x in per[n]) for n in names[i:])
                min_dev_rest[i] = sum(min(x[1] for x in per[n]) for n in names[i:])
            req = {e.name: model.required_positions(p, e, c, L) for e in p.enzymes}
            assigned = {}

            def dfs(i, count, dev):
                nonlocal best
                stats["join_nodes"] += 1
                if budget_s and (stats["join_nodes"] & 1023) == 0 and time.time() - t0 > budget_s: raise Budget()
                if best is not None:
                    lb = (count + min_count_rest[i], dev + min_dev_rest[i])
                    if lb > best[:2]: return
                if i == len(names):
                    stats["leaves"] += 1
                    cuts = {n: sorted(req[n] + list(assigned[n])) for n in names}
                    m = Map(c, L, cuts)
                    ok, total = model.evaluate(p, m)
                    if not ok: return
                    k = model.key(p, m, total)
                    if best is None or k < best[:5]: best = k + (m,)
                    return
                n = names[i]
                for qs, sdev, t in per[n]:
                    assigned[n] = qs
                    # check every multi digest whose enzymes are all assigned and include n
                    ok = True; extra = 0
                    cuts = {m_: sorted(req[m_] + list(assigned[m_])) for m_ in assigned}
                    for d in multi:
                        if n in d.enzymes and all(x in assigned for x in d.enzymes):
                            good, ddev = model.match(model.predicted_bands(cuts, d.enzymes, N, p.detection_limit), d.bands, p.error_percent)
                            if not good: ok = False; break
                            extra += ddev
                    if ok: dfs(i + 1, count + t, dev + sdev + extra)
                    del assigned[n]
            dfs(0, 0, 0)
    stats["seconds"] = round(time.time() - t0, 3)
    if best is None: return None, stats
    return best[5], stats
