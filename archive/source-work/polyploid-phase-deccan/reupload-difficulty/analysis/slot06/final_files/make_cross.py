#!/usr/bin/env python3
"""Generates synthetic crosses for scaling experiments."""
import json, random, math, sys

def gen(seed, m=28, n=32, slope=1.5, dosage_bias=None, noise=0.0, dr=True):
    rnd = random.Random(seed)
    if dosage_bias is None:
        dosage_bias = [0.15, 0.2, 0.3, 0.2, 0.15]
    def dos():
        x = rnd.random(); acc = 0.0
        for d, w in enumerate(dosage_bias):
            acc += w
            if x <= acc:
                return d
        return 4
    dos_a = [dos() for _ in range(m)]
    dos_b = [dos() for _ in range(m)]
    # planted haplotypes consistent with dosage
    def haps(dosage):
        rows = [[0] * m for _ in range(4)]
        for mm in range(m):
            hs = rnd.sample(range(4), dosage[mm])
            for h in hs:
                rows[h][mm] = 1
        return rows
    hap_a = haps(dos_a)
    hap_b = haps(dos_b)
    # planted paths: mostly constant, occasional switch
    paths_a, paths_b = [], []
    states = [(a, b) for a in range(4) for b in range(a, 4)]
    for c in range(n):
        pa, pb = [], []
        sa = rnd.choice(states); sb = rnd.choice(states)
        for mm in range(m):
            if rnd.random() < 0.04:
                sa = rnd.choice(states)
            if rnd.random() < 0.04:
                sb = rnd.choice(states)
            pa.append(sa); pb.append(sb)
        paths_a.append(pa); paths_b.append(pb)
    child_gl = []
    for c in range(n):
        rows = []
        for mm in range(m):
            truth = (hap_a[paths_a[c][mm][0]][mm] + hap_a[paths_a[c][mm][1]][mm]
                     + hap_b[paths_b[c][mm][0]][mm] + hap_b[paths_b[c][mm][1]][mm])
            row = []
            for d in range(5):
                v = -slope * abs(d - truth)
                if noise:
                    v += rnd.gauss(0, noise)
                row.append(v)
            rows.append(row)
        child_gl.append(rows)

    def parent(dosage):
        rec = [round(rnd.uniform(0.02, 0.35), 6) for _ in range(m - 1)]
        allowed = [bool(dr and rnd.random() < 0.6) for _ in range(m)]
        rate = [round(rnd.uniform(0.05, 0.35), 6) if a else 0.0 for a in allowed]
        # legality for the planted paths
        for mm in range(m):
            if any(paths_a[c][mm][0] == paths_a[c][mm][1] for c in range(n)):
                allowed[mm] = True
                if rate[mm] == 0.0:
                    rate[mm] = 0.2
        return {"recombination": rec, "allowed": allowed, "rate": rate}

    pa_p = parent(dos_a)
    pb_p = parent(dos_b)
    # force DR legality where planted paths need it
    for mm in range(m):
        if any(paths_b[c][mm][0] == paths_b[c][mm][1] for c in range(n)):
            pb_p["allowed"][mm] = True
            if pb_p["rate"][mm] == 0.0:
                pb_p["rate"][mm] = 0.2
    return {
        "format_version": 1,
        "markers": m,
        "offspring": n,
        "parent_dosage": {"A": dos_a, "B": dos_b},
        "child_gl": child_gl,
        "recombination": {"A": pa_p["recombination"], "B": pb_p["recombination"]},
        "double_reduction": {
            "A": {"allowed": pa_p["allowed"], "rate": pa_p["rate"]},
            "B": {"allowed": pb_p["allowed"], "rate": pb_p["rate"]},
        },
    }

if __name__ == "__main__":
    seed = int(sys.argv[1])
    m = int(sys.argv[2]) if len(sys.argv) > 2 else 28
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 32
    slope = float(sys.argv[4]) if len(sys.argv) > 4 else 1.5
    bias = None
    if len(sys.argv) > 5:
        bias = [float(x) for x in sys.argv[5].split(",")]
    out = gen(seed, m, n, slope, bias)
    path = f"/tmp/gen{seed}_{m}_{n}_{slope}.json"
    with open(path, "w") as f:
        json.dump(out, f)
    print(path)
