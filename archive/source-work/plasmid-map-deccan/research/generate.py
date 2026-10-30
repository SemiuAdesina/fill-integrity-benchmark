#!/usr/bin/env python3
"""Real-data construct generator (author-side, never ships): a real vector
record, a real insert fragment excised from a real source record between two
restriction sites, ligated at one of the vector's unique polylinker sites; the
true recombinant is scanned for every enzyme of the panel; digests are
simulated with comigration and sizing error; the truth is kept aside."""
import sys, os, random, math, json
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model
from model import Enzyme, Band, Digest, Problem, Map

DATA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data")
VECTORS = ["pUC19", "pBR322", "pACYC184", "pBluescriptIISK", "pUC18", "pGEX-2T", "pEGFP-N1", "pACYC177"]
SOURCES = ["lambda", "phiX174", "M13mp18", "ecoli_lac_region", "T7"]


def load(name):
    return model.read_fasta(os.path.join(DATA, name + ".fasta"))


def unique_sites(seq, names):
    out = []
    for n in names:
        s, o = model.ENZYMES[n]
        sites = model.scan_sites(seq, s, o)
        if len(sites) == 1: out.append((n, sites[0][1]))
    return out


def real_fragment(rng, src_seq, lo, hi, tries=400):
    """A fragment of the source between two restriction sites, length in [lo, hi]."""
    names = sorted(model.ENZYMES)
    for _ in range(tries):
        a, b = rng.choice(names), rng.choice(names)
        sa = [cut for _, cut in model.scan_sites(src_seq, *model.ENZYMES[a])]
        sb = [cut for _, cut in model.scan_sites(src_seq, *model.ENZYMES[b])]
        if not sa or not sb: continue
        x = rng.choice(sa); y = rng.choice(sb)
        if x > y: x, y = y, x
        if lo <= y - x <= hi and y <= len(src_seq):
            return (a, x, b, y), src_seq[x:y]
    return None


def simulate_digest(rng, cuts, enzymes, N, D, e, comigrate=0.012):
    frags = sorted(f for f in model.fragments(set().union(*[set(cuts[x]) for x in enzymes]), N) if f >= D)
    noisy = []
    lo_u = -0.85 * (e / 100) / (1 + e / 100); hi_u = 0.85 * e / 100
    for f in frags:
        u = rng.uniform(lo_u, hi_u); noisy.append((f, int(round(f * (1 + u)))))
    # comigration: consecutive fragments within `comigrate` merge into one band at the mean observed length
    bands = []
    i = 0
    while i < len(noisy):
        j = i
        while j + 1 < len(noisy) and noisy[j + 1][0] <= noisy[i][0] * (1 + comigrate): j += 1
        group = noisy[i:j + 1]
        if len(group) > 1:
            mean = int(round(sum(o for _, o in group) / len(group)))
            if all(abs(f - mean) <= model.tolerance(mean, e) for f, _ in group):
                bands.append(Band(mean, len(group))); i = j + 1; continue
        bands.append(Band(noisy[i][1], 1)); i += 1
    # merge identical lengths
    out = []
    for b in sorted(bands, key=lambda b: b.length):
        if out and out[-1].length == b.length: out[-1].multiplicity += b.multiplicity
        else: out.append(Band(b.length, b.multiplicity))
    return out


def build(rng, vector=None, source=None, n_enzymes=4, insert_range=(800, 5500), e=None, D=None, K=3, n_doubles=4, n_held=2, bounds=(500, 6000), n_cloning=3, comigrate=0.012, include_cloning_enzyme=False, clone_into_multisite=False):
    vname = vector or rng.choice(VECTORS); sname = source or rng.choice(SOURCES)
    vh, vseq = load(vname); sh, sseq = load(sname)
    V = len(vseq)
    e = e if e is not None else rng.choice([1, 2, 2, 3]); D = D if D is not None else rng.choice([100, 150, 200, 250])
    # cloning sites: unique cutters in the vector
    uniques = unique_sites(vseq, sorted(model.ENZYMES))
    by_pos = {}
    for n_, c_ in uniques: by_pos.setdefault(c_, (n_, c_))
    uniques = sorted(by_pos.values())
    if len(uniques) < n_cloning: return None
    cl = rng.sample(uniques, n_cloning)
    cname, c = rng.choice(cl)
    multisite_enzyme = None
    if clone_into_multisite:
        # the insert goes into one site of an enzyme that cuts the vector two to four times,
        # as in cloning by partial digestion; that enzyme stays anchored by its other sites
        # and the window at the opening is torn
        candidates = []
        for n_ in sorted(model.ENZYMES):
            s_, o_ = model.ENZYMES[n_]
            sites_ = model.scan_sites(vseq, s_, o_)
            if 2 <= len(sites_) <= 4: candidates.append((n_, sites_))
        if not candidates: return None
        multisite_enzyme, sites_ = rng.choice(candidates)
        cname, c = multisite_enzyme, rng.choice(sites_)[1]
        cl = [x for x in cl if x[1] != c][: n_cloning - 1] + [(cname, c)]
    frag = real_fragment(rng, sseq, *insert_range)
    if frag is None: return None
    (fa, fx, fb, fy), ins = frag
    L = len(ins)
    if not (bounds[0] <= L <= bounds[1]): return None
    recomb = vseq[:c] + ins + vseq[c:]
    N = V + L
    # panel: enzymes with 0..4 vector sites and <= K internal sites in the truth, at least one internal site overall
    pool = []
    for n in sorted(model.ENZYMES):
        s, o = model.ENZYMES[n]
        vs = model.scan_sites(vseq, s, o)
        if len(vs) > 4: continue
        rs = [cut for _, cut in model.scan_sites(recomb, s, o)]
        internal = [q for q in rs if c <= q <= c + L]
        if len(internal) > K: continue
        if not vs: continue
        # anchored at every cloning site: at least one non-spanning backbone site
        if any(all(model.spans_junction(st, len(s), cc, V) for st, _ in vs) for _, cc in cl): continue
        pool.append((n, len(vs), len(internal), rs))
    rng.shuffle(pool)
    with_internal = [x for x in pool if x[2] >= 1]
    if len(with_internal) < 2: return None
    panel = rng.sample(with_internal, min(len(with_internal), rng.randint(2, n_enzymes)))
    if include_cloning_enzyme or multisite_enzyme is not None:
        ce = [x for x in pool if x[0] == cname]
        if not ce: return None
        if cname not in [x[0] for x in panel]: panel = ce + panel[: n_enzymes - 1]
    rest = [x for x in pool if x not in panel]
    while len(panel) < n_enzymes and rest: panel.append(rest.pop())
    panel.sort(key=lambda x: x[0])
    enzymes = [Enzyme(n, model.ENZYMES[n][0], model.ENZYMES[n][1]) for n, _, _, _ in panel]
    truth = Map(c, L, {n: sorted(rs) for n, _, _, rs in panel})
    prob = Problem(vseq, enzymes, sorted(cl, key=lambda x: x[1]), e, D, bounds[0], bounds[1], K, [], [])
    if model.valid_map(prob, truth) is not None: return None
    if not model.anchored(prob): return None
    names = [x.name for x in enzymes]
    pairs = [sorted(pr) for pr in __import__("itertools").combinations(names, 2)]
    rng.shuffle(pairs)
    observed_pairs = pairs[:n_doubles]; held = pairs[n_doubles:n_doubles + n_held]
    if len(held) < n_held: return None
    obs = []
    for n in names:
        obs.append(Digest(n, [n], simulate_digest(rng, truth.cuts, [n], N, D, e, comigrate=comigrate)))
    for pr in observed_pairs:
        obs.append(Digest(model.digest_id(pr), pr, simulate_digest(rng, truth.cuts, pr, N, D, e, comigrate=comigrate)))
    prob.observed = obs
    prob.held_out = [(model.digest_id(pr), pr) for pr in held]
    held_truth = {model.digest_id(pr): simulate_digest(rng, truth.cuts, pr, N, D, e, comigrate=comigrate) for pr in held}
    ok, dev = model.evaluate(prob, truth)
    if not ok: return None
    meta = {"vector": vname, "vector_header": vh, "source": sname, "source_header": sh, "fragment": {"left_enzyme": fa, "left_cut": fx, "right_enzyme": fb, "right_cut": fy},
            "cloning_site": [cname, c], "insert_length": L, "truth_internal": model.internal_count(prob, truth), "truth_deviation": dev}
    meta["recombinant"] = recomb; meta["insert"] = ins
    return prob, truth, held_truth, meta, vh


if __name__ == "__main__":
    import oracle
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    rng = random.Random(seed)
    r = None
    while r is None:
        r = build(rng)
    prob, truth, held_truth, meta, vh = r
    print("vector=%s V=%d source=%s L=%d c=%s e=%d D=%d enzymes=%s truth_internal=%d truth_dev=%d" % (
        meta["vector"], prob.V, meta["source"], truth.L, meta["cloning_site"], prob.error_percent, prob.detection_limit,
        [(x.name, len(truth.cuts[x.name])) for x in prob.enzymes], meta["truth_internal"], meta["truth_deviation"]))
    for d in prob.observed: print("  %-16s %s" % (d.id, [(b.length, b.multiplicity) for b in d.bands]))
    stats = {}
    try:
        best, stats = oracle.recover(prob, budget_s=600, stats=stats)
    except oracle.Budget:
        print("  ORACLE BUDGET EXCEEDED", stats); sys.exit(0)
    print("  oracle:", stats)
    if best is None: print("  NO MAP"); sys.exit(0)
    ok, dev = model.evaluate(prob, best)
    print("  best: c=%d L=%d internal=%d dev=%d  == truth? %s" % (best.c, best.L, model.internal_count(prob, best), dev, best.cuts == truth.cuts and best.c == truth.c and best.L == truth.L))
    ho = model.held_out_predictions(prob, best)
    for hid, bands in ho.items():
        ok, hdev = model.match([b.length for b in bands for _ in range(b.multiplicity)], held_truth[hid], prob.error_percent)
        print("  held-out %-16s match=%s dev=%s" % (hid, ok, hdev))
