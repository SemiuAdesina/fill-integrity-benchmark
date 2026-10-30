#!/usr/bin/env python3
"""Oracle B: an independent exact branch and bound in Python (author-side,
never ships). Same contract as the Rust reference, written separately:
intervals for the insert length and every insert-region position, fragment
constraints branched by band window, an assignment lower bound (Hungarian)
for digests whose pairing is open, a tangent-plane bound for digests whose
pairing is fixed, and exact evaluation with the model's rule at leaves.
Levels by total insert-region count; the first level with a map wins."""
import sys, os, time, itertools, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model
from model import Problem, Map, Band

INF = 1 << 50


class Budget(Exception):
    pass


def hungarian(cost):
    n = len(cost)
    if n == 0: return 0
    u = [0] * (n + 1); v = [0] * (n + 1); p = [0] * (n + 1); way = [0] * (n + 1)
    for i in range(1, n + 1):
        p[0] = i; j0 = 0; minv = [INF * 4] * (n + 1); used = [False] * (n + 1)
        while True:
            used[j0] = True; i0 = p[j0]; delta = INF * 4; j1 = 0
            for j in range(1, n + 1):
                if used[j]: continue
                cur = cost[i0 - 1][j - 1] - u[i0] - v[j]
                if cur < minv[j]: minv[j] = cur; way[j] = j0
                if minv[j] < delta: delta = minv[j]; j1 = j
            for j in range(n + 1):
                if used[j]: u[p[j]] += delta; v[j] -= delta
                else: minv[j] -= delta
            j0 = j1
            if p[j0] == 0: break
        while True:
            j1 = way[j0]; p[j0] = p[j1]; j0 = j1
            if j0 == 0: break
    return sum(cost[p[j] - 1][j - 1] for j in range(1, n + 1) if p[j])


class SiteSearch:
    def __init__(self, p: Problem, c: int, composition, t0, budget):
        self.p = p; self.c = c; self.comp = composition; self.t0 = t0; self.budget = budget
        self.V = p.V; self.D = p.detection_limit; self.e = p.error_percent
        self.names = [x.name for x in p.enzymes]
        self.fixed = []; self.moving = []
        for en in p.enzymes:
            surv = sorted({cut for st, cut in model.scan_sites(p.backbone, en.site, en.offset) if not model.spans_junction(st, len(en.site), c, self.V)})
            self.fixed.append([q for q in surv if q < c]); self.moving.append([q for q in surv if q >= c])
        self.first = []; n = 0
        for t in composition: self.first.append(n); n += t
        self.nvar = n
        self.digests = []
        for d in p.observed:
            idx = [self.names.index(x) for x in d.enzymes]
            exp = model.expand(d.bands)
            self.digests.append((idx, exp, sorted(set(exp)), d))
        self.nodes = 0; self.best = None  # (key, Map, dev)
        self.ties = set()

    # cuts: ('F', p) fixed, ('M', p) moving, ('B', i) box
    def interval(self, cut, L, boxes):
        k, x = cut
        if k == 'F': return (x, x)
        if k == 'M': return (x + L[0], x + L[1])
        return boxes[x]

    def enzyme_of(self, i):
        e = 0
        while e + 1 < len(self.first) and self.first[e + 1] <= i: e += 1
        return e

    def ordered(self, di, L, boxes):
        idx = self.digests[di][0]
        fixed = sorted({q for e in idx for q in self.fixed[e]}); moving = sorted({q for e in idx for q in self.moving[e]})
        bx = sorted(((boxes[self.first[e] + r], self.first[e] + r) for e in idx for r in range(self.comp[e])), key=lambda x: x[0][0])
        for a, b in zip(bx, bx[1:]):
            if not (a[0][1] < b[0][0] or (b[1] == a[1] + 1 and self.enzyme_of(a[1]) == self.enzyme_of(b[1]))): return None
        return [('F', q) for q in fixed] + [('B', i) for _, i in bx] + [('M', q) for q in moving]

    def frags(self, cuts):
        out = [(cuts[i], cuts[i + 1], False) for i in range(len(cuts) - 1)]
        out.append((cuts[-1], cuts[0], True))
        return out

    def frag_iv(self, f, L, boxes):
        a = self.interval(f[0], L, boxes); b = self.interval(f[1], L, boxes)
        if f[2]: return (max(1, b[0] + self.V + L[0] - a[1]), b[1] + self.V + L[1] - a[0])
        return (max(1, b[0] - a[1]), b[1] - a[0])

    def tighten(self, f, lo, hi, L, boxes):
        """Returns (L, boxes) restricted so that f in [lo, hi], or None."""
        minus, plus, wrap = f
        wl, wc = (1, self.V) if wrap else (0, 0)
        def restrict(cut, rng, L, boxes):
            k, x = cut
            if k == 'F': return (L, boxes) if rng[0] <= x <= rng[1] else None
            if k == 'M':
                nl = (max(L[0], rng[0] - x), min(L[1], rng[1] - x))
                return (nl, boxes) if nl[0] <= nl[1] else None
            nb = list(boxes); nb[x] = (max(boxes[x][0], rng[0]), min(boxes[x][1], rng[1]))
            return (L, nb) if nb[x][0] <= nb[x][1] else None
        pl = self.interval(plus, L, boxes); mn = self.interval(minus, L, boxes)
        r = restrict(plus, (lo - (wl * L[1] + wc) + mn[0], hi - (wl * L[0] + wc) + mn[1]), L, boxes)
        if r is None: return None
        L, boxes = r
        pl = self.interval(plus, L, boxes)
        r = restrict(minus, (pl[0] + wl * L[0] + wc - hi, pl[1] + wl * L[1] + wc - lo), L, boxes)
        if r is None: return None
        L, boxes = r
        coef = (plus[0] == 'M') - (minus[0] == 'M') + wl
        if coef:
            base = lambda cut, side: (cut[1] if cut[0] != 'B' else boxes[cut[1]][side])
            blo = base(plus, 0) - base(minus, 1) + wc; bhi = base(plus, 1) - base(minus, 0) + wc
            if coef > 0: nl = (max(L[0], lo - bhi), min(L[1], hi - blo))
            else: nl = (max(L[0], blo - hi), min(L[1], bhi - lo))
            if nl[0] > nl[1]: return None
            L = nl
        return (L, boxes)

    def propagate(self, L, boxes, constraints):
        c = self.c
        for _ in range(8):
            L0, b0 = L, list(boxes)
            for e, t in enumerate(self.comp):
                base = self.first[e]
                for r in range(t):
                    i = base + r
                    lo, hi = boxes[i]
                    lo = max(lo, c); hi = min(hi, c + L[1])
                    if r > 0: lo = max(lo, boxes[i - 1][0] + 1)
                    boxes[i] = (lo, hi)
                for r in range(t - 1, -1, -1):
                    i = base + r
                    lo, hi = boxes[i]
                    if r + 1 < t: hi = min(hi, boxes[i + 1][1] - 1)
                    if lo > hi: return None
                    boxes[i] = (lo, hi)
                    L = (max(L[0], lo - c), L[1])
            if L[0] > L[1]: return None
            for (di, fi, lo, hi) in constraints:
                cuts = self.ordered(di, L, boxes)
                if cuts is None: continue
                r = self.tighten(self.frags(cuts)[fi], lo, hi, L, boxes)
                if r is None: return None
                L, boxes = r[0], list(r[1])
            if L == L0 and boxes == b0: break
        return (L, boxes)

    def bound(self, L, boxes, warm):
        D, e = self.D, self.e
        loose = 0; terms = []
        for di, (idx, exp, distinct, d) in enumerate(self.digests):
            cuts = self.ordered(di, L, boxes)
            if cuts is None: continue
            frs = self.frags(cuts); m = len(exp)
            certain = 0; possible = 0; vis = []; settled = True; amb = []
            for f in frs:
                iv = self.frag_iv(f, L, boxes)
                if iv[1] < D: continue
                possible += 1
                if iv[0] < D: settled = False; amb.append(iv); continue
                certain += 1; vis.append((iv, f))
            if certain > m or possible < m: return None
            vis.sort(key=lambda x: x[0][0])
            paired = settled and certain == m and all(vis[i][0][1] < vis[i + 1][0][0] for i in range(len(vis) - 1))
            if paired:
                for k, (iv, f) in enumerate(vis):
                    b = exp[k]; gap = iv[0] - b if b < iv[0] else (b - iv[1] if b > iv[1] else 0)
                    if gap > model.tolerance(b, e): return None
                    terms.append((f, b))
            else:
                rows = [(iv, True) for iv, _ in vis] + [(iv, False) for iv in amb]
                n = len(rows); cost = [[INF] * n for _ in range(n)]
                for r, (iv, cert) in enumerate(rows):
                    for col, b in enumerate(exp):
                        gap = iv[0] - b if b < iv[0] else (b - iv[1] if b > iv[1] else 0)
                        if gap <= model.tolerance(b, e): cost[r][col] = gap * gap
                    for col in range(m, n): cost[r][col] = INF if cert else 0
                a = hungarian(cost)
                if a >= INF: return None
                loose += a
        if not terms: return Some(loose) if False else loose
        nv = self.nvar + 1
        lo = [L[0]] + [b[0] for b in boxes]; hi = [L[1]] + [b[1] for b in boxes]
        if len(warm) != nv: warm[:] = [(lo[i] + hi[i]) / 2 for i in range(nv)]
        for i in range(nv): warm[i] = min(max(warm[i], lo[i]), hi[i])
        def coef(f, i):
            a = 0.0
            for cut, sgn in ((f[1], 1.0), (f[0], -1.0)):
                if cut[0] == 'B' and cut[1] + 1 == i: a += sgn
                if cut[0] == 'M' and i == 0: a += sgn
            if f[2] and i == 0: a += 1.0
            return a
        def value(f, y):
            at = lambda cut: (cut[1] if cut[0] == 'F' else (cut[1] + y[0] if cut[0] == 'M' else y[cut[1] + 1]))
            return at(f[1]) - at(f[0]) + (self.V + y[0] if f[2] else 0.0)
        tc = [(f, b, [coef(f, i) for i in range(nv)]) for f, b in terms]
        for _ in range(12):
            for i in range(nv):
                num = 0.0; den = 0.0
                for f, b, cf in tc:
                    a = cf[i]
                    if a == 0.0: continue
                    r = value(f, warm) - b - a * warm[i]; num += a * r; den += a * a
                if den > 0: warm[i] = min(max(-num / den, lo[i]), hi[i])
        q = 0.0; grad = [0.0] * nv
        for f, b, cf in tc:
            r = value(f, warm) - b; q += r * r
            for i in range(nv):
                if cf[i]: grad[i] += 2 * cf[i] * r
        tangent = q + sum(min(grad[i] * (lo[i] - warm[i]), grad[i] * (hi[i] - warm[i]), 0.0) for i in range(nv))
        return loose + (max(0, math.floor(tangent - 1e-6)) if tangent > 0 else 0)

    def unsettled(self, L, boxes, constraints):
        pick = None
        for di, (idx, exp, distinct, d) in enumerate(self.digests):
            cuts = self.ordered(di, L, boxes)
            if cuts is None: continue
            for fi, f in enumerate(self.frags(cuts)):
                if any(k[0] == di and k[1] == fi for k in constraints): continue
                iv = self.frag_iv(f, L, boxes)
                if iv[1] < self.D: continue
                opts = sum(1 for b in distinct if b + model.tolerance(b, self.e) >= iv[0] and b - model.tolerance(b, self.e) <= iv[1]) + (1 if iv[0] < self.D else 0)
                if opts <= 1: continue
                # differs from the reference: the narrowest unsettled fragment first
                if pick is None or (iv[1] - iv[0]) < (pick[2][1] - pick[2][0]): pick = (di, fi, iv)
        return pick

    def leaf(self, L, boxes):
        c = self.c; Lv = L[0]
        cuts = {}
        for e, en in enumerate(self.p.enzymes):
            ps = list(self.fixed[e]) + [q + Lv for q in self.moving[e]]
            for r in range(self.comp[e]):
                q = boxes[self.first[e] + r][0]
                if q < c or q > c + Lv: return
                ps.append(q)
            ps.sort()
            if len(set(ps)) != len(ps): return
            cuts[en.name] = ps
        m = Map(c, Lv, cuts)
        ok, dev = model.evaluate(self.p, m)
        if not ok: return
        k = model.key(self.p, m, dev)
        if self.best is None or k < self.best[0]:
            if self.best is not None and k[:2] != self.best[0][:2]: self.ties = set()
            self.best = (k, m, dev)
        if self.best is not None and k[:2] == self.best[0][:2]: self.ties.add((tuple(k[2]), k[3], k[4]))

    def recurse(self, L, boxes, constraints, warm):
        self.nodes += 1
        if (self.nodes & 1023) == 0 and time.time() - self.t0 > self.budget: raise Budget()
        r = self.propagate(L, list(boxes), constraints)
        if r is None: return
        L, boxes = r
        lb = self.bound(L, boxes, warm)
        if lb is None: return
        if self.best is not None and lb > self.best[2]: return
        if L[0] == L[1] and all(b[0] == b[1] for b in boxes): self.leaf(L, boxes); return
        pick = self.unsettled(L, boxes, constraints)
        if pick is not None:
            di, fi, iv = pick; distinct = self.digests[di][2]; mid = (iv[0] + iv[1]) // 2
            opts = []
            for b in distinct:
                tol = model.tolerance(b, self.e)
                if b + tol >= iv[0] and b - tol <= iv[1]: opts.append((abs(b - mid), max(1, b - tol), b + tol))
            if iv[0] < self.D: opts.append((abs(mid), 1, self.D - 1))
            # differs from the reference: farthest window first
            for _, lo, hi in sorted(opts, reverse=True):
                self.recurse(L, boxes, constraints + [(di, fi, lo, hi)], list(warm))
            return
        # bisect: prefer the insert length while it is wider than 64
        widths = [b[1] - b[0] for b in boxes]
        if L[1] - L[0] >= max(widths + [0]) or (L[1] - L[0] > 64):
            mid = (L[0] + L[1]) // 2
            self.recurse((L[0], mid), boxes, constraints, list(warm)); self.recurse((mid + 1, L[1]), boxes, constraints, list(warm))
        else:
            i = max(range(len(boxes)), key=lambda j: widths[j]); lo, hi = boxes[i]; mid = (lo + hi) // 2
            nb = list(boxes); nb[i] = (lo, mid); self.recurse(L, nb, constraints, list(warm))
            nb = list(boxes); nb[i] = (mid + 1, hi); self.recurse(L, nb, constraints, list(warm))


def minima(p: Problem, c: int):
    out = []
    for en in p.enzymes:
        surv = sorted({cut for st, cut in model.scan_sites(p.backbone, en.site, en.offset) if not model.spans_junction(st, len(en.site), c, p.V)})
        a = [q for q in surv if q < c]; b = [q for q in surv if q >= c]
        gaps = [a[i + 1] - a[i] for i in range(len(a) - 1)] + [b[i + 1] - b[i] for i in range(len(b) - 1)]
        if a and b: gaps.append(p.V - b[-1] + a[0])
        vis = sum(1 for g in gaps if g >= p.detection_limit)
        m = len(model.expand(next(d for d in p.observed if d.enzymes == [en.name]).bands))
        out.append(max(0, m - vis - 1))
    return out


def compositions(mins, k, total):
    n = len(mins); out = []
    def rec(i, left, cur):
        if i == n:
            if left == 0: out.append(list(cur))
            return
        for t in range(mins[i], k + 1):
            rest_min = sum(mins[i + 1:]); rest_max = (n - i - 1) * k
            if t > left or left - t < rest_min or left - t > rest_max: continue
            cur.append(t); rec(i + 1, left - t, cur); cur.pop()
    rec(0, total, [])
    return out


def recover_bnb(p: Problem, budget_s=3600):
    t0 = time.time()
    if not model.valid_map(p, Map(p.cloning_sites[0][1], p.insert_min, {e.name: model.required_positions(p, e, p.cloning_sites[0][1], p.insert_min) for e in p.enzymes})) is None:
        pass
    k = p.max_internal; n = len(p.enzymes)
    sites = [c for _, c in p.cloning_sites]
    mins = {c: minima(p, c) for c in sites}
    start = min(sum(v) for v in mins.values())
    stats = {"nodes": 0}
    for level in range(start, n * k + 1):
        best = None; ties = set()
        for c in sites:
            if sum(mins[c]) > level: continue
            for comp in compositions(mins[c], k, level):
                s = SiteSearch(p, c, comp, t0, budget_s)
                s.best = best; s.ties = ties
                L = (p.insert_min, p.insert_max); boxes = [(c, c + p.insert_max)] * s.nvar
                s.recurse(L, boxes, [], [])
                stats["nodes"] += s.nodes
                best = s.best; ties = s.ties
        if best is not None:
            stats["seconds"] = round(time.time() - t0, 2); stats["ties_at_count_and_deviation"] = len(ties)
            return best[1], stats
    stats["seconds"] = round(time.time() - t0, 2)
    return None, stats


if __name__ == "__main__":
    import json, bridge
    prob = bridge.read_instance(sys.argv[1])
    m, st = recover_bnb(prob, budget_s=float(sys.argv[2]) if len(sys.argv) > 2 else 3600)
    print(st); print(None if m is None else (m.c, m.L, m.cuts))
