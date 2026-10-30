# Restore FP8 delayed-scaling state after resharding

Implement `fp8::restore` in `/app/src/restore.cpp` using the normative contract
in `/app/spec/reshard.md`. Restore by logical quantizer identity across tensor
and pipeline parallel placement changes, preserve forward/backward separation,
validate replicated state, rotate history exactly once from its next-write
cursor, resize it canonically, handle tied tensors and skipped targets, and emit
deterministically ordered target states.

No GPU is involved. Hidden validation runs the supplied CPU delayed-scaling step
on restored state. Grading uses pristine model/build files and only your
`restore.cpp`.

After building, run `/app/build/fp8_demo`. It uses synthetic public inputs and
reports the restored canonical state; use it together with the normative spec
to diagnose identity, cursor rotation, resize, and next-step behavior.
