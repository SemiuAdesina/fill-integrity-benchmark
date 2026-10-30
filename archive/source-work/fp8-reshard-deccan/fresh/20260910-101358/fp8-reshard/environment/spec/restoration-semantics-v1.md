# Restoration semantics, version 1

This document defines exactly what `/app/restored.json` must contain for a
given checkpoint and target layout. Every rule below is graded; nothing is
graded that is not stated here or in the other three specification files.

## 1. Identity

The identity of a block is `(group, role, global_block)` where `group` is the
tensor's `tie_group` when it is not `null` and the tensor name otherwise. A
target for tensor `T` with tie group `G` is restored from records of **any**
tensor in `G`. Placement (`replica`, `stage`, `shard`) and record order are
never part of identity. Forward and backward roles are distinct identities.

## 2. Validation of the checkpoint

Every record in the checkpoint is validated, whether or not a target needs it:

* **zero history length**: `amax_history` is empty → reject;
* **invalid cursor**: `cursor ≥ length of amax_history` → reject.

## 3. Rotation

For a valid record with ring `h[0..L-1]` and cursor `c`, the
**chronological history** is `h[c], h[c+1], …, h[L-1], h[0], …, h[c-1]`:
oldest sample first, newest sample last. `h[c]` is the oldest sample because
`c` is the slot the next write would overwrite. A ring is rotated exactly
once; the chronological history is what every rule below refers to.

## 4. Consistency of records for one block

For every identity that has at least one record in the checkpoint (again,
whether or not a target needs it), all of its records must agree:

* **scale disagreement**: two records with different `scale` or different
  `inverse_scale` → reject;
* **suffix disagreement**: for two records whose chronological histories have
  lengths `m ≤ n`, the newest `m` samples of the longer history must equal the
  shorter history sample for sample; otherwise → reject. Samples older than
  the shorter history's oldest sample are not compared.

## 5. Source selection

Among the consistent records of an identity, the **source** is the record with
the longest chronological history; among several of that length, the one with
the smallest `(replica, stage, shard, tensor)`, compared component by
component with `tensor` compared bytewise. (By section 4 all longest
histories are identical, so the choice cannot change the output; it is the
documented rule and it is deterministic.)

## 6. Targets

Targets are processed in the following way.

* **duplicate targets**: two targets (active or not) that name the same
  `tensor` and `role` and whose intervals overlap, or that have equal
  `(tensor, role, stage, shard)` → reject;
* **zero history length**: a target with `history_length = 0` → reject;
* an inactive target (`active = false`) produces no state and requires no
  coverage;
* an active target with interval `[b, e)` requires one state per block
  `k = floor(b / B) … floor((e - 1) / B)`.

## 7. Coverage

For every active target, every logical index `i` in `[logical_begin,
logical_end)` must lie inside the interval of at least one record whose
`(group, role)` equals the target's. Records of any placement and any tensor of
the target's tie group count. Otherwise → **incomplete coverage**, reject.

## 8. Resizing

Let the source's chronological history be `s[0..m-1]` (oldest first) and the
target's `history_length` be `L`.

* `L ≤ m`: the restored history is the newest `L` samples, `s[m-L..m-1]`;
* `L > m`: the restored history is `L - m` zeros followed by `s[0..m-1]`
  (zeros are the oldest samples).

The restored history is stored oldest-to-newest with `cursor = 0`, so the
next write overwrites the oldest sample.

## 9. Restored state

For each active target and each of its blocks `k`, the state is:

| field | value |
|---|---|
| `tensor` | the **target's** tensor name (not the tie group) |
| `role` | the target's role |
| `stage`, `shard` | the target's placement |
| `global_block` | `k` |
| `scale`, `inverse_scale` | the values shared by the block's records |
| `amax_history` | the resized history of section 8 |
| `cursor` | `0` |

## 10. Ordering

States are ordered by `tensor` (bytewise), then `role` (`forward` before
`backward`), then `stage`, `shard` and `global_block` numerically.

## 11. Outcome

If any rule in sections 2, 4, 6 or 7 rejects, the output is the reject
document and no state is written; the reason for the rejection is not part
of the graded output. Otherwise the output is the complete ordered list of
states of section 9.
