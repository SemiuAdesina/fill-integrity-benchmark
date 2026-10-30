#!/usr/bin/env python3
"""Generate, certify and characterise semantic-corner candidates for one axis.

usage: corner_sweep.py <axis> <out_dir> <first_seed> <count>
Writes <out_dir>/<name>.cross.json / .expected.json for every candidate that
certifies, and <out_dir>/results.jsonl with one record per candidate."""
import json, os, sys, subprocess, time, random
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import corner_gen as cg
import independent_certify as ic

R = os.path.expanduser("~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty")
BENCH = f"{R}/lab/shipcrate/target/release/refbench"

def axis_config(axis, seed):
    rng = random.Random(seed * 7919 + 13)
    allow_mix = lambda p: [rng.random() < p for _ in range(cg.M)]
    if axis == "dr0":
        # Marker 0: parent A simplex with DR allowed, parent B nulliplex with DR
        # forbidden, so a progeny with two alternate copies there can only have
        # them by double reduction in A. Marker 1: parent A nulliplex, parent B
        # simplex with DR forbidden in both, so two copies there would need the
        # forbidden form in B and the legal optimum must settle for one.
        da = allow_mix(0.5); da[0] = True; da[1] = False
        db = allow_mix(0.5); db[0] = False; db[1] = False
        rec = [rng.choice([0.1, 0.2, 0.3]) for _ in range(cg.M - 1)]
        rec[0] = rng.choice([0.35, 0.4, 0.45])   # a loosely linked first interval, so leaving a
                                                  # marker-0 state is not itself prohibitive
        return dict(free_a={0: 1, 4: 2, 8: 2}, free_b={1: 1, 6: 2}, pin_a={1: 0}, pin_b={0: 0},
                    dr_allowed_a=da, dr_allowed_b=db, dr_rate=rng.choice([0.3, 0.35, 0.4]),
                    rec=rec, coarse=rng.uniform(0.9, 1.3), shift_prob=(0.3 if seed >= 400 else 0.18),
                    depth_boost={0: rng.uniform(1.8, 2.6), 1: (rng.uniform(3.5, 4.5) if seed >= 400 else rng.uniform(1.4, 2.0))})
    if axis == "lowinfo":
        # Uninformative cells sit on the segregating markers, where the state
        # actually matters, so the path there has to come from linkage to the
        # neighbouring markers and from the shared parental phase.
        return dict(free_a={2: 2, 5: 1, 9: 2}, free_b={3: 2, 7: 1, 10: 2},
                    dr_allowed_a=allow_mix(0.5), dr_allowed_b=allow_mix(0.5), dr_rate=0.25,
                    rec=[rng.choice([0.15, 0.25, 0.35]) for _ in range(cg.M - 1)],
                    flat_cells=rng.randint(5, 9), nearflat_cells=rng.randint(6, 12), coarse=rng.uniform(1.0, 1.5),
                    flat_on_free=True, shift_prob=0.1)
    if axis == "mixed":
        return dict(free_a={3: 2, 8: 2, 10: 1}, free_b={2: 2, 6: 2},
                    dr_allowed_a=allow_mix(0.5), dr_allowed_b=allow_mix(0.5), dr_rate=0.25,
                    rec=[rng.choice([0.1, 0.2, 0.3]) for _ in range(cg.M - 1)], ibd_a=True, coarse=rng.uniform(0.7, 1.2))
    if axis == "unlinked":
        k = rng.choice([1, 2])
        unl = tuple(sorted(rng.sample(range(2, cg.M - 2), k)))
        return dict(free_a={1: 2, 4: 2, 9: 2}, free_b={3: 2, 7: 1},
                    dr_allowed_a=allow_mix(0.5), dr_allowed_b=allow_mix(0.5), dr_rate=0.25,
                    rec=[rng.choice([0.1, 0.2, 0.3]) for _ in range(cg.M - 1)], unlinked=unl,
                    ibd_b=rng.random() < 0.7, coarse=rng.uniform(0.7, 1.1))
    raise SystemExit("unknown axis")

def run_bench(cross_path, out_path):
    r = subprocess.run([BENCH, cross_path, out_path], capture_output=True, text=True, timeout=120)
    if r.returncode != 0: return None
    return json.loads(r.stdout.strip().splitlines()[-1])

def main():
    axis, out, first, count = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
    os.makedirs(out, exist_ok=True)
    log = open(f"{out}/results.jsonl", "a")
    for seed in range(first, first + count):
        name = f"{axis}_s{seed}"
        cfg = axis_config(axis, seed)
        cross = cg.make_cross(seed, axis, **cfg)
        cpath = f"{out}/{name}.cross.json"; epath = f"{out}/{name}.expected.json"
        json.dump(cross, open(cpath, "w"), separators=(",", ":"))
        t0 = time.time()
        bench = run_bench(cpath, epath)
        if bench is None:
            print(f"{name}: reference failed"); continue
        bench2 = run_bench(cpath, epath + ".run2")
        deterministic = open(epath, "rb").read() == open(epath + ".run2", "rb").read()
        os.remove(epath + ".run2")
        exp = json.load(open(epath))
        # exhaustive enumeration of the parental assignment space
        best, opts, space = cg.enumerate_optima(cross)
        enum_matches = abs(best - exp["total_log_likelihood"]) <= 1e-8
        # independent required answer (paths included) when the tie set is small
        path_level = None
        if len(opts) <= 300:
            ra, rb, cp = cg.required_answer(cross, opts)
            path_level = (ra == exp["haplotypes"]["A"] and rb == exp["haplotypes"]["B"] and cp == exp["paths"])
        else:
            mats = sorted({(tuple(map(tuple, sorted(cg.rows_of(ca)))), tuple(map(tuple, sorted(cg.rows_of(cb))))) for ca, cb in opts})
            path_level = "matrix-only:" + str(mats[0] == (tuple(map(tuple, exp["haplotypes"]["A"])), tuple(map(tuple, exp["haplotypes"]["B"]))))
        relaxed = None
        if axis == "dr0":
            relaxed, _, _ = cg.enumerate_optima(cross, legality="relaxed")
        # independent spec-derived checks
        try:
            s = ic.score(cross, exp); score_ok = abs(s - exp["total_log_likelihood"]) <= 1e-8
        except AssertionError as e:
            score_ok = False
        canon_ok = ic.is_canonical(exp); dig_ok = ic.digest(exp) == exp["canonical_digest"]
        p = cg.props(cross, exp, opts, relaxed)
        rec = dict(name=name, axis=axis, seed=seed, M=cg.M, N=cg.N, space=space, ref_secs=bench["secs"], nodes=bench["expanded"],
                   optimum=exp["total_log_likelihood"], digest=exp["canonical_digest"], deterministic=deterministic,
                   enum_matches=enum_matches, path_level=path_level, score_ok=score_ok, canon_ok=canon_ok, dig_ok=dig_ok,
                   ref_distinct=bench["distinct_optima"], elapsed=round(time.time() - t0, 1), **p)
        certified = deterministic and enum_matches and score_ok and canon_ok and dig_ok and (path_level is True or str(path_level).endswith("True"))
        rec["certified"] = certified
        log.write(json.dumps(rec) + "\n"); log.flush()
        if not certified:
            os.remove(cpath); os.remove(epath)
        print(f"{name:<16} cert={certified} space={space:<6} ties={len(opts):<5} ref={bench['secs']:.2f}s "
              f"drA0={p['dr_a_marker0']} drB0={p['dr_b_marker0']} tempt={p['forbidden_dr_tempting']} rr={p['repeated_rows']} "
              f"rule4={p['rule4_needed']} unl={p['unlinked_intervals']} flat={p['flat_cells']} near={p['nearflat_cells']} "
              f"rule={p['rule_deciding']} sw={p['switches']} paths={path_level}", flush=True)

if __name__ == "__main__":
    main()
