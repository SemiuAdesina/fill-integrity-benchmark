# Recover the VX relocation programs for the linker migration

The VX toolchain's linker is being replaced. Its relocation processing was
never documented, and binary compatibility with the objects it linked for
years depends on reproducing it exactly: a relocation that is silently
patched differently links cleanly and fails at run time.

What the migration team has is a corpus: relocatable objects built by the
legacy toolchain, the outputs the legacy linker produced from them, and the
link maps of those links, including controlled perturbations of symbol
values, addends and section bases. From that evidence you must recover, for
every relocation type the corpus uses, a machine-readable relocation program
the replacement linker can execute, and demonstrate it by predicting the
words the legacy linker would produce for links whose output is not
supplied.

## What is in `/app`

| Path | State | Purpose |
|---|---|---|
| `src/json.rs` | frozen | JSON reader and writer. |
| `src/elf.rs` | frozen | ELF64 reader for the corpus subset. |
| `src/corpus.rs` | frozen | Loads the manifest, objects and linked outputs. |
| `src/program.rs` | frozen | Relocation program schema, validation, canonical key, result document codec. |
| `src/bits.rs` | frozen | Container-word and slice helpers. |
| `src/recover.rs` | **yours** | The recovery entry point. |
| `src/recover/` | **yours, optional** | Helper modules used by the recovery. |
| `src/bin/reloc-recover.rs` | frozen | Command-line driver. |
| `src/bin/reloc-diag.rs` | frozen | Public diagnostics. |
| `spec/` | normative | The complete public contract. |
| `corpus/` | input | The corpus (`spec/elf-corpus-v1.md`). |

`src/recover.rs` currently decodes the corpus and emits a placeholder
document that recovers nothing. Replacing it is the task. You may add any
number of Rust modules under `src/recover/`; keep every other source file,
the manifest and the corpus unchanged.

## The specification

`/app/spec/` is the authority on every question of detail:

- `elf-corpus-v1.md` — the corpus files, the manifest, and the exact
  definition of every replay operand.
- `relocation-program-v1.md` — the finite space of relocation programs, their
  normal form, and the published canonical key.
- `replay-semantics-v1.md` — what a program computes and encodes, step by
  step.
- `result-format-v1.md` — the result document and every condition that
  rejects it.

Read all four before writing code. Several requirements there are stated
nowhere else, including in this file.

## Input, output, commands

Input is `/app/corpus/`. Write the result to `/app/relocations.json`.

```sh
cargo build --release
/app/target/release/reloc-recover        # reads /app/corpus, writes /app/relocations.json
/app/target/release/reloc-diag           # compares the result document with the corpus
/app/target/release/reloc-diag --list    # the diagnostic groups; --group NAME runs one
```

The diagnostics report where a result document disagrees with what the
legacy linker recorded — by site, link, object, bit position and pair member.
They hold no answer and evaluate no program; agreement with the supplied
corpus is necessary, not sufficient.

## What is required of the result

1. One program per relocation type, valid under `relocation-program-v1.md`,
   that reproduces every observation of that type across the whole corpus.
2. Where several programs reproduce the observations, the one with the
   smallest published canonical key.
3. A predicted word, or `null` for a rejection, for every site of every link
   in the manifest, equal to what the submitted programs produce under
   `replay-semantics-v1.md`.
4. Byte-identical output on repeated runs over the same corpus.

The recovery is graded on corpora other than the one supplied, with the same
manifest format and the same specification; it must work for any corpus that
conforms to `spec/`, not only for `/app/corpus`.

## Constraints

- Rust only, no third-party crates. The environment has no network access.
- Keep the crate layout and the `reloc-recover` command-line behaviour.
- No system linker, assembler or external tool is available or needed.
