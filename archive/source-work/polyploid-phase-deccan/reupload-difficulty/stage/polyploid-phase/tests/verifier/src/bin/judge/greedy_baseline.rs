
//! The marker-at-a-time solver, implemented so its score can be measured.
//!
//! A fixture is only evidence against locally greedy reconstruction if a
//! locally greedy reconstruction actually scores worse on it. Asserting that
//! requires running one, so the verifier runs one rather than assuming.
//!
//! The greedy solver here chooses each marker's parental allele columns on
//! their own, maximising that marker's emission and double reduction terms
//! summed over progeny, and only then recovers each progeny's best path by
//! exact dynamic programming with the full transition terms. Its weakness is
//! confined to the haplotype choice, which is the point: it is not a strawman
//! that also gets the paths wrong.

use verifier_core::cross_input::Cross;
use verifier_core::cross_model::{Haplotypes, Parent, ALL_STATES, HOMOLOGS, STATES};
use verifier_core::scoring::{double_reduction_term, transition_term};

/// Every allele column consistent with a parent's dosage at one marker,
/// in ascending mask order.
fn columns(cross: &Cross, parent: Parent, marker: usize) -> Vec<[u8; HOMOLOGS]> {
    let dosage = u32::from(cross.params(parent).dosage[marker]);
    (0u8..16)
        .filter(|mask| mask.count_ones() == dosage)
        .map(|mask| [mask & 1, (mask >> 1) & 1, (mask >> 2) & 1, (mask >> 3) & 1])
        .collect()
}

/// Legal joint states at a marker, as indices into `ALL_STATES` for each parent.
fn legal_joint(cross: &Cross, marker: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(STATES * STATES);
    for (ia, sa) in ALL_STATES.iter().enumerate() {
        if sa.is_double_reduction() && !cross.params(Parent::A).dr_allowed[marker] {
            continue;
        }
        for (ib, sb) in ALL_STATES.iter().enumerate() {
            if sb.is_double_reduction() && !cross.params(Parent::B).dr_allowed[marker] {
                continue;
            }
            out.push((ia, ib));
        }
    }
    out
}

/// Picks each marker's columns without reference to any other marker.
fn greedy_haplotypes(cross: &Cross) -> (Haplotypes, Haplotypes) {
    let mut haps_a = Haplotypes::zeros(cross.markers);
    let mut haps_b = Haplotypes::zeros(cross.markers);

    for m in 0..cross.markers {
        let joint = legal_joint(cross, m);
        let mut best: Option<(f64, [u8; HOMOLOGS], [u8; HOMOLOGS])> = None;

        for col_a in columns(cross, Parent::A, m) {
            for col_b in columns(cross, Parent::B, m) {
                let mut here = 0.0f64;
                for c in 0..cross.offspring {
                    let mut top = f64::NEG_INFINITY;
                    for (ia, ib) in &joint {
                        let sa = ALL_STATES[*ia];
                        let sb = ALL_STATES[*ib];
                        let dosage = col_a[sa.low() as usize]
                            + col_a[sa.high() as usize]
                            + col_b[sb.low() as usize]
                            + col_b[sb.high() as usize];
                        let value = cross.child_gl[c][m][dosage as usize]
                            + double_reduction_term(cross, Parent::A, sa, m)
                            + double_reduction_term(cross, Parent::B, sb, m);
                        if value > top {
                            top = value;
                        }
                    }
                    here += top;
                }
                // Strictly greater, so the first column pair in ascending mask
                // order wins a tie and the result stays deterministic.
                if best.as_ref().is_none_or(|(score, _, _)| here > *score) {
                    best = Some((here, col_a, col_b));
                }
            }
        }

        let (_, col_a, col_b) = best.expect("every dosage admits at least one column");
        for h in 0..HOMOLOGS {
            haps_a.set(h, m, col_a[h]);
            haps_b.set(h, m, col_b[h]);
        }
    }
    (haps_a, haps_b)
}

/// Total log likelihood a marker-at-a-time solver attains on this cross.
pub fn marker_at_a_time_score(cross: &Cross) -> f64 {
    let (haps_a, haps_b) = greedy_haplotypes(cross);

    let mut total = 0.0f64;
    for c in 0..cross.offspring {
        let mut best = vec![f64::NEG_INFINITY; STATES * STATES];
        for m in 0..cross.markers {
            let mut next = vec![f64::NEG_INFINITY; STATES * STATES];
            for (ia, ib) in legal_joint(cross, m) {
                let sa = ALL_STATES[ia];
                let sb = ALL_STATES[ib];
                let dosage = haps_a.row(sa.low() as usize)[m]
                    + haps_a.row(sa.high() as usize)[m]
                    + haps_b.row(sb.low() as usize)[m]
                    + haps_b.row(sb.high() as usize)[m];
                let here = cross.child_gl[c][m][dosage as usize]
                    + double_reduction_term(cross, Parent::A, sa, m)
                    + double_reduction_term(cross, Parent::B, sb, m);

                let arrival = if m == 0 {
                    0.0
                } else {
                    let mut top = f64::NEG_INFINITY;
                    for (pa, pb) in legal_joint(cross, m - 1) {
                        let prior = best[pa * STATES + pb];
                        if prior == f64::NEG_INFINITY {
                            continue;
                        }
                        let moved = prior
                            + transition_term(cross, Parent::A, ALL_STATES[pa], sa, m - 1)
                            + transition_term(cross, Parent::B, ALL_STATES[pb], sb, m - 1);
                        if moved > top {
                            top = moved;
                        }
                    }
                    top
                };
                next[ia * STATES + ib] = arrival + here;
            }
            best = next;
        }
        total += best.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    }
    total
}
