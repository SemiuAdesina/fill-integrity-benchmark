# plasmid-map — Stage 1 package report (author-side draft; numbers filled after regression)

**A. Task.** `benchmark-suite/plasmid-map`, Science / Genomics and Computational Biology, 4 h. Built from proposal revision 2 (`analysis/PROPOSAL_REV2.md`, 1990 chars, squared deviation), which is not yet accepted at Stage 0; the package follows polyploid-phase structurally.

**B. Package.** `~/Downloads/plasmid-map-deccan-stage1-final.zip` — sha256 d71196aedbf136d9ac6065fdd44c506a47df4cc2fe20c17102ded569d09c8a99, 142234 bytes, 135 members, root `plasmid-map/`, no symlinks, no author files. Two August toy ZIPs archived under `archive/` by SHA prefix.

**C. Contract.** Three specs: `construct-model-v1.md` (coordinates, sites, torn junction windows, required and insert-region positions, digests, detection limit, bands, sorted-pairing match with ceil(e·b/100), squared deviation), `input-format-v1.md` (three files, every rule, anchored-enzyme guarantee, graded envelope), `map-format-v1.md` (document, validity incl. held-out consistency, the five-part order, determinism). Frozen Rust modules define reproduction and the objective; `src/solver.rs` (+ `src/solver/`) is the edit surface.

**D. Real data.** Vectors: pUC18 L08752.1, pUC19 L09137.2, pBR322 J01749.1, pACYC177 X06402.1, pACYC184 X06403.1, pBluescript II SK(+) X52328.1, pGEX-2T U13850.1, pEGFP-N1 U55762.1. Inserts: restriction fragments of lambda J02459.1, T7 V01146.1, phiX174 J02482.1, M13mp18 X02513.1, E. coli K-12 U00096.3:360000-372000. Enzymes: real Type IIP recognition sequences and offsets. Gels simulated from the true construct with comigration and sizing error. Public problem: pBluescript II SK(+), 4 enzymes.

**E. Reference.** Interval branch and bound by site-count level: band-window branching, order branching for narrow overlapping cuts of different enzymes (incl. coincidence), Bellman-Ford propagation over difference constraints (rank order, insert region, fragment windows, wrap windows with L bounds, visibility-count windows), order-statistics feasibility, assignment (Hungarian) bound, tangent-plane bound with joint pairing enumeration, probe point for early incumbents. Exact on 460/460 synthetic constructs against brute force, on real-data instances against the enumeration solver, and on every graded fixture against an independently written Python branch and bound.

**F. Fixtures (12).** public, h_parsimony (least map has 2 sites, truth 3; deviation-first variant differs), h_sub_detection, h_comigration (merged band), h_junction (cloned into one site of a multi-site enzyme; torn window), h_ties (2 tied maps), h_cloning_sites (4 sites, answer not first), h_three_sites, s_large_a/b/c (5 enzymes, 7 sites; enumeration solver exhausts its cap), h_low_error (1%). Every fixture: reference ≤ 8 s, Python certifier agrees, held-out predictions match the true construct. Truth files never ship.

**G. Verifier.** 18 probe jobs (12 as supplied, 3 repeats, 3 rotations), 13 checks A–M, negative control, certified answers on stdin, fail-closed isolation (runuser/nobody mandatory), budgets 300 s/job, 1200 s phase, 600 s build, 30 s/check, inside 1800 s.

**H. Docker regression (lab/final).** oracle 1 (13/13, control fails, probe phase 17 s); NOP 0 (1/13); population 2/5 (reference, bisection-only pass; enumeration, no-tangent-bound, interval-propagation fail closed on budget); battery 10/14 caught, b02/b04/b07/b13 benign on the corpus, ablations p03/p04 pass; snapshots S0/S1/S3/S4 = 0, S2/S5/S6 = 1; security 22 controls all at 0 (symlink reward, forged held-out, worst map, hang 305 s fail-closed, build FATALs, no_runuser FATAL; prewritten reward with the reference = 1 as expected).

**I. Variant separation.** From the battery: tolerance floor, torn windows kept, sub-detection counted, multiplicity ignored, least count assumed, first cloning site, first map found, deviation before count, wrap dropped, held-out unfiltered are each caught by named checks; the four benign mutations are documented in task.toml and analysis/contract_matrix.md.

**J. TB4.** Agent 28800, closing sentence, schema 1.4, canary on line 1 plus colon form, reward file default mode.

**K. Hint audit.** Scan of pkg/environment and instruction.md for grading, oracle, fixture, bound and search vocabulary: empty. Canary on line 1 of the six core files plus the colon form.

**L. Fresh extraction.** Unzipped tree identical to pkg; all 12 certified answers reproduced by the reference, the 9 non-large ones also by the Python certifier; on the extraction: oracle 1 (14 s), NOP 0, hostile h01 0, hang 0 fail-closed at 305 s, deviation-first variant 0, enumeration 0 fail-closed.

**M. Long horizon.** Rust crate, three specs, an exact search with several coupled parts: expected well above 80 turns (polyploid at 219). **N. Difficulty expectation.** Enumeration and weak-bound searches fail closed on the large fixtures; passing needs sound propagation and a tight bound. Expect at most 2 of 8, not certain. **O. Caveats.** Proposal revision 2 is not yet accepted at Stage 0; the reference's worst case on rejected instances is unbounded but graded fixtures are ≤ 8 s; ties on count and deviation are common (unresolved orderings), so the rotation check grades count and deviation rather than positions. **P. History.** First regression pass in lab/final_pass1; superseded junction fixture in archive/fixtures_superseded. **Q. Where to look.** analysis/contract_matrix.md, author/fixtures/*/certification.json, lab/final. **R. Upload.** Upload the ZIP; submit proposal revision 2 first if not yet done.

**S. READY: YES.**
