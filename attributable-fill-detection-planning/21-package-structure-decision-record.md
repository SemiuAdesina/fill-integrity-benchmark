# 21 — Package-structure decision record

Written before implementation, from the evidence actually available: the live Terminal Bench
form (observed), the Tasking Hub companion (4 pages, re-read this session), and the local
task examples. The Official Contributor Guidelines PDF is still absent; this record states
what was derived and from where, and what remains uncertain.

## Documents and examples used

| Source | Title / identity | Revision evidence |
|---|---|---|
| Hub companion | *Tasking hub — official guide, updates & tips*, rustLabs × Deccan, 4 pages | no printed date; SHA-256 `4026aabd…`, downloaded 2026-08-04 21:33 |
| Platform guide | *rustLabs platform guide*, 3 pages | SHA-256 `25633d82…`; account/onboarding/payment only, §06 referenced by the hub for pay |
| Local examples, Pluto | 8 packages under `~/Documents/pluto-tasks` | mtimes 2026-05-20 13:21 → 2026-05-21 00:54 |
| Local examples, Silver | 4 packages under `~/Documents/silver-tasks` | mtimes 2026-05-11 → 2026-05-13 18:05 |
| Live platform | Terminal Bench Evaluation Control Surface, Rust task program, mode `free_text` | observed by the user this session |

Newest example overall: `pluto-tasks/stacked-borrows-rust` (2026-05-21 00:54). Newest Silver:
`task-3` (2026-05-13 18:05).

## Selected structure

```
attributable-fill-detection/
├── task.toml
├── instruction.md
├── environment/
│   ├── Dockerfile
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── spec/
│   └── src/
│   └── tests/
├── solution/
│   └── solve.sh
└── tests/
    ├── test.sh
    └── graded.rs
```

**Supported by all 12 local examples without exception.** Every package, in both families
and across a 10-day spread, has exactly `task.toml`, `instruction.md`, `environment/`,
`solution/solve.sh`, `tests/test.sh` at the ZIP root. This is the single strongest
structural signal available, and the two families agree on it even where they disagree on
everything else.

`environment/spec/` is added on the hub's explicit instruction: *"Mirror schema/spec files
under `environment/spec/`. The Structured Data Schema Clarity check's path discovery misses
`environment/data/`."*

## Conflict resolution

### `task.toml` schema — the significant conflict

Two incompatible schemas exist locally:

| Family | Shape | Timeout location |
|---|---|---|
| Pluto (newer, 05-21) | flat keys + `[resources]` | `[resources].timeout_sec` |
| Silver (older, 05-13) | `[metadata]` `[repo]` `[verifier]` `[agent]` `[environment]` | `[agent].timeout_sec` |

Resolved in favour of the **Silver sectioned schema**, against example recency, because the
hub correction is decisive and sits above examples in the priority order:

> **SET [AGENT].TIMEOUT_SEC = 7200, NOT 14400** — "The Sanity Gate gives a trial a
> 7200-second budget. If `task.toml` sets a longer agent timeout, the gate can never finish
> inside its own budget and fails with *"Timeout after 7200s · Trials: 0"*… **Both merged
> example tasks use 7200.**"

`[agent].timeout_sec` is only expressible in the sectioned schema — the flat Pluto form has
no `[agent]` section at all. The hub also says the merged (accepted) reference tasks use this
key, which makes the sectioned form the one the current platform validates. The newer Pluto
files are therefore treated as the older convention despite their mtimes.

### `subcategory`

The hub is explicit and names both sides:

> "`check-task-fields` and the platform disagree about `subcategory`. The local static check
> requires it in `task.toml [metadata]`; the platform's Rubric then fails
> `TASK_TOML_SCHEMA` calling it unrecognized. **Follow the platform.**"

**Omitted from `task.toml`.** Category and subcategory stay manual upload-form fields
(planning doc 20). The local static check will complain; that is the documented,
expected sacrifice.

### Canary syntax

Per-file-type placement is taken from `pluto-tasks/spsc-loom-rust`, which carries it on line
1 of every text file, by comment syntax:

| Type | Syntax | Line |
|---|---|---|

The shebang exception is not invented — both `solve.sh` and `test.sh` in every Pluto example
put the shebang on line 1 and the canary on line 2. A shebang is only honoured on line 1.

proposal supplies a bare 32-hex token and specifies the instruction.md line verbatim as
`.md`, and the same no-`GUID` form is used consistently across the other types. The
generated `Cargo.lock` is left without a canary, exactly as the examples leave it.

missing it fails Proposal Match and blocks Agent Trials and Cheat Trial.

### Verifier contract

From `tests/test.sh` in the Pluto examples, which agree exactly:

- write `/logs/verifier/reward.txt`, `0` first as a floor
- run the graded suite
- on success only, overwrite with `1`
- exit with the suite's return code

Binary 0.0 / 1.0. This satisfies the NOP-0.0 / oracle-1.0 gate directly.

### Docker and dependencies

The hub is absolute:

> **DON'T APT-INSTALL ANYTHING** — "Sanity fails apt packages installed without version
> pins. Rubric Review (ENVIRONMENT_HYGIENE) fails the next version for pinning them…
> A task that apt-installs anything therefore cannot pass both gates. The workaround that
> works: **no apt at all** — start from a digest-pinned base image."

Every local Rust example violates this — each `apt-get install`s `python3-pytest` because its
verifier is pytest. **So the examples' verifier technology is rejected along with their
apt use.** The graded suite is plain `cargo test`, driven by bash. No Python, no pip, no apt,
nothing to pin outside the image.

Base image digest-pinned to `rust@sha256:540c902e…` (resolved locally from
`rust:1.83-slim-bookworm`, `rustc 1.83.0`).

**Zero third-party crates.** The crate depends on nothing at all: the restart port stores a
`Snapshot` by value rather than serialising it, which removes the last reason for `serde`.
This supersedes planning doc 17's five-crate plan. "Pin dependencies exactly" is then
satisfied in its strongest form — there is nothing to pin, nothing to vendor, and
`cargo build --offline` cannot reach the network because it has no reason to. The trade-off
is that the snapshot is not forced through a serialiser; the restart invariant still binds,
because state must be `Clone` and fully self-contained to survive the engine being dropped.

No `rust-toolchain.toml` is shipped. A channel pin that did not match the image would make
the container download a toolchain, which is precisely the network access being excluded.
The image *is* the pin.

### Hidden-test placement

The agent's crate lives at `/workspace`, built from `environment/`. The root `tests/`
directory is not part of `/workspace` and is only present when the verifier runs — this is
the mechanism every local example relies on. So:

- `environment/tests/` — visible harness and a few sample scenarios, so the agent can
  iterate as `instruction.md` tells them to
- `tests/graded.rs` — the graded V1–V13 suite, copied into the crate by `test.sh` at
  verification time

The fakes are deliberately visible; they are inert test doubles, and hiding them would make
the task guesswork rather than engineering.

## Remaining uncertainties, recorded and continued past

1. **`[repo]` section.** Silver includes it for a checked-out upstream repo. This task ships
   its own source, so it is omitted. Low risk.
2. **`human_solve_time` vs `expert_time_estimate_hours`.** The families use different keys.
   Both are supplied under `[metadata]` — a superfluous key is far cheaper than a missing
   required one.
3. **`reference_plan.md`.** Present in all 4 Silver packages, absent from all 8 Pluto ones.
   Included, since a shared requirement is more likely than a family quirk, and it is inert
   if unrequired.
4. **Rubric-review procedure.** The hub says to run it yourself with an LLM against the 13
   dimensions but does not enumerate them. Self-review is performed against the dimensions
   the hub names by title (`ENVIRONMENT_HYGIENE`, `TASK_TOML_SCHEMA`, `Structured Data Schema
   Clarity`) plus the general quality bar.
5. **Exact static-check command names.** `check-task-fields` is named but not published
   locally. Equivalent checks are implemented directly: schema parse, canary presence per
   file, required-file presence, offline build, fmt, clippy, secret scan.
6. **Reward granularity.** Binary is used, matching every local example. Whether fractional
   reward is supported is unknown and unnecessary.

None of these blocks the build. Each takes the strongest supported interpretation.
