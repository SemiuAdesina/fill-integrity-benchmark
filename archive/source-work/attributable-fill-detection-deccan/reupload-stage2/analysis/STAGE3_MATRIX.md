# Stage 3 editorial matrix (final package)

| criterion | verdict | evidence |
|---|---|---|
| proposal alignment | PASS | same title, category, subcategory, expert time, frozen surface, ports, value types; the accepted contract (attribution, restarts, neutralization) is unchanged; the additions compose its published rules (WELL_SPECIFIED_MAPPING.md) |
| well-specified | PASS | every graded case maps to a section of venue-semantics.md or reconciliation-contract.md; the two clarifying sentences state consequences of rules already present; no hidden rule |
| solvable | PASS | reference passes 53/53 in the container (analysis/final/replay/oracle) and all 24 public diagnostics; three files change; no network, no timing dependence |
| verifiable | PASS | raw observation records compared byte for byte by a verifier-owned judge; expected records generated from the reference and re-verified in the container |
| legitimate difficulty | PASS | difficulty comes from identifier recycling, lost acknowledgements on entry legs, block finality, working quantity and durable state; no timeouts, no volume, no obscurity (a full verifier run takes about two seconds) |
| industrial relevance | PASS | at-least-once drop-copy feeds, identifier recycling, lost acks, block allocations and restart recovery are the daily concerns of an execution reconciler |
| outcome-verified | PASS | judge never links or reads candidate source; only the frozen surface and six ports are driven; two structurally different historical designs (07, 08) pass |
| no hidden rule | PASS | see WELL_SPECIFIED_MAPPING.md; ungraded corners (finalized order re-named by a block; block-closed leg) are deliberately left ungraded or public |
| no artificial difficulty | PASS | starter bytes unchanged; no added files beyond six diagnostic suites; no sleeps, no nondeterminism, no giant inputs; instruction 436 words |
| deterministic | PASS | virtual clock and seeded generator; v13b repeatability; expected records identical across native and container runs |
| reference truth | PASS | reference reproduces all 32 original records byte for byte and the five new ones; passes the six public suites |
| test reviewability | PASS | every scenario is a declarative list of venue actions with a doc comment stating the outcome and the rule; assertions erased at build |
| Class C isolation | PASS | candidate compiled into an unprivileged runner (nobody) from src/*.rs only; judge is a separate shell process comparing files; /tests unreadable to the runner (leak probe) |
| reward integrity | PASS | reward 0 pre-written; log dir closed; reward and report removed and recreated after the candidate phase; reward 1 only from the judge on a regular file; 15 hostile controls score 0 |
| timeout safety | PASS | build budget 300 s, case budget 60 s, phase budget 600 s, fail-closed on first overrun; hang → normal exit in 64 s; build hang → FATAL at 303 s; envelope 900 s |
| runtime margin | PASS | full graded run ≈ 2 s of a 900 s envelope; worst hostile case 303 s |
| metadata truth | PASS | task.toml rewritten to the final package: 53 records (36 scenarios + 16 probes + control), starter fails 22/53, 24 public diagnostics with 9 passing on the starter, 32 variants (28 zero, 4 reported benign with reasons), controls list, replay statement |
| candidate clarity | PASS | instruction states goal, contract location, edit surface, commands, deliverable and offline rules only; source-map is a neutral file map; diagnostics report observable outcomes |
| expert-time plausibility | PASS | the reference change is three files (~330 changed lines); a reconciliation engineer reading two 8 KB documents and iterating against six suites fits ~3 hours; the six historical passers took 25–81 turns with the checklist, and the added axes are two |
