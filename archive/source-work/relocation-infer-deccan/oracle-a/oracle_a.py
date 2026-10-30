#!/usr/bin/env python3
"""ORACLE A — exhaustive fit-then-encode enumeration over the public grammar.

For every scalar hypothesis (expression, scale, signedness, overflow, width) the
logical bit pattern of every linked observation is computed first; a depth-first
walk over the logical bits then places each logical bit on a compatible physical
bit of the container, building at most three slices in normalised form.  A
hypothesis survives when every linked observation reproduces exactly, every
non-slice bit is preserved, and every legacy-rejected observation is rejected.
Pairs are enumerated jointly over (expression, scale, split, roles) with the two
members fitted on their components.  The canonical answer is the minimum
published key.  Author/verifier side only; never ships.
"""
import sys, os, itertools
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "research"))
from model import (Program, EXPRS, SCALES, SIGNS, OVFS, ROLES, MAX_SLICES, evaluate, trunc_div,
                   split_pair, encode_bits, load_corpus)


class ObsIndex:
    """Per-type physical view of the observed words, built once: signature of
    every container bit across the observed words, and which bits are preserved."""
    def __init__(self, sites, container):
        self.obs = [(s.obj_word, s.out_word) for s in sites if s.out_word is not None]
        self.container = container
        self.preserved = [all(((outw >> j) & 1) == ((ow >> j) & 1) for ow, outw in self.obs) for j in range(container)]
        self.by_sig = {}
        for j in range(container):
            sig = tuple((outw >> j) & 1 for _, outw in self.obs)
            self.by_sig.setdefault(sig, []).append(j)


def layouts_for(index: ObsIndex, bits_list, width):
    """Place logical bits 0..width-1 on compatible physical bits, at most three
    normalised slices; a hypothesis dies at the first logical bit no physical bit
    can carry."""
    compat = []
    for t in range(width):
        cs = index.by_sig.get(tuple((b >> t) & 1 for b in bits_list))
        if not cs:
            return []
        compat.append(cs)
    results = []
    preserved, container = index.preserved, index.container

    def dfs(t, prev, used, slices):
        if t == width:
            for j in range(container):
                if not (used >> j) & 1 and not preserved[j]:
                    return
            results.append(tuple((l, w) for l, w in slices))
            return
        for j in compat[t]:
            if (used >> j) & 1:
                continue
            if prev is not None and j == prev + 1:
                slices[-1][1] += 1
                dfs(t + 1, j, used | (1 << j), slices)
                slices[-1][1] -= 1
            else:
                if len(slices) == MAX_SLICES:
                    continue
                slices.append([j, 1])
                dfs(t + 1, j, used | (1 << j), slices)
                slices.pop()

    dfs(0, None, 0, [])
    return results


def component_fits(values, sites, index: ObsIndex):
    """values: per site the mathematical component (already split for pairs).
    Enumerate (sign, ovf, width, layout) reproducing every observation and every
    legacy rejection/acceptance."""
    fits = []
    if not index.obs:
        return fits
    for sign in SIGNS:
        for ovf in OVFS:
            for width in range(1, index.container + 1):
                ok = True
                bits_list = []
                for v, s in zip(values, sites):
                    if s.rejected is None:
                        continue
                    b = encode_bits(v, width, sign, ovf)
                    if s.rejected and b is not None:
                        ok = False
                        break
                    if not s.rejected:
                        if b is None:
                            ok = False
                            break
                        if s.out_word is not None:
                            bits_list.append(b)
                if not ok:
                    continue
                for lay in layouts_for(index, bits_list, width):
                    fits.append((sign, ovf, lay))
    return fits


def fit_unpaired(sites, container):
    fits = []
    index = ObsIndex(sites, container)
    for expr in EXPRS:
        for scale in SCALES:
            vals = [trunc_div(evaluate(expr, s.S, s.A, s.P, s.B), scale) for s in sites]
            for sign, ovf, lay in component_fits(vals, sites, index):
                fits.append(Program(expr, scale, sign, ovf, lay))
    return fits


def fit_pair(group, t_first, t_second, instances, containers):
    """instances: list of (site_a, site_b) where site_a has type t_first (lower id)
    and site_b type t_second; both share S, A, B and the pair's P."""
    fits = []
    a_sites = [a for a, _ in instances]
    b_sites = [b for _, b in instances]
    ia = ObsIndex(a_sites, containers[t_first])
    ib = ObsIndex(b_sites, containers[t_second])
    for expr in EXPRS:
        for scale in SCALES:
            qs = [trunc_div(evaluate(expr, a.S, a.A, a.pair_P, a.B), scale) for a, _ in instances]
            for k in range(1, 64):
                comps = [split_pair(q, k) for q in qs]
                for roles in (("hi", "lo"), ("lo", "hi")):
                    v1 = [c[0] if roles[0] == "hi" else c[1] for c in comps]
                    v2 = [c[0] if roles[1] == "hi" else c[1] for c in comps]
                    f1 = component_fits(v1, a_sites, ia)
                    if not f1:
                        continue
                    f2 = component_fits(v2, b_sites, ib)
                    for (s1, o1, l1) in f1:
                        for (s2, o2, l2) in f2:
                            fits.append((Program(expr, scale, s1, o1, l1, (group, roles[0], k)),
                                         Program(expr, scale, s2, o2, l2, (group, roles[1], k))))
    return fits


def solve(root):
    man, objs, outs, sites = load_corpus(root)
    types = {t["type"]: t for t in man["relocation_types"]}
    result = {}
    groups = {}
    for tid, t in types.items():
        if t["pair_group"] is None:
            ss = [s for s in sites if s.type == tid]
            fits = fit_unpaired(ss, t["container_bits"])
            result[tid] = {"fits": fits, "canonical": min(fits, key=lambda p: p.key()) if fits else None}
        else:
            groups.setdefault(t["pair_group"], []).append(tid)
    for g, tids in groups.items():
        t1, t2 = sorted(tids)
        by_pid = {}
        for s in sites:
            if s.pair_id is not None and s.type in (t1, t2):
                by_pid.setdefault(s.pair_id, {})[s.type] = s
        instances = [(v[t1], v[t2]) for v in by_pid.values() if t1 in v and t2 in v]
        fits = fit_pair(g, t1, t2, instances, {t1: types[t1]["container_bits"], t2: types[t2]["container_bits"]})
        canon = min(fits, key=lambda pp: (pp[0].key(), pp[1].key())) if fits else None
        result[("pair", g)] = {"types": (t1, t2), "fits": fits, "canonical": canon}
    return result


if __name__ == "__main__":
    import json
    r = solve(sys.argv[1])
    for k, v in r.items():
        c = v["canonical"]
        if isinstance(k, tuple):
            print(k, "fits=%d" % len(v["fits"]), "canonical=", None if c is None else (c[0].to_json(v["types"][0]), c[1].to_json(v["types"][1])))
        else:
            print("type", k, "fits=%d" % len(v["fits"]), "canonical=", None if c is None else json.dumps(c.to_json(k)))
