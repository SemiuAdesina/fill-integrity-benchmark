#!/usr/bin/env python3
"""Exhaustive search for tiny instances: every cloning site, every insert
length, every set of at most K insert-region positions per enzyme."""
import sys, os, itertools
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model
from model import Map


def brute(p):
    best = None
    for _, c in p.cloning_sites:
        for L in range(p.insert_min, p.insert_max + 1):
            region = list(range(c, c + L + 1))
            per = []
            for e in p.enzymes:
                req = model.required_positions(p, e, c, L)
                sets = []
                for t in range(p.max_internal + 1):
                    for comb in itertools.combinations(region, t):
                        sets.append(sorted(req + list(comb)))
                per.append(sets)
            for combo in itertools.product(*per):
                m = Map(c, L, {e.name: list(s) for e, s in zip(p.enzymes, combo)})
                ok, dev = model.evaluate(p, m)
                if not ok: continue
                k = model.key(p, m, dev)
                if best is None or k < best[0]: best = (k, m)
    return None if best is None else best[1]
