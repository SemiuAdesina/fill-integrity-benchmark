# Result format, version 1

The solver writes exactly one document, `/app/result.json`.

## 1. The ok document

```
{
  "format_version": 1,
  "status": "ok",
  "paths": [["a-x", "x-s"], ["b-x", "x-s"]],
  "combine": ["X"],
  "sink_value": 40
}
```

* `paths`: one array of link ids per source, in ascending source id, each
  the source's path in link order (`fabric-replay-v1.md`, section 2).
* `combine`: the selected switch ids in ascending order; `[]` when none.
* `sink_value`: the plan's sink value (`fabric-replay-v1.md`, section 5).
* Members are exactly those shown, in any order; unknown members are
  rejected. Numbers are JSON integers in decimal.

## 2. The invalid and infeasible documents

```
{"status": "invalid"}
{"status": "infeasible"}
```

An optional string member `"reason"` may be added to either for the
reader's benefit; it is ignored by the verifier. No other member is
permitted.

## 3. Rejection of the document itself

A document that is not valid JSON, that has none of the three shapes, whose
`paths` count differs from the number of sources, whose paths are not in
ascending source id order or not legal, whose `combine` is not ascending, or
whose `sink_value` is not the replayed value of the plan is wrong, whatever
else it contains.

## 4. Determinism

Two runs over the same problem must produce byte-identical documents. The
supplied writer (`fabric::write_result` in `src/model.cpp`) emits the
canonical serialisation shown above; a solver that uses it needs no further
care.
