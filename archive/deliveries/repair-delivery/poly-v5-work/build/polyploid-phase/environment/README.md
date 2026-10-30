
# polyploid-phase

Recovers the haplotypes of two autotetraploid parents, and the inheritance path
of every progeny, from low depth progeny genotype likelihoods.

## Layout

| Path | Contents |
|---|---|
| `spec/` | Normative formats and the scoring model. The authority on detail. |
| `src/json.rs` | Minimal JSON reader and writer. |
| `src/cross_model.rs` | Domain types: parents, inheritance states, haplotypes. |
| `src/cross_input.rs` | Parses and validates `cross.json`. |
| `src/scoring.rs` | The objective. Scores a reconstruction against a cross. |
| `src/digest.rs` | Canonical digest of a reconstruction. |
| `src/result_output.rs` | Encodes and decodes `result.json`. |
| `src/solver.rs` | The search entry point. |
| `src/solver/` | Optional helper modules for the search. |
| `src/bin/phase.rs` | Binary: reads `cross.json`, writes `result.json`. |
| `examples/` | One small cross with its expected result. |

Every module except `src/solver.rs` and Rust modules beneath `src/solver/` is
frozen. `src/scoring.rs` in particular
defines what the right answer is; editing it does not change the answer, it just
makes this crate disagree with the specification.

## Building and running

```sh
cargo build --release
./target/release/phase
```

`phase` reads `/app/cross.json` and writes `/app/result.json` by default. Both
paths can be overridden positionally. It exits non-zero with a message on
standard error if the input is invalid or the answer it produced does not score.

The worked example is deliberately smaller than the accepted task range so it
can be inspected by hand. It illustrates the two document formats and the
canonical representative; the normal input reader correctly rejects it at the
published 12-marker lower bound. Use the public diagnostics below for an
executable check of the supplied mechanics.

The public diagnostics cover the supplied domain types, unordered inheritance
states, multiset transition semantics, relabelling, result encoding and the
canonical digest without revealing or testing a particular search strategy:

```sh
cargo test --offline --locked --test diagnostics
```

## Dependencies

None. The crate is standard library only, including its JSON handling, and the
environment has no network access. `cargo build` works offline because there is
nothing to fetch.

## Scale

`cross.json` holds 12 to 28 markers and 8 to 32 progeny. Both parents are
tetraploid, so each has four homologs and there are ten unordered inheritance
states per parent — a hundred jointly. `spec/cross-format-v1.md` gives the
validation rules in full.
