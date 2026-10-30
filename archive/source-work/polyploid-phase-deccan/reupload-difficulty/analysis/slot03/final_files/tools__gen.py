#!/usr/bin/env python3
"""Generate random crosses for stress testing the solver."""
import json, random, sys
from math import comb

def gen(seed, M, N, slope=2.0, depth_noise=0.0, dosages=None, dr_frac=0.5):
    rng = random.Random(seed)
    if dosages is None:
        dos_a = [rng.choice([0,1,2,2,2,3,4]) for _ in range(M)]
        dos_b = [rng.choice([0,1,2,2,2,3,4]) for _ in range(M)]
    else:
        dos_a, dos_b = dosages
    # planted truth: columns consistent with dosage
    def cols(d):
        return [c for c in range(16) if bin(c).count('1') == d]
    ca = [rng.choice(cols(d)) for d in dos_a]
    cb = [rng.choice(cols(d)) for d in dos_b]
    def bit(mask, h): return (mask >> h) & 1
    states = [(0,0),(0,1),(0,2),(0,3),(1,1),(1,2),(1,3),(2,2),(2,3),(3,3)]
    def dosage(sa, sb, m):
        return (bit(ca[m],sa[0])+bit(ca[m],sa[1])+bit(cb[m],sb[0])+bit(cb[m],sb[1]))
    gl = []
    for c in range(N):
        # a random path, mostly constant with occasional switches
        sa = rng.choice(states); sb = rng.choice(states)
        rows = []
        for m in range(M):
            if rng.random() < 0.08: sa = rng.choice(states)
            if rng.random() < 0.08: sb = rng.choice(states)
            d = dosage(sa, sb, m)
            row = [-slope*abs(dd-d) + rng.gauss(0, depth_noise) for dd in range(5)]
            row = [round(x, 6) for x in row]
            rows.append(row)
        gl.append(rows)
    rec_a = [round(rng.uniform(0.02, 0.30), 6) for _ in range(M-1)]
    rec_b = [round(rng.uniform(0.02, 0.30), 6) for _ in range(M-1)]
    allowed_a = [rng.random() < dr_frac for _ in range(M)]
    allowed_b = [rng.random() < dr_frac for _ in range(M)]
    rate_a = [round(rng.uniform(0.05,0.4),4) if a else 0.0 for a in allowed_a]
    rate_b = [round(rng.uniform(0.05,0.4),4) if a else 0.0 for a in allowed_b]
    return {
        "format_version": 1, "markers": M, "offspring": N,
        "parent_dosage": {"A": dos_a, "B": dos_b},
        "child_gl": gl,
        "recombination": {"A": rec_a, "B": rec_b},
        "double_reduction": {
            "A": {"allowed": allowed_a, "rate": rate_a},
            "B": {"allowed": allowed_b, "rate": rate_b}},
    }

if __name__ == "__main__":
    seed = int(sys.argv[1]); M = int(sys.argv[2]); N = int(sys.argv[3])
    slope = float(sys.argv[4]) if len(sys.argv) > 4 else 2.0
    noise = float(sys.argv[5]) if len(sys.argv) > 5 else 0.0
    path = sys.argv[6] if len(sys.argv) > 6 else "/tmp/cross.json"
    json.dump(gen(seed, M, N, slope, noise), open(path, "w"))
    print(path)
