
//! Restriction mapping of a cloning construct from digest gels.
//!
//! The crate separates the parts that define the problem from the part that
//! solves it:
//!
//! - [`fasta`] reads the backbone record.
//! - [`input`] reads and validates `enzymes.json` and `digests.json`.
//! - [`sites`] finds recognition sites on a circle and maps the backbone's
//!   sites onto the construct for a cloning site and an insert length.
//! - [`digest`] turns cut positions into fragments and bands and holds the
//!   band matching rule. It defines what "reproduces a gel" means.
//! - [`map_model`] is the map document's validity and the objective.
//! - [`result_output`] encodes and decodes `map.json`.
//! - [`solver`] is the search, and is the part left to write.
//!
//! `spec/` carries the normative description of all of it.

pub mod digest;
pub mod fasta;
pub mod input;
pub mod json;
pub mod map_model;
pub mod result_output;
pub mod sites;
pub mod solver;
