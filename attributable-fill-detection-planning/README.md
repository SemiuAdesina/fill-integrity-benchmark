# attributable-fill-detection — planning workspace

**These are design documents. They are not task-package files.**

The official contributor guidelines PDF is not present on this
machine. Until it is read, the package root layout, required filenames, `task.toml`
schema, verifier format, reference-solution format, environment format, Docker rules,
per-file canary syntax, scoring contract, local validation commands, ZIP layout, and
hidden-test handling are all unknown and must not be guessed.

Nothing in this directory may be renamed into a package file without first reading that
guide. In particular this workspace deliberately contains no file named `task.toml`,
`instruction.md`, `solution/solve.sh`, `tests/test.sh`, or `environment/Dockerfile`, and
no final verifier implementation.

No canary value appears anywhere in this workspace.

## Contents

| Doc | Subject |
|---|---|
| `01-sanitized-source-allowlist.md` | Files admitted from the source repository |
| `02-file-inclusion-justification.md` | Per-file reason for admission |
| `03-exclusion-list.md` | Excluded subsystems, secrets, generated artefacts |
| `04-architecture-map.md` | Sanitized runtime architecture |
| `05-agent-visible-public-api-plan.md` | Frozen public surface |
| `06-balance-source-design.md` | `BalanceSource` boundary |
| `07-order-gateway-design.md` | `OrderGateway` boundary |
| `08-fill-event-source-design.md` | `FillEventSource` boundary |
| `09-state-store-restart-design.md` | `StateStore` and restart snapshots |
| `10-fake-clock-design.md` | Virtual clock |
| `11-deterministic-event-schedule.md` | Schedule representation and driver |
| `12-behavioural-invariants.md` | Invariants I1–I14 |
| `13-verifier-scenario-matrix.md` | V1–V13 |
| `14-partial-fix-rejection-matrix.md` | P1–P10 against V1–V13 |
| `15-starting-broken-state-design.md` | Shipped defect, and the pre-correction |
| `16-reference-solution-design.md` | Solution design, no implementation |
| `17-offline-dependency-plan.md` | Pinned, offline dependency set |
| `18-crate-module-plan.md` | Crate and module layout |
| `19-security-redaction-plan.md` | Secret scan and redaction procedure |
| `20-manual-upload-manifest.md` | Website field values (manual entry) |

## Source repository

`/Users/macbok/Documents/poly-arb-engine` is read-only for the entire duration of this
work. No file there is created, modified, deleted, renamed, formatted, or committed. No
canary is written there. `.env` is never opened.
