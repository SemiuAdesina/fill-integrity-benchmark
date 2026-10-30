#!/usr/bin/env python3
"""Random-fabric agreement: oracle C (pruned) must equal oracle A (exhaustive) on
status, plan and sink value. Mix of telemetry densities, with and without
completion, shared endpoints, incapable switches. Never ships."""
import sys, os, random, time, itertools
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "author"))
import model, bnb
from gen_fixtures import random_fabric, latent_telemetry

def main(n, seed0):
    rng = random.Random(seed0)
    agree = 0; total = 0; t_a = 0; t_c = 0; statuses = {}
    for k in range(n):
        ns = rng.randint(2, 5); nr = rng.randint(1, 4); npl = rng.randint(0, 2); extra = rng.randint(0, 5)
        p = random_fabric(rng, ns, nr, npl, extra, shared_endpoint=rng.random() < 0.25)
        mode = rng.random()
        if mode < 0.15:
            p.link_bytes = {}; p.completion = None           # pure optimisation
        else:
            r = latent_telemetry(rng, p, listed_fraction=rng.choice([0.0, 0.3, 0.6, 0.9]), with_completion=rng.random() < 0.5, zero_listed=rng.random() < 0.6)
            if r is None: continue
            if rng.random() < 0.15:  # perturb into likely infeasibility
                if p.link_bytes and rng.random() < 0.5:
                    lid = rng.choice(list(p.link_bytes)); p.link_bytes[lid] += p.payload_bytes
                else:
                    p.completion = (p.completion or 5) - 1
        t0 = time.time(); sa, pa, _ = model.recover(p); t_a += time.time() - t0
        t0 = time.time(); sc, pc, info = bnb.recover_c(p); t_c += time.time() - t0
        total += 1
        a = None if pa is None else (pa["paths"], pa["combine"], pa["sink_value"])
        c = None if pc is None else (pc["paths"], pc["combine"], pc["sink_value"])
        statuses[sa] = statuses.get(sa, 0) + 1
        if sa == sc and a == c:
            agree += 1
        else:
            print("DISAGREE seed", seed0, "case", k, sa, sc, a, c)
            import json; json.dump(p.to_json(), open("/tmp/disagree_%d_%d.json" % (seed0, k), "w"), indent=1)
    print("seed %d: %d/%d agree; statuses %s; A %.1fs C %.1fs" % (seed0, agree, total, statuses, t_a, t_c))

if __name__ == "__main__":
    main(int(sys.argv[1]), int(sys.argv[2]))
