//! Verifier-owned frozen model for the convex recovery task.
//!
//! This crate is linked by every verifier binary and never contains candidate
//! code. It holds two things and nothing else:
//!
//! * `cases` — the *inputs* of each named case, because the probe has to
//!   receive them in order to run the candidate at all;
//! * `record` — the codec for the raw functional values a probe observed.
//!
//! The types here are the verifier's own copies of the published data contract.
//! They are deliberately independent of whatever the candidate file declares:
//! the probe translates between the two, so a candidate can never hand the
//! judge a value of a type it controls.
pub mod cases;
pub mod record;

/// Exact rational, kept as an unnormalised numerator/denominator pair so the
/// judge can inspect both components exactly as the published contract does.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Rat {
    pub n: i64,
    pub d: i64,
}

/// One supplied plane family: a fixed integer normal with bounded offset choices.
#[derive(Clone, Debug)]
pub struct Family {
    pub normal: [i64; 3],
    pub offsets: Vec<Rat>,
}

/// One exact support measurement in an integer direction.
#[derive(Clone, Debug)]
pub struct Support {
    pub direction: [i64; 3],
    pub value: Rat,
}

/// A complete case input.
#[derive(Clone, Debug)]
pub struct Input {
    pub families: Vec<Family>,
    pub supports: Vec<Support>,
}

/// A selected halfspace, as observed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawPlane {
    pub family: u64,
    pub normal: [i64; 3],
    pub offset: Rat,
}

/// A recovered vertex, as observed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawVertex {
    pub xyz: [Rat; 3],
}

/// A face-to-vertex incidence record, as observed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawFace {
    pub plane: u64,
    pub vertices: Vec<u64>,
}

/// The complete raw functional value a probe observed from one `recover` call.
///
/// Note what is absent: there is no status, verdict, marker, case name, count
/// or summary field anywhere in this structure, and therefore no slot in the
/// serialized form in which a candidate could assert that it passed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawResult {
    pub planes: Vec<RawPlane>,
    pub vertices: Vec<RawVertex>,
    pub faces: Vec<RawFace>,
}
