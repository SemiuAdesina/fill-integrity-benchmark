
## 2026-09-10 — version 4 after client Stage 3 findings on v3 (79fad006…, passed Stages 1 and 2: 2/8, median 130.5)
Findings and fixes (no fixture, expectation, scorer or grading-logic change):
- isolation was conditional (isolated=1 only with runuser + nobody; otherwise candidate-linked code built and ran as root and the chmod on /tests was skipped) → now fails closed: runuser missing, nobody missing, or chmod failure is FATAL before any candidate code is built; control: a verifier image without runuser yields FATAL + reward 0.
- budgets: PROBE_TIMEOUT 300→180, PROBE_PHASE_BUDGET 1500→900, judge 60→30 s per check; worst case 900 + 14×30 < 1800 s verifier timeout; hang control fails closed at 184 s; task.toml text updated to the new numbers.
- three comments (probe/main.rs, judge/main.rs, judge/support.rs) said certified answers were deleted before the first probe; rewritten to the actual mechanism (root-only private workspace, sandbox scanned, delivered to the judge on stdin after the last probe exits).
- instruction module table paths made absolute (/app/src/...).
- TB4: agent timeout 28800, closing sentence, canary hedge lines.
v4: `~/Downloads/polyploid-phase-deccan-stage1-final.zip` sha256 d9dd172074d626506c62d5da1c285dc8b8fb21220d13442356db35d1305a9d1a, 179759 bytes, 108 members, root polyploid-phase/. Fresh extraction: cold builds ok, Oracle 1 (13/13), NOP 0 (3/13), no-runuser control FATAL/0, hang 0 at 184 s. v3 archived here.
