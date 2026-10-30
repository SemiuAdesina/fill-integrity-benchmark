#!/usr/bin/env python3
"""Scale trial: telemetry-free optimum, near-optimal perturbed latent plan, telemetry,
then oracle C timing with and without completion. Author-side."""
import sys, os, random, time, json
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model, bnb, scale
from model import Illegal
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "oracle-b"))
import oracle_d

def tree_of(p, paths):
    links = {l.id: l for l in p.links}
    parent = {}
    for s, path in zip(sorted(p.sources, key=lambda s: s.id), paths):
        node = s.node
        for lid in path:
            parent[node] = lid; node = links[lid].to
    return parent

def perturbed_latent(rng, p, opt_paths, opt_combine, delta_max, tries=400, min_combines=0, combine_prob=0.6):
    links = {l.id: l for l in p.links}
    out = {}
    for l in p.links: out.setdefault(l.frm, []).append(l)
    opt_parent = tree_of(p, opt_paths)
    ends = sorted({s.node for s in p.sources})
    srcs = sorted(p.sources, key=lambda s: s.id)
    capable = {w.id for w in p.switches if w.reduction_capable and w.accumulator_capacity >= p.payload_bytes}
    best = None
    for _ in range(tries):
        parent = {}
        ok = True
        for e in ends:
            node = e; onpath = {e}
            while node != p.sink and node not in parent:
                choices = [l for l in out.get(node, []) if l.to not in onpath]
                if not choices: ok = False; break
                if node in opt_parent and rng.random() < 0.75:
                    l = links[opt_parent[node]]
                    if l.to in onpath: l = rng.choice(choices)
                else:
                    l = rng.choice(choices)
                parent[node] = l.id; node = l.to; onpath.add(node)
            if not ok: break
        if not ok: continue
        good = True
        for e in ends:
            node = e; seen = set()
            while node != p.sink:
                if node in seen: good = False; break
                seen.add(node); node = links[parent[node]].to
            if not good: break
        if not good: continue
        paths = []
        for s in srcs:
            node = s.node; path = []
            while node != p.sink: path.append(parent[node]); node = links[parent[node]].to
            paths.append(path)
        visited = [n for n in parent if n in capable]
        for _ in range(10):
            sub = sorted(n for n in visited if rng.random() < combine_prob)
            if len(sub) < min_combines: continue
            try: rep = model.replay(p, paths, sub)
            except Illegal: continue
            if (paths, sub) == (opt_paths, opt_combine): continue
            d = rep["completion"] - best_c[0]
            if 0 <= d <= delta_max:
                return paths, sub, rep
            if best is None or d < best[0]: best = (d, paths, sub, rep)
    return None

if __name__ == "__main__":
    seed = int(sys.argv[1]); n_src = int(sys.argv[2]); delta_max = int(sys.argv[3]); listed = float(sys.argv[4])
    n_leaf = int(sys.argv[5]) if len(sys.argv) > 5 else 5; n_spine = int(sys.argv[6]) if len(sys.argv) > 6 else 4; n_core = int(sys.argv[7]) if len(sys.argv) > 7 else 2
    outdir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "scale_trials"); os.makedirs(outdir, exist_ok=True)
    rng = random.Random(seed)
    p = scale.scale_fabric(rng, n_src, n_leaf, n_spine, n_core)
    p.link_bytes = {}; p.completion = None
    t0 = time.time()
    try:
        st, opt, info = bnb.recover_c(p, budget_s=300)
    except bnb.Budget:
        print("seed %d: optimum search exceeded 300 s" % seed); sys.exit(0)
    if st != "ok":
        print("seed %d: %s" % (seed, st)); sys.exit(0)
    best_c = [opt["completion"]]
    print("seed %d: sources=%d links=%d optimum completion=%d alloc=%d bytes=%d combines=%d in %.1fs nodes=%d leaves=%d" % (seed, n_src, len(p.links), opt["completion"], opt["allocation"], opt["total_bytes"], len(opt["combine"]), time.time() - t0, info["nodes"], info["leaves"]))
    lat = perturbed_latent(rng, p, opt["paths"], opt["combine"], delta_max)
    if lat is None:
        print("  no near-optimal latent within delta", delta_max); sys.exit(0)
    paths, sub, rep = lat
    print("  latent: completion=%d (+%d) alloc=%d bytes=%d combines=%d" % (rep["completion"], rep["completion"] - opt["completion"], rep["allocation"], rep["total_bytes"], len(sub)))
    for with_completion in (True, False):
        rng2 = random.Random(seed * 7 + int(with_completion))
        scale.telemetry_from(rng2, p, rep, listed_fraction=listed, zero_fraction=0.3, with_completion=with_completion)
        t0 = time.time()
        try:
            st, plan, info = bnb.recover_c(p, budget_s=300)
            same_opt = plan is not None and plan["paths"] == opt["paths"] and plan["combine"] == opt["combine"]
            same_lat = plan is not None and plan["paths"] == paths and plan["combine"] == sub
            print("  completion %s listed=%d: %s C=%s A=%s T=%s nodes=%d leaves=%d replays=%d matching=%d %.1fs %s" % (
                "listed" if with_completion else "absent", len(p.link_bytes), st, plan and plan["completion"], plan and plan["allocation"], plan and plan["total_bytes"],
                info["nodes"], info["leaves"], info["replays"], info["matching"], info["seconds"],
                "== telemetry-free optimum" if same_opt else ("== latent" if same_lat else "differs from both")))
        except bnb.Budget:
            print("  completion %s: BUDGET EXCEEDED (300 s)" % ("listed" if with_completion else "absent"))
        json.dump(p.to_json(), open(os.path.join(outdir, "s%d_n%d_%s.json" % (seed, n_src, "c" if with_completion else "nc")), "w"), indent=1)
        t0 = time.time()
        try:
            sd, pd, infod = oracle_d.recover_d(p.to_json(), budget_s=300)
            agree = sd == st and ((pd is None and plan is None) or (pd is not None and plan is not None and pd["paths"] == plan["paths"] and pd["combine"] == plan["combine"] and pd["sink_value"] == plan["sink_value"]))
            print("    oracle D: %s nodes=%d leaves=%d matching=%d %.1fs %s" % (sd, infod["nodes"], infod["leaves"], infod["matching"], infod["seconds"], "AGREES with C" if agree else "DISAGREES WITH C"))
        except oracle_d.BudgetD:
            print("    oracle D: BUDGET EXCEEDED (300 s)")
