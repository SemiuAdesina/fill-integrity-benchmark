#!/usr/bin/env python3
"""Random-fabric agreement of every solver against the exhaustive oracle A:
Python oracle C, Python oracle D, the C++ reference (lab/cand/ref2) and the
C++ solver_d (lab/cand/solver_d). Small fabrics so oracle A finishes. Never ships."""
import sys, os, random, time, json, subprocess, tempfile
HERE = os.path.dirname(os.path.abspath(__file__)); W = os.path.join(HERE, "..")
sys.path.insert(0, HERE); sys.path.insert(0, os.path.join(W, "author")); sys.path.insert(0, os.path.join(W, "oracle-b"))
import model, bnb, oracle_d
from gen_fixtures import random_fabric, latent_telemetry

def run_bin(path, doc):
    with tempfile.TemporaryDirectory() as d:
        pp = os.path.join(d, "p.json"); out = os.path.join(d, "r.json")
        json.dump(doc, open(pp, "w"))
        subprocess.run([path, "--problem", pp, "--out", out], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120)
        r = json.load(open(out))
    if r["status"] != "ok": return r["status"], None
    return "ok", (r["paths"], r["combine"], r["sink_value"])

def main(n, seed0):
    rng = random.Random(seed0)
    agree = 0; total = 0; statuses = {}
    for k in range(n):
        ns = rng.randint(1, 4); nr = rng.randint(1, 3); npl = rng.randint(0, 2); extra = rng.randint(0, 3)
        p = random_fabric(rng, ns, nr, npl, extra, shared_endpoint=rng.random() < 0.25, rates=rng.choice([(4, 8, 16), (8,), (4, 8)]), lats=rng.choice([(0, 1, 2, 3), (1,), (0, 0, 1)]))
        mode = rng.random()
        if mode < 0.15:
            p.link_bytes = {}; p.completion = None
        else:
            r = latent_telemetry(rng, p, listed_fraction=rng.choice([0.0, 0.3, 0.6, 0.9]), with_completion=rng.random() < 0.5, zero_listed=rng.random() < 0.6)
            if r is None: continue
            if rng.random() < 0.15:
                if p.link_bytes and rng.random() < 0.5:
                    lid = rng.choice(list(p.link_bytes)); p.link_bytes[lid] += p.payload_bytes
                else:
                    p.completion = (p.completion or 5) - 1
        doc = p.to_json()
        sa, pa, _ = model.recover(p)
        a = None if pa is None else (pa["paths"], pa["combine"], pa["sink_value"])
        sc, pc, _ = bnb.recover_c(p)
        c = None if pc is None else (pc["paths"], pc["combine"], pc["sink_value"])
        sd, pd, _ = oracle_d.recover_d(doc)
        d = None if pd is None else (pd["paths"], pd["combine"], pd["sink_value"])
        sr, r = run_bin(os.path.join(W, "lab/cand/ref2/reduction-recover"), doc)
        ss, s = run_bin(os.path.join(W, "lab/cand/solver_d/reduction-recover"), doc)
        total += 1; statuses[sa] = statuses.get(sa, 0) + 1
        if sa == sc == sd == sr == ss and a == c == d == r == s:
            agree += 1
        else:
            print("DISAGREE seed", seed0, "case", k, "A", sa, a, "| C", sc, c, "| D", sd, d, "| ref", sr, r, "| solver_d", ss, s)
            json.dump(doc, open("/tmp/disagree_cd_%d_%d.json" % (seed0, k), "w"), indent=1)
    print("seed %d: %d/%d agree (A = C = D = reference = solver_d); statuses %s" % (seed0, agree, total, statuses))

if __name__ == "__main__":
    main(int(sys.argv[1]), int(sys.argv[2]))
