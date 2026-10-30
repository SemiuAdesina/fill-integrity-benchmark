#!/usr/bin/env python3
"""Author-side generator for the scale fixtures: tiered fabrics far beyond
path-product or tree enumeration, telemetry from a near-optimal latent plan,
certified by four exact searches (Python oracle C, Python oracle D, the C++
reference, the C++ solver_d) that must agree within budget, with the two
exhaustive solvers required to exhaust the probe budget. Latent plans never
ship. Writes author/fixtures/<id>/ like gen_fixtures.py.

usage: gen_scale.py [fixture ids | all]
"""
import sys, os, json, random, time, subprocess, tempfile, shutil
HERE = os.path.dirname(os.path.abspath(__file__)); W = os.path.join(HERE, "..")
sys.path.insert(0, os.path.join(W, "research")); sys.path.insert(0, os.path.join(W, "oracle-b"))
import model, bnb, scale, scale_trial, oracle_d
from model import Illegal

REF = os.path.join(W, "lab/cand/ref2/reduction-recover")
SOLVER_D = os.path.join(W, "lab/cand/solver_d/reduction-recover")
NAIVE = {"reference_v1_path_product": os.path.join(W, "lab/cand/naive_ref/reduction-recover"), "solver_b_tree_enumeration": os.path.join(W, "lab/cand/solver_b/reduction-recover")}
COUNT = os.path.join(W, "adversarial/count/count_trees")
REF_MAX_S = 3.0          # the reference must solve every graded fabric within this on the author machine
NAIVE_CAP_S = 130.0      # both exhaustive solvers must still be running at the probe budget
PY_C_BUDGET = 900.0
PY_D_BUDGET = 2400.0
CPP_D_BUDGET = 900.0


def run_bin(path, doc, timeout):
    with tempfile.TemporaryDirectory() as d:
        pp = os.path.join(d, "p.json"); out = os.path.join(d, "r.json")
        json.dump(doc, open(pp, "w"))
        t0 = time.time()
        try:
            subprocess.run([path, "--problem", pp, "--out", out], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=timeout)
        except subprocess.TimeoutExpired:
            return "timeout", None, time.time() - t0
        secs = time.time() - t0
        if not os.path.exists(out): return "no-output", None, secs
        r = json.load(open(out))
    if r["status"] != "ok": return r["status"], None, secs
    return "ok", {"paths": r["paths"], "combine": r["combine"], "sink_value": r["sink_value"]}, secs


def plan_eq(a, b):
    if a is None or b is None: return a is b
    return a["paths"] == b["paths"] and a["combine"] == b["combine"] and a["sink_value"] == b["sink_value"]


def optimum(p):
    q = model.Problem(p.payload_bytes, p.sink, p.sources, p.switches, p.links, {}, None)
    st, plan, secs = run_bin(REF, q.to_json(), 120)
    if st != "ok": return None
    rep = model.replay(q, plan["paths"], plan["combine"])
    return plan, rep


def pin_reduced_links(rng, p, paths, sub, rep, extra_fraction, zero_fraction=0.1):
    """For every selected switch of the latent plan, list the link it leaves on
    (one payload) and the fewest links entering it whose payload counts reach
    two, so any plan reproducing the telemetry must reduce there; plus a
    fraction of the other links. Pins are kept minimal so the rest of the
    tree stays open."""
    lb = {}
    links = {l.id: l for l in p.links}
    parent = scale_trial.tree_of(p, paths)
    pinned = set()
    B = p.payload_bytes
    for c in sub:
        pinned.add(parent[c])
        entering = sorted([lid for n, lid in parent.items() if links[lid].to == c], key=lambda lid: -rep["link_bytes"].get(lid, 0))
        total = 0
        for lid in entering:
            pinned.add(lid); total += rep["link_bytes"].get(lid, 0) // B
            if total >= 2: break
    for l in p.links:
        b = rep["link_bytes"].get(l.id, 0)
        if l.id in pinned or (b > 0 and rng.random() < extra_fraction) or (b == 0 and rng.random() < zero_fraction):
            lb[l.id] = b
    p.link_bytes = lb


# ------------------------------------------------------------ fixture builders
# each: (params) -> (problem, expected status, requirements dict)
def build(fid, rng):
    spec = FIXTURES[fid]
    p = scale.scale_fabric(rng, spec["n"], spec["leaf"], spec["spine"], spec["core"])
    opt = optimum(p)
    if opt is None: return None
    opt_plan, opt_rep = opt
    scale_trial.best_c = [opt_rep["completion"]]
    lat = scale_trial.perturbed_latent(rng, p, opt_plan["paths"], opt_plan["combine"], spec["delta"], min_combines=spec.get("min_latent_combines", 0), combine_prob=spec.get("combine_prob", 0.6))
    if lat is None: return None
    paths, sub, rep = lat
    if spec.get("pin_reduced"):
        pin_reduced_links(rng, p, paths, sub, rep, spec["listed"], spec.get("zeros", 0.1))
        p.completion = rep["completion"] if spec["completion"] else None
    else:
        scale.telemetry_from(rng, p, rep, listed_fraction=spec["listed"], zero_fraction=spec.get("zeros", 0.3), with_completion=spec["completion"])
    if spec.get("infeasible") == "completion_below":
        # every listed link consistent with the latent plan, completion one tick below any plan that reproduces them
        p.completion = rep["completion"] - 1
    return p, spec["expect"], {"optimum": opt_plan, "optimum_rep": opt_rep, "latent": (paths, sub, rep)}


FIXTURES = {
    "s_listed_completion_10": dict(n=10, leaf=6, spine=5, core=3, delta=2, listed=0.15, zeros=0.15, completion=True, expect="ok", seed=6000),
    "s_open_completion_10":   dict(n=10, leaf=6, spine=5, core=3, delta=2, listed=0.15, zeros=0.15, completion=False, expect="ok", seed=6100, min_matching=100),
    "s_reduction_pinned_10":  dict(n=10, leaf=6, spine=5, core=3, delta=4, listed=0.05, zeros=0.05, completion=True, expect="ok", seed=6200, pin_reduced=True, min_latent_combines=2, min_canonical_combines=2, combine_prob=0.7, min_residual=5000000),
    "s_listed_completion_12": dict(n=12, leaf=6, spine=5, core=3, delta=2, listed=0.12, zeros=0.15, completion=True, expect="ok", seed=6300),
    "s_open_completion_12":   dict(n=12, leaf=6, spine=5, core=3, delta=2, listed=0.12, zeros=0.15, completion=False, expect="ok", seed=6400, min_matching=100),
    "s_infeasible_12":        dict(n=12, leaf=6, spine=5, core=3, delta=2, listed=0.15, zeros=0.15, completion=True, expect="infeasible", seed=6500, infeasible="completion_below"),
    "x_scale_sample":         dict(n=10, leaf=6, spine=5, core=3, delta=2, listed=0.15, zeros=0.15, completion=True, expect="ok", seed=6900, require_residual=False),
}


def certify(p, expect, aux, spec):
    doc = p.to_json()
    cert = {}
    st_ref, plan_ref, t_ref = run_bin(REF, doc, 120); cert["reference"] = dict(status=st_ref, seconds=round(t_ref, 3))
    if st_ref != expect: return cert, "reference status %s, expected %s" % (st_ref, expect), None
    if t_ref > REF_MAX_S: return cert, "reference too slow %.2fs" % t_ref, None
    st_d, plan_d, t_d = run_bin(SOLVER_D, doc, CPP_D_BUDGET); cert["solver_d"] = dict(status=st_d, seconds=round(t_d, 3))
    if st_d != st_ref or not plan_eq(plan_d, plan_ref): return cert, "solver_d disagrees (%s)" % st_d, None
    t0 = time.time()
    try:
        st_c, plan_c, info_c = bnb.recover_c(p, budget_s=PY_C_BUDGET)
    except bnb.Budget:
        return cert, "python oracle C exceeded budget", None
    cert["oracle_c"] = dict(status=st_c, seconds=round(time.time() - t0, 3), nodes=info_c["nodes"], leaves=info_c["leaves"], matching=info_c["matching"])
    pc = None if plan_c is None else {"paths": plan_c["paths"], "combine": plan_c["combine"], "sink_value": plan_c["sink_value"]}
    if st_c != st_ref or not plan_eq(pc, plan_ref): return cert, "oracle C disagrees", None
    t0 = time.time()
    try:
        st_dd, plan_dd, info_d = oracle_d.recover_d(doc, budget_s=PY_D_BUDGET)
    except oracle_d.BudgetD:
        return cert, "python oracle D exceeded budget", None
    cert["oracle_d"] = dict(status=st_dd, seconds=round(time.time() - t0, 3), nodes=info_d["nodes"], leaves=info_d["leaves"], matching=info_d["matching"])
    pdd = None if plan_dd is None else {"paths": plan_dd["paths"], "combine": plan_dd["combine"], "sink_value": plan_dd["sink_value"]}
    if st_dd != st_ref or not plan_eq(pdd, plan_ref): return cert, "oracle D disagrees", None
    if expect == "ok":
        if info_c["matching"] < spec.get("min_matching", 2): return cert, "fewer than %d telemetry-consistent plans (%d)" % (spec.get("min_matching", 2), info_c["matching"]), None
        if plan_eq(plan_ref, aux["optimum"]): return cert, "canonical plan equals the telemetry-free optimum (telemetry would not matter)", None
        if spec.get("min_canonical_combines", 0) > len(plan_ref["combine"]): return cert, "canonical plan has too few combines", None
    with tempfile.TemporaryDirectory() as d:
        pp = os.path.join(d, "p.json"); json.dump(doc, open(pp, "w"))
        out_tel = subprocess.run([COUNT, pp, "60", "telemetry"], capture_output=True, text=True).stdout.strip()
        cert["tree_enumeration_telemetry_aware"] = out_tel.split(" ", 1)[1] if " " in out_tel else out_tel
        # an enumerator that removes zero-listed links and forces listed links must still face more trees than it can count in 60 s
        if spec.get("require_residual", True):
            import re
            m = re.search(r"trees=(\d+)", out_tel); residual = int(m.group(1)) if m else 0
            min_residual = spec.get("min_residual")  # None: must be capped (uncountable in 60 s)
            if (min_residual is None and "(capped)" not in out_tel) or (min_residual is not None and residual < min_residual):
                return cert, "telemetry-aware tree enumeration too small: %s" % cert["tree_enumeration_telemetry_aware"], None
        out = subprocess.run([COUNT, pp, "60"], capture_output=True, text=True).stdout.strip()
        cert["tree_enumeration"] = out.split(" ", 1)[1] if " " in out else out
    # exhaustive solvers must exhaust the probe budget
    for name, path in NAIVE.items():
        st_n, _, t_n = run_bin(path, doc, NAIVE_CAP_S)
        cert[name] = dict(status=st_n, seconds=round(t_n, 1))
        if st_n != "timeout": return cert, "%s finished in %.0fs" % (name, t_n), None
    counts = scale.path_product(p)
    prod = 1
    for c in counts: prod *= c
    cert["simple_paths_per_source"] = counts; cert["path_product"] = "%.3e" % prod
    return cert, None, plan_ref


def run(fid, out_root, max_tries=40):
    spec = FIXTURES[fid]
    for attempt in range(max_tries):
        rng = random.Random(spec["seed"] + attempt)
        built = build(fid, rng)
        if built is None:
            print("REJECTED %-24s attempt=%d no usable latent plan" % (fid, attempt), flush=True); continue
        p, expect, aux = built
        t0 = time.time()
        cert, why, plan = certify(p, expect, aux, spec)
        if why:
            print("REJECTED %-24s attempt=%d %s (%.0fs) %s" % (fid, attempt, why, time.time() - t0, json.dumps(cert)), flush=True); continue
        d = os.path.join(out_root, fid)
        shutil.rmtree(d, ignore_errors=True); os.makedirs(d)
        json.dump(p.to_json(), open(os.path.join(d, "problem.json"), "w"), indent=1)
        obs = None
        if plan is not None:
            rep = model.replay(p, plan["paths"], plan["combine"])
            obs = {k: rep[k] for k in ("completion", "allocation", "total_bytes", "link_bytes")}
        json.dump({"fixture": fid, "status": expect, "plan": plan, "observables": obs}, open(os.path.join(d, "expected.json"), "w"), indent=1)
        open(os.path.join(d, "result.expected.json"), "w").write(model.result_json(expect, plan))
        cert.update({"fixture": fid, "seed": spec["seed"] + attempt, "sources": len(p.sources), "links": len(p.links), "switches": len(p.switches), "listed_links": len(p.link_bytes), "completion_listed": p.completion is not None,
                     "latent_completion": aux["latent"][2]["completion"], "latent_combines": len(aux["latent"][1]), "optimum_completion": aux["optimum_rep"]["completion"], "agreement": "reference = solver_d = oracle C = oracle D"})
        json.dump(cert, open(os.path.join(d, "certification.json"), "w"), indent=1)
        print("RETAINED %-24s seed=%d status=%s sources=%d links=%d listed=%d ref=%.2fs solver_d=%.1fs C=%.1fs D=%.1fs matching=%s combines=%s (%.0fs)" % (
            fid, spec["seed"] + attempt, expect, len(p.sources), len(p.links), len(p.link_bytes), cert["reference"]["seconds"], cert["solver_d"]["seconds"], cert["oracle_c"]["seconds"], cert["oracle_d"]["seconds"], cert["oracle_c"]["matching"], None if plan is None else len(plan["combine"]), time.time() - t0), flush=True)
        return
    raise SystemExit("no retained fixture for " + fid)


if __name__ == "__main__":
    out_root = os.path.join(HERE, "fixtures")
    which = sys.argv[1:] or list(FIXTURES)
    if which == ["all"]: which = list(FIXTURES)
    for fid in which: run(fid, out_root)
