#!/usr/bin/env python3
"""ORACLE B — physical-first, decode-then-fit.

Organisation (deliberately different from oracle A):
  1. Per relocation type, the set C of container bits the legacy linker ever
     changed is computed from the linked observations.  Every bit of C is a
     field bit; B assumes the field is exactly C (a fixture where a field bit
     never changes is not identifiable and is rejected by certification when
     oracle A reports extra fits).
  2. All normalised ways of cutting C into at most three ordered slices are
     enumerated from the physical side.
  3. For each layout the logical value of every observation is DECODED from the
     legacy word, and only then are the scalar semantics fitted by direct
     equation checking, using B's own arithmetic (bit masking for wrap,
     explicit interval tests for ranges, floor shifts for the pair split).
  4. Pair coupling is solved as a separate join over the two members' decoded
     values.  Canonicalisation happens last, over the complete fit set.
Author/verifier side only; never ships.
"""
import sys, os, itertools
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "research"))
from model import Program, EXPRS, SCALES, SIGNS, OVFS, load_corpus

MAX_SLICES = 3


def runs_of(mask, container):
    runs, j = [], 0
    while j < container:
        if (mask >> j) & 1:
            k = j
            while k < container and (mask >> k) & 1:
                k += 1
            runs.append((j, k))
            j = k
        else:
            j += 1
    return runs


def layouts_from_mask(mask, container):
    runs = runs_of(mask, container)
    if not runs or len(runs) > MAX_SLICES:
        return []
    out = []
    # distribute piece counts over runs
    for counts in itertools.product(range(1, MAX_SLICES + 1), repeat=len(runs)):
        if sum(counts) > MAX_SLICES:
            continue
        per_run_options = []
        for (lo, hi), m in zip(runs, counts):
            n = hi - lo
            if m > n:
                per_run_options = None
                break
            opts = []
            for cuts in itertools.combinations(range(lo + 1, hi), m - 1):
                bounds = [lo] + list(cuts) + [hi]
                opts.append([(bounds[i], bounds[i + 1] - bounds[i]) for i in range(m)])
            per_run_options.append(opts)
        if per_run_options is None:
            continue
        for choice in itertools.product(*per_run_options):
            pieces = [p for group in choice for p in group]
            for perm in itertools.permutations(pieces):
                ok = True
                for (l0, w0), (l1, _) in zip(perm, perm[1:]):
                    if l0 + w0 == l1:
                        ok = False
                        break
                if ok:
                    out.append(tuple(perm))
    return out


def decode(word, layout):
    v, shift = 0, 0
    for lsb, w in layout:
        v |= ((word >> lsb) & ((1 << w) - 1)) << shift
        shift += w
    return v


# ---- B's own arithmetic -----------------------------------------------------
def b_eval(expr, S, A, P, B):
    table = {"S": S, "S+A": S + A, "S-P": S - P, "S+A-P": S + A - P, "S+B+A-P": S + B + A - P}
    return table[expr]


def b_trunc(v, d):
    return -((-v) // d) if v < 0 else v // d


def b_in_range(x, w, sign):
    if sign == "signed":
        return -(1 << (w - 1)) <= x < (1 << (w - 1))
    return 0 <= x < (1 << w)


def b_pattern(x, w):
    return x & ((1 << w) - 1)


def b_split(q, k):
    lo = q & ((1 << k) - 1)
    hi = (q - lo) >> k
    return hi, lo


def check_sign_ovf(values, sites, w):
    """Given that the observed patterns already match, which (sign, ovf) pairs
    are consistent with every acceptance/rejection record?"""
    fits = []
    for sign in SIGNS:
        for ovf in OVFS:
            ok = True
            for x, s in zip(values, sites):
                if s.rejected is None:
                    continue
                inr = b_in_range(x, w, sign)
                if s.rejected:
                    if inr or ovf != "reject":
                        ok = False
                        break
                elif not inr and ovf == "reject":
                    ok = False
                    break
            if ok:
                fits.append((sign, ovf))
    return fits


class DecodedIndex:
    """Physical-first view of one relocation type: every normalised cut of the
    changed-bit set C into <=3 ordered slices, DECODED once from the observed
    legacy words, and indexed by (width, decoded tuple)."""
    def __init__(self, sites, container):
        self.sites = sites
        self.linked = [s for s in sites if s.out_word is not None]
        mask = 0
        for s in self.linked:
            mask |= s.out_word ^ s.obj_word
        self.by_width = {}
        if self.linked:
            for lay in layouts_from_mask(mask, container):
                w = sum(x for _, x in lay)
                key = tuple(decode(s.out_word, lay) for s in self.linked)
                self.by_width.setdefault(w, {}).setdefault(key, []).append(lay)

    def match(self, values):
        """values: mathematical component per site (all sites, in order).
        Returns [(sign, ovf, layout)] whose decoded patterns equal the expected
        patterns and whose range/rejection behaviour is consistent."""
        out = []
        observed_vals = [v for v, s in zip(values, self.sites) if s.out_word is not None]
        for w, table in self.by_width.items():
            key = tuple(b_pattern(v, w) for v in observed_vals)
            lays = table.get(key)
            if not lays:
                continue
            so = check_sign_ovf(values, self.sites, w)
            for lay in lays:
                for sign, ovf in so:
                    out.append((sign, ovf, lay))
        return out


def fit_unpaired(sites, container):
    idx = DecodedIndex(sites, container)
    fits = []
    for expr in EXPRS:
        for scale in SCALES:
            vals = [b_trunc(b_eval(expr, s.S, s.A, s.P, s.B), scale) for s in sites]
            for sign, ovf, lay in idx.match(vals):
                fits.append(Program(expr, scale, sign, ovf, lay))
    return fits


def fit_pair(group, t1, t2, instances, containers):
    a_sites = [a for a, _ in instances]
    b_sites = [b for _, b in instances]
    ia = DecodedIndex(a_sites, containers[t1])
    ib = DecodedIndex(b_sites, containers[t2])
    fits = []
    for expr in EXPRS:
        for scale in SCALES:
            qs = [b_trunc(b_eval(expr, a.S, a.A, a.pair_P, a.B), scale) for a in a_sites]
            for k in range(1, 64):
                comps = [b_split(q, k) for q in qs]
                for ra, rb in (("hi", "lo"), ("lo", "hi")):
                    va = [c[0] if ra == "hi" else c[1] for c in comps]
                    fa = ia.match(va)
                    if not fa:
                        continue
                    vb = [c[0] if rb == "hi" else c[1] for c in comps]
                    fb = ib.match(vb)
                    for (sa, oa, la) in fa:
                        for (sb, ob, lb) in fb:
                            fits.append((Program(expr, scale, sa, oa, la, (group, ra, k)),
                                         Program(expr, scale, sb, ob, lb, (group, rb, k))))
    return fits


def solve(root):
    man, objs, outs, sites = load_corpus(root)
    types = {t["type"]: t for t in man["relocation_types"]}
    result, groups = {}, {}
    for tid, t in types.items():
        if t["pair_group"] is None:
            fits = fit_unpaired([s for s in sites if s.type == tid], t["container_bits"])
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
