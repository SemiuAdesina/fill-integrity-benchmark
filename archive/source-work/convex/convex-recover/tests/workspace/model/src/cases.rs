//! Verifier-owned case inputs.
//!
//! Only the *inputs* live here, because the candidate-tainted probe has to
//! receive them in order to run `recover` at all. The expected results are held
//! exclusively by the `judge` crate, which the candidate never links and which
//! the graded account cannot read off the filesystem.
//!
//! The fixtures are the frozen V1 fixtures, unchanged: five cases share the
//! seven-family cube evidence set, and `boundedness` uses the same set with the
//! families truncated to five and the support evidence cleared.
use crate::{Family, Input, Rat, Support};

/// Case ids whose evaluation requires executing the candidate `recover`.
///
/// The first seven entries are the complete V1 case population, in the V1 order;
/// the last closes a public-contract gap the V1 fixtures left open, because none
/// of them exercised a non-integer rational. Each entry carries the public
/// behaviour it makes load-bearing and the plausible wrong implementation it is
/// there to reject. The expected values themselves stay in the judge; nothing
/// below states what the answer is.
pub const CANDIDATE_CASES: &[&str] = &[
    // Behaviour: the returned facet set is the minimum nonredundant one for the
    // supplied evidence.
    // Rejects: a solver that keeps every plane compatible with the visible
    // supports, so the looser redundant family survives into the answer.
    "minimum_facets",
    // Behaviour: vertices are exact reduced rational triple intersections.
    // Rejects: floating-point or epsilon-tolerant intersection, which admits the
    // too-tight offset and yields unreduced or near-miss coordinates.
    "exact_vertices",
    // Behaviour: plane-to-vertex incidence is complete and canonically ordered,
    // with a vertex lying exactly on a plane counted as incident.
    // Rejects: incidence built with a strict inequality, or faces emitted in
    // discovery order with unsorted vertex indices.
    "face_incidence",
    // Behaviour: the reconstruction supports a direction that was never supplied
    // as evidence, so the body itself must be right.
    // Rejects: a solver that fits only the visible supports -- curve-fitting the
    // input rather than recovering geometry -- which lands on a body that differs
    // exactly where no support was queried.
    "withheld_support",
    // Behaviour: an unbounded selection is rejected on recession alone.
    // Rejects: omitting the boundedness test. Every plane offered here owns a
    // genuine positive-area facet and the triple intersections are plentiful and
    // distinct, so no other published rule can stand in for it and a solver
    // without recession detection returns a reconstruction instead of nothing.
    "boundedness",
    // Behaviour: exact support equality, coincident triple intersections,
    // redundancy exclusion and canonical ordering all hold at once.
    // Rejects: any solver correct on one rule but not their conjunction -- the
    // deduplication, the exclusion and the ordering are all checked together.
    "compound",
    // Behaviour: minimum facet count first, then lexicographic (family, offset),
    // over selections that are bounded, full-dimensional and all-facet-owning.
    // Rejects: taking the first legal selection; scoring support as an
    // inequality rather than attained equality; dropping both the recession
    // test and the positive-area facet test, since the cheaper
    // support-consistent selection here is caught by either one alone; and
    // resolving the minimum-count tie by anything other than the canonical
    // order.
    "compound_tie",
    // Behaviour: offsets, vertices and supports are exact reduced rationals with
    // positive denominators, so a vertex coordinate may be a true fraction and a
    // coordinate whose canonical form is a whole number still has to be reduced
    // to reach it.
    // Rejects: an integer-only solver, which cannot represent this body at all;
    // dividing the intersection out without reducing, which leaves a coordinate
    // in an unreduced pair; and leaving the determinant sign in the denominator.
    "rational_offset_forces_reduced_coordinates",
];

/// Whole-number rational.
fn r(n: i64) -> Rat {
    Rat { n, d: 1 }
}

/// The frozen cube evidence set: six axis families plus a seventh, looser
/// `x <= 2` family that is compatible with every visible support yet redundant.
/// Family zero additionally offers the too-tight `x <= 0` choice.
fn cube() -> Input {
    let ns = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
        [1, 0, 0],
    ];
    Input {
        families: ns
            .iter()
            .enumerate()
            .map(|(k, &normal)| Family {
                normal,
                offsets: if k == 0 {
                    vec![r(0), r(1)]
                } else {
                    vec![r(if k == 6 { 2 } else { 1 })]
                },
            })
            .collect(),
        supports: ns[..6]
            .iter()
            .map(|&direction| Support {
                direction,
                value: r(1),
            })
            .collect(),
    }
}

/// The unbounded evidence set: an open wedge whose faces are all genuine.
///
/// Every normal here has a non-negative z component, so `(0, 0, -1)` is a
/// recession direction of *every* selection and no subset is bounded. The two
/// slanted cuts trim the top corners from opposite sides, which is what makes
/// this case load-bearing: each of the four vertical planes meets the body in
/// three non-collinear vertices and so owns a positive-area facet, and the
/// triple intersections yield well over four distinct vertices. Boundedness is
/// therefore the only rule that rejects this selection, and a solution that
/// omits the check returns a reconstruction instead of nothing.
fn open_wedge() -> Input {
    let ns = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [1, 1, 1],
        [-1, -1, 1],
    ];
    let offsets = [1, 1, 1, 1, 1, 2, 2];
    Input {
        families: ns
            .iter()
            .enumerate()
            .map(|(k, &normal)| Family {
                normal,
                offsets: vec![r(offsets[k])],
            })
            .collect(),
        supports: Vec::new(),
    }
}

/// The compound-tie evidence set: the cube corner is cut, and four published
/// rules have to interact before a single answer survives.
///
/// Families 0..5 are the axis halfspaces; family 0 additionally offers the
/// too-tight `x <= 0`. Family 5 (`-z <= 1`) is the only lower bound on z.
/// Family 6 offers the real corner cut `x+y+z <= 2` and a looser `x+y+z <= 3`
/// that meets the body in a single point. Family 7 is a different cut,
/// `x+y <= 1`, that reaches the same diagonal maximum. Family 8 is
/// `x+y+z <= 9`, compatible with every support yet redundant.
///
/// The `-z` support is deliberately not queried, so a selection that omits the
/// only lower bound on z still matches every supplied support while using fewer
/// planes than the answer.
fn cut_cube() -> Input {
    let ns = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
        [1, 1, 1],
        [1, 1, 0],
        [1, 1, 1],
    ];
    let offsets: [&[i64]; 9] = [
        &[0, 1],
        &[1],
        &[1],
        &[1],
        &[1],
        &[1],
        &[2, 3],
        &[1],
        &[9],
    ];
    Input {
        families: ns
            .iter()
            .enumerate()
            .map(|(k, &normal)| Family {
                normal,
                offsets: offsets[k].iter().map(|&v| r(v)).collect(),
            })
            .collect(),
        supports: [
            ([1, 0, 0], 1),
            ([-1, 0, 0], 1),
            ([0, 1, 0], 1),
            ([0, -1, 0], 1),
            ([0, 0, 1], 1),
            ([1, 1, 1], 2),
        ]
        .iter()
        .map(|&(direction, value)| Support {
            direction,
            value: r(value),
        })
        .collect(),
    }
}

/// The half-step evidence set: the axis box whose upper halfspace carries a
/// normal of two, so that halfspace bounds the body at a height the integers
/// cannot express.
///
/// All six families are needed for boundedness and each offers a single offset,
/// so the selection itself is forced. That is deliberate: this case is about the
/// arithmetic rather than the choice. Every triple intersection that produces an
/// upper vertex divides by a determinant of two, which leaves two of that
/// vertex's coordinates in a pair that is only canonical after reduction, and
/// the third is a genuine fraction. The one support that is not a whole number
/// is measured in a direction no family supplies.
fn half_step() -> Input {
    let ns = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 2],
        [0, 0, -1],
    ];
    Input {
        families: ns
            .iter()
            .map(|&normal| Family {
                normal,
                offsets: vec![r(1)],
            })
            .collect(),
        supports: vec![
            ([1, 0, 0], Rat { n: 1, d: 1 }),
            ([-1, 0, 0], Rat { n: 1, d: 1 }),
            ([0, 1, 0], Rat { n: 1, d: 1 }),
            ([0, -1, 0], Rat { n: 1, d: 1 }),
            ([0, 0, 1], Rat { n: 1, d: 2 }),
            ([0, 0, -1], Rat { n: 1, d: 1 }),
        ]
        .into_iter()
        .map(|(direction, value)| Support { direction, value })
        .collect(),
    }
}

/// Inputs for a named case. Returns `None` for an unknown id so neither probe
/// can be steered onto an undefined problem.
pub fn case(id: &str) -> Option<Input> {
    match id {
        // Behaviour: the minimum nonredundant facet set, so the looser
        // evidence-compatible plane must be excluded.
        // Rejects: keeping every support-compatible plane.
        "minimum_facets" => Some(cube()),
        // Behaviour: vertices are reduced exact rational triple intersections.
        // Rejects: epsilon or floating-point intersection, which also lets the
        // too-tight offset past the support check.
        "exact_vertices" => Some(cube()),
        // Behaviour: canonical, complete face incidence with on-plane vertices
        // counted as incident.
        // Rejects: strict-inequality incidence, or unsorted faces and indices.
        "face_incidence" => Some(cube()),
        // Behaviour: the support of a direction never supplied as evidence is
        // derived from the returned vertices.
        // Rejects: fitting the visible supports instead of reconstructing the
        // body, which differs precisely where nothing was queried.
        "withheld_support" => Some(cube()),
        // No selection here is bounded, so the answer is nothing. Every plane
        // offered owns a positive-area facet and the triple intersections are
        // plentiful and distinct, so unboundedness is the sole ground for
        // rejection: no other published rule can stand in for it.
        "boundedness" => Some(open_wedge()),
        // Behaviour: equality, coincident triples, redundancy and canonical
        // ordering composed into one case.
        // Rejects: a solver right on any single one of those rules but not on
        // their conjunction.
        "compound" => Some(cube()),
        // Four published rules must interact: a support-consistent selection
        // that omits the only lower bound on z uses fewer planes, and is caught
        // by the recession test and by the positive-area facet test alike; a
        // redundant plane and a too-tight offset are both excluded, the latter
        // only by attained support equality; and two different bounded
        // selections reach the minimum facet count, so the lexicographic
        // (family, offset) order decides between them.
        "compound_tie" => Some(cut_cube()),
        // Behaviour: exact reduced rational arithmetic, with a fractional
        // coordinate and coordinates that are only canonical after reduction.
        // Rejects: integer-only arithmetic, unreduced division, and a
        // determinant sign left in the denominator.
        "rational_offset_forces_reduced_coordinates" => Some(half_step()),
        _ => None,
    }
}
