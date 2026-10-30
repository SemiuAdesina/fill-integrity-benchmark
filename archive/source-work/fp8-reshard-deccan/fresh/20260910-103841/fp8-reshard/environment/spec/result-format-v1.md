# Result format, version 1

The solver writes exactly one document, `/app/restored.json`.

## 1. Restored document

```
{
  "format_version": 1,
  "status": "ok",
  "states": [
    {
      "tensor": "decoder.layers.3.mlp.fc1.weight",
      "role": "forward",
      "stage": 0,
      "shard": 1,
      "global_block": 2,
      "scale": 16384,
      "inverse_scale": 1024,
      "amax_history": [0, 0, 0, 0, 3, 9, 0, 7],
      "cursor": 0
    }
  ]
}
```

* `states` holds exactly one entry per active target and block
  (`restoration-semantics-v1.md`, sections 6 and 9), in the order of
  section 10, and nothing else.
* `amax_history` has exactly the target's `history_length` entries, oldest
  first; `cursor` is `0`.
* All numbers are JSON integers in decimal. Object members are exactly those
  shown, in any order; unknown members are rejected.

## 2. Reject document

```
{"status": "reject"}
```

An optional string member `"reason"` may be added for the reader's benefit;
it is ignored by the verifier. No other member is permitted.

## 3. Rejection of the document itself

A document that is not valid JSON, that has neither the shape of section 1
nor of section 2, that lists a state for an inactive target or a missing
block, that omits a required state, that repeats a state, that is out of
order, or whose history lengths or cursors do not follow section 1 is wrong,
whatever its contents otherwise are.

## 4. Determinism

Two runs over the same inputs must produce byte-identical documents. The
supplied writer (`fp8::write_result` in `src/model.cpp`) emits the canonical
serialisation shown above; a solver that uses it needs no further care.
