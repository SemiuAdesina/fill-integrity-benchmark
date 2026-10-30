#!/usr/bin/env python3
"""Timing sweep of the reference on full-size real-data instances (author-side)."""
import sys, os, random, time, subprocess, json
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generate, bridge, model
BIN = os.path.expanduser(sys.argv[3]) if len(sys.argv) > 3 else os.path.expanduser("~/benchmark-suite-work/plasmid-map-deccan/lab/ref/target/release/plasmid-map")
n = int(sys.argv[1]); seed = int(sys.argv[2]); cap = 180
rng = random.Random(seed)
for i in range(n):
    r = None
    while r is None: r = generate.build(rng, n_enzymes=rng.choice([4, 5, 5]), e=rng.choice([1, 2, 2, 3]), D=rng.choice([100, 150, 200, 250]), K=3, n_doubles=rng.randint(3, 6), n_held=2, insert_range=(800, 5800), bounds=(500, 6000))
    prob, truth, held_truth, meta, vh = r
    d = "/tmp/pm_sweep_%d_%d" % (seed, i); bridge.write_instance(d, prob, vh)
    t0 = time.time()
    try:
        p = subprocess.run([BIN, d, d + "/map.json"], capture_output=True, text=True, timeout=cap)
        el = time.time() - t0; status = "ok" if p.returncode == 0 else "rc=%d" % p.returncode
    except subprocess.TimeoutExpired:
        el = cap; status = "TIMEOUT"
    line = "  %2d %-16s V=%4d L=%4d e=%d D=%3d enz=%d truth_internal=%d doubles=%d: %s %.1fs" % (i, meta["vector"], prob.V, truth.L, prob.error_percent, prob.detection_limit, len(prob.enzymes), meta["truth_internal"], sum(1 for x in prob.observed if len(x.enzymes) > 1), status, el)
    if status == "ok":
        m, held = bridge.read_map(d + "/map.json"); ok, dev = model.evaluate(prob, m)
        line += " -> internal=%d dev=%s truth_dev=%d same_as_truth=%s" % (model.internal_count(prob, m), dev, meta["truth_deviation"], m.cuts == truth.cuts and m.c == truth.c and m.L == truth.L)
    print(line, flush=True)
