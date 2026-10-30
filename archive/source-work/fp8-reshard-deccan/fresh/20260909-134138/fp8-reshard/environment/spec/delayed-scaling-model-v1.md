# Integer delayed-scaling model, version 1

The frozen CPU model of the quantizer that consumes restored state. It is
supplied in the crate (`fp8::step` in `src/model.cpp`) and the verifier
replays it on restored states to check the full next-state trace. The solver
does not implement it; it defines what a restored state has to be correct
*for*. All arithmetic is over exact integers.

## 1. State

A quantizer state is `(scale, inverse_scale, amax_history[0..L-1], cursor)`
with `L ≥ 1` and `0 ≤ cursor < L`. In a restored state the history is
oldest-to-newest and `cursor = 0`.

## 2. Step

`step(state, observed_amax, value)` with `0 ≤ observed_amax < 2^20` and
`-2^31 ≤ value < 2^31`:

1. `amax_history[cursor] = observed_amax`; `cursor = (cursor + 1) mod L`.
2. `peak = max(amax_history)`; if `peak < 1` then `peak = 1`.
3. `inverse_scale` = the smallest power of two that is `≥ peak`.
4. `scale = 2^24 / inverse_scale` (exact: `inverse_scale ≤ 2^20`).
5. `quantized = trunc(value × scale / 2^24)`, truncating toward zero, then
   saturated to `[-448, 448]`.

The new state is `(scale, inverse_scale, amax_history, cursor)`; the step's
trace entry is that state together with `quantized`.

## 3. Trace

Replaying a sequence of `(observed_amax, value)` pairs from a restored state
produces one trace entry per pair. Two restored states are behaviourally
equal only if every entry of every replay agrees; the ring order and the
cursor therefore matter beyond the newest sample.

## 4. Relation to checkpoint records

The `scale` and `inverse_scale` of a record are the values the model held at
checkpoint time. Restoration copies them; it never recomputes them from a
resized history.
