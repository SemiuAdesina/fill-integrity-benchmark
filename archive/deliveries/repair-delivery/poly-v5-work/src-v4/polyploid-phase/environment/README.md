
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
| `src/solver.rs` | The search. |
| `src/bin/phase.rs` | Binary: reads `cross.json`, writes `result.json`. |
| `examples/` | One small cross with its expected result. |

Every module except `src/solver.rs` is frozen. `src/scoring.rs` in particular
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

To check against the worked example:

```sh
./target/release/phase examples/small.cross.json /tmp/out.json
diff /tmp/out.json examples/small.result.json
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
