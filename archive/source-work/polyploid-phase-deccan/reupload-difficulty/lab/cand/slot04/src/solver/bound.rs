
//! Upper bounds on the objective, for branch and bound.

use crate::solver::core::{Tables, NJ, NS};

/// Best emission a progeny could possibly get at each marker: the largest of
/// its five dosage likelihoods, summed over progeny and markers.
pub fn emission_ceiling(t: &Tables) -> f64 {
    let mut total = 0.0f64;
    for c in 0..t.offspring {
        for m in 0..t.markers {
            let mut best = f64::NEG_INFINITY;
            for d in 0..5 {
                if t.gl[c][m][d] > best {
                    best = t.gl[c][m][d];
                }
            }
            total += best;
        }
    }
    total
}

/// Best double reduction score: every progeny takes the better branch at every
/// marker, for both parents.
pub fn dr_ceiling(t: &Tables) -> f64 {
    let mut per_progeny = 0.0f64;
    for m in 0..t.markers {
        for p in 0..2 {
            let mut best = f64::NEG_INFINITY;
            for s in 0..NS {
                if t.dr[p][m][s] > best {
                    best = t.dr[p][m][s];
                }
            }
            per_progeny += best;
        }
    }
    per_progeny * t.offspring as f64
}

/// Best transition score: no progeny ever changes homolog.
pub fn transition_ceiling(t: &Tables) -> f64 {
    let mut per_progeny = 0.0f64;
    for m in 0..t.markers.saturating_sub(1) {
        for p in 0..2 {
            per_progeny += 2.0 * t.trans[p][m][0];
        }
    }
    per_progeny * t.offspring as f64
}

/// The loosest useful bound: every progeny attains its best dosage, never
/// recombines, and takes the best double reduction branch.
pub fn trivial_bound(t: &Tables) -> f64 {
    emission_ceiling(t) + dr_ceiling(t) + transition_ceiling(t)
}

/// Per marker bound. At each marker every progeny picks its best legal joint
/// state for the chosen columns, and the columns are chosen per marker, which
/// is not a relaxation at all. The only slack is that the states are free per
/// marker and the transitions are at their ceiling.
pub fn per_marker_bound(t: &Tables) -> f64 {
    let mut total = transition_ceiling(t);
    for m in 0..t.markers {
        let na = t.contrib[0][m].len();
        let nb = t.contrib[1][m].len();
        let mut best = f64::NEG_INFINITY;
        for oa in 0..na {
            let ea = &t.contrib[0][m][oa];
            for ob in 0..nb {
                let eb = &t.contrib[1][m][ob];
                let mut sum = 0.0f64;
                for c in 0..t.offspring {
                    let mut best_state = f64::NEG_INFINITY;
                    for j in 0..NJ {
                        if !t.legalj[m][j] {
                            continue;
                        }
                        let dosage = ea[j / NS] + eb[j % NS];
                        let value = t.gl[c][m][usize::from(dosage)] + t.drj[m][j];
                        if value > best_state {
                            best_state = value;
                        }
                    }
                    sum += best_state;
                }
                if sum > best {
                    best = sum;
                }
            }
        }
        total += best;
    }
    total
}

/// Optimistic emission per joint state: the best dosage any admissible column
/// pair can produce for that state, per progeny and marker.
pub fn optimistic_emissions(t: &Tables) -> Vec<Vec<[f64; NJ]>> {
    let mut out = Vec::with_capacity(t.offspring);
    for c in 0..t.offspring {
        let mut per_marker = Vec::with_capacity(t.markers);
        for m in 0..t.markers {
            let mut best = [f64::NEG_INFINITY; NJ];
            for oa in 0..t.contrib[0][m].len() {
                let ea = &t.contrib[0][m][oa];
                for ob in 0..t.contrib[1][m].len() {
                    let eb = &t.contrib[1][m][ob];
                    for j in 0..NJ {
                        let dosage = ea[j / NS] + eb[j % NS];
                        let value = t.gl[c][m][usize::from(dosage)];
                        if value > best[j] {
                            best[j] = value;
                        }
                    }
                }
            }
            per_marker.push(best);
        }
        out.push(per_marker);
    }
    out
}

/// Per progeny bound: each progeny chooses its own columns, so the emissions
/// are optimistic while the paths stay consistent and the transitions exact.
pub fn per_progeny_bound(t: &Tables) -> f64 {
    let optimistic = optimistic_emissions(t);
    let mut total = 0.0f64;
    for c in 0..t.offspring {
        let mut v = [f64::NEG_INFINITY; NJ];
        for j in 0..NJ {
            if t.legalj[0][j] {
                v[j] = optimistic[c][0][j] + t.drj[0][j];
            }
        }
        for m in 1..t.markers {
            let ta = &t.trans[0][m - 1];
            let tb = &t.trans[1][m - 1];
            let mut x = [f64::NEG_INFINITY; NJ];
            for pa in 0..NS {
                for sb in 0..NS {
                    let mut best = f64::NEG_INFINITY;
                    for pb in 0..NS {
                        let value = v[pa * NS + pb] + tb[pb * NS + sb];
                        if value > best {
                            best = value;
                        }
                    }
                    x[pa * NS + sb] = best;
                }
            }
            let mut next = [f64::NEG_INFINITY; NJ];
            for sa in 0..NS {
                for sb in 0..NS {
                    let j = sa * NS + sb;
                    if !t.legalj[m][j] {
                        continue;
                    }
                    let mut best = f64::NEG_INFINITY;
                    for pa in 0..NS {
                        let value = x[pa * NS + sb] + ta[pa * NS + sa];
                        if value > best {
                            best = value;
                        }
                    }
                    next[j] = best + optimistic[c][m][j] + t.drj[m][j];
                }
            }
            v = next;
        }
        let mut best = f64::NEG_INFINITY;
        for j in 0..NJ {
            if v[j] > best {
                best = v[j];
            }
        }
        total += best;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cross_input::parse_cross;
    use crate::solver::core::Tables;
    use crate::solver::search;

    #[test]
    fn bounds_on_the_real_cross() {
        let text = std::fs::read_to_string("/app/cross.json").expect("cross.json");
        let cross = parse_cross(&text).expect("valid cross");
        let t = Tables::build(&cross);
        let (value, _, _) = search::search(&t, 24, 24, 0x9e3779b97f4a7c15);
        println!("search      = {value:.6}");
        println!("emission ub = {:.6}", emission_ceiling(&t));
        println!("dr ub       = {:.6}", dr_ceiling(&t));
        println!("trans ub    = {:.6}", transition_ceiling(&t));
        println!("trivial     = {:.6}", trivial_bound(&t));
        println!("per marker  = {:.6}", per_marker_bound(&t));
        println!("per progeny = {:.6}", per_progeny_bound(&t));
    }
}
