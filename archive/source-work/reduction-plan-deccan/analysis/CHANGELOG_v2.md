# reduction-plan: version 2 (after the Stage 2 difficulty failure)

Stage 2 on version 1 (ZIP sha256 4f63b197…, 18 fixtures, largest 4 sources
and 24 links): FAIL on difficulty, 7 of 8 rollouts at full credit; long
horizon met (median 151 assistant turns). Every fixture could be solved by
enumerating the path product and the combine subsets, which is exactly what
the version-1 reference did and what the spec promised ("small enough that an
exact search completes in well under a second").

## What changed

- **Six graded-size fixtures** (`s_*`): tiered fabrics of 10–12 sources,
  82–102 links and 14 switches, path products above 10^29 and more than
  10^8 candidate trees (still more than 10^8 after propagating every listed
  link, except the reduction-pinned fabric at 1.7×10^7), telemetry from a near-optimal latent plan (a
  perturbation of the telemetry-free optimum, so the legacy routing differs
  from the optimiser's choice): completion listed (10, 12 sources), completion
  absent (10, 12), reductions pinned by count-1 links downstream of the
  latent's selected switches, and one infeasible by a completion no plan
  reaches. Both exhaustive solvers exhaust the 120 s probe budget on each.
- **A public sample of graded size**, `examples/scale_sample/`, with its
  document, so a candidate can measure a solver on a representative problem.
- **Spec §4 rewritten**: sizes, the 120 s per-problem budget on two cores,
  "cannot all be enumerated", pruning allowed only when it never discards a
  possible minimum. Instruction item 5 states the budget.
- **Reference rewritten** as an exact branch-and-bound (source-by-source
  routing, bound = partial tree replayed with every capable switch reducing
  plus unloaded costs of unrouted sources, telemetry propagation); proof in
  `analysis/bound_proof.md`. The version-1 reference survives as the
  exhaustive development snapshot `S7_exhaustive_final` and in
  `archive/v1-src/`.
- **Four exact searches** certify every graded-size fixture: the C++
  reference, the independently designed C++ `solver_d`, and the Python
  oracles C (`research/bnb.py`) and D (`oracle-b/oracle_d.py`); 1,600
  random small fabrics agree with the exhaustive oracle A
  (`research/validate_cd.py`).
- **Verifier**: 25 jobs, 14 checks (new: `graded_fabrics_recovered_within_budget`),
  isolation fails closed (missing runuser or account = FATAL, reward 0;
  the previous version silently ran the candidate as root in that case),
  judge check budget 30 s so every budget fits the 1800 s timeout.
- **Variants**: regenerated from the new reference; b03/b18/b22 retired
  (shapes the pruned search never produces); b23 (bound without reductions)
  and b24 (bound applied at equality) added.

## What did not change

The frozen proposal, the four public formats, every rule of the model, the
18 small fixtures and their certified answers, the Class C verifier
architecture, the hostile-control suite, TB4 fields (agent 28800, closing
sentence), the canary lines.

## Package

`~/Downloads/reduction-plan-deccan-stage1-final.zip` sha256 `e84981db6a40b9d2721089c65da474e85e59b695addb9fcaccdac573c94f75a1`, 95299 bytes, 132 members, root `reduction-plan/`. Superseded builds archived by SHA prefix: `archive/4f63b197-…` (v1, the Stage 2 FAIL), `archive/cb935a19-…-superseded-refplan.zip` (v2 before the reference-plan note was rewritten).
