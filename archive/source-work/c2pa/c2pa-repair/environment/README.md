# Synthetic C2PA update repair

This offline Rust crate models a bounded unsigned C2PA update manifest. The
starting `repair` is deliberately greedy. The normative validator and legal edit
interpreter are documented in `spec/update-validation.md`.

## Public feedback ladder

Run `cargo test --offline --locked --test diagnostics`. The diagnostics use
small synthetic manifests that are distinct from the graded fixtures.

| Semantic family | Public diagnostic | Hidden graded coverage | Load-bearing |
| --- | --- | --- | --- |
| canonical edit keys and operation order | yes | yes | yes |
| stable-ID deletion and preserved assertion order | yes | yes | yes |
| asset, cardinality, metadata and prior-store validation | yes | yes | yes |
| every hashed URI and backward label resolution | yes | yes | yes |
| validator-derived reference edges | yes | yes | yes |
| global minimum-cardinality subset | yes | yes | yes |
| equal-cardinality canonical tie | yes | yes | yes |
| unique legal universe and twenty-edit bound | yes | yes | yes |

Diagnostics report the violated public rule by name. They intentionally do not
publish the verifier's compound manifests or expected repair sequences.
