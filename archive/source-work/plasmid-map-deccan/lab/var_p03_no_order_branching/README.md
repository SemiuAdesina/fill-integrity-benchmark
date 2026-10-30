# plasmid-map

Restriction mapping of a cloning construct from digest gels. The task itself
is described in the instruction you were given; `spec/` is the normative
contract; `src/solver.rs` is the file to write.

```sh
cargo build --release
./target/release/plasmid-map                 # reads /app/{backbone.fasta,enzymes.json,digests.json}, writes /app/map.json
./target/release/plasmid-map DIR OUT         # any instance directory, any output path
cargo test --offline --locked --test diagnostics
cargo test --offline --locked --release --test solver_checks
```
