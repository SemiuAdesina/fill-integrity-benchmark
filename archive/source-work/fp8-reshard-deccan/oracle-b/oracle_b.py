#!/usr/bin/env python3
"""ORACLE B — independent restoration written from the specification text.

Organisation deliberately differs from the model/oracle A:
  * works on raw JSON dicts, not dataclasses;
  * coverage via explicit boolean index maps per (group, role);
  * rings handled NEWEST-FIRST (reversed rotation) so suffix comparison is a
    prefix comparison and resizing is a truncation / right-padding;
  * duplicates detected after sorting targets, not pairwise over the raw list;
  * its own integer step model (bit_length for the power of two, explicit
    sign handling for truncation).
Author/verifier side only; never ships.
"""
import json


class RejectB(Exception):
    pass


def group_of(x):
    return x["tie_group"] if x["tie_group"] is not None else x["tensor"]


def newest_first(rec):
    h, c, L = rec["amax_history"], rec["cursor"], len(rec["amax_history"])
    # newest sample is the slot before the cursor; walk backwards
    return [h[(c - 1 - i) % L] for i in range(L)]


def restore_b(ckpt, layout):
    records, targets = ckpt["records"], layout["targets"]
    for r in records:
        if len(r["amax_history"]) == 0 or r["cursor"] >= len(r["amax_history"]):
            raise RejectB("record")
    blocks = {}
    for r in records:
        blocks.setdefault((group_of(r), r["role"], r["logical_begin"] // r["block_size"]), []).append(r)
    merged = {}
    for key, rs in blocks.items():
        scales = {(r["scale"], r["inverse_scale"]) for r in rs}
        if len(scales) != 1:
            raise RejectB("scale")
        nf = [newest_first(r) for r in rs]
        for a in nf:
            for b in nf:
                n = min(len(a), len(b))
                if a[:n] != b[:n]:
                    raise RejectB("suffix")
        order = sorted(range(len(rs)), key=lambda i: (-len(nf[i]), rs[i]["replica"], rs[i]["stage"], rs[i]["shard"], rs[i]["tensor"].encode()))
        merged[key] = (scales.pop(), nf[order[0]])
    # targets
    if any(t["history_length"] == 0 for t in targets):
        raise RejectB("zero target length")
    placements = set()
    for t in targets:
        p = (t["tensor"], t["role"], t["stage"], t["shard"])
        if p in placements:
            raise RejectB("duplicate placement")
        placements.add(p)
    by_tr = {}
    for t in targets:
        by_tr.setdefault((t["tensor"], t["role"]), []).append((t["logical_begin"], t["logical_end"]))
    for ivs in by_tr.values():
        ivs.sort()
        for (a0, a1), (b0, b1) in zip(ivs, ivs[1:]):
            if b0 < a1:
                raise RejectB("overlap")
    # coverage maps
    cover = {}
    for r in records:
        k = (group_of(r), r["role"])
        m = cover.setdefault(k, bytearray())
        if len(m) < r["logical_end"]:
            m.extend(b"\0" * (r["logical_end"] - len(m)))
        for i in range(r["logical_begin"], r["logical_end"]):
            m[i] = 1
    states = []
    for t in targets:
        if not t["active"]:
            continue
        m = cover.get((group_of(t), t["role"]), bytearray())
        if len(m) < t["logical_end"] or any(m[i] == 0 for i in range(t["logical_begin"], t["logical_end"])):
            raise RejectB("coverage")
        B = t["block_size"]
        for k in range(t["logical_begin"] // B, (t["logical_end"] - 1) // B + 1):
            (scale, inv), nf = merged[(group_of(t), t["role"], k)]
            L = t["history_length"]
            kept = nf[:L] + [0] * max(0, L - len(nf))
            states.append({"tensor": t["tensor"], "role": t["role"], "stage": t["stage"], "shard": t["shard"], "global_block": k,
                           "scale": scale, "inverse_scale": inv, "amax_history": list(reversed(kept)), "cursor": 0})
    states.sort(key=lambda s: (s["tensor"].encode(), 0 if s["role"] == "forward" else 1, s["stage"], s["shard"], s["global_block"]))
    return states


def step_b(state, amax, value):
    h = list(state["amax_history"])
    c = state["cursor"]
    h[c] = amax
    c = (c + 1) % len(h)
    peak = max(h)
    if peak < 1:
        peak = 1
    inv = 1 << (peak - 1).bit_length() if peak > 1 else 1
    scale = (1 << 24) // inv
    prod = value * scale
    if prod >= 0:
        q = prod >> 24
    else:
        q = -((-prod) >> 24)
    q = min(448, max(-448, q))
    return {"amax_history": h, "cursor": c, "scale": scale, "inverse_scale": inv}, q


def trace_b(state, pairs):
    cur = dict(state)
    out = []
    for a, v in pairs:
        cur, q = step_b(cur, a, v)
        out.append({"amax_history": list(cur["amax_history"]), "cursor": cur["cursor"], "scale": cur["scale"], "inverse_scale": cur["inverse_scale"], "quantized": q})
    return out


if __name__ == "__main__":
    import sys
    ck = json.load(open(sys.argv[1])); tg = json.load(open(sys.argv[2]))
    try:
        print(json.dumps(restore_b(ck, tg), indent=1))
    except RejectB as e:
        print('{"status": "reject", "reason": "%s"}' % e)
