#!/usr/bin/env python3
"""Per-snapshot pass counts against a reference probe table.
usage: snapshot_matrix.py <runs_dir> [prefix-filter]"""
import sys, os, re, collections
runs = sys.argv[1]
def load(p):
    d = {}
    for l in open(p):
        parts = l.split()
        if len(parts) == 3: d[parts[0]] = (parts[1], parts[2])
    return d
ref = load(os.path.join(runs, "ref.txt.table"))
graded = [c for c in ref if not c.startswith("zz_")]
rows = []
for f in sorted(os.listdir(runs)):
    m = re.match(r"(slot\d\d)_(\d\d\d)_step(\w+?)_([0-9a-f]+)\.txt\.table$", f)
    if not m: continue
    t = load(os.path.join(runs, f))
    if not t:
        rows.append((m.group(1), m.group(2), m.group(3), "no build", "", "")); continue
    passed = [c for c in graded if t.get(c) == ref[c]]
    failed = [c for c in graded if t.get(c) != ref[c]]
    fam = collections.Counter(c.split("_")[0] for c in failed)
    rows.append((m.group(1), m.group(2), m.group(3), f"{len(passed)}/{len(graded)}", " ".join(f"{k}:{v}" for k, v in sorted(fam.items())), " ".join(failed[:6])))
print(f"{'slot':<7}{'#':<5}{'step':<8}{'passed':<9}{'failing families':<40} first failing cases")
for r in rows: print(f"{r[0]:<7}{r[1]:<5}{r[2]:<8}{r[3]:<9}{r[4]:<40} {r[5]}")
