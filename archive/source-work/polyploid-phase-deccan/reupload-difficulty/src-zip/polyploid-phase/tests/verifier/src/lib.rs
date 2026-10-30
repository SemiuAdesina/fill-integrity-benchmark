
//! Verifier core: the frozen modules, and nothing the candidate wrote.
//!
//! Every module here is the verifier's own copy, taken from the pristine
//! environment source and not from `/app`. A candidate that edited its
//! `scoring.rs` to make its answer look optimal changed nothing that is
//! consulted here.
//!
//! There is deliberately no `solver` module. The candidate's solver is compiled
//! into the `probe` binary only, so the `judge` binary — which holds the run
//! certified answers and the grading checks — links no candidate code at all. The two
//! modules that connect them are [`probe_plan`], which fixes how many times the
//! candidate is executed and on what, and [`probe_artifact`], which is the only
//! format in which a probe's result travels to the judge.

pub mod cross_input;
pub mod cross_model;
pub mod digest;
pub mod json;
pub mod probe_artifact;
pub mod probe_plan;
pub mod result_output;
pub mod scoring;
