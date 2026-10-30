#!/usr/bin/env python3
"""Compare candidate probe tables against the reference: PASS when rc and digest match.
usage: compare_runs.py <runs_dir> [labels...]"""
import sys, os
runs = sys.argv[1]; labels = sys.argv[2:] or ["nop"] + [f"slot{i:02d}" for i in range(1, 9)]
def load(label):
    p = os.path.join(runs, f"{label}.txt.table"); d = {}
    if os.path.exists(p):
        for l in open(p):
            parts = l.split()
            if len(parts) == 3: d[parts[0]] = (parts[1], parts[2])
    return d
ref = load("ref"); tabs = {l: load(l) for l in labels}
cases = list(ref)
w = max(len(c) for c in cases) + 2
print(f"{'case':<{w}} " + " ".join(f"{l:>6}" for l in labels))
tot = {l: 0 for l in labels}
for c in cases:
    row = []
    for l in labels:
        t = tabs[l].get(c)
        ok = t is not None and t == ref[c]
        if t is None: row.append(f"{'-':>6}")
        else:
            row.append(f"{'PASS' if ok else 'FAIL':>6}")
            if ok: tot[l] += 1
    print(f"{c:<{w}} " + " ".join(row))
print(f"{'passed':<{w}} " + " ".join(f"{tot[l]:>3}/{len(cases):<2}" for l in labels))
