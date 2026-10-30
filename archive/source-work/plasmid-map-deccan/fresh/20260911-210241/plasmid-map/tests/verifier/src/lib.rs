
//! The verifier's copy of the frozen model, plus the probe plan and the
//! transform the plan applies. `solver` is deliberately absent: the
//! candidate's search is compiled only into the `probe` binary.

pub mod digest;
pub mod fasta;
pub mod input;
pub mod json;
pub mod map_model;
pub mod probe_plan;
pub mod result_output;
pub mod sites;
