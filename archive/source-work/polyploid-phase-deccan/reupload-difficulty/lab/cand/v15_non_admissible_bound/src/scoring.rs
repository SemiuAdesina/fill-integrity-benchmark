
//! The authoritative scorer.
//!
//! This module implements `spec/scoring-model.md` and is frozen: it defines the
//! objective, it is not part of the search you are asked to write, and the
//! graded run scores answers with its own pristine copy of this file. Editing
//! it changes what your build measures and nothing about what is graded.
//!
//! Accumulation order is fixed by the specification. Every sum here runs
//! progeny ascending, then marker ascending, and within a marker takes the
//! emission term, then parent A's double reduction term, then parent B's, then
//! parent A's transition term, then parent B's. Reordering these sums stays
//! within the documented `1e-8` tolerance but is not bit identical, so the
//! order is part of the contract rather than an implementation detail.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, Reconstruction, State};

/// Absolute tolerance for comparing two log likelihoods, from the specification.
pub const SCORE_TOLERANCE: f64 = 1e-8;

/// Why a reconstruction could not be scored.
///
/// These are structural rejections. None of them is a low score: a
/// reconstruction that trips any of them is not on the search space at all.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ScoreError {
    /// A haplotype row, path or state list had the wrong length.
    ShapeMismatch(String),
    /// A haplotype value was neither 0 nor 1.
    AlleleOutOfRange { parent: &'static str, homolog: usize, marker: usize },
    /// A parent's alternate allele count disagreed with `parent_dosage`.
    DosageViolation { parent: &'static str, marker: usize, found: u8, required: u8 },
    /// A double reduction state appeared where that parent forbids it.
    IllegalDoubleReduction { parent: &'static str, progeny: usize, marker: usize },
}

impl std::fmt::Display for ScoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScoreError::ShapeMismatch(what) => write!(f, "shape mismatch: {what}"),
            ScoreError::AlleleOutOfRange { parent, homolog, marker } => write!(
                f,
                "parent {parent} homolog {homolog} marker {marker} is not a 0/1 allele"
            ),
            ScoreError::DosageViolation { parent, marker, found, required } => write!(
                f,
                "parent {parent} marker {marker} carries {found} alternate alleles but \
                 parent_dosage requires {required}"
            ),
            ScoreError::IllegalDoubleReduction { parent, progeny, marker } => write!(
                f,
                "progeny {progeny} uses a double reduction state from parent {parent} at \
                 marker {marker}, where it is not allowed"
            ),
        }
    }
}

impl std::error::Error for ScoreError {}

/// Checks a reconstruction's structure against the cross.
///
/// Called by `score` before any arithmetic. Exposed separately so a solver can
/// check a candidate without paying for the full sum.
pub fn validate(cross: &Cross, answer: &Reconstruction) -> Result<(), ScoreError> {
    for parent in Parent::BOTH {
        let haps = answer.haplotypes(parent);
        if haps.markers() != cross.markers {
            return Err(ScoreError::ShapeMismatch(format!(
                "parent {} has {} markers, expected {}",
                parent.label(),
                haps.markers(),
                cross.markers
            )));
        }
        for h in 0..4 {
            for m in 0..cross.markers {
                if haps.row(h)[m] > 1 {
                    return Err(ScoreError::AlleleOutOfRange {
                        parent: parent.label(),
                        homolog: h,
                        marker: m,
                    });
                }
            }
        }
        let required = &cross.params(parent).dosage;
        // Bounded by the cross's marker count, not by the length of the dosage
        // vector, so the cross stays the authority on the shape. The reader
        // rejects a cross whose dosage vector is not `markers` long, so the two
        // agree; iterating the vector instead would move that authority.
        #[allow(clippy::needless_range_loop)]
        for m in 0..cross.markers {
            let found = haps.dosage_at(m);
            if found != required[m] {
                return Err(ScoreError::DosageViolation {
                    parent: parent.label(),
                    marker: m,
                    found,
                    required: required[m],
                });
            }
        }
    }

    if answer.paths.len() != cross.offspring {
        return Err(ScoreError::ShapeMismatch(format!(
            "{} paths, expected {}",
            answer.paths.len(),
            cross.offspring
        )));
    }

    for (c, path) in answer.paths.iter().enumerate() {
        for parent in Parent::BOTH {
            let states = path.states(parent);
            if states.len() != cross.markers {
                return Err(ScoreError::ShapeMismatch(format!(
                    "progeny {c} parent {} has {} states, expected {}",
                    parent.label(),
                    states.len(),
                    cross.markers
                )));
            }
            let allowed = &cross.params(parent).dr_allowed;
            for (m, state) in states.iter().enumerate() {
                if state.is_double_reduction() && !allowed[m] {
                    return Err(ScoreError::IllegalDoubleReduction {
                        parent: parent.label(),
                        progeny: c,
                        marker: m,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Total log likelihood of a reconstruction under the cross.
///
/// Returns an error rather than a low score when the reconstruction is not
/// structurally admissible.
pub fn score(cross: &Cross, answer: &Reconstruction) -> Result<f64, ScoreError> {
    validate(cross, answer)?;

    let mut total = 0.0f64;
    for c in 0..cross.offspring {
        let path = &answer.paths[c];
        for m in 0..cross.markers {
            let dosage = progeny_dosage(answer, path.from_a[m], path.from_b[m], m);
            total += cross.child_gl[c][m][dosage as usize];

            for parent in Parent::BOTH {
                total += double_reduction_term(cross, parent, path.states(parent)[m], m);
            }
            if m + 1 < cross.markers {
                for parent in Parent::BOTH {
                    let states = path.states(parent);
                    total += transition_term(cross, parent, states[m], states[m + 1], m);
                }
            }
        }
    }
    Ok(total)
}

/// Alternate allele dosage a progeny carries at one marker.
///
/// A double reduction state contributes the same homolog twice, which is the
/// intended reading of receiving that homolog twice.
pub fn progeny_dosage(
    answer: &Reconstruction,
    from_a: State,
    from_b: State,
    marker: usize,
) -> u8 {
    let a = answer.haplotypes_a.row(from_a.low() as usize)[marker]
        + answer.haplotypes_a.row(from_a.high() as usize)[marker];
    let b = answer.haplotypes_b.row(from_b.low() as usize)[marker]
        + answer.haplotypes_b.row(from_b.high() as usize)[marker];
    a + b
}

/// Double reduction contribution for one parent at one marker.
///
/// Both branches contribute. A marker where the progeny did not double reduce
/// still carries `ln(1 - rate)`; dropping that term inflates every double
/// reduction state by the same amount and changes which path wins.
pub fn double_reduction_term(cross: &Cross, parent: Parent, state: State, marker: usize) -> f64 {
    let rate = cross.params(parent).dr_rate[marker];
    if state.is_double_reduction() {
        rate.ln()
    } else {
        (1.0 - rate).ln()
    }
}

/// Transition contribution for one parent across the interval starting at
/// `marker`.
pub fn transition_term(
    cross: &Cross,
    parent: Parent,
    from: State,
    to: State,
    marker: usize,
) -> f64 {
    let r = cross.params(parent).recombination[marker];
    let k = f64::from(from.switch_count(to));
    k * r.ln() + (2.0 - k) * (1.0 - r).ln()
}

/// True when two totals are equal within the documented tolerance.
pub fn scores_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= SCORE_TOLERANCE
}
