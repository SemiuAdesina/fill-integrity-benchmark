# Delayed-scaling restoration contract

A logical quantizer key is `(tie if nonempty else tensor, role)`. Forward and
backward roles never share state. Physical stage, shard, replica, and checkpoint
record order are not identity. Inactive target modules emit no state.

`cursor` is the next write slot. Chronological history starts at cursor, wraps
once, and ends at the newest sample. Replicas for one key must have equal scale
and inverse scale. Their chronological histories may have different lengths but
the overlapping newest suffix must match. Otherwise restoration rejects. Choose
the longest history, then the lowest `(replica, stage, shard, tensor)` source.

A shorter target retains the newest samples. A longer target left-pads zeros.
Canonical target history is stored oldest-to-newest with cursor zero. Output is
ordered by `(tensor, role, stage, shard)` and duplicate target placements reject.
The supplied CPU step overwrites the cursor, advances with wrap, selects the next
power of two of the maximum absolute history (minimum one) as inverse scale,
sets integer `scale_q = 1024 / inverse`, then truncates `value_q * scale_q / 1024`
toward zero and saturates to `[-127,127]`. CPU-step inputs require
`abs(value_q) <= 1000000000`, keeping the documented integer product exact.
