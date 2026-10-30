
//! Recovering autotetraploid parental haplotypes from low depth progeny
//! genotype likelihoods.
//!
//! The crate separates the parts that define the problem from the part that
//! solves it:
//!
//! - [`cross_model`] holds the domain types. An inheritance state is an
//!   unordered pair by construction.
//! - [`cross_input`] reads and validates `cross.json`.
//! - [`scoring`] is the authoritative scorer. It defines the objective.
//! - [`digest`] computes the canonical digest.
//! - [`result_output`] encodes and decodes `result.json`.
//! - [`solver`] is the search, and is the part left to write.
//!
//! `spec/` carries the normative description of all of it.

pub mod cross_input;
pub mod cross_model;
pub mod digest;
pub mod json;
pub mod result_output;
pub mod scoring;
pub mod solver;
