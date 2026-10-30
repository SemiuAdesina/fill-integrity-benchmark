
# Reconstruct a cloning construct's restriction map from digest gels

A cloning core verifies a new construct by restriction mapping before it goes
to sequencing: a known vector was opened at one of its cloning sites and an
insert of unknown sequence and length was ligated in; the construct was then
cut with a panel of enzymes, alone and in pairs, and the fragments were sized
on gels. The gels are all there is.

Your job is the mapping tool. Given the vector's sequence, the enzyme panel,
the candidate cloning sites and the observed bands, recover the construct:
which cloning site was used, how long the insert is, and every cut position
of every panel enzyme on the construct, so that every observed digest is
reproduced within the gel's sizing error. Then predict the bands of the
digests that were held back.

## What is already written

The crate at `/app` is complete except for the search.

| Module | State | Purpose |
|---|---|---|
| `/app/src/fasta.rs` | frozen | Reads the backbone record. |
| `/app/src/input.rs` | frozen | Reads and validates `enzymes.json` and `digests.json`. |
| `/app/src/sites.rs` | frozen | Recognition sites on a circle; where the backbone's sites land on the construct. |
| `/app/src/digest.rs` | frozen | **Fragments, bands and the matching rule. It defines what reproducing a gel means.** |
| `/app/src/map_model.rs` | frozen | **The map's validity and the order that names the answer.** |
| `/app/src/result_output.rs` | frozen | Encodes and decodes `map.json`. |
| `/app/src/json.rs` | frozen | JSON reader and writer. |
| `/app/src/solver.rs` | **yours** | The search entry point. |
| `/app/src/solver/` | **yours, optional** | Helper modules used by the search. |

`/app/src/solver.rs` holds a placeholder that returns the backbone opened at
the first cloning site with the shortest allowed insert and no insert-region
site. It compiles and writes a valid document, and it is not the answer.
Replacing it is the task. Do not modify any frozen module; the verifier uses
its own copies.

## The specification

`/app/spec/` is normative and is the authority on every question of detail.

- `/app/spec/construct-model-v1.md` — coordinates, recognition sites, how
  the backbone's sites move onto the construct, digests, the detection
  limit, bands, the matching rule and its tolerance.
- `/app/spec/input-format-v1.md` — the three input files, every validation
  rule, and what graded problems look like.
- `/app/spec/map-format-v1.md` — the output document, physical validity,
  and the order that names the answer among the maps that reproduce the gels.

Read all three before writing code. Several requirements are stated there
and nowhere else, including in this file.

## Input and output

Input is `/app/backbone.fasta`, `/app/enzymes.json` and `/app/digests.json`.
Write your answer to `/app/map.json`.

```sh
cargo build --release
/app/target/release/plasmid-map          # reads the three inputs, writes /app/map.json
/app/target/release/plasmid-map DIR OUT  # any instance directory, any output path
```

`/app/examples/` holds a small worked example with its expected map, so you
can check your reading of the formats before trusting your search. Two
public test suites are available: the first exercises the frozen model on
hand-built cases; the second runs your `solve` on constructs small enough to
enumerate and holds the answer the specification requires. Agreement there
says nothing about constructs that cannot be enumerated, which is what the
graded run uses.

```sh
cargo test --offline --locked --test diagnostics
cargo test --offline --locked --release --test solver_checks
```

## What is required of the answer

1. **Valid.** The cloning site is one of those listed, the insert length is
   within bounds, every position outside the insert region is one of the
   backbone's surviving sites, and the held-out predictions are what the
   map's own positions produce.
2. **Reproduces every observed digest** under the matching rule, with its
   tolerance and detection limit, exactly as `digest.rs` computes it.
3. **The least such map** under the order in `map-format-v1.md`: fewest
   insert-region sites, then least total squared deviation, then the
   position order, then the site, then the length. Not a close fit, and not
   the first map found.
4. **Deterministic.** Repeated runs on the same input produce byte-identical
   output.

## Things worth knowing before you start

- Every panel enzyme cuts the backbone at least once outside every cloning
  junction, so every enzyme has fixed positions that do not depend on the
  insert; only the backbone sites at or after the opening move with it.
- The gel's sizing error admits many positions for every insert-region cut,
  and the least map is decided by all digests together, not by any single
  digest.
- Fragments shorter than the detection limit never appear, so a gel can
  show fewer bands than there are cuts and the bands can sum to less than
  the construct.
- Graded constructs have up to five enzymes, up to three insert-region sites
  per enzyme, and inserts of up to six kilobases. Each graded problem must
  be answered within 300 seconds; the reference answers each in under ten.

## Constraints

- Rust only, no third-party crates. The environment has no network access.
- Keep the existing crate layout and the `plasmid-map` binary's behaviour.
- Your solver must work for any valid input in the declared range, not only
  for the problem in `/app`.

You have 28800 seconds to complete this task. Do not cheat by using online solutions or hints specific to this task.
