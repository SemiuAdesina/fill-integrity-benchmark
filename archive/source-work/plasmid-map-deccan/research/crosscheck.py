#!/usr/bin/env python3
"""Agreement of brute force, oracle A and the Rust reference on tiny synthetic
instances, and of oracle A and the reference on real-data instances."""
import sys, os, random, time, json, tempfile, shutil
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model, oracle, brute, synthetic, generate, bridge as pio

BIN = os.path.expanduser("~/benchmark-suite-work/plasmid-map-deccan/lab/ref/target/release/plasmid-map")


def rust_solve(prob, header="synthetic"):
    d = tempfile.mkdtemp(prefix="pm-")
    try:
        pio.write_instance(d, prob, header)
        rc, out, err = pio.run_binary(BIN, d, os.path.join(d, "map.json"))
        if rc != 0: return None, err
        m, held = pio.read_map(os.path.join(d, "map.json"))
        return m, None
    finally:
        shutil.rmtree(d)


def key_of(prob, m):
    if m is None: return None
    err = model.valid_map(prob, m)
    if err: return ("INVALID", err)
    ok, dev = model.evaluate(prob, m)
    if not ok: return ("NOMATCH",)
    return model.key(prob, m, dev)


def tiny_run(n, seed):
    rng = random.Random(seed); agree = 0; total = 0; t_b = t_o = t_r = 0
    for i in range(n):
        r = synthetic.tiny(rng, n_enz=rng.choice([1, 2, 2, 3]), K=rng.choice([1, 1, 2]) if rng.random() < 0.7 else 1)
        if r is None: continue
        prob, truth = r
        if prob.max_internal == 2 and len(prob.enzymes) > 2: prob.max_internal = 1
        t0 = time.time(); mb = brute.brute(prob); t_b += time.time() - t0
        t0 = time.time(); mo, _ = oracle.recover(prob); t_o += time.time() - t0
        t0 = time.time(); mr, err = rust_solve(prob); t_r += time.time() - t0
        kb, ko, kr = key_of(prob, mb), key_of(prob, mo), key_of(prob, mr)
        total += 1
        if (kb == ko == kr and kb is not None) or (kb is None and ko is None and kr == ("NOMATCH",)):
            agree += 1
        else:
            print("DISAGREE seed=%d case=%d brute=%s oracle=%s rust=%s err=%s" % (seed, i, kb and kb[:2], ko and ko[:2], kr and (kr[:2] if isinstance(kr, tuple) and len(kr) > 2 else kr), err))
            d = "/tmp/pm_disagree_%d_%d" % (seed, i); pio.write_instance(d, prob, "synthetic"); print("  written to", d)
    print("tiny seed %d: %d/%d agree (brute %.1fs, oracle %.1fs, rust %.1fs)" % (seed, agree, total, t_b, t_o, t_r))


def real_run(n, seed, budget=240):
    rng = random.Random(seed); agree = 0; total = 0
    for i in range(n):
        r = None
        while r is None: r = generate.build(rng, n_enzymes=3, e=1, D=100, K=2, n_doubles=2, n_held=1, insert_range=(600, 2500), bounds=(400, 3000))
        prob, truth, held_truth, meta, vh = r
        t0 = time.time()
        try:
            mo, st = oracle.recover(prob, budget_s=budget)
        except oracle.Budget:
            print("  case %d: oracle over budget, skipped" % i); continue
        to = time.time() - t0
        t0 = time.time(); mr, err = rust_solve(prob, vh); tr = time.time() - t0
        ko, kr = key_of(prob, mo), key_of(prob, mr)
        total += 1
        same = ko == kr and ko is not None
        agree += same
        print("  case %d %s V=%d L=%d enz=%d: oracle %.1fs rust %.2fs %s truth_internal=%d best_internal=%s" % (i, meta["vector"], prob.V, truth.L, len(prob.enzymes), to, tr, "agree" if same else "DISAGREE %s %s %s" % (ko and ko[:2], kr and kr[:2], err), meta["truth_internal"], ko and ko[0]))
        if not same:
            d = "/tmp/pm_real_disagree_%d_%d" % (seed, i); pio.write_instance(d, prob, vh); print("  written to", d)
    print("real seed %d: %d/%d agree" % (seed, agree, total))


if __name__ == "__main__":
    mode = sys.argv[1]; n = int(sys.argv[2]); seed = int(sys.argv[3])
    if mode == "tiny": tiny_run(n, seed)
    else: real_run(n, seed)
