#!/usr/bin/env python3
"""Author-side fixture generator with certification.

A fixture is a latent world (tensors, tie groups, per-block true histories and
scale pairs) materialised into a source topology (replicas, stages, ragged
shards, per-block records with rings of varying length and random cursors)
and a target topology (moved stages, new ragged shard cuts, new history
lengths, inactive placements). Reject fixtures apply exactly one perturbation
to a valid world. Certification: oracle A (research/model.py) and oracle B
(oracle-b/oracle_b.py) must produce identical outcomes, and both step models
must produce identical replay traces on hidden (amax, value) sequences.

Writes author/fixtures/<id>/{checkpoint.json,target.json,expected.json,certification.json}.
Latent worlds never ship.
"""
import sys, os, json, random, shutil, copy
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "research"))
sys.path.insert(0, os.path.join(HERE, "..", "oracle-b"))
import model
from model import Record, Target, restore, Reject, checkpoint_scale, trace, State, result_json, well_formed
import oracle_b

REPLAY_STEPS = 6


class World:
    def __init__(self, rng, tensors, ring_max=8):
        # tensors: list of (name, tie_group|None, length, block_size)
        self.rng = rng
        self.tensors = tensors
        self.hist = {}   # (group, role, block) -> true chronological history (oldest first)
        for name, tie, length, bs in tensors:
            group = tie if tie else name
            for role in model.ROLES:
                for k in range((length - 1) // bs + 1):
                    key = (group, role, k)
                    if key in self.hist:
                        continue
                    n = rng.randint(3, ring_max)
                    h = []
                    for _ in range(n):
                        r = rng.random()
                        h.append(0 if r < 0.12 else rng.randint(1, 1 << 12) if r < 0.7 else rng.randint(1 << 12, (1 << 20) - 1))
                    self.hist[key] = h

    def scale_pair(self, key):
        return checkpoint_scale(self.hist[key])


def ragged_cuts(rng, length, parts, block_size):
    """parts intervals covering [0, length) with cut points not aligned to blocks."""
    if parts == 1:
        return [(0, length)]
    cuts = set()
    while len(cuts) < parts - 1:
        c = rng.randint(1, length - 1)
        if rng.random() < 0.75 and c % block_size == 0:
            c += rng.choice([-3, -1, 1, 5, 17]) if 0 < c + 17 < length else 0
        if 0 < c < length:
            cuts.add(c)
    pts = [0] + sorted(cuts) + [length]
    return [(pts[i], pts[i + 1]) for i in range(parts)]


def source_records(world, rng, replicas, stages, shards_per_stage, length_policy=None, drop_rate=0.0):
    """Materialise records. Each tensor is assigned a source stage; each replica
    stores every tensor of its stages in `shards_per_stage` ragged shards. Ring
    length per (replica) may be shorter than the true history."""
    records = []
    stage_of = {name: rng.randrange(stages) for name, _, _, _ in world.tensors}
    for name, tie, length, bs in world.tensors:
        group = tie if tie else name
        for role in model.ROLES:
            for rep in range(replicas):
                cuts = ragged_cuts(rng, length, shards_per_stage, bs)
                for shard, (a, b) in enumerate(cuts):
                    if rng.random() < drop_rate:
                        continue
                    for k in range(a // bs, (b - 1) // bs + 1):
                        lo, hi = max(a, k * bs), min(b, (k + 1) * bs)
                        key = (group, role, k)
                        full = world.hist[key]
                        n = length_policy(rng, rep, len(full)) if length_policy else len(full)
                        n = max(1, min(n, len(full)))
                        chron = full[len(full) - n:]
                        c = rng.randrange(n)
                        ring = chron[n - c:] + chron[:n - c]  # so that rotating from c yields chron
                        scale, inv = world.scale_pair(key)
                        records.append(Record(name, tie, role, bs, lo, hi, rep, stage_of[name], shard, scale, inv, ring, c))
    rng.shuffle(records)
    return records


def target_layout(world, rng, stages, shards_per_stage, length_choice, inactive_rate=0.15, move=True):
    targets = []
    stage_of = {name: rng.randrange(stages) for name, _, _, _ in world.tensors}
    for name, tie, length, bs in world.tensors:
        for role in model.ROLES:
            cuts = ragged_cuts(rng, length, shards_per_stage, bs)
            for shard, (a, b) in enumerate(cuts):
                L = length_choice(rng)
                targets.append(Target(name, tie, role, bs, stage_of[name], shard, a, b, L, rng.random() >= inactive_rate))
    rng.shuffle(targets)
    return targets


def std_lengths(rng, rep, full):
    # replica 0 keeps everything; other replicas may have shorter rings (a later config change)
    return full if rep == 0 or rng.random() < 0.5 else rng.randint(1, full)


def mixed_target_length(rng):
    return rng.choice([2, 3, 4, 6, 8, 10, 12, 16])


# ----------------------------------------------------------------- builders
def w_public(rng):
    return World(rng, [("decoder.layers.0.attn.qkv.weight", None, 3000, 1024), ("decoder.layers.0.mlp.fc1.weight", None, 4096, 1024),
                       ("embed.tokens.weight", "tied_embed", 2500, 500), ("lm_head.weight", "tied_embed", 2500, 500)])


def build_public(rng):
    w = w_public(rng)
    rec = source_records(w, rng, replicas=2, stages=2, shards_per_stage=2, length_policy=std_lengths)
    tg = target_layout(w, rng, stages=3, shards_per_stage=3, length_choice=mixed_target_length)
    return w, rec, tg, "accept"


def build_h_topology(rng):
    w = World(rng, [("decoder.layers.%d.attn.out.weight" % i, None, rng.choice([2048, 3072, 5000]), rng.choice([512, 1024])) for i in range(4)], ring_max=10)
    rec = source_records(w, rng, replicas=1, stages=2, shards_per_stage=2, length_policy=std_lengths)
    tg = target_layout(w, rng, stages=4, shards_per_stage=3, length_choice=mixed_target_length, inactive_rate=0.1)
    return w, rec, tg, "accept"


def build_h_tied_replicas(rng):
    w = World(rng, [("embed.weight", "tie0", 4000, 1000), ("head.weight", "tie0", 4000, 1000), ("decoder.norm.weight", None, 1500, 1000)], ring_max=12)
    rec = source_records(w, rng, replicas=3, stages=2, shards_per_stage=2, length_policy=lambda rng, rep, full: [full, max(1, full - 3), max(1, full // 2)][rep])
    # the target for head.weight must be restored from embed.weight records only: drop every head.weight record
    rec = [r for r in rec if r.tensor != "head.weight"]
    tg = target_layout(w, rng, stages=2, shards_per_stage=2, length_choice=lambda rng: rng.choice([4, 8, 14]), inactive_rate=0.0)
    return w, rec, tg, "accept"


def build_h_roles_resize(rng):
    w = World(rng, [("decoder.layers.1.mlp.fc2.weight", None, 6000, 1024), ("decoder.layers.1.attn.qkv.weight", None, 2048, 256)], ring_max=6)
    rec = source_records(w, rng, replicas=2, stages=1, shards_per_stage=3, length_policy=std_lengths)
    tg = target_layout(w, rng, stages=2, shards_per_stage=2, length_choice=lambda rng: rng.choice([1, 2, 12, 16]), inactive_rate=0.0)
    return w, rec, tg, "accept"


def build_h_older_samples_differ(rng):
    """A longer ring differs from a shorter one only in samples older than the shorter ring: ACCEPT."""
    w = World(rng, [("decoder.layers.2.mlp.fc1.weight", None, 2048, 1024)], ring_max=8)
    rec = source_records(w, rng, replicas=2, stages=1, shards_per_stage=2, length_policy=lambda rng, rep, full: full if rep == 0 else max(1, full - 2))
    # perturb an OLD sample (outside every overlap) of a replica-0 record whose ring is the full one
    for r in rec:
        if r.replica == 0 and len(r.amax_history) >= 4:
            chron = model.chronological(r)
            # every other record for the block has length <= full-2, so index 0 of chron is outside all overlaps
            chron[0] = (chron[0] + 7) % (1 << 20)
            c = r.cursor
            r.amax_history = chron[len(chron) - c:] + chron[:len(chron) - c]
            break
    tg = target_layout(w, rng, stages=1, shards_per_stage=2, length_choice=lambda rng: 12, inactive_rate=0.0)
    return w, rec, tg, "accept"


def perturbed(base_builder, kind):
    def b(rng):
        w, rec, tg, _ = base_builder(rng)
        if kind == "scale_unneeded_block":
            # make a block that no active target needs, then break its scale
            victim = rec[0]
            for t in tg:
                if t.tensor == victim.tensor and t.role == victim.role and victim.block in t.blocks():
                    t.active = False
            twin = [r for r in rec if (r.group, r.role, r.block) == (victim.group, victim.role, victim.block)]
            twin[-1].inverse_scale *= 2
        elif kind == "suffix":
            # change a NEWEST sample of one record that shares its block with another record
            by = {}
            for r in rec:
                by.setdefault((r.group, r.role, r.block), []).append(r)
            cands = []
            for v in by.values():
                if len(v) < 2:
                    continue
                for r in v:
                    if sum(1 for x in v if len(x.amax_history) == len(r.amax_history)) == 1:
                        cands.append((len(v), r))
            if cands:
                r = max(cands, key=lambda c: c[0])[1]
            else:
                r = next(v for v in by.values() if len(v) >= 2)[0]
            newest = (r.cursor - 1) % len(r.amax_history)
            r.amax_history[newest] = (r.amax_history[newest] + 1) % (1 << 20)
        elif kind == "cursor":
            r = rec[len(rec) // 2]
            r.cursor = len(r.amax_history)
        elif kind == "empty_ring":
            r = rec[len(rec) // 3]
            r.amax_history, r.cursor = [], 0
        elif kind == "coverage_gap":
            # open a one-index hole inside a block that an active target needs, keeping the block described
            t = next(t for t in tg if t.active)
            k = list(t.blocks())[0]
            rs = [r for r in rec if (r.group, r.role, r.block) == (t.group, t.role, k)]
            # shrink every record of that block that starts at the block's first covered index by one
            lo = min(r.logical_begin for r in rs)
            for r in rs:
                if r.logical_begin == lo and r.logical_end - r.logical_begin > 1:
                    r.logical_begin += 1
            # the hole is now at index lo if it is inside the target's interval; extend the target to include it
            if not (t.logical_begin <= lo < t.logical_end):
                t.logical_begin = min(t.logical_begin, lo)
                for u in tg:
                    if u is not t and u.tensor == t.tensor and u.role == t.role and u.logical_begin < t.logical_end and t.logical_begin < u.logical_end:
                        u.active = False
                        u.logical_end = min(u.logical_end, t.logical_begin) if u.logical_begin < t.logical_begin else u.logical_end
                        if u.logical_end <= u.logical_begin:
                            u.logical_end = u.logical_begin + 1
        elif kind == "duplicate_overlap":
            t = next(t for t in tg if t.active)
            d = copy.deepcopy(t)
            d.shard = max(x.shard for x in tg) + 1
            d.logical_begin = t.logical_end - 1
            d.logical_end = t.logical_end
            tg.append(d)
        elif kind == "zero_target_length":
            t = next(t for t in tg if not t.active) if any(not t.active for t in tg) else tg[0]
            t.history_length = 0
        return w, rec, tg, "reject"
    return b


def build_h_inactive_gap_accept(rng):
    """Coverage is missing only under an inactive target: ACCEPT. The builder
    verifies that the same layout with the target active would be rejected."""
    w = World(rng, [("decoder.layers.5.mlp.fc1.weight", None, 4096, 1024)], ring_max=6)
    rec = source_records(w, rng, replicas=1, stages=1, shards_per_stage=2, length_policy=std_lengths)
    tg = target_layout(w, rng, stages=1, shards_per_stage=2, length_choice=lambda rng: 6, inactive_rate=0.0)
    t = tg[0]
    t.active = False
    # remove every record of the target's identity whose interval lies inside the target's interval
    rec = [r for r in rec if not (r.group == t.group and r.role == t.role and r.logical_begin >= t.logical_begin and r.logical_end <= t.logical_end)]
    # the gap must be real: with the target active the layout must reject for coverage
    probe = copy.deepcopy(tg)
    probe[0].active = True
    try:
        restore(rec, probe)
        return w, rec, tg, "retry"
    except Reject as e:
        if str(e) != "coverage":
            return w, rec, tg, "retry"
    return w, rec, tg, "accept"


def build_h_boundaries(rng):
    w = World(rng, [("decoder.layers.7.attn.qkv.weight", None, 3000, 1000), ("decoder.layers.7.mlp.fc1.weight", None, 2001, 1000)], ring_max=5)
    rec = source_records(w, rng, replicas=1, stages=1, shards_per_stage=3, length_policy=std_lengths)
    # force block-aligned cut points for a few records and targets
    tg = target_layout(w, rng, stages=1, shards_per_stage=3, length_choice=lambda rng: rng.choice([3, 5, 7]), inactive_rate=0.0)
    return w, rec, tg, "accept"


def build_h_ordering(rng):
    w = World(rng, [("Zeta.weight", None, 2048, 1024), ("alpha.weight", None, 2048, 1024), ("layer.10.weight", None, 1024, 512), ("layer.2.weight", None, 1024, 512)], ring_max=5)
    rec = source_records(w, rng, replicas=1, stages=1, shards_per_stage=2, length_policy=std_lengths)
    tg = target_layout(w, rng, stages=12, shards_per_stage=2, length_choice=lambda rng: 4, inactive_rate=0.0)
    for t in tg:
        t.stage = rng.choice([2, 10, 11])
        t.shard = t.shard * 9  # 0 and 9 vs 10-style comparisons
    return w, rec, tg, "accept"


BUILDERS = {
    "public": (build_public, 100),
    "h_topology": (build_h_topology, 200),
    "h_tied_replicas": (build_h_tied_replicas, 300),
    "h_roles_resize": (build_h_roles_resize, 400),
    "h_older_samples_differ": (build_h_older_samples_differ, 500),
    "h_inactive_gap_accept": (build_h_inactive_gap_accept, 600),
    "h_boundaries": (build_h_boundaries, 700),
    "h_ordering": (build_h_ordering, 800),
    "h_reject_scale_unneeded": (perturbed(build_public, "scale_unneeded_block"), 900),
    "h_reject_suffix": (perturbed(build_h_tied_replicas, "suffix"), 1000),
    "h_reject_cursor": (perturbed(build_h_topology, "cursor"), 1100),
    "h_reject_empty_ring": (perturbed(build_h_roles_resize, "empty_ring"), 1200),
    "h_reject_coverage_gap": (perturbed(build_h_topology, "coverage_gap"), 1300),
    "h_reject_duplicate_targets": (perturbed(build_h_boundaries, "duplicate_overlap"), 1400),
    "h_reject_zero_target_length": (perturbed(build_public, "zero_target_length"), 1500),
}


def materialise(fid, w, rec, tg, expect, out_root, rng):
    d = os.path.join(out_root, fid)
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    ck = {"format_version": 1, "records": [r.to_json() for r in rec]}
    ly = {"format_version": 1, "targets": [t.to_json() for t in tg]}
    json.dump(ck, open(os.path.join(d, "checkpoint.json"), "w"), indent=1)
    json.dump(ly, open(os.path.join(d, "target.json"), "w"), indent=1)
    return d, ck, ly


def certify(fid, d, ck, ly, rec, tg, expect, rng):
    rows = {"fixture": fid, "expected_outcome": expect}
    assert well_formed(rec, tg), "generator produced a malformed fixture"
    try:
        states = restore(rec, tg)
        a = "accept"
    except Reject as e:
        states = None
        a = "reject:%s" % e
    try:
        sb = oracle_b.restore_b(ck, ly)
        b = "accept"
    except oracle_b.RejectB as e:
        sb = None
        b = "reject:%s" % e
    rows["oracle_a"], rows["oracle_b"] = a, b
    ok = (a.split(":")[0] == b.split(":")[0] == expect)
    if states is not None and sb is not None:
        ok &= [s.to_json() for s in states] == sb
        rows["states"] = len(states)
    replay = []
    if states is not None and ok:
        for i, s in enumerate(states):
            pairs = [(rng.choice([0, rng.randint(1, 1 << 12), rng.randint(1 << 12, (1 << 20) - 1)]), rng.choice([rng.randint(-(1 << 31), (1 << 31) - 1), rng.randint(-5000, 5000), 0])) for _ in range(REPLAY_STEPS)]
            ta = trace(s, pairs)
            tb = oracle_b.trace_b(s.to_json(), pairs)
            if ta != tb:
                ok = False
                rows["trace_disagreement"] = i
            replay.append({"state_index": i, "pairs": [[a_, v_] for a_, v_ in pairs], "trace": ta})
    rows["retained"] = ok
    expected = {"fixture": fid, "outcome": expect, "result": None if states is None else [s.to_json() for s in states], "replay": replay}
    json.dump(expected, open(os.path.join(d, "expected.json"), "w"), indent=1)
    open(os.path.join(d, "restored.expected.json"), "w").write(result_json(states))
    json.dump(rows, open(os.path.join(d, "certification.json"), "w"), indent=1)
    return rows


def run(fid, out_root, max_tries=30):
    builder, seed = BUILDERS[fid]
    for attempt in range(max_tries):
        rng = random.Random(seed + attempt)
        w, rec, tg, expect = builder(rng)
        d, ck, ly = materialise(fid, w, rec, tg, expect, out_root, rng)
        if expect == "retry":
            print("REJECTED %-28s attempt=%d (trap not realised)" % (fid, attempt)); continue
        rows = certify(fid, d, ck, ly, rec, tg, expect, random.Random(seed * 7 + attempt))
        if rows["retained"]:
            print("RETAINED %-28s seed=%d A=%s B=%s states=%s records=%d targets=%d" % (fid, seed + attempt, rows["oracle_a"], rows["oracle_b"], rows.get("states"), len(rec), len(tg)))
            return
        print("REJECTED %-28s attempt=%d A=%s B=%s expected=%s" % (fid, attempt, rows["oracle_a"], rows["oracle_b"], expect))
    raise SystemExit("no retained fixture for " + fid)


if __name__ == "__main__":
    out_root = os.path.join(HERE, "fixtures")
    which = sys.argv[1:] or list(BUILDERS)
    if which == ["all"]:
        which = list(BUILDERS)
    for fid in which:
        run(fid, out_root)
