# Checkpoint and target formats, version 1

This document defines the two input documents, `/app/checkpoint.json` and
`/app/target.json`. Together with `restoration-semantics-v1.md`,
`delayed-scaling-model-v1.md` and `result-format-v1.md` it is the complete
public contract. All numbers in both documents are JSON integers written in
decimal; strings are UTF-8 without escapes other than the standard JSON
escapes.

## 1. Logical layout

A tensor is a one-dimensional array of **logical indices** `0 .. length-1`.
Delayed-scaling calibration state is kept per **block**: for logical index
`i` and the tensor's `block_size` `B`, the block is
`global_block = floor(i / B)`. Blocks are numbered from 0; the last block of a
tensor may be shorter than `B`.

A **placement** is one physical location in a distributed job: `stage`
(pipeline stage), `shard` (tensor-parallel shard) and, for source records,
`replica` (data-parallel replica). Placement is not identity: the state of a
block is the same state wherever it was stored.

Tensors may belong to a **tie group**: tensors that share parameters (an
embedding tied to an output projection, for instance) and therefore share
calibration state. Members of one tie group have the same `block_size` and
the same logical length, and their blocks are identified by the group name.

## 2. `/app/checkpoint.json`

```
{
  "format_version": 1,
  "records": [
    {
      "tensor": "decoder.layers.3.mlp.fc1.weight",
      "tie_group": null,
      "role": "forward",
      "block_size": 1024,
      "logical_begin": 2048,
      "logical_end": 2560,
      "replica": 0,
      "stage": 1,
      "shard": 2,
      "scale": 16384,
      "inverse_scale": 1024,
      "amax_history": [3, 9, 0, 7],
      "cursor": 2
    }
  ]
}
```

One record is the delayed-scaling state of one block as stored by one
placement. Fields:

| field | type | meaning |
|---|---|---|
| `tensor` | non-empty string | the tensor the placement stored |
| `tie_group` | string or `null` | the tensor's tie group, if any |
| `role` | `"forward"` or `"backward"` | forward-activation or backward-gradient quantizer; the two never share state |
| `block_size` | integer ≥ 1 | the tensor's block size `B` |
| `logical_begin`, `logical_end` | integers, `0 ≤ begin < end` | the half-open interval of logical indices the placement stored **within this block**: `floor(begin / B) = floor((end - 1) / B)` |
| `replica`, `stage`, `shard` | integers ≥ 0 | the placement |
| `scale`, `inverse_scale` | integers ≥ 1 | the block's scale pair at checkpoint time |
| `amax_history` | array of integers in `[0, 2^20)` | the amax ring buffer, in storage order |
| `cursor` | integer ≥ 0 | the ring's next-write slot |

A record's block is `floor(logical_begin / block_size)`. A placement that
stored a shard spanning several blocks appears as one record per block. Two
records may describe the same block from different placements, with
different intervals inside the block and rings of different lengths.

## 3. `/app/target.json`

```
{
  "format_version": 1,
  "targets": [
    {
      "tensor": "decoder.layers.3.mlp.fc1.weight",
      "tie_group": null,
      "role": "forward",
      "block_size": 1024,
      "stage": 0,
      "shard": 1,
      "logical_begin": 0,
      "logical_end": 3000,
      "history_length": 8,
      "active": true
    }
  ]
}
```

One target is one shard interval of the new topology. Fields:

| field | type | meaning |
|---|---|---|
| `tensor`, `tie_group`, `role`, `block_size` | as above | the tensor, its tie group, the quantizer role, the block size |
| `stage`, `shard` | integers ≥ 0 | the new placement |
| `logical_begin`, `logical_end` | integers, `0 ≤ begin < end` | the interval the new placement holds; it may span any number of blocks and need not align to block boundaries |
| `history_length` | integer ≥ 0 | the ring length the new placement uses |
| `active` | boolean | whether the placement participates in the resumed job |

## 4. Well-formedness

Both documents are objects with exactly the members shown. Every record and
every target that names one tensor carries the same `tie_group` and the same
`block_size`; every record and target in one tie group carries the same
`block_size`. A document that violates this section, or that is not valid
JSON, is malformed; the solver writes the reject document
(`result-format-v1.md`, section 3) and nothing else is required of it. Every
graded input is well formed.
