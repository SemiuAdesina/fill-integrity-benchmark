
# Recover autotetraploid parental haplotypes from low-depth progeny likelihoods

Two autotetraploid parents were crossed. Each carries four homologous copies of
one linkage group. Their progeny were sequenced at low depth, which is enough to
say what a progeny's alternate allele dosage plausibly is at a marker, but not
enough to say what it definitely is.

Your job is to recover the parents. Given the read evidence for every progeny,
find the assignment of alleles to the four homologs of each parent, and the
inheritance path of every progeny, that together maximise the total log
likelihood of the observed data.

## What is already written

The crate at `/app` is complete except for one file.

| Module | State | Purpose |
|---|---|---|
| `src/cross_model.rs` | frozen | Domain types. An inheritance state is an unordered pair by construction. |
| `src/cross_input.rs` | frozen | Reads and validates `cross.json`. |
| `src/scoring.rs` | frozen | **The scorer. It defines the objective.** |
| `src/digest.rs` | frozen | Computes the canonical digest. |
| `src/result_output.rs` | frozen | Encodes and decodes `result.json`. |
| `src/json.rs` | frozen | JSON reader and writer. |
| `src/solver.rs` | **yours** | The search. |

`src/solver.rs` currently holds a placeholder. It compiles and produces a
structurally valid answer, and it is not the right answer: it fixes each
parent's alleles marker by marker without reference to any other marker, so it
ignores the recombination structure that links them, and it never revisits a
choice. Replacing it is the task.

Do not modify any frozen module. The scorer in particular is the definition of
correctness, not an implementation detail — changing it changes nothing about
what the right answer is, and the verifier uses its own copy.

## The specification

`/app/spec/` is normative and is the authority on every question of detail.

- `spec/cross-format-v1.md` — the input document and every validation rule.
- `spec/scoring-model.md` — the objective, term by term, and the order in which
  the terms are summed.
- `spec/result-format-v1.md` — the output document, canonical form, how ties
  between distinct optima are broken, and every condition that rejects a result.

Read all three before writing code. Several requirements there are load-bearing
and are stated nowhere else, including in this file.

## Input and output

Input is `/app/cross.json`. Write your answer to `/app/result.json`.

```sh
cargo build --release
./target/release/phase          # reads /app/cross.json, writes /app/result.json
```

`/app/examples/` holds a small worked example with its expected result, so you
can check your understanding of both formats before trusting your search.

## What is required of the answer

1. **Global optimum.** The reported reconstruction must attain the maximum total
   log likelihood over every parental haplotype assignment consistent with
   `parent_dosage` and every legal inheritance state path. Not a good local
   optimum, and not the best of a few restarts. Agreement is to within `1e-8`.
2. **Internally consistent.** `total_log_likelihood` must equal what the scorer
   computes for the haplotypes and paths in the same document. An optimal number
   printed beside a suboptimal reconstruction fails twice over.
3. **Canonical.** Homolog labels are arbitrary. Exactly one relabelling of an
   answer is accepted, and `spec/result-format-v1.md` defines which. Note that
   sorting the haplotype rows does not by itself determine it.
4. **Deterministic.** When two genuinely different reconstructions tie at the
   optimum, the spec names which one is required. Repeated runs on the same
   input must produce byte-identical output.

## Things worth knowing before you start

The search space is large but heavily constrained, and the constraints are what
make an exact answer reachable:

- `parent_dosage` fixes how many homologs carry the alternate allele at each
  marker, so a parent has only `C(4, dosage)` admissible allele columns there —
  at most six, never sixteen.
- Progeny are conditionally independent given the parental haplotypes. Fix the
  haplotypes and each progeny reduces to a small finite-state Viterbi problem
  over the markers.
- Only one term of the objective depends on the parental alleles at all.

Two approaches that look reasonable and are not:

- Hard calling each progeny's dosage from its likelihoods and phasing the calls.
  At these depths the five values per marker frequently do not identify a dosage,
  and calling throws away the evidence that makes the cross solvable.
- Optimising each marker independently. The recombination terms couple adjacent
  markers, so a column that looks best in isolation is often not part of any
  optimal solution.

Double reduction states — where a progeny receives two copies of the same
parental homolog — are legal only where the input says they are, and legality is
per parent and per marker. Both allowing them everywhere and forbidding them
everywhere give wrong answers.

## Constraints

- Rust only, no third-party crates. The environment has no network access.
- Keep the existing crate layout and the `phase` binary's behaviour.
- Your solver must work for any valid input in the declared range, not only for
  the fixture in `/app/cross.json`.
