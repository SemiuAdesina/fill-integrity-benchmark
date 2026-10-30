# polyploid-phase: final semantic-corner hardening pass (2026-09-08)

Final ZIP: ~/Downloads/polyploid-phase-deccan-stage1-final.zip
SHA256: 79fad00686f70c7085b37f9108101db0f43237b6b66c01053d9d3c7da153cb35
179,326 bytes, 108 members, single root polyploid-phase/, no symlinks. NOT UPLOADED.

## A. Candidate corners
Axis | generated | certified | kept
dr0 (asymmetric marker-0 DR) | 96 | 96 | 1 (dr0_s420 -> marker0_dr)
lowinfo (flat / near-flat cells) | 40 | 40 | 1 (lowinfo_s214 -> low_depth)
mixed (dosage / homolog symmetry) | 40 | 40 | 0 (redundant with unlinked_s132 + coupled_ties)
unlinked (r = 0.5 + ties) | 40 | 40 | 1 (unlinked_s132 -> unlinked_ties)
Replaced: ties, recombination (never discriminated any trajectory; retired to lab/retired/).

## B. Certification (all three kept corners)
Two byte-identical reference runs; exhaustive enumeration of the whole assignment space
(3456 / 20736 / 1296 assignments) matches the optimum; independent forward lex-min path
derivation matches; independent spec-derived certifier: score, 24-permutation canonical
audit, digest all PASS (re-run on the packaged copies).

## C. Replay (same eight solvers, local verifier)
slot | old live | prev hardened (12 fx) | final (13 fx) | fixture / reason on final
01 | 1 | 1 | 0 | unlinked_ties: tied optimum, parent A not the required representative
02 | 0 | 0 | 0 | path_order: probe panics, no artifact
03 | 0 | 0 | 0 | path_order: probe panics, no artifact
04 | 0 | 0 | 0 | coupled_ties: parent B not canonical
05 | 1 | 0 | 0 | shared_phase: per-job budget (exit 124), fail-closed
06 | 0 | 0 | 0 | coupled_ties path differs; unlinked_ties progeny 0 path differs
07 | 1 | 1 | 1 | correct exact solver
08 | 1 | 1 | 1 | correct exact solver
Result 4/8 -> 3/8 -> 2/8. Gate count_true <= 2 met. Long horizon untouched.

## D. Regression on the final package
oracle 1 (13/13, 19/19 artifacts), NOP 0 (3/13). Battery 28 variants: 22 wrong -> 0,
control v22c -> 1, v05/v07/v08/v09/v11 -> 1 exactly as documented in task.toml.
Exploit: prewritten reward 0, Cargo/build.rs 0, pass marker 0, symlink solver 0 (FATAL),
symlink under solver/ 0, non-Rust file 0, no-compile 0 (FATAL), reward forged from probe 0,
forged score+digest document 0, malformed document 0. Hanging solver: reward 0, normal
completion in 306 s. Hanging build: reward 0 after 605 s (FATAL).
Cold fresh extraction (no-cache images): preflight 18/18, identical to pkg, oracle 1
(runner_execution 42 s), NOP 0, hang 0 in 304 s.

## E. Runtime
In-image reference: shared_phase 24.4 s, low_information 15.1 s, unlinked_interval 4.6 s,
ten others <= 0.31 s; graded runs 42 s and 57 s of the 1500 s phase budget.
