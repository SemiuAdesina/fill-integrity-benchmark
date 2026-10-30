# Problem format, version 1

`/app/problem.json` describes one collective reduction over a scale-up
fabric together with the telemetry the legacy plan produced. This file,
`fabric-replay-v1.md`, `plan-recovery-v1.md` and `result-format-v1.md` are the
complete public contract. All numbers are JSON integers in decimal; strings
are UTF-8; string comparison anywhere in the contract is bytewise.

## 1. Document

```
{
  "format_version": 1,
  "payload_bytes": 8,
  "sink": "S",
  "sources": [
    {"id": "a", "node": "A", "value": 90},
    {"id": "b", "node": "B", "value": -50}
  ],
  "switches": [
    {"id": "X", "reduction_capable": true, "accumulator_capacity": 16},
    {"id": "T", "reduction_capable": false, "accumulator_capacity": 0}
  ],
  "links": [
    {"id": "a-x", "from": "A", "to": "X", "rate": 8, "latency": 1},
    {"id": "b-x", "from": "B", "to": "X", "rate": 8, "latency": 1},
    {"id": "x-s", "from": "X", "to": "S", "rate": 8, "latency": 1}
  ],
  "telemetry": {
    "link_bytes": {"x-s": 8},
    "completion": 4
  }
}
```

| member | meaning |
|---|---|
| `payload_bytes` | the size of every payload, including every reduced payload |
| `sink` | the id of the sink node, the final reducer |
| `sources` | the endpoint sources: `id` is the origin id of the source's payload, `node` the endpoint node it starts at, `value` its int32 value |
| `switches` | the switch nodes: `reduction_capable` says whether the switch can be selected for reduction, `accumulator_capacity` its accumulator budget in bytes |
| `links` | directed links: `from` and `to` are node ids, `rate` bytes per tick, `latency` ticks |
| `telemetry.link_bytes` | observed total bytes per link id, for the links that were observed; an observed link that carried nothing is listed with `0` |
| `telemetry.completion` | the observed completion tick; absent when completion was not observed |

## 2. Nodes

The node ids are the endpoint nodes named by sources, the switch ids, and the
sink. A node is exactly one of the three kinds. Several sources may start at
the same endpoint node. Links may connect any two distinct nodes.

## 3. Validity

The problem is **invalid**, and the result is the invalid document, when any
of the following holds:

* `payload_bytes < 1`; a link with `rate < 1` or `latency < 0`; a switch with
  `accumulator_capacity < 0`; a source `value` outside the int32 range
  `[-2^31, 2^31 - 1]`;
* `sources` is empty; an `id`, `node`, `from`, `to` or `sink` string is empty;
* two sources share an `id`; two switches share an `id`; two links share an
  `id`;
* a node id belongs to more than one kind: the sink equals an endpoint node
  or a switch id, or a switch id equals an endpoint node;
* a link's `from` or `to` is not a node id, or `from` equals `to`.

Validity is decided before anything else: an invalid problem is never
infeasible or ok. A document that is not valid JSON or does not have the
shape above is malformed; the supplied loader reports it and the solver
writes the invalid document for it as well.
