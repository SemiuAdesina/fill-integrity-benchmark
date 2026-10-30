
# Recover Bluetooth Channel Sounding range calibration

The Rust crate at `/app` implements a deterministic CPU model for synthetic
Bluetooth Channel Sounding. It must recover distance, clock bias, and one phase
bias per antenna path from PBR observations across frequencies and RTT
observations. The current implementation over-relies on RTT and mishandles
wrapped phases.

Repair `/app/src/lib.rs` without changing its public API. The normative model is
`/app/spec/channel-model.md`; its fixed-point arithmetic, phase interval,
candidate bounds, global objective, output ordering, and tie rule are exact.

The result must use both evidence streams globally, distinguish antenna paths,
include all supplied candidate endpoints, return canonical predictions, and
derive those predictions by executing the model. Do not use floating point,
linear unwrapped error, RTT-only fitting, input iteration order, or fixture
answers. Everything is offline. Build with `cargo build --offline`.

Run `cargo test --offline --locked --test diagnostics` for synthetic public
checks of the individual model, boundary, global-scoring and canonical-order
rules. The starter is incomplete, so these checks form a feedback ladder: each
failure names the public rule still violated. They contain no graded fixtures or
hidden expected answers.

The verifier is separate and includes hidden compound cases. You have 7000
seconds.

Public verification groups cover joint PBR/RTT recovery, ambiguity resolution
in both directions, inclusive grids, input-order invariance, full canonical
ties, circular phase boundaries, model-derived predictions, distinct path
identity, exact RTT operation order, invalid distance/path handling, and two
compound cases: one combining wrap, RTT ambiguity, path identity and input
order, and one in which no candidate combination reproduces the evidence
exactly, so the whole residual tuple and the whole returned calibration are
compared. Fixtures and answers remain verifier-owned.
