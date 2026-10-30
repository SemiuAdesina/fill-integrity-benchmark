# Reference plan

The reference restoration (`fixed/src/restore.cpp`) follows the specification
section by section, block table first.

1. Validate every record: a non-empty ring and a cursor inside it
   (restoration-semantics-v1.md §2).
2. Group records by identity (tie group or tensor, role, block). Rotate each
   ring once from its cursor into oldest-first order (§3). Require equal scale
   pairs and, for every pair of records, equality of the shorter history with
   the newest suffix of the longer one (§4). Keep the longest history, lowest
   `(replica, stage, shard, tensor)` on ties (§5).
3. Validate targets: zero history length, equal placements, overlapping
   intervals of one tensor and role (§6).
4. For every active target, check index-level coverage against the sorted,
   merged intervals of its identity group and role (§7).
5. Emit one state per active target block: newest samples kept or zeros
   padded on the old side, cursor 0 (§8–§9), then sort by the published order
   (§10).

Any rejection short-circuits to the reject document; the outcome does not
depend on which rule fired first because the document is the same.
