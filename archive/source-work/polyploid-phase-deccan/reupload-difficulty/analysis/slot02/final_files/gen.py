#!/usr/bin/env python3
"""Generate a synthetic cross document for timing and robustness checks."""
import json, math, random, sys

def gen(M, N, seed, dosages=None, out="/tmp/syn.json"):
    rnd = random.Random(seed)
    if dosages is None:
        dosages = ([2] * M, [2] * M)
    doc = {"format_version": 1, "markers": M, "offspring": N}
    doc["parent_dosage"] = {"A": dosages[0][:], "B": dosages[1][:]}

    # Truth: random haplotypes consistent with the dosages.
    def haps(dose):
        rows = [[0] * M for _ in range(4)]
        for m in range(M):
            ones = set(rnd.sample(range(4), dose[m]))
            for h in range(4):
                rows[h][m] = 1 if h in ones else 0
        return rows
    hapA = haps(dosages[0])
    hapB = haps(dosages[1])

    # Paths: a random walk over the ten inheritance states.
    def walk():
        path = []
        a, b = rnd.randrange(4), rnd.randrange(4)
        for m in range(M):
            if rnd.random() < 0.25:
                a = rnd.randrange(4)
            if rnd.random() < 0.25:
                b = rnd.randrange(4)
            if a > b:
                a, b = b, a
            path.append([a, b])
        return path
    paths = [(walk(), walk()) for _ in range(N)]

    gl = []
    for c in range(N):
        row = []
        for m in range(M):
            pa, pb = paths[c]
            d = 0
            for h in pa[0][m:m+1]:
                pass
            (a1, a2), (b1, b2) = pa[m], pb[m]
            d = hapA[a1][m] + hapA[a2][m] + hapB[b1][m] + hapB[b2][m]
            vals = []
            for k in range(5):
                # Low depth: a few reads at the true dosage, weakly informative.
                base = -abs(k - d) * rnd.uniform(0.3, 2.0)
                vals.append(base + rnd.uniform(-0.6, 0.6))
            row.append(vals)
        gl.append(row)
    doc["child_gl"] = gl

    doc["recombination"] = {
        "A": [rnd.uniform(0.02, 0.3) for _ in range(M - 1)],
        "B": [rnd.uniform(0.02, 0.3) for _ in range(M - 1)],
    }
    dr = {}
    for P in "AB":
        allowed = [rnd.random() < 0.5 for _ in range(M)]
        rate = [rnd.uniform(0.01, 0.2) if allowed[m] else 0.0 for m in range(M)]
        dr[P] = {"allowed": allowed, "rate": rate}
    doc["double_reduction"] = dr
    with open(out, "w") as fh:
        json.dump(doc, fh)
    return out

if __name__ == "__main__":
    M = int(sys.argv[1]) if len(sys.argv) > 1 else 28
    N = int(sys.argv[2]) if len(sys.argv) > 2 else 32
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    out = sys.argv[4] if len(sys.argv) > 4 else "/tmp/syn.json"
    print(gen(M, N, seed, None, out))
