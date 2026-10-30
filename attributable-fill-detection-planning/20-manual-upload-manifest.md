# 20 — Manual-upload manifest (external website fields)

These values are entered **by you, by hand, on the live Deccan interface**. They are not part
of the ZIP.

I will not automate the website upload, will not upload the ZIP, will not create the task
container, and will not select the category or language on your behalf.

## Confirmed platform context

| Item | Value |
|---|---|
| Surface | Terminal Bench Evaluation Control Surface |
| Project | the Rust task program |
| Action | `New task` |
| Mode | `free_text` |
| Workflow | New task → fresh container → upload creates version 1 → select ZIP → **Create Task And Upload Version** |

## Field values

| # | Field | Value | Status |
|---|---|---|---|
| 1 | Task name | `attributable-fill-detection` | Ready |
| 2 | Programming language | `Rust` | Ready |
| 3 | Task category | — | **UNRESOLVED** |
| 4 | Subcategory | — | **UNRESOLVED** |
| 5 | ZIP file | `/Users/macbok/Downloads/attributable-fill-detection-v1.zip` | Not yet built |
| 6 | `canary_string` | the private accepted-proposal value | Ready, never printed here |

### 3 — Task category — UNRESOLVED

Must be read from the live dropdown at upload time. I have not seen the option list, and
guessing risks a wrong classification on version 1.

When you open the dropdown, the task's shape is: a concurrency- and state-correctness defect
in an async Rust service — fill attribution against an external venue, with restart and
idempotency requirements. If the list contains something like *concurrency*, *distributed
systems*, *state management*, or *debugging*, those are the closest fits in roughly that
order. Tell me the options and I will recommend one.

### 4 — Subcategory — UNRESOLVED

Same reasoning. Also depends on the category chosen, since subcategory lists are usually
filtered by it.

Note deliberately: **subcategory is not being added to `task.toml`** merely because an older
local checker requests it. It is a website field. Whether it *also* belongs in `task.toml` is
a question the Official Contributor Guidelines answer, and they have not been read.

### 6 — `canary_string`

Two destinations, both required:

1. inside every text file of the task package
2. this separate platform upload field

The visible upload page does not display this field. Do **not** conclude it is unnecessary,
and do **not** upload without finding and completing it. If it is not on the main form,
check an advanced/optional section, an edit view after creation, or the version-upload step.

It must **not** be entered into: Task name, Task category, Programming language,
Subcategory, or the ZIP filename.

## What is *not* duplicated inside the ZIP

Because fields 1–5 are website-side, the package does not invent duplicate metadata for
them. Whether `task.toml` independently requires a name, language, or category key is
determined by the official guide's schema — not by mirroring this form.

## Hub-correction limits to keep in view when uploading

| Limit | Value |
|---|---|
| Versions per task | max 10 |
| Active tasks | 2 |
| Evaluations per hour | 2 |
| Unique-version runs per day | 5 |

With 10 versions total and 2 evaluations per hour, a version spent on a guessed category or
a schema-invalid `task.toml` is expensive. This is the concrete reason the build is gated on
the official guide rather than on a best guess.

## Pre-upload checklist

Do not upload until every line is true:

- [ ] Official contributor guidelines PDF located and read
- [ ] Package structure matches that guide, not inferred from Pluto / Silver / Harbor tasks
- [ ] `task.toml` matches the guide's schema, with `[agent].timeout_sec = 7200`
- [ ] Canary present in every text file, in the guide's exact syntax
- [ ] Base image digest-pinned; no `apt` installation
- [ ] Dependencies exactly pinned; `cargo build --offline` and `cargo test --offline` pass
- [ ] Local validation run: **NOP fails, oracle passes**
- [ ] Security scan clean on the built package *and* on the extracted ZIP
- [ ] Schema/spec mirrored under `environment/spec`
- [ ] ZIP built at `/Users/macbok/Downloads/attributable-fill-detection-v1.zip`
- [ ] Original repository re-verified against baselines, unchanged
- [ ] Task category and Subcategory resolved from the live dropdowns
- [ ] `canary_string` field located on the platform

## Current state

Blocked at line 1. Nothing has been uploaded and no ZIP exists.
