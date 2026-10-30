//! Relocation recovery for the legacy VX toolchain migration.
//!
//! Frozen modules: `json`, `elf`, `corpus`, `program`, `bits`.
//! Edit surface: `recover` (`src/recover.rs` and any modules under `src/recover/`).

pub mod bits;
pub mod corpus;
pub mod elf;
pub mod json;
pub mod program;
pub mod recover;
