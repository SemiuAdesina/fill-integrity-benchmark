# Baseline replay of the eight reconstructed final implementations (verifier = Stage-1 ZIP 7008be8c)
slot | reward | passed | failing checks
01 | 0 | 69/70 | b_cancelled_job_never_resumes
02 | 0 | 69/70 | b_cancelled_job_never_resumes
03 | 1 | 70/70 |
04 | 0 | 23/70 | untouched starter (OutputTokenExceededError, no edits)
05 | 1 | 70/70 |
06 | 0 | 69/70 | b_cancelled_job_never_resumes
07 | 0 | 69/70 | b_cancelled_job_never_resumes
08 | 1 | 70/70 |
oracle 1 (70/70); nop 0 (23/70). Baseline 3/8 reproduced: YES (03, 05, 08).
