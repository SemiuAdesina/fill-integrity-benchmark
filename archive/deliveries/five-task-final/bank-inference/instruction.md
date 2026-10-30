# Recover the shared-memory bank mapping

A small C++20 model under `/app` replays shared-memory bank accesses. Its
bounded grammar supports ordered address-bit XOR stages and an optional lane
rotation. The supplied traces contain partial per-lane bank observations and
conflict degrees. The starter recovery routine returns an unswizzled guess.

Repair `/app/src/recover.cpp` so `bank::recover` returns the legal mapping that
matches all observations globally. Select minimum operation count, then minimum
stage count, then lexicographically smallest `bank::canonical` string. Throw
`std::runtime_error` when no legal mapping exists.

The normative word-boundary, XOR ordering, lane, broadcast, conflict, and tie
semantics are in `/app/spec/mapping.md`. Do not change the public API. The
verifier replaces the interpreter, header, and build configuration with pristine
copies and grades only `src/recover.cpp`.

`examples/diagnostics.cpp` is a public check of one small synthetic instance per
published rule. Build and run it to see which rules an implementation still gets
wrong:

    cmake -S /app -B /app/build && cmake --build /app/build
    /app/build/bank_diagnostics

Every failure names the rule it violated. These fixtures are public and are not
the graded population, which combines the same rules adversarially. Only
`src/recover.cpp` is collected, so helper types and functions are welcome inside
that file, but no new translation unit or build step will reach the verifier.
