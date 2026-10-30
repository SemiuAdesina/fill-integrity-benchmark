#!/usr/bin/env python3
"""Tiny synthetic constructs for brute-force cross-checks (author-side)."""
import random, sys, os, itertools
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model
from model import Enzyme, Band, Digest, Problem, Map
import generate


def plant(rng, length, sites):
    seq = [rng.choice("ACGT") for _ in range(length)]
    for pos, site in sites:
        for k, b in enumerate(site): seq[(pos + k) % length] = b
    return "".join(seq)


def tiny(rng, V=None, n_enz=2, K=1, lrange=(8, 30), e=None, D=None):
    V = V or rng.randint(120, 220)
    names = rng.sample(["EcoRI", "BamHI", "HindIII", "PstI", "SmaI", "XbaI"], n_enz)
    enzymes = sorted([Enzyme(n, *model.ENZYMES[n]) for n in names], key=lambda x: x.name)
    e = e if e is not None else rng.choice([2, 5, 8]); D = D if D is not None else rng.choice([1, 4, 8])
    for _ in range(50):
        # vector with 1-2 planted sites per enzyme
        planted = []
        for en in enzymes:
            for _ in range(rng.randint(1, 2)): planted.append((rng.randrange(V), en.site))
        vseq = plant(rng, V, planted)
        L = rng.randint(*lrange)
        iplanted = []
        for en in enzymes:
            for _ in range(rng.randint(0, K)): iplanted.append((rng.randrange(L), en.site))
        ins = plant(rng, L, iplanted)
        cl = sorted(rng.sample(range(V), rng.randint(1, 3)))
        c = rng.choice(cl)
        recomb = vseq[:c] + ins + vseq[c:]
        N = V + L
        cuts = {en.name: sorted(cut for _, cut in model.scan_sites(recomb, en.site, en.offset)) for en in enzymes}
        truth = Map(c, L, cuts)
        prob = Problem(vseq, enzymes, [("site", x) for x in cl], e, D, lrange[0], lrange[1], K, [], [])
        if model.valid_map(prob, truth) is not None or not model.anchored(prob): continue
        if sum(len(v) for v in cuts.values()) == 0: continue
        pairs = [sorted(p) for p in itertools.combinations([x.name for x in enzymes], 2)]
        rng.shuffle(pairs)
        n_obs = rng.randint(0, len(pairs)); obs_pairs = pairs[:n_obs]; held = pairs[n_obs:n_obs + 1]
        obs = [Digest(en.name, [en.name], generate.simulate_digest(rng, cuts, [en.name], N, D, e, comigrate=0.0)) for en in enzymes]
        obs += [Digest(model.digest_id(p), p, generate.simulate_digest(rng, cuts, p, N, D, e, comigrate=0.0)) for p in obs_pairs]
        prob.observed = obs; prob.held_out = [(model.digest_id(p), p) for p in held]
        ok, _ = model.evaluate(prob, truth)
        if not ok: continue
        return prob, truth
    return None
