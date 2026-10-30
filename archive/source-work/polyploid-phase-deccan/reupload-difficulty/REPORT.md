# polyploid-phase — Stage 2 difficulty re-upload + Stage 3 editorial readiness

Author-side report. Nothing in this file ships inside the task package.

Package under test: `~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty/pkg`
Baseline (currently uploaded, Stage 1 PASS): `~/Downloads/polyploid-phase-deccan-stage1-final.zip`,
SHA256 `9ef21e60727a651d4e2848186c763f455a35c71e2d492dde4e8b416e0bdf474e`, 156318 bytes, 98 members,
root `polyploid-phase/` — located by hash, not by filename, and left untouched.

---

## A. TRAJECTORY FINDINGS

Export `~/Downloads/all-trajectories (1).tar`, evaluation `s2eval_c72be04257e9d15c9c6e63e7084126a6`,
arm `hy4_difficulty`, 8 selected attempts, model `tencent/hy4-preview`. Manifest rewards and turn
counts match the platform summary exactly.

Every attempt was **replayed inside a fresh task container**: each `Write`/`Edit`/`MultiEdit` and every
`Bash` command was re-executed in order against a clean `/app` (build tools shimmed to no-ops, tool
calls that the transcript shows erroring were skipped). The reconstructed `src/solver.rs` and
`src/solver/` were then compiled and run against the real verifier.

**Replay fidelity check:** the local verifier reproduces the platform result for all eight attempts —
slots 01, 05, 07, 08 reward 1; slots 02, 03, 04, 06 reward 0. Every fully-read solver file in each
transcript matches the replayed file, or differs only because the agent wrote to it after that read.

| slot | reward | turns | architecture | exact / heuristic | bound | incumbent | symmetry | canonicalisation | tie handling | fallback | public suites | why it passed / failed |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 01 | 1 | 191 | local ascent → exact B&B → reduced-column filtering | exact, node-budgeted | per-progeny relaxed + iterated column striking | 160–640 coordinate ascents | row-order prune during search | rows → signature → image tuple | tolerance-preserving | node budget `clamp(12e6/N, 2e5..1e6)`, then keeps the local answer | 6/6 + 4/4 | correct on every fixture |
| 02 | 0 | 153 | memetic population + exhaustive shortcut | heuristic core; optimistic bound deliberately removed | none retained | population + strong local search | yes | yes | yes | — | 6/6 | **panics** on `path_order` (`every marker admits a legal state`) → no artifact → 7/13 |
| 03 | 0 | 137 | local search → Lagrangian prices → B&B | exact | subgradient dual + chain-relaxed bound | local search | yes | yes | yes | — | 6/6 | **panics** on `path_order` (`the optimum is attainable`) → no artifact → 7/13 |
| 04 | 0 | 182 | alternating optimisation + B&B + tie-break | exact, budgeted | per-progeny | 6 seeded restarts | yes | yes | yes | search budget | 6/6 + 4/4 | `coupled_ties`: parent B haplotypes not in canonical form → 12/13 |
| 05 | 1 | 108 | B&B with a Lagrangian dual bound | exact | state-dependent prices | alternating restarts | yes | yes | yes | — | 4/4 | correct, but by far the slowest of the eight |
| 06 | 0 | 151 | enumeration → beam → B&B | exact where it closes, beam otherwise | shared relaxation | beam + kicks | yes | yes | yes | beam incumbent when B&B does not close | 6/6 | `coupled_ties`: progeny 1 parent A path differs → 12/13 |
| 07 | 1 | 130 | 512-restart local search + B&B under a 30 s wall-clock cap | exact until the cap | per-progeny (Lagrangian tried, proved flat, dropped) | 512 restarts | yes | yes | yes | 30 s cap → local answer | 6/6 | correct on every fixture |
| 08 | 1 | 132 | local search + B&B + explicit enumeration of tied assignments | exact, no cap | per-progeny (pricing tried and removed as invalid) | alternating | yes | rows → signature → image tuple | enumerates every tied assignment, then takes the documented smallest | none | last recorded `solver_checks` 5/6 at step 24, before its canonicalisation was finished; not re-run afterwards | correct on every fixture |

Corrections to the pre-supplied reading of the trajectories:

* Slots 02 and 03 do **not** fail by producing a worse answer. Both **abort** on `path_order` with a
  failed internal assertion, so no artifact reaches the judge and six checks fail for a missing answer.
* Slot 04's failure is not "did not generalise": it is one specific defect — parent B is reported
  under a non-canonical relabelling on `coupled_ties`. Its scores are right.
* Slot 06's beam/budget logic is not what fails it; it reports a wrong parent A path on `coupled_ties`.
* Slot 05 is not weak. Its dual is real and its answers are right; it is simply the slowest.
* Slot 07 abandoned its Lagrangian after **proving** it flat (`L(λ) ≥ L(0)`), and spent the effort on
  node cost instead. That is sound work, not a dead end.

## B. WHY 4/8 PASSED

The common mechanism: an exact branch and bound over the **shared parental columns**, marker by
marker, carrying every progeny's dynamic programme forward together, with (i) homolog symmetry
quotiented during the search, (ii) canonical form implementing all three published rules — ascending
rows, then least path signature, then least image tuple — and (iii) pruning that preserves ties, so
the documented representative survives.

Their independent differences are real but did not decide the outcome: slot 01 adds iterated column
striking; slot 05 adds a working state-dependent dual; slot 07 relies on a very strong 512-restart
local search behind a wall-clock cap; slot 08 enumerates the tied set explicitly.

**These are not cheats.** They are correct implementations. That fact drives everything below.

## C. HARDENING CHOSEN

### C1. Three new hidden fixtures (the only corpus change)

All three were simulated from the accepted model — Poisson read depth, binomial allele sampling,
Haldane map distances, double reduction admissible only distally and at a rate rising toward the
telomere — and retained under the same generation-and-rejection rule as the rest: the exact oracle
proved the optimum inside a fixed budget, or the cross was discarded.

| | `shared_phase` | `low_information` |
|---|---|---|
| M × N | 12 × 10 | 13 × 10 |
| joint column space | 3.80e14 | 4.23e13 |
| optimum | −188.2917176341 | −182.2582000580 |
| digest | `5cca9b48897fe473` | `56aff2e2954c72aa` |
| reference wall time | 33.4 s | 18.3 s |
| reference nodes expanded | 531 281 | 325 688 |
| root gap (bound − optimum) | 19.99 nats | 12.58 nats |
| initial incumbent gap | 0.000 | 0.000 |
| which bound closes it | the pair-of-progeny relaxation | the pair-of-progeny relaxation |
| distinct optima within 1e-8 | **2** | 1 |
| double reduction in the optimum | no | no |
| repeated homolog rows | no | no |
| recombination events in the optimum | 12 | 0 |
| duplex (dosage 2) parental markers | 11 of 24 | 9 of 26 |
| median recombination fraction | 0.091 | 0.087 |
| marker×progeny cells with < 0.5 nat separation | 49 / 120 | 79 / 130 |

**Third fixture — `unlinked_interval` (M = 12, N = 10, reference 9.6 s).** Added after a second
round of searching. `cross-format-v1.md` permits any recombination fraction with
`0.0 < r <= 0.5`, and **no fixture in the corpus had ever used a value above 0.45**. This cross
carries two intervals per parent at exactly one half — what a marker pair far enough apart on a
sparse map really has. At `r = 0.5` the transition term is `ln(0.5)` whichever states an interval
joins, so a switch costs nothing and the co-optimal set explodes: **55 296 distinct parental
assignments attain the optimum**, and exactly one is the answer the result format names. Every
other fixture in the corpus has at most two. Certified the same way; independent checker PASS;
byte-identical across runs.

**Scientific rationale (author-side).**

* `shared_phase` is an ordinary tightly-linked, duplex-rich linkage group. Eleven of its
  twenty-four parental marker dosages are two, so six allele columns stay admissible at each of
  them, and the median recombination fraction is below a tenth, so a phase choice at one marker is
  still felt many markers away. This is the common case in an autotetraploid mapping population
  where most segregating markers are duplex and the chromosome is short. Its ambiguity is a direct
  consequence of low-depth likelihoods: with several plausible dosages per progeny, the parental
  columns are identified by what all ten progeny jointly imply, not by any single progeny. Its
  optimum is attained by two distinct parental assignments tying within 1e-8, which is the ordinary
  consequence of a symmetric configuration rather than a constructed puzzle.
* `low_information` is the low-depth case pushed to where it actually bites: sixty per cent of its
  marker-progeny cells separate the best dosage from the next by under half a nat. Nothing about it
  is adversarial; it is what a 2.5× mean-depth run looks like.

**Public contract anchor.** Both are ordinary `cross-format-v1` documents inside the accepted ranges
(`12 ≤ M ≤ 28`, `8 ≤ N ≤ 32`), scored by the published model, graded by the same thirteen checks
against the same published ordering rules. No new hidden rule, no new field, no new semantics.

**Why this is not artificial difficulty.** The difficulty is exact phasing under the accepted
likelihood model. Nothing is obscured, nothing is random, no file is padded, no command is required,
and no fixture is retained for its running time — each is retained for an inference property
(shared-column ambiguity; equality retention at scale; low information).

### C2. Verifier hardening: the reward file (defect found during controls)

A control that had not been run before — a candidate solver that writes `/logs/verifier/reward.txt`
from **inside its probe** — showed the currently uploaded package returns **reward 1** while the
judge's own verdict line reads `reward=0` and only 3 of 13 checks pass. The probe was confirmed
running as `uid=65534(nobody)`, i.e. isolation was active; the write still succeeded because the log
directory is a bind mount whose permissions the container cannot rely on.

**This defect is present in the live task** (reproduced against the Stage-1-passing package: `REWARD=1`).

Fix in `tests/test.sh`: the log directory is `chmod 700`-ed (first line only — a bind mount may
ignore it), and the reward file is **rewritten with a zero the moment the probe phase ends**, after
the last candidate process has exited and before anything is graded. The only value that can survive
to the end of a run is one the verifier writes after counting the checks. Verified: the same control
now returns `REWARD=0`, and the oracle still returns 1.

### C3. Reference solution upgraded

`solution/fixed/` gains `warm_start.rs` (local search supplying the first incumbent) and
`pair_bound.rs` (the pair-of-progeny relaxation). Both are exact-search machinery with stated
proofs; neither changes any answer.

Ablation on the two new fixtures and the existing nine (identical digests throughout):

| mechanism | effect |
|---|---|
| pair-of-progeny relaxation | **decisive**: `low_information` 325 688 nodes / 27 s → 4 894 202 nodes / 240 s without it |
| warm-start incumbent | 2×: `low_information` 27 s → 61 s without it |
| Lagrangian prices | **no gain** — bound never improved on the root bound; removed |
| shared-column window bound | **no gain** — identical node counts; removed |
| iterated column filtering | **no gain** — removed no pair on any fixture; removed |
| min over several disjoint pairings | **no gain** — identical node counts; not adopted |

### C4. Second round of searching (after the first report)

Four further axes were tried, on the explicit goal of reaching ≤ 2:

| axis | outcome |
|---|---|
| Deep tree, few progeny (M = 16–26, N = 8) — the per-progeny bound is tightest with few progeny | reference times out past 120 s from M = 20; **uncertifiable** |
| `r = 0.5` unlinked intervals — a legal, realistic, wholly untested boundary | **certifiable and kept** (`unlinked_interval`), 55 296 co-optimal assignments, reference 9.6 s — but slots 01, 05, 07 and 08 all return the required representative on three such crosses |
| Higher tie multiplicity (4–6 unlinked intervals) | the reference times out too — it must visit every tying leaf, so it scales with the co-optimal set exactly as the candidates do. **No separation** |
| Joint double reduction (both parents in a `(x, x)` state at the same marker for the same progeny) — never once exercised by any fixture | 24 crosses simulated with DR admissible chromosome-wide in both parents at the top of the realistic rate range; joint DR **never survived into a maximum-likelihood optimum**. Not constructible without an unrealistic rate |

One measurement worth recording: on the tie-heavy crosses slot 08 runs at **6–10× the reference**
(61 s against 6.2 s) because it enumerates the whole tied set rather than keeping the row-major
smallest incrementally. That is a genuine scaling weakness, but converting it into a failure would
need a fixture sized so that 300 s separates it from the reference — which is the probe budget used
as the difficulty mechanism, and is out.

## D. HARDENING REJECTED

| idea | why rejected |
|---|---|
| Crosses at M = 16–18, N = 12 (these *do* split the families: slots 01/04/06 return suboptimal answers, 02/07 do not) | the exact reference cannot prove their optimum inside 600 s, so they fail the generation-and-rejection rule and Stage 3 solvability |
| High progeny counts (N = 16–32), the regime where the per-progeny relaxation is loosest | reference times out beyond 120 s even with the pair bound and with the pairing count capped; uncertifiable |
| Fixtures selected because the oracle nearly times out | forbidden by the brief and by Stage 3 runtime margin |
| A fixture exercising canonical rule 5 (image tuple) | measured: all four surviving families implement rule 5 correctly, so it would add cost and catch nothing |
| A fixture exercising ordering rule 3 (paths decide between distinct optima) | unreachable by construction: two optima whose canonical matrices agree for both parents canonicalise to the same reconstruction |
| Exploiting slot 07's 30 s wall-clock cap for nondeterminism between the group K replicate runs | measured three runs each on three hard crosses: byte-identical every time. Not a real defect on these instances |
| Lowering the per-probe budget, or adding fixtures to exhaust the phase budget | that is using verifier timeouts as the difficulty mechanism |
| More public diagnostics, or naming the new property in the instruction | would cut investigation turns and reduce long horizon |

## E. TRAJECTORY-DERIVED REGRESSION

Each replayed solver, run against the real verifier, old corpus vs new corpus:

| family | old corpus | new corpus | probe-phase runtime (new) | reason |
|---|---|---|---|---|
| slot 01 — exact B&B + column striking, node-budgeted | 1 (13/13) | **1** (13/13) | 47 s | budget bites, but its 640-restart local search already holds the optimum |
| slot 02 — memetic | 0 (7/13) | 0 (7/13) | — | aborts on `path_order` |
| slot 03 — dual + B&B | 0 (7/13) | 0 (7/13) | — | aborts on `path_order` |
| slot 04 — alternating + B&B | 0 (12/13) | 0 (12/13) | — | `coupled_ties` parent B not canonical |
| slot 05 — Lagrangian dual B&B | 1 (13/13) | **0** (5/13) | 304 s, failed closed | `shared_phase` exceeds the 300 s per-job budget (exit 124); 9/17 artifacts |
| slot 06 — beam + B&B | 0 (12/13) | 0 (12/13) | — | `coupled_ties` parent A path differs |
| slot 07 — local + capped B&B | 1 (13/13) | **1** (13/13) | 52 s | cap bites; its local search holds the optimum |
| slot 08 — B&B + tie enumeration | 1 (13/13) | **1** (13/13) | 155 s | exact, uncapped, slowest survivor but well inside budget |

**Measured: 4/8 → 3/8**, on both the 11-fixture corpus and the final 12-fixture corpus.

The surviving families were additionally run against independently certified crosses spanning
M = 12–15, N = 8–12 and every structural knob (read depth 2.5–5, recombination 0.04–0.20,
duplex-rich and binomial dosage spectra, identity-by-descent in one or both parents):

* slots **01 and 07 on all 39** certified crosses — both reached the certified optimum, in canonical
  form, on every one, with **zero** failures of any kind;
* slots **05 and 08 on a 12-cross subset** — slot 08 always correct, slot 05 correct but repeatedly
  over budget (86 s, 173 s, 194 s, and outright timeouts where the reference took 18–33 s);
* all four on three `r = 0.5` crosses with 31 000–55 000 co-optimal assignments — **all four
  returned the required representative**.

There is no cross inside the reference-certifiable range that separates slots 01, 07 and 08.

### Partial-fix battery (28 variants, complete)

All 28 ran against the enlarged corpus. **Every result matches its old-corpus outcome**, and the
corpus is a strict superset of the old one, so no variant that failed before can now pass.

* 22 variants score **0**: marker-at-a-time columns; the placeholder's fixed guess; set rather than
  multiset intersection; omitting the `ln(1 − rate)` term; a non-admissible bound; pruning branches
  that only tie; keeping the first optimum found (both sibling orders); backward traceback; settling
  both parents marker by marker; skipping canonical form, sorting rows without the signature rule,
  one permutation for both parents, and the permutation applied in the wrong direction — each of the
  last four in its scrambled form; exact float equality; and the hard-coded visible answer.
* 5 score **1** and are the documented benign-at-1 set (v05, v07, v08, v09, v11) — mutations that
  are masked when the search already emits canonical rows and lex-min paths. They are reported, not
  counted, exactly as the metadata says. Re-run against the 12-fixture corpus: still 1.
* **v22c, the deliberately correct control, scores 1** on both the 11- and 12-fixture corpora. This
  is the check that matters most for Stage 3: the new fixtures do **not** reject a correct solver.

## F. STAGE 1

| gate | result | evidence |
|---|---|---|
| G01 archive shape | PASS | single root `polyploid-phase/`, no symlinks, no `.DS_Store`/`__MACOSX`, no build outputs |
| G02 instruction paths | PASS | zero `./` or `../` filesystem paths; line 58 is `/app/target/release/phase`; re-scanned from the extracted copy |
| G03 schema and metadata | PASS | `schema_version = "1.4"`, `name = "benchmark-suite/polyploid-phase"`, category `Science`, subcategory `Genomics and Computational Biology`, `relevant_experience` present, `expert_time_estimate_hours = 4.0` |
| G05 structure | PASS | `environment_mode = "separate"`, verifier 1800 s, agent 7000 s, environment block unchanged |

## G. CONTROLS

| control | result |
|---|---|
| Oracle (reference solution) | **reward 1**, 13/13, control fails as required, 17/17 artifacts, probe phase 76 s |
| NOP (untouched starter) | **reward 0**, 3/13 |
| Negative control | fails as required in every run |
| Hanging candidate | **reward 0** by normal completion — observed live: slot 05, `shared_phase` exit 124 at 300 s, fail-closed, no further candidate code run |
| Hanging build | **reward 0**, `FATAL … exceeded its build budget`, 604 s |
| Reward file pre-written to 1 in `/app` | reward 0 |
| Candidate `Cargo.toml` + `build.rs` writing a reward | reward 0 |
| Solver printing the pass marker | reward 0 |
| **Solver writing `/logs/verifier/reward.txt` and a forged CTRF report from inside its probe** | **reward 0** (was **1** before the fix, and is still 1 in the live package) |
| `src/solver.rs` replaced by a symlink | reward 0, `FATAL candidate solver.rs missing or not regular` |
| Symlink planted under `src/solver/` | reward 0, rc 2 |
| Non-Rust file under `src/solver/` | reward 0, rc 2 |
| Candidate does not compile | reward 0, `FATAL candidate-linked runner did not build` |
| Forged score / forged digest | impossible by construction — a probe artifact carries only haplotypes and paths; the judge recomputes score and digest with its own scorer |

## H. VERIFIER TRUST

* **Class C intact.** Candidate source is compiled only into `probe`, built unprivileged in a sandbox
  containing frozen public model code and cross inputs; the runner refuses to continue if a certified
  answer or judge source appears there.
* **Judge links no candidate code.** Built root-only from a manifest with no probe target, in a tree
  checked to contain no candidate-shaped solver source.
* **Candidate never sees expected answers.** Certified answers are loaded only after the last probe
  has exited, and are delivered on stdin to a process that links no candidate code.
* **Reward authority.** Verifier-owned, and now *provably* so: the reward file is rewritten with zero
  after the probe phase, so a value written by a candidate cannot survive.
* Both populations — 17 probe jobs, 13 graded checks — are still parsed from verifier-owned source,
  never from a candidate-linked binary.

## I. STAGE 2 ASSESSMENT

Old: 4/8 full credit, median 144 assistant turns (108, 130, 132, 137, 151, 153, 182, 191).

New, measured on the same eight implementations: **3/8**.

* **Difficulty risk: HIGH.** The predicate is `count_true ≤ 2`. The hardening removes one family
  (the slow exact solver) and is legitimate, but on this sample it lands at 3, not ≤ 2.
* **Long-horizon risk: LOW.** No candidate-visible byte changed — `instruction.md` and the whole of
  `environment/` (starter, three specs, both public suites, Dockerfile, worked example) are
  byte-identical to the Stage-1-passing package. The run → observe → diagnose → repair loop that
  produced a median of 144 turns is untouched.

No claim is made about what a fresh rollout would produce. A fresh attempt must now additionally be
fast enough on `shared_phase`, which is a genuine extra way to fail, so the expected count is lower
than the observed 3 — but "reliably ≤ 2" is not established by anything I measured.

## J. STAGE 3 EDITORIAL READINESS

| criterion | verdict | evidence |
|---|---|---|
| proposal alignment | PASS | no new phenomenon, scoring term, output field, hidden rule, accepted range, required process or implementation restriction; revision 4 remains literally true |
| scientific realism | PASS | both fixtures simulated from the accepted model with Poisson depth, binomial reads, Haldane map, distal DR gradient; sizes and parameters ordinary for an autotetraploid mapping population |
| well-specified | PASS | every graded behaviour anchored in `instruction.md` or the three specs; the new fixtures exercise existing rules only |
| solvable | PASS | reference proves both new optima (33.4 s, 18.3 s), byte-identical on repeat, inside a 300 s per-job and 1500 s phase budget; oracle scores 1 from a fresh extraction and cold build |
| verifiable | PASS | Class C intact and strictly stronger than the live package (reward forgery closed) |
| difficult for a legitimate reason | PASS *as a property*, INSUFFICIENT *against the gate* | difficulty is exact phasing under the accepted model; but measured 3/8 vs the required ≤ 2 |
| interesting | PASS | the shared-parent coupling that makes the obvious relaxation loose is a real property of the biology |
| outcome-verified | PASS | no method is prescribed; the ablation shows the reference's own extra machinery changes only node counts, and any exact method that returns the documented optimum passes |
| no artificial difficulty | PASS | no padding, obscurity, nondeterminism, starvation, hidden parsing, undecidable edge case or misleading diagnostic; no fixture retained for runtime |
| no hidden rule | PASS | check-to-public-rule matrix below is complete |
| deterministic | PASS | reference byte-identical across runs on all eleven fixtures; group K still grades repeat agreement |
| fixture certification | PASS | see section on certification; exhaustive enumeration is **not** claimed for the two new fixtures and the metadata says so |
| verifier independence | PASS | judge recomputes legality, score, canonical form over all 24 relabellings, and the digest |
| metadata truth | PASS | difficulty, solution and verification explanations rewritten to describe the delivered package, including the new fixtures, the upgraded reference, the timings and the reward-reset |
| test reviewability | PASS | `probe_plan.rs` carries maintainer-facing rationale for both new fixtures, in the language of the science, naming no model, rollout, slot or gate |
| runtime margin | PASS | reference 76 s across 17 jobs against a 1500 s phase budget and 300 s per job; slowest correct family measured at 155 s |

### Hidden-check → public-rule matrix

| verifier behaviour | public rule | public file + section | fixture exercising it |
|---|---|---|---|
| output exists, decodes, holds optimum and own digest | requirement 2, "Internally consistent" | `instruction.md` §What is required; `result-format-v1.md` §`canonical_digest` | all 11 |
| haplotype rows match `parent_dosage` | dosage is a hard constraint | `cross-format-v1.md` §`parent_dosage` | all 11 |
| states are ordered pairs of existing homologs | ten states, `a ≤ b` | `scoring-model.md` §Inheritance states | all 11 |
| no DR where forbidden | legality decided by `allowed` | `scoring-model.md` §2; `cross-format-v1.md` §`double_reduction` | all 11 |
| DR used where optimal | the same section (DR is a real event, not a degenerate encoding) | `scoring-model.md` §2 | `dr_legality`, `compound` |
| answer is the canonical representative | canonical form, steps 1–6 | `result-format-v1.md` §Canonical form | all 11 |
| canonicalisation remaps paths, is idempotent | step 6 and the idempotence sentence | `result-format-v1.md` §Canonical form | all 11 |
| exact global optimum | requirement 1, "Global optimum" | `instruction.md`; `scoring-model.md` §What is being maximised | all 11 |
| marker-at-a-time falls short | grades the fixture, not the agent | (verifier-side precondition) | `greedy_trap`, `compound` |
| recombination across intervals is scored | transition term | `scoring-model.md` §3 | `recombination`, `compound` |
| ties resolve to the documented representative; repeats agree | requirement 4; "Choosing between distinct optima" | `instruction.md`; `result-format-v1.md` | `ties`, `symmetry`, `compound` (+ `coupled_ties`, `path_order`, `shared_phase` via the all-fixture sweeps) |
| compound case still combines all four difficulties | (verifier-side precondition) | — | `compound` |
| answers follow the supplied cross | the objective is a function of the input | `scoring-model.md` | `baseline`, `symmetry`, `compound` shifted |

## K. FILE CHANGES

Added:
* `tests/fixtures/shared_phase.cross.json`, `shared_phase.expected.json` — new certified hidden fixture
* `tests/fixtures/low_information.cross.json`, `low_information.expected.json` — new certified hidden fixture
* `solution/fixed/src/solver/warm_start.rs` — local search supplying the first incumbent
* `solution/fixed/src/solver/pair_bound.rs` — pair-of-progeny relaxation

Changed:
* `tests/verifier/src/probe_plan.rs` — `ALL_FIXTURES` 9 → 11, `PLAN` 15 → 17 jobs, maintainer rationale for the two new crosses
* `tests/test.sh` — job count 15 → 17, certified-answer count 9 → 11, log directory `chmod 700`, **reward file rewritten with zero after the probe phase**
* `solution/fixed/src/solver.rs`, `solver/bounds.rs`, `solver/search.rs`, `solver/progeny_dp.rs` — wire in the warm start and the pair bound; bounds built over column pairs; dosage lookup table
* `solution/reference_plan.md` — describes the incumbent and the third bound, and how the pair relaxation was checked
* `task.toml` — difficulty, solution and verification explanations updated for the delivered package

Unchanged (verified byte-identical): `instruction.md` and the whole of `environment/` — starter,
`spec/cross-format-v1.md`, `spec/scoring-model.md`, `spec/result-format-v1.md`, `tests/diagnostics.rs`,
`tests/solver_checks.rs`, `Dockerfile`, `examples/`, `cross.json`, `Cargo.toml`, `Cargo.lock`.

## L. FINAL ZIP

Built and fully verified, but **deliberately not placed in `~/Downloads`**. Section 35 conditions the
final package on all checks passing, and the Stage 2 difficulty check does not. `~/Downloads` still
holds exactly one `*polyploid*final*.zip` — the currently uploaded Stage-1-passing package,
byte-for-byte (`9ef21e60…`, 156318 bytes). Nothing has been archived, moved or deleted.

```
path:    ~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty/polyploid-phase-deccan-reupload.zip
bytes:   175819
sha256:  4739ebaf1de0c77773aa52a2fdd76cd699e4b6b96e729b8d06530d0be02ca411
members: 106      root: polyploid-phase/      symlinks: 0
```

From a fresh extraction and a cold build of that archive: all 12 fixtures independently re-certified,
oracle **reward 1**, NOP **reward 0**, instruction absolute-path scan clean.

Promoting it is one command: archive `9ef21e60…` into
`~/benchmark-suite-work/polyploid-phase-deccan/archive/` under its SHA, then copy this file to
`~/Downloads/polyploid-phase-deccan-stage1-final.zip`.

## M. FINAL DECISION

**READY FOR REUPLOAD: NO** — against the stated rule, because criterion 1 is not met.

1. Stage 2 hardening is legitimate, but measured **3/8**, not 1–2/8 — on the 11-fixture corpus and
   again on the final 12-fixture corpus, after a second round of searching. By the brief's own test —
   "if all four still obviously pass, hardening is insufficient" — three of the four still pass.
2. The Stage 3 editorial matrix is otherwise entirely PASS, and the package is strictly stronger
   than the live one.

The honest finding behind that verdict: **the four passing implementations are correct exact
solvers**, and every cross that separates them lies outside the range the exact reference can
certify. Fixture-only hardening cannot reach ≤ 2 without either retaining an uncertified fixture or
using the probe budget as the difficulty mechanism — both of which the brief forbids and both of
which would fail Stage 3.

One consideration that points the other way and is the user's call: the re-upload **closes a reward
forgery hole that exists in the live task today** — a candidate that writes `/logs/verifier/reward.txt`
from inside its probe currently scores 1 while failing ten of thirteen checks.

Nothing has been uploaded.
