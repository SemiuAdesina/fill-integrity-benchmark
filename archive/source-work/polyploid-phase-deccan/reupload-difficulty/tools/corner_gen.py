#!/usr/bin/env python3
"""Author-side generator and exhaustive certifier for small semantic-corner crosses.

Every cross is a legal cross-format-v1 document at the minimum size (M=12, N=8)
with most parental dosages pinned to 0 or 4, so the joint column space is small
enough to enumerate outright. Everything here is derived from the specification
text alone and shares no code with the Rust reference.

Axes:
  dr0   asymmetric marker-0 double reduction (per parent, per marker, contributes
        at marker 0; forbidden form is illegal, not penalised)
  lowinfo  finite but flat / near-flat likelihoods on some progeny-markers
  mixed    0/4 pinned markers beside dosage-2 markers, repeated homolog rows
  unlinked r = 0.5 interval together with repeated rows / tied reconstructions
"""
import json, math, random, itertools, sys, os, subprocess
import numpy as np

STATES = [(0,0),(0,1),(0,2),(0,3),(1,1),(1,2),(1,3),(2,2),(2,3),(3,3)]
JOINT = [(a, b) for a in range(10) for b in range(10)]
M, N = 12, 8

def cols(d):
    return [tuple((bits >> h) & 1 for h in range(4)) for bits in range(16) if bin(bits).count("1") == d]

def switch_count(s, t):
    r = list(t); sh = 0
    for x in s:
        if x in r:
            r.remove(x); sh += 1
    return 2 - sh

# ----------------------------------------------------------------------------- generation
def make_cross(seed, axis, *, free_a, free_b, dr_allowed_a, dr_allowed_b, dr_rate=0.3,
               rec=None, unlinked=(), flat_cells=0, nearflat_cells=0, ibd_a=False, ibd_b=False,
               coarse=0.8, rows_a=None, rows_b=None, pin_a=None, pin_b=None, shift_prob=0.0, flat_on_free=False, depth_boost=None):
    """free_a/free_b: {marker: dosage} for the non-pinned markers; the rest are 0 or 4.
    dr_allowed_*: list of M booleans. rec: list of M-1 fractions (default 0.2) with
    `unlinked` intervals forced to 0.5. ibd_*: make rows 0 and 1 identical."""
    rng = random.Random(seed)
    dos_a = [rng.choice([0, 4]) for _ in range(M)]; dos_b = [rng.choice([0, 4]) for _ in range(M)]
    for m, d in (pin_a or {}).items(): dos_a[m] = d
    for m, d in (pin_b or {}).items(): dos_b[m] = d
    for m, d in free_a.items(): dos_a[m] = d
    for m, d in free_b.items(): dos_b[m] = d
    # truth rows consistent with the dosages
    def truth_rows(dos, ibd):
        rows = [[0]*M for _ in range(4)]
        for m in range(M):
            ones = rng.sample(range(4), dos[m])
            for h in ones: rows[h][m] = 1
        if ibd:
            # rows 0 and 1 identical: re-derive the dosage the rows now have
            for m in range(M): rows[1][m] = rows[0][m]
        return rows
    hap_a = rows_a or truth_rows(dos_a, ibd_a); hap_b = rows_b or truth_rows(dos_b, ibd_b)
    dos_a = [sum(hap_a[h][m] for h in range(4)) for m in range(M)]
    dos_b = [sum(hap_b[h][m] for h in range(4)) for m in range(M)]
    rate_a = [round(dr_rate, 6) if dr_allowed_a[m] else 0.0 for m in range(M)]
    rate_b = [round(dr_rate, 6) if dr_allowed_b[m] else 0.0 for m in range(M)]
    if rec is None: rec = [0.2]*(M-1)
    rec_a = list(rec); rec_b = list(rec)
    for m in unlinked: rec_a[m] = 0.5; rec_b[m] = 0.5
    # sample paths from the model
    def sample_path(allowed, rate, r):
        def w(m, s):
            if s[0] == s[1]: return rate[m] if allowed[m] else 0.0
            return 1 - rate[m]
        cur = rng.choices(range(10), weights=[w(0, s) for s in STATES])[0]; path = [cur]
        for m in range(1, M):
            ws = []
            for j, s in enumerate(STATES):
                k = switch_count(STATES[cur], s); ws.append((r[m-1]**k) * ((1-r[m-1])**(2-k)) * w(m, s))
            cur = rng.choices(range(10), weights=ws)[0]; path.append(cur)
        return path
    gl = []
    free_markers = sorted(set(free_a) | set(free_b))
    cells = [(c, m) for c in range(N) for m in (free_markers if flat_on_free else range(M))]
    flat = set(rng.sample(cells, min(flat_cells, len(cells)))); near = set(rng.sample([x for x in cells if x not in flat], min(nearflat_cells, len(cells) - len(flat))))
    for c in range(N):
        pa = sample_path(dr_allowed_a, rate_a, rec_a); pb = sample_path(dr_allowed_b, rate_b, rec_b)
        row = []
        for m in range(M):
            sa, sb = STATES[pa[m]], STATES[pb[m]]
            truth = hap_a[sa[0]][m] + hap_a[sa[1]][m] + hap_b[sb[0]][m] + hap_b[sb[1]][m]
            if (c, m) in flat:
                slot = [0.0]*5
            elif (c, m) in near:
                slot = [round(-0.02*abs(d-truth) - 0.01*rng.random(), 6) for d in range(5)]
            else:
                # Coverage varies by marker: a well covered marker separates
                # neighbouring dosages more sharply than a thinly covered one.
                k = coarse * rng.uniform(0.6, 1.4) * ((depth_boost or {}).get(m, 1.0))
                # Low depth occasionally puts the likelihood peak one dosage off
                # the truth: the reads happened to over- or under-represent an
                # allele. The likelihoods stay finite and well formed.
                peak = truth
                if rng.random() < shift_prob:
                    peak = max(0, min(4, truth + rng.choice([-1, 1])))
                slot = [round(-k*abs(d-peak), 6) for d in range(5)]
                if rng.random() < 0.3 and truth < 4: slot[truth+1] = slot[truth]
            row.append([min(0.0, v) for v in slot])
        gl.append(row)
    return {"format_version": 1, "markers": M, "offspring": N,
            "parent_dosage": {"A": dos_a, "B": dos_b}, "child_gl": gl,
            "recombination": {"A": rec_a, "B": rec_b},
            "double_reduction": {"A": {"allowed": dr_allowed_a, "rate": rate_a},
                                 "B": {"allowed": dr_allowed_b, "rate": rate_b}}}

# ----------------------------------------------------------------------------- exhaustive enumeration
def tables(cross, legality="strict", penal_rate=0.3):
    """Node legality/DR terms and transition matrices. legality='relaxed' treats a
    forbidden DR form as allowed at penal_rate (author-side probe only)."""
    dr = cross["double_reduction"]; rec = cross["recombination"]
    drt = np.zeros((M, 100))
    for m in range(M):
        for j, (ia, ib) in enumerate(JOINT):
            v = 0.0
            for P, s in (("A", STATES[ia]), ("B", STATES[ib])):
                if s[0] == s[1]:
                    if dr[P]["allowed"][m]: v += math.log(dr[P]["rate"][m])
                    elif legality == "relaxed": v += math.log(penal_rate)
                    else: v = -math.inf
                else:
                    v += math.log(1 - dr[P]["rate"][m])
            drt[m, j] = v
    T = np.zeros((M-1, 100, 100))
    for m in range(M-1):
        ra, rb = rec["A"][m], rec["B"][m]
        for j, (ia, ib) in enumerate(JOINT):
            for k, (ja, jb) in enumerate(JOINT):
                ka = switch_count(STATES[ia], STATES[ja]); kb = switch_count(STATES[ib], STATES[jb])
                T[m, j, k] = ka*math.log(ra) + (2-ka)*math.log(1-ra) + kb*math.log(rb) + (2-kb)*math.log(1-rb)
    dose = np.zeros((16, 16, 100), dtype=int)
    for ma in range(16):
        for mb in range(16):
            ca = [(ma >> h) & 1 for h in range(4)]; cb = [(mb >> h) & 1 for h in range(4)]
            for j, (ia, ib) in enumerate(JOINT):
                sa, sb = STATES[ia], STATES[ib]
                dose[ma, mb, j] = ca[sa[0]] + ca[sa[1]] + cb[sb[0]] + cb[sb[1]]
    return drt, T, dose

def pack(col): return sum(b << h for h, b in enumerate(col))

def value_of(cross, drt, T, dose, ca, cb):
    """Exact total for one column assignment: sum over progeny of a Viterbi max."""
    gl = np.array(cross["child_gl"])  # N, M, 5
    node = np.empty((N, M, 100))
    for m in range(M):
        d = dose[pack(ca[m]), pack(cb[m])]            # 100
        node[:, m, :] = gl[:, m, :][:, d] + drt[m][None, :]
    f = node[:, 0, :]
    for m in range(1, M):
        f = np.max(f[:, :, None] + T[m-1][None, :, :], axis=1) + node[:, m, :]
    return float(np.sum(np.max(f, axis=1)))

def enumerate_optima(cross, legality="strict", tol=1e-9):
    drt, T, dose = tables(cross, legality)
    A = [cols(d) for d in cross["parent_dosage"]["A"]]; B = [cols(d) for d in cross["parent_dosage"]["B"]]
    space = 1
    for m in range(M): space *= len(A[m]) * len(B[m])
    best = -math.inf; opts = []
    for ca in itertools.product(*A):
        for cb in itertools.product(*B):
            v = value_of(cross, drt, T, dose, ca, cb)
            if v > best + tol: best, opts = v, [(ca, cb)]
            elif v >= best - tol: opts.append((ca, cb))
    return best, opts, space

def rows_of(colseq): return [[colseq[m][h] for m in range(M)] for h in range(4)]

def required_paths(cross, hap_a, hap_b, legality="strict"):
    """The required path per progeny for fixed haplotypes: lexicographically smallest
    signature (parent A's states over all markers, then parent B's) among the
    optimal paths, derived from the specification's ordering rule."""
    drt, T, _ = tables(cross, legality)
    gl = cross["child_gl"]
    out = []
    for c in range(N):
        node = np.full((M, 100), -np.inf)
        for m in range(M):
            for j, (ia, ib) in enumerate(JOINT):
                if drt[m, j] == -math.inf: continue
                sa, sb = STATES[ia], STATES[ib]
                d = hap_a[sa[0]][m] + hap_a[sa[1]][m] + hap_b[sb[0]][m] + hap_b[sb[1]][m]
                node[m, j] = gl[c][m][d] + drt[m, j]
        # backward table: best completion strictly after m from joint state j
        back = np.zeros((M, 100))
        for m in range(M-2, -1, -1):
            back[m] = np.max(T[m] + (node[m+1] + back[m+1])[None, :], axis=1)
        opt = float(np.max(node[0] + back[0]))
        # settle parent A marker by marker: smallest state that still admits an optimum
        a_seq = []; reach = None  # reach: best prefix score indexed by B state at last marker
        for m in range(M):
            for cand in range(10):
                if m == 0:
                    nxt = np.array([node[0, cand*10+ib] for ib in range(10)])
                else:
                    nxt = np.full(10, -np.inf)
                    for ib in range(10):
                        j = cand*10 + ib
                        if node[m, j] == -math.inf: continue
                        best = -math.inf
                        for pb in range(10):
                            if reach[pb] == -math.inf: continue
                            v = reach[pb] + T[m-1, a_seq[-1]*10+pb, j]
                            if v > best: best = v
                        if best != -math.inf: nxt[ib] = best + node[m, j]
                ok = any(nxt[ib] != -math.inf and nxt[ib] + back[m, cand*10+ib] >= opt - 1e-7 for ib in range(10))
                if ok:
                    a_seq.append(cand); reach = nxt; break
            else:
                raise RuntimeError("no state admits an optimum")
        # settle parent B against the fixed A sequence
        backb = np.zeros((M, 10))
        for m in range(M-2, -1, -1):
            for ib in range(10):
                best = -math.inf
                for nb in range(10):
                    j = a_seq[m+1]*10 + nb
                    if node[m+1, j] == -math.inf: continue
                    v = T[m, a_seq[m]*10+ib, j] + node[m+1, j] + backb[m+1, nb]
                    if v > best: best = v
                backb[m, ib] = best
        optb = max(node[0, a_seq[0]*10+ib] + backb[0, ib] for ib in range(10))
        b_seq = []; running = 0.0
        for m in range(M):
            for cand in range(10):
                j = a_seq[m]*10 + cand
                if node[m, j] == -math.inf: continue
                v = node[0, j] if m == 0 else running + T[m-1, a_seq[m-1]*10+b_seq[-1], j] + node[m, j]
                if v + backb[m, cand] >= optb - 1e-7:
                    b_seq.append(cand); running = v; break
            else:
                raise RuntimeError("no B state admits an optimum")
        out.append({"A": [list(STATES[s]) for s in a_seq], "B": [list(STATES[s]) for s in b_seq]})
    return out

def canonical_parent(rows, states):
    best = None
    for p in itertools.permutations(range(4)):
        nr = [None]*4
        for h in range(4): nr[p[h]] = rows[h]
        if not all(nr[i] <= nr[i+1] for i in range(3)): continue
        sig = []
        for prog in states:
            for a, b in prog:
                x, y = sorted((p[a], p[b])); sig += [x, y]
        key = (tuple(map(tuple, nr)), tuple(sig), tuple(p))
        if best is None or key < best[0]: best = (key, p, nr)
    _, p, nr = best
    return nr, [[sorted((p[a], p[b])) for a, b in prog] for prog in states], p

def canonicalise(hap_a, hap_b, paths):
    ra, sa, _ = canonical_parent(hap_a, [pr["A"] for pr in paths])
    rb, sb, _ = canonical_parent(hap_b, [pr["B"] for pr in paths])
    return ra, rb, [{"A": sa[c], "B": sb[c]} for c in range(N)]

def distinct_key(ra, rb, paths):
    return (tuple(v for r in ra for v in r), tuple(v for r in rb for v in r),
            tuple(v for pr in paths for P in "AB" for s in pr[P] for v in s))

def required_answer(cross, opts):
    """Among the enumerated tied assignments, the one the result format requires:
    canonicalise each with its required paths, order by the documented key."""
    best = None
    for ca, cb in opts:
        ha, hb = rows_of(ca), rows_of(cb)
        paths = required_paths(cross, ha, hb)
        ra, rb, cp = canonicalise(ha, hb, paths)
        key = distinct_key(ra, rb, cp)
        if best is None or key < best[0]: best = (key, ra, rb, cp)
    return best[1], best[2], best[3]

# ----------------------------------------------------------------------------- properties
def props(cross, exp, opts, relaxed_opt):
    dr = cross["double_reduction"]
    paths = exp["paths"]; ha, hb = exp["haplotypes"]["A"], exp["haplotypes"]["B"]
    dr_a0 = sum(1 for p in paths if p["A"][0][0] == p["A"][0][1])
    dr_b0 = sum(1 for p in paths if p["B"][0][0] == p["B"][0][1])
    dr_any = sum(1 for p in paths for P in "AB" for s in p[P] if s[0] == s[1])
    rr = {P: sum(1 for i in range(3) if exp["haplotypes"][P][i] == exp["haplotypes"][P][i+1]) for P in "AB"}
    # canonical rule 4 needed: >1 permutation survives the row rule with distinct signatures
    rule4 = {}
    for P, rows in (("A", ha), ("B", hb)):
        surv = []
        for p in itertools.permutations(range(4)):
            nr = [None]*4
            for h in range(4): nr[p[h]] = rows[h]
            if all(nr[i] <= nr[i+1] for i in range(3)): surv.append(p)
        rule4[P] = len(surv) > 1
    unl = sum(1 for P in "AB" for r in cross["recombination"][P] if r == 0.5)
    flat = sum(1 for c in range(N) for m in range(M) if len(set(cross["child_gl"][c][m])) == 1)
    near = sum(1 for c in range(N) for m in range(M) if (max(cross["child_gl"][c][m]) - min(cross["child_gl"][c][m])) < 0.1 and len(set(cross["child_gl"][c][m])) > 1)
    sw = sum(1 for p in paths for P in "AB" for a, b in zip(p[P], p[P][1:]) if set(a) != set(b))
    mats = {(tuple(map(tuple, sorted(rows_of(ca)))), tuple(map(tuple, sorted(rows_of(cb))))) for ca, cb in opts}
    ua = {a for a, b in mats}; ub = {b for a, b in mats}
    return dict(distinct_assignments=len(opts), distinct_canonical=len(mats), rule_deciding=("A" if len(ua) > 1 else ("B" if len(ub) > 1 else "paths")),
                dr_a_marker0=dr_a0, dr_b_marker0=dr_b0, dr_states=dr_any, dr_allowed_a0=dr["A"]["allowed"][0], dr_allowed_b0=dr["B"]["allowed"][0],
                forbidden_dr_tempting=(relaxed_opt is not None and relaxed_opt > exp["total_log_likelihood"] + 1e-6),
                repeated_rows=rr, rule4_needed=rule4, unlinked_intervals=unl, flat_cells=flat, nearflat_cells=near, switches=sw)

if __name__ == "__main__":
    pass
