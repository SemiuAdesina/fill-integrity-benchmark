# Bank inference project

The project in `/app` contains a deterministic shared-memory bank interpreter
and an incomplete recovery routine. Edit only `/app/src/recover.cpp`.

Build with `cmake -S /app -B /app/build` followed by
`cmake --build /app/build`. The verifier uses its own pristine interpreter and
build files, and collects only the recovery source.

`/app/build/bank_diagnostics` runs the public rule diagnostics in
`examples/diagnostics.cpp`: one small synthetic instance per published mapping
rule, each failure naming the rule that was violated. They are a public feedback
ladder, not the graded population.
