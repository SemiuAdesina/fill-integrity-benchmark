#!/usr/bin/env python3
"""Scale fabrics: tiered accelerator fabrics far too large for path-product or
tree enumeration, with a latent legal plan built directly (no enumeration)
whose replay supplies the telemetry. Author-side, never ships."""
import sys, os, random, math, itertools, time, json
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model
from model import Problem, Source, Switch, Link, Illegal


def scale_fabric(rng, n_src, n_leaf, n_spine, n_core, payload=8, rates=(4, 8, 16, 32), lats=(0, 1, 2, 3, 4), shared=0, plain_frac=0.2):
    names = ["s%02d" % i for i in range(n_src)]
    rng.shuffle(names)
    ends = ["E%02d" % i for i in range(n_src)]
    for k in range(shared):  # some endpoints host two sources
        ends[n_src - 1 - k] = ends[k]
    sources = [Source(names[i], ends[i], rng.randint(-5000, 5000)) for i in range(n_src)]
    def cap():
        return rng.choice([payload - 1, payload, payload, payload * 2])
    leaves = ["L%02d" % i for i in range(n_leaf)]; spines = ["P%02d" % i for i in range(n_spine)]; cores = ["C%02d" % i for i in range(n_core)]
    switches = []
    for w in leaves + spines + cores:
        if rng.random() < plain_frac: switches.append(Switch(w, False, 0))
        else: switches.append(Switch(w, True, cap()))
    links = []; lid = [0]
    def add(frm, to):
        lid[0] += 1
        links.append(Link("k%03d" % lid[0], frm, to, rng.choice(rates), rng.choice(lats)))
    for e in sorted(set(ends)):
        for w in rng.sample(leaves, min(n_leaf, rng.randint(2, 3))):
            add(e, w)
            if rng.random() < 0.3: add(e, w)  # parallel link, different parameters
        if rng.random() < 0.3: add(e, rng.choice(spines))
        if rng.random() < 0.2: add(e, "S")
    for w in leaves:
        for x in rng.sample(spines, min(n_spine, rng.randint(2, 3))): add(w, x)
        for x in rng.sample([y for y in leaves if y != w], min(n_leaf - 1, rng.randint(1, 2))): add(w, x)
        if rng.random() < 0.3: add(w, "S")
    for w in spines:
        for x in rng.sample(cores, min(n_core, rng.randint(1, 2))): add(w, x)
        for x in rng.sample([y for y in spines if y != w], min(n_spine - 1, 1)): add(w, x)
        if rng.random() < 0.5: add(w, "S")
        if rng.random() < 0.3: add(w, rng.choice(leaves))  # back link
    for w in cores:
        add(w, "S")
        for x in [y for y in cores if y != w][:1]: add(w, x)
        if rng.random() < 0.4: add(w, rng.choice(spines))
    # shuffle link document order so ids do not follow tiers
    rng.shuffle(links)
    return Problem(payload, "S", sources, switches, links, {}, None)


def random_legal_plan(rng, p, combine_prob=0.5, tries=200):
    """A random in-tree routing every endpoint to the sink, plus a random legal combine set."""
    out = {}
    for l in p.links: out.setdefault(l.frm, []).append(l)
    link_to = {l.id: l.to for l in p.links}
    ends = sorted({s.node for s in p.sources})
    for _ in range(tries):
        parent = {}
        ok = True
        for e in ends:
            node = e; onpath = {e}
            while node != p.sink and node not in parent:
                choices = [l for l in out.get(node, []) if l.to not in onpath and (l.to == p.sink or l.to in parent or l.to not in onpath)]
                if not choices: ok = False; break
                l = rng.choice(choices)
                parent[node] = l.id; node = l.to; onpath.add(node)
            if not ok: break
        if not ok: continue
        # the tree must be acyclic: check each endpoint resolves
        good = True
        for e in ends:
            node = e; seen = set()
            while node != p.sink:
                if node in seen: good = False; break
                seen.add(node); node = link_to[parent[node]]
            if not good: break
        if not good: continue
        srcs = sorted(p.sources, key=lambda s: s.id)
        paths = []
        for s in srcs:
            node = s.node; path = []
            while node != p.sink: path.append(parent[node]); node = link_to[parent[node]]
            paths.append(path)
        capable = {w.id for w in p.switches if w.reduction_capable and w.accumulator_capacity >= p.payload_bytes}
        visited = [n for n in parent if n in capable]
        for _ in range(20):
            sub = sorted(n for n in visited if rng.random() < combine_prob)
            try:
                rep = model.replay(p, paths, sub)
                return paths, sub, rep
            except Illegal:
                continue
    return None


def telemetry_from(rng, p, rep, listed_fraction=0.5, zero_fraction=0.3, with_completion=True):
    lb = {}
    for l in p.links:
        b = rep["link_bytes"].get(l.id, 0)
        if (b > 0 and rng.random() < listed_fraction) or (b == 0 and rng.random() < zero_fraction): lb[l.id] = b
    p.link_bytes = lb
    p.completion = rep["completion"] if with_completion else None


def path_product(p):
    srcs = sorted(p.sources, key=lambda s: s.id)
    counts = []
    for s in srcs:
        counts.append(len(model.simple_paths(p, s.node)))
    return counts


def count_trees(p, cap=10 ** 7):
    """Number of in-trees routing every endpoint to the sink (oracle B's enumeration), capped."""
    out = {}
    for l in p.links: out.setdefault(l.frm, []).append(l)
    link_to = {l.id: l.to for l in p.links}
    ends = sorted({s.node for s in p.sources}); sink = p.sink
    n = [0]
    def rec(pending, parent):
        pending = [x for x in pending if x != sink and x not in parent]
        if not pending:
            n[0] += 1
            if n[0] >= cap: raise StopIteration
            return
        node = pending[0]
        for l in out.get(node, []):
            chain, cur, cyc = set(), l.to, False
            while cur != sink and cur in parent:
                if cur == node or cur in chain: cyc = True; break
                chain.add(cur); cur = link_to[parent[cur]]
            if cyc or cur == node: continue
            parent[node] = l.id
            rec(pending[1:] + ([l.to] if l.to != sink and l.to not in parent else []), parent)
            del parent[node]
    try:
        rec(ends, {})
    except StopIteration:
        return cap, True
    return n[0], False


if __name__ == "__main__":
    import bnb
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    n_src = int(sys.argv[2]) if len(sys.argv) > 2 else 8
    rng = random.Random(seed)
    p = scale_fabric(rng, n_src, 5, 4, 2)
    plan = random_legal_plan(rng, p)
    if plan is None:
        print("no legal plan"); sys.exit(0)
    paths, sub, rep = plan
    telemetry_from(rng, p, rep, listed_fraction=0.4, zero_fraction=0.3, with_completion=True)
    pp = path_product(p)
    prod = 1
    for c in pp: prod *= c
    trees, capped = count_trees(p, 2 * 10 ** 6)
    print("seed %d: sources=%d switches=%d links=%d paths/source=%s product=%.2e trees=%s%s latent combines=%d completion=%d listed=%d" % (
        seed, len(p.sources), len(p.switches), len(p.links), pp, prod, trees, "+" if capped else "", len(sub), rep["completion"], len(p.link_bytes)))
    t0 = time.time()
    try:
        st, plan, info = bnb.recover_c(p, budget_s=120)
        print("  oracle C: %s %s" % (st, info), "canonical==latent" if plan and plan["paths"] == paths and plan["combine"] == sub else "canonical differs from latent")
    except bnb.Budget:
        print("  oracle C: BUDGET EXCEEDED")
    p.completion = None
    try:
        st, plan, info = bnb.recover_c(p, budget_s=120)
        print("  oracle C (no completion): %s %s" % (st, info))
    except bnb.Budget:
        print("  oracle C (no completion): BUDGET EXCEEDED")
