#!/usr/bin/env python3
"""Fixture generation and certification for plasmid-map (author-side).

Each fixture is a real vector record, a real insert fragment and simulated
gels. A fixture is retained only when: the Rust reference answers within its
time cap; the independent Python branch and bound agrees within budget; the
enumeration solver, when it finishes, agrees; the certified map's held-out
predictions match the true construct's held-out bands under the matching
rule; and the family's property holds. The truth never ships.

Writes author/fixtures/<id>/{backbone.fasta,enzymes.json,digests.json,expected.json,certification.json}.
"""
import sys, os, json, random, time, subprocess, shutil, tempfile
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "research"))
import model, generate, bridge, oracle_bnb
from model import Map, Band

REF = os.path.expanduser("~/benchmark-suite-work/plasmid-map-deccan/lab/ref/target/release/plasmid-map")
ENUM = os.path.expanduser("~/benchmark-suite-work/plasmid-map-deccan/lab/enum/target/release/plasmid-map")
REF_CAP = 30.0       # seconds the reference may take on a graded fixture
ENUM_CAP = 120.0     # the enumeration solver's cap (a timeout is recorded, not fatal)
PY_BUDGET = 2400.0   # the independent certifier's budget


def run_solver(binary, d, cap):
    out = os.path.join(d, "map.out.json")
    if os.path.exists(out): os.remove(out)
    t0 = time.time()
    try:
        r = subprocess.run([binary, d, out], capture_output=True, text=True, timeout=cap)
    except subprocess.TimeoutExpired:
        return None, "timeout", cap
    el = time.time() - t0
    if r.returncode != 0 or not os.path.exists(out): return None, "rc=%d %s" % (r.returncode, r.stderr[-200:]), el
    m, held = bridge.read_map(out)
    return m, "ok", el


def key_of(prob, m):
    err = model.valid_map(prob, m)
    if err: return None
    ok, dev = model.evaluate(prob, m)
    return None if not ok else model.key(prob, m, dev)


def held_out_matches(prob, m, held_truth):
    pred = model.held_out_predictions(prob, m)
    for hid, bands in pred.items():
        ok, _ = model.match([b.length for b in bands for _ in range(b.multiplicity)], held_truth[hid], prob.error_percent)
        if not ok: return False
    return True


def certify(prob, truth, held_truth, vh, py_budget=PY_BUDGET, property_check=None):
    d = tempfile.mkdtemp(prefix="pm-cert-")
    try:
        bridge.write_instance(d, prob, vh)
        cert = {}
        m_ref, st, el = run_solver(REF, d, REF_CAP)
        cert["reference"] = {"status": st, "seconds": round(el, 2)}
        if m_ref is None: return None, cert, "reference %s" % st
        k_ref = key_of(prob, m_ref)
        if k_ref is None: return None, cert, "reference map invalid or not matching"
        cert["reference"].update({"internal": k_ref[0], "deviation": k_ref[1], "c": m_ref.c, "L": m_ref.L})
        # cheap rejections before the expensive certifiers
        if not held_out_matches(prob, m_ref, held_truth): return None, cert, "held-out bands of the optimum do not match the true construct"
        if property_check is not None and not property_check(m_ref): return None, cert, "family property does not hold on the optimum"
        m_enum, st, el = run_solver(ENUM, d, ENUM_CAP)
        cert["enumeration_solver"] = {"status": st, "seconds": round(el, 2)}
        if m_enum is not None and key_of(prob, m_enum) != k_ref: return None, cert, "enumeration solver disagrees"
        t0 = time.time()
        try:
            m_py, stats = oracle_bnb.recover_bnb(prob, budget_s=py_budget)
        except oracle_bnb.Budget:
            cert["python_bnb"] = {"status": "budget"}; return None, cert, "python certifier over budget"
        cert["python_bnb"] = {"status": "ok", "seconds": round(time.time() - t0, 1), "nodes": stats["nodes"], "ties_at_count_and_deviation": stats.get("ties_at_count_and_deviation")}
        if m_py is None or key_of(prob, m_py) != k_ref: return None, cert, "python certifier disagrees"
        if not held_out_matches(prob, m_ref, held_truth): return None, cert, "held-out bands of the optimum do not match the true construct"
        cert["held_out"] = "matches the true construct within tolerance"
        cert["truth"] = {"internal": model.internal_count(prob, truth), "deviation": model.evaluate(prob, truth)[1], "optimum_equals_truth": m_ref.cuts == truth.cuts and m_ref.c == truth.c and m_ref.L == truth.L}
        return m_ref, cert, None
    finally:
        shutil.rmtree(d)


# ------------------------------------------------------------- properties
def has_sub_detection_single(prob, truth):
    N = prob.V + truth.L
    for e in prob.enzymes:
        if any(f < prob.detection_limit for f in model.fragments(truth.cuts[e.name], N)): return True
    return False


def has_multiplicity(prob):
    return any(b.multiplicity > 1 for d in prob.observed for b in d.bands)


def junction_spanning_site(prob, truth):
    for e in prob.enzymes:
        for st, cut in model.scan_sites(prob.backbone, e.site, e.offset):
            if model.spans_junction(st, len(e.site), truth.c, prob.V): return True
    return False


def junction_cut(prob, truth):
    return any(q in (truth.c, truth.c + truth.L) for e in prob.enzymes for q in truth.cuts[e.name])


def unresolved_enzyme(prob, truth):
    """An enzyme with two or more insert-region cuts that no observed multi-enzyme digest contains."""
    for e in prob.enzymes:
        internal = [q for q in truth.cuts[e.name] if truth.c <= q <= truth.c + truth.L]
        if len(internal) >= 2 and not any(e.name in d.enzymes for d in prob.observed if len(d.enzymes) > 1) and not any(e.name in enz for _, enz in prob.held_out):
            return True
    return False


FAMILIES = {
    # id: (seed, generator kwargs, property predicate(prob, truth, meta, cert_map) or None, description)
    "public": (100, dict(n_enzymes=4, e=2, D=200, K=3, n_doubles=4, n_held=2, insert_range=(1500, 4000)), None, "the public problem"),
    "h_parsimony": (200, dict(n_enzymes=4, e=3, D=250, K=3, n_doubles=4, n_held=1, insert_range=(2500, 5800)), "parsimony", "a closer fit exists with more insert-region sites, so fewest sites must come first"),
    "h_sub_detection": (300, dict(n_enzymes=4, e=2, D=250, K=3, n_doubles=4, n_held=2, insert_range=(1000, 5000)), "subd", "a single digest misses a fragment below the detection limit"),
    "h_comigration": (400, dict(n_enzymes=4, e=2, D=150, K=3, n_doubles=4, n_held=2, insert_range=(1000, 5000), comigrate=0.03), "multi", "an observed band carries two fragments"),
    "h_junction": (500, dict(n_enzymes=4, e=2, D=200, K=3, n_doubles=4, n_held=2, insert_range=(1000, 5000), clone_into_multisite=True), "junction", "the insert sits in one site of an enzyme that cuts the vector several times, so that window is torn, and a site is regenerated at a junction"),
    "h_ties": (600, dict(n_enzymes=4, e=2, D=200, K=2, n_doubles=2, n_held=1, insert_range=(1000, 4000)), "ties", "an enzyme's two insert-region cuts are ordered by no digest"),
    "h_cloning_sites": (700, dict(n_enzymes=4, e=2, D=200, K=3, n_doubles=4, n_held=2, insert_range=(1000, 5000), n_cloning=4), "sites", "four cloning sites and the optimum is not at the first"),
    "h_three_sites": (800, dict(n_enzymes=5, e=2, D=200, K=3, n_doubles=5, n_held=2, insert_range=(2000, 5800)), "three", "an enzyme with three insert-region sites"),
    "s_large_a": (900, dict(n_enzymes=5, e=2, D=200, K=3, n_doubles=5, n_held=2, insert_range=(3000, 5800)), "large", "seven or more insert-region sites"),
    "s_large_b": (1000, dict(n_enzymes=5, e=3, D=250, K=3, n_doubles=6, n_held=2, insert_range=(3000, 5800)), "large", "seven or more insert-region sites at three percent"),
    "s_large_c": (1100, dict(n_enzymes=5, e=2, D=150, K=3, n_doubles=4, n_held=2, insert_range=(3000, 5800)), "large", "seven or more insert-region sites, sparse doubles"),
    "h_low_error": (1200, dict(n_enzymes=4, e=1, D=100, K=3, n_doubles=4, n_held=2, insert_range=(1000, 5000)), None, "one percent sizing error"),
}


def property_holds(name, prob, truth, meta, m):
    if name is None: return True
    if name == "parsimony":
        if m is None: return True
        d = tempfile.mkdtemp(prefix="pm-b11-")
        try:
            bridge.write_instance(d, prob, "check")
            alt, st, _ = run_solver(os.path.expanduser("~/benchmark-suite-work/plasmid-map-deccan/lab/var_b11_deviation_before_count/target/release/plasmid-map"), d, 60)
        finally:
            shutil.rmtree(d)
        if alt is None: return False
        return not (alt.c == m.c and alt.L == m.L and alt.cuts == m.cuts)
    if name == "subd": return has_sub_detection_single(prob, truth)
    if name == "multi": return has_multiplicity(prob)
    if name == "junction": return junction_spanning_site(prob, truth) and junction_cut(prob, truth)
    if name == "ties": return unresolved_enzyme(prob, truth)
    if name == "sites": return len(prob.cloning_sites) >= 4 and m.c != prob.cloning_sites[0][1]
    if name == "three": return any(sum(1 for q in truth.cuts[e.name] if truth.c <= q <= truth.c + truth.L) == 3 for e in prob.enzymes)
    if name == "large": return model.internal_count(prob, truth) >= 7
    return False


def pre_property(name, prob, truth, meta):
    """Properties decidable before certification (saves the certifier's time)."""
    if name in ("subd", "multi", "junction", "ties", "three", "large"): return property_holds(name, prob, truth, meta, None)
    if name == "parsimony": return True
    if name == "sites": return len(prob.cloning_sites) >= 4
    return True


def run(fid, out_root, max_tries=300):
    seed, kwargs, prop, desc = FAMILIES[fid]
    for attempt in range(max_tries):
        rng = random.Random(seed + attempt)
        r = generate.build(rng, **kwargs)
        if r is None: continue
        prob, truth, held_truth, meta, vh = r
        if not pre_property(prop, prob, truth, meta):
            continue
        t0 = time.time()
        m, cert, why = certify(prob, truth, held_truth, vh, property_check=lambda mm: property_holds(prop, prob, truth, meta, mm))
        if m is None:
            print("REJECTED %-18s attempt=%d %s (%ds) %s" % (fid, attempt, why, time.time() - t0, json.dumps(cert)[:160]), flush=True); continue
        if not property_holds(prop, prob, truth, meta, m):
            print("REJECTED %-18s attempt=%d property '%s' does not hold on the optimum (%ds)" % (fid, attempt, prop, time.time() - t0), flush=True); continue
        d = os.path.join(out_root, fid)
        shutil.rmtree(d, ignore_errors=True)
        bridge.write_instance(d, prob, vh)
        exp = {"fixture": fid, "map": model.map_to_doc(prob, m), "internal": model.internal_count(prob, m), "deviation": model.evaluate(prob, m)[1],
               "held_out_truth": {hid: [{"length": b.length, "multiplicity": b.multiplicity} for b in bands] for hid, bands in held_truth.items()},
               "ties_at_count_and_deviation": cert["python_bnb"].get("ties_at_count_and_deviation")}
        json.dump(exp, open(os.path.join(d, "expected.json"), "w"), indent=1)
        cert.update({"fixture": fid, "seed": seed + attempt, "description": desc, "vector": meta["vector"], "vector_header": meta["vector_header"], "source": meta["source"], "source_header": meta["source_header"],
                     "fragment": meta["fragment"], "cloning_site": meta["cloning_site"], "insert_length": meta["insert_length"], "enzymes": [e.name for e in prob.enzymes],
                     "error_percent": prob.error_percent, "detection_limit": prob.detection_limit, "observed": len(prob.observed), "held_out": [h for h, _ in prob.held_out]})
        json.dump(cert, open(os.path.join(d, "certification.json"), "w"), indent=1)
        # the true construct, kept beside the fixture for the author only (never synced)
        json.dump({"c": truth.c, "L": truth.L, "cuts": truth.cuts, "insert": meta["insert"]}, open(os.path.join(d, "truth.author.json"), "w"))
        print("RETAINED %-18s seed=%d %s V=%d L=%d e=%d D=%d enzymes=%s internal=%d (truth %d) dev=%d ref=%.2fs enum=%s py=%ss ties=%s (%ds)" % (
            fid, seed + attempt, meta["vector"], prob.V, m.L, prob.error_percent, prob.detection_limit, len(prob.enzymes), exp["internal"], cert["truth"]["internal"], exp["deviation"],
            cert["reference"]["seconds"], cert["enumeration_solver"]["status"], cert["python_bnb"]["seconds"], exp["ties_at_count_and_deviation"], time.time() - t0), flush=True)
        return
    raise SystemExit("no retained fixture for " + fid)


if __name__ == "__main__":
    out_root = os.path.join(HERE, "fixtures")
    which = sys.argv[1:] or list(FAMILIES)
    for fid in which: run(fid, out_root)
