# Relocation artifacts (all OLD-CONTRACT, preserved untouched in ~/Downloads)
| path | sha256 (12) | bytes | members | root | mtime | status | contract |
|---|---|---|---|---|---|---|---|
| ~/Downloads/relocation-infer-v1.zip | b5d72f128ac7 | 13685 | 23 | relocation-infer | 2026-08-09 02:08 | SUPERSEDED | OLD-CONTRACT (toy JSON-site library task, V1 in-process harness) |
| ~/Downloads/relocation-infer-v1-trustfix.zip | 95cf72e5f96a | 33316 | 44 | relocation-infer | 2026-08-09 06:17 | SUPERSEDED | OLD-CONTRACT (Class C split of the toy task) |
| ~/Downloads/benchmark-suite-next-final/relocation-infer-v1-final.zip | 63d71ba77184 | 33775 | 46 | relocation-infer | 2026-08-12 15:29 | REFERENCE-ONLY | OLD-CONTRACT (latest toy build; audited NO-GO on 2026-09-08) |
No worktree for the old contract exists under ~/benchmark-suite-work. The accepted proposal (ELF corpus, field-layout recovery, pairs, verifier-owned perturbations) is a new contract; nothing from these ZIPs is patched into the new package. Reused only as architecture reference (Class C verifier layout).

## 2026-09-10 — version 2 after the platform Stage 1 NOT_READY at G02
- v1 `relocation-infer-deccan-stage1-final.zip` sha256 bded3b24092e1951815aa68bbe43df554de639c9e420ed74e093cbe7a1f11472 uploaded 2026-09-10 09:08, Stage 1 stopped at G02: HARBOR.TASK_CONFIG_INVALID ("TB4 requires explicit [agent] timeout_sec = 28800"; "instruction.md must end with a blank line followed by: You have 28800 seconds to complete this task. Do not cheat by using online solutions or hints specific to this task.") plus non-blocking canary warnings. Archived here as `relocation-infer-deccan-stage1-v1-bded3b24-stage1-notready.zip`.
- v2 `~/Downloads/relocation-infer-deccan-stage1-final.zip` sha256 a5f85ce085a19cd6d78d759a3a679172a803654a7daf71985d5d0bb95b348e2f, 217050 bytes, 224 members, root `relocation-infer/`. Changes: `[agent] timeout_sec = 28800`; instruction ends with the mandated sentence; canary comment moved to line 1 of instruction.md; hedge `canary:` comment line added to the six core files. No verifier, fixture, source or spec change. CURRENT FINAL.

## 2026-09-10 — version 3 after Stage 1 CHECK_INCOMPLETE at G05 (v2)
- v2 sha256 a5f85ce0… passed G01–G04; G05 blocked with "unrecognized exception type PermissionError; nop/verifier: [Errno 13] Permission denied: '[HOST_PATH]'". Cause: test.sh re-created reward.txt with chmod 600 (root-owned on the bind-mounted host path), unreadable by Harbor's non-root host process; the two packages that passed G05 never restrict the reward mode. Archived as relocation-infer-deccan-stage1-v2-a5f85ce0-stage1-check-incomplete.zip.
- v3 `~/Downloads/relocation-infer-deccan-stage1-final.zip` sha256 8b3c12d1ce07d762a125f395e5dcf259a057280f5c3ca2fc5a6b9a336513c620. Only change: the chmod 600 line removed (reward keeps mode 644; still removed and re-created after the probe phase). CURRENT FINAL.
