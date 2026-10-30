#!/usr/bin/env python3
"""Author-side executable model of the PUBLIC FP8 resharding contract
(spec/checkpoint-format-v1.md, restoration-semantics-v1.md,
delayed-scaling-model-v1.md, result-format-v1.md). Used by the generator and
by oracle A. Never ships."""
import json
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple

ROLES = ["forward", "backward"]
AMAX_LIMIT = 1 << 20


@dataclass
class Record:
    tensor: str
    tie_group: Optional[str]
    role: str
    block_size: int
    logical_begin: int
    logical_end: int
    replica: int
    stage: int
    shard: int
    scale: int
    inverse_scale: int
    amax_history: List[int]
    cursor: int

    @property
    def group(self):
        return self.tie_group if self.tie_group is not None else self.tensor

    @property
    def block(self):
        return self.logical_begin // self.block_size

    def to_json(self):
        return {"tensor": self.tensor, "tie_group": self.tie_group, "role": self.role, "block_size": self.block_size,
                "logical_begin": self.logical_begin, "logical_end": self.logical_end, "replica": self.replica,
                "stage": self.stage, "shard": self.shard, "scale": self.scale, "inverse_scale": self.inverse_scale,
                "amax_history": list(self.amax_history), "cursor": self.cursor}


@dataclass
class Target:
    tensor: str
    tie_group: Optional[str]
    role: str
    block_size: int
    stage: int
    shard: int
    logical_begin: int
    logical_end: int
    history_length: int
    active: bool

    @property
    def group(self):
        return self.tie_group if self.tie_group is not None else self.tensor

    def blocks(self):
        return range(self.logical_begin // self.block_size, (self.logical_end - 1) // self.block_size + 1)

    def to_json(self):
        return {"tensor": self.tensor, "tie_group": self.tie_group, "role": self.role, "block_size": self.block_size,
                "stage": self.stage, "shard": self.shard, "logical_begin": self.logical_begin,
                "logical_end": self.logical_end, "history_length": self.history_length, "active": self.active}


@dataclass
class State:
    tensor: str
    role: str
    stage: int
    shard: int
    global_block: int
    scale: int
    inverse_scale: int
    amax_history: List[int]
    cursor: int = 0

    def key(self):
        return (self.tensor.encode(), ROLES.index(self.role), self.stage, self.shard, self.global_block)

    def to_json(self):
        return {"tensor": self.tensor, "role": self.role, "stage": self.stage, "shard": self.shard,
                "global_block": self.global_block, "scale": self.scale, "inverse_scale": self.inverse_scale,
                "amax_history": list(self.amax_history), "cursor": self.cursor}


class Reject(Exception):
    pass


# ------------------------------------------------------------------ loading
def load_records(doc) -> List[Record]:
    assert doc["format_version"] == 1
    out = []
    for r in doc["records"]:
        out.append(Record(r["tensor"], r["tie_group"], r["role"], r["block_size"], r["logical_begin"], r["logical_end"],
                          r["replica"], r["stage"], r["shard"], r["scale"], r["inverse_scale"], list(r["amax_history"]), r["cursor"]))
    return out


def load_targets(doc) -> List[Target]:
    assert doc["format_version"] == 1
    return [Target(t["tensor"], t["tie_group"], t["role"], t["block_size"], t["stage"], t["shard"], t["logical_begin"],
                   t["logical_end"], t["history_length"], t["active"]) for t in doc["targets"]]


def well_formed(records: List[Record], targets: List[Target]) -> bool:
    """checkpoint-format-v1.md §2–§4 (structural rules the corpus always satisfies)."""
    tie, bs = {}, {}
    for r in records:
        if not r.tensor or r.role not in ROLES or r.block_size < 1 or not (0 <= r.logical_begin < r.logical_end):
            return False
        if r.logical_begin // r.block_size != (r.logical_end - 1) // r.block_size:
            return False
        if min(r.replica, r.stage, r.shard, r.cursor) < 0 or r.scale < 1 or r.inverse_scale < 1:
            return False
        if any(not (0 <= a < AMAX_LIMIT) for a in r.amax_history):
            return False
        if tie.setdefault(r.tensor, r.tie_group) != r.tie_group or bs.setdefault(r.tensor, r.block_size) != r.block_size:
            return False
    for t in targets:
        if not t.tensor or t.role not in ROLES or t.block_size < 1 or not (0 <= t.logical_begin < t.logical_end):
            return False
        if min(t.stage, t.shard, t.history_length) < 0:
            return False
        if tie.setdefault(t.tensor, t.tie_group) != t.tie_group or bs.setdefault(t.tensor, t.block_size) != t.block_size:
            return False
    group_bs = {}
    for x in list(records) + list(targets):
        if x.tie_group is not None and group_bs.setdefault(x.tie_group, x.block_size) != x.block_size:
            return False
    return True


# ---------------------------------------------------------------- semantics
def chronological(r: Record) -> List[int]:
    """§3: rotate once from the next-write cursor, oldest first."""
    h, c = r.amax_history, r.cursor
    return h[c:] + h[:c]


def restore(records: List[Record], targets: List[Target]) -> List[State]:
    """The complete public restoration. Raises Reject."""
    # §2 validation of every record
    for r in records:
        if len(r.amax_history) == 0 or r.cursor >= len(r.amax_history):
            raise Reject("record")
    # §4 consistency per identity, over every identity present
    by_id: Dict[Tuple[str, str, int], List[Record]] = {}
    for r in records:
        by_id.setdefault((r.group, r.role, r.block), []).append(r)
    chosen: Dict[Tuple[str, str, int], Tuple[int, int, List[int]]] = {}
    for ident, rs in by_id.items():
        scale, inv = rs[0].scale, rs[0].inverse_scale
        chron = [chronological(r) for r in rs]
        for r in rs:
            if r.scale != scale or r.inverse_scale != inv:
                raise Reject("scale")
        for i in range(len(rs)):
            for j in range(len(rs)):
                a, b = chron[i], chron[j]
                if len(a) <= len(b) and a != b[len(b) - len(a):]:
                    raise Reject("suffix")
        # §5 source selection
        best = min(range(len(rs)), key=lambda i: (-len(chron[i]), rs[i].replica, rs[i].stage, rs[i].shard, rs[i].tensor.encode()))
        chosen[ident] = (scale, inv, chron[best])
    # §6 targets: duplicates, zero length
    for i in range(len(targets)):
        if targets[i].history_length == 0:
            raise Reject("zero target history")
        for j in range(i + 1, len(targets)):
            a, b = targets[i], targets[j]
            if a.tensor == b.tensor and a.role == b.role:
                if a.stage == b.stage and a.shard == b.shard:
                    raise Reject("duplicate placement")
                if a.logical_begin < b.logical_end and b.logical_begin < a.logical_end:
                    raise Reject("overlap")
    # §7 coverage (index level) and §8–§9 states
    intervals: Dict[Tuple[str, str], List[Tuple[int, int]]] = {}
    for r in records:
        intervals.setdefault((r.group, r.role), []).append((r.logical_begin, r.logical_end))
    states = []
    for t in targets:
        if not t.active:
            continue
        ivs = sorted(intervals.get((t.group, t.role), []))
        pos = t.logical_begin
        for lo, hi in ivs:
            if hi <= pos:
                continue
            if lo > pos:
                break
            pos = max(pos, hi)
            if pos >= t.logical_end:
                break
        if pos < t.logical_end:
            raise Reject("coverage")
        for k in t.blocks():
            ident = (t.group, t.role, k)
            if ident not in chosen:
                raise Reject("coverage")  # unreachable when index coverage holds, kept for safety
            scale, inv, s = chosen[ident]
            L = t.history_length
            hist = s[len(s) - L:] if L <= len(s) else [0] * (L - len(s)) + s
            states.append(State(t.tensor, t.role, t.stage, t.shard, k, scale, inv, hist, 0))
    states.sort(key=State.key)
    return states


# ------------------------------------------------------------ CPU model §2
def pow2ceil(x: int) -> int:
    p = 1
    while p < x:
        p *= 2
    return p


def step(state: State, observed_amax: int, value: int):
    assert 0 <= observed_amax < AMAX_LIMIT and -(1 << 31) <= value < (1 << 31)
    h = list(state.amax_history)
    L = len(h)
    h[state.cursor] = observed_amax
    cursor = (state.cursor + 1) % L
    peak = max(1, max(h))
    inv = pow2ceil(peak)
    scale = (1 << 24) // inv
    prod = value * scale
    q = abs(prod) // (1 << 24)
    q = q if prod >= 0 else -q
    q = max(-448, min(448, q))
    new = State(state.tensor, state.role, state.stage, state.shard, state.global_block, scale, inv, h, cursor)
    return new, q


def trace(state: State, pairs: List[Tuple[int, int]]):
    out = []
    cur = state
    for a, v in pairs:
        cur, q = step(cur, a, v)
        out.append({"amax_history": list(cur.amax_history), "cursor": cur.cursor, "scale": cur.scale,
                    "inverse_scale": cur.inverse_scale, "quantized": q})
    return out


def checkpoint_scale(history: List[int]) -> Tuple[int, int]:
    """The (scale, inverse_scale) the model would hold after that history (generator realism)."""
    inv = pow2ceil(max(1, max(history) if history else 1))
    return (1 << 24) // inv, inv


# --------------------------------------------------------------- documents
def result_json(states: Optional[List[State]]) -> str:
    """Canonical serialisation of result-format-v1.md (mirrors fp8::write_result)."""
    if states is None:
        return '{"status": "reject"}\n'
    lines = ['{', ' "format_version": 1,', ' "status": "ok",', ' "states": [']
    for i, s in enumerate(states):
        lines.append('  {"tensor": %s, "role": "%s", "stage": %d, "shard": %d, "global_block": %d, "scale": %d, "inverse_scale": %d, "amax_history": [%s], "cursor": %d}%s' % (
            json.dumps(s.tensor), s.role, s.stage, s.shard, s.global_block, s.scale, s.inverse_scale,
            ", ".join(str(x) for x in s.amax_history), s.cursor, "," if i + 1 < len(states) else ""))
    lines.append(' ]'); lines.append('}')
    return "\n".join(lines) + "\n"


def parse_result(text: str):
    """Strict reader for restored.json: returns list of State, or None for reject; raises on malformed."""
    doc = json.loads(text)
    if set(doc) <= {"status", "reason"} and doc.get("status") == "reject":
        return None
    if set(doc) != {"format_version", "status", "states"} or doc["format_version"] != 1 or doc["status"] != "ok":
        raise ValueError("shape")
    out = []
    for s in doc["states"]:
        if set(s) != {"tensor", "role", "stage", "shard", "global_block", "scale", "inverse_scale", "amax_history", "cursor"}:
            raise ValueError("state members")
        out.append(State(s["tensor"], s["role"], s["stage"], s["shard"], s["global_block"], s["scale"], s["inverse_scale"], list(s["amax_history"]), s["cursor"]))
    return out


def self_test():
    r1 = Record("w", None, "forward", 4, 0, 4, 0, 0, 0, 1 << 24 >> 3, 8, [5, 7, 1, 3], 2)
    assert chronological(r1) == [1, 3, 5, 7]
    r2 = Record("w", None, "forward", 4, 4, 6, 1, 0, 1, 1 << 24 >> 3, 8, [7, 5], 0)  # block 1, shorter ring, consistent suffix with nothing (different block)
    t = Target("w", None, "forward", 4, 0, 0, 0, 6, 6, True)
    st = restore([r1, r2], [t])
    assert [s.global_block for s in st] == [0, 1]
    assert st[0].amax_history == [0, 0, 1, 3, 5, 7] and st[1].amax_history == [0, 0, 0, 0, 7, 5]
    s2, q = step(st[0], 9, 1 << 30)
    assert s2.amax_history == [9, 0, 1, 3, 5, 7] and s2.cursor == 1 and s2.inverse_scale == 16 and s2.scale == (1 << 24) // 16
    assert q == 448
    # suffix disagreement
    r3 = Record("w", None, "forward", 4, 1, 3, 2, 0, 0, 1 << 24 >> 3, 8, [3, 5, 8], 0)  # chron [3,5,8] vs [1,3,5,7] newest 3 = [3,5,7] -> mismatch
    try:
        restore([r1, r3], [t]); assert False
    except Reject:
        pass
    # coverage gap
    try:
        restore([r1], [t]); assert False
    except Reject:
        pass
    assert parse_result(result_json(st)) == st and parse_result(result_json(None)) is None
    print("fp8 model self-test ok")


if __name__ == "__main__":
    self_test()
