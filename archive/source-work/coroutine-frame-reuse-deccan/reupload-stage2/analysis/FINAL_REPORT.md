# coroutine-frame-reuse: Deccan re-upload, Stage 2 repair + Stage 3 readiness (2026-09-09)
Final ZIP ~/Downloads/coroutine-frame-reuse-deccan-stage1-final.zip
SHA256 c282187f81d1c0d1b3bb9fbc5f350572b9391a66f5b9111ed5a08706099adec0, 98,413 bytes, 51 members, root coroutine-frame-reuse/. NOT UPLOADED.
Source package 7008be8c... preserved at ~/benchmark-suite-work/coroutine-frame-reuse-deccan/archive/stage1-uploaded-7008be8cacc3.zip.
Trajectory export: ~/Downloads/all-trajectories (2).tar (sha 5e178f99...), manifest export_id exp-58bcd09d63d546d892ac, eval s2eval_9a7f5587..., arm hy4_difficulty, 8 attempts verified.
Original population (reward/turns): 01 0/60, 02 0/93, 03 1/82, 04 0/16 OutputTokenExceededError, 05 1/60, 06 0/88, 07 0/79, 08 1/57.
Reconstruction: exact bytes, all eight; baseline replay 0,0,1,0,1,0,0,1 -> 3/8 reproduced.
Difficulty: new hidden family N (4 compound cases); final replay 03=1, 08=1, others 0 -> 2/8.
Long horizon: hint-free instruction, five public diagnostics; risk MEDIUM (see LONG_HORIZON.md).
Security: macro-tamper and overload-hijack holes found and closed; log dir closed; build budgets; 17 controls reward 0; hang completes in 742 s.
Regression: oracle 1 (74/74), NOP 0 (23/74), battery 21/22 wrong variants 0, Class C link line clean, cold fresh extraction oracle 1 / NOP 0 / hang 0, diagnostics ref all pass, starter fails 4 groups.
See POSTMORTEM.md, MATRIX_03_05_08.md, WELL_SPECIFIED_MAPPING.md, LONG_HORIZON.md, battery_v74.txt, exploit_controls_xc.txt, replay_final/RESULTS.txt.
