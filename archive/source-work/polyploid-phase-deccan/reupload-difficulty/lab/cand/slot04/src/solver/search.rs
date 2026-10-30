
//! The outer search over parental haplotype assignments.
//!
//! With the parental haplotypes fixed every progeny is an independent Viterbi
//! pass, and with the paths fixed every marker's columns can be chosen on its
//! own. The two halves are alternated to a fixed point, and the whole thing is
//! wrapped in restarts and perturbations.

use crate::solver::core::{optimize_columns, Tables};

/// A deterministic xorshift generator, so repeated runs agree exactly.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % (n as u64)) as usize
    }
}

/// Alternates paths and columns until neither improves.
///
/// Returns the total, and leaves the columns and the paths at a fixed point.
pub fn local_search(
    t: &Tables,
    xa: &mut Vec<usize>,
    xb: &mut Vec<usize>,
    paths: &mut Vec<Vec<usize>>,
) -> f64 {
    let mut current = f64::NEG_INFINITY;
    for _ in 0..64 {
        let (value, found) = t.best_paths(xa, xb);
        *paths = found;
        let (raised, next_a, next_b) = optimize_columns(t, paths);
        if raised <= current + 1e-11 {
            return value.max(current);
        }
        current = value;
        *xa = next_a;
        *xb = next_b;
    }
    current
}

/// One pass of steepest ascent over the column pair at each marker.
///
/// Every marker is offered every admissible column pair and the best is kept,
/// so a pass ends at a point no single marker change can improve.
pub fn sweep_markers(t: &Tables, xa: &mut Vec<usize>, xb: &mut Vec<usize>) -> f64 {
    let mut current = t.evaluate(xa, xb);
    loop {
        let mut improved = false;
        for m in 0..t.markers {
            let na = t.contrib[0][m].len();
            let nb = t.contrib[1][m].len();
            let old = (xa[m], xb[m]);
            let mut best = current;
            let mut best_pair = old;
            for oa in 0..na {
                for ob in 0..nb {
                    if (oa, ob) == old {
                        continue;
                    }
                    xa[m] = oa;
                    xb[m] = ob;
                    let value = t.evaluate(xa, xb);
                    if value > best + 1e-11 {
                        best = value;
                        best_pair = (oa, ob);
                    }
                }
            }
            xa[m] = best_pair.0;
            xb[m] = best_pair.1;
            if best_pair != old {
                improved = true;
                current = best;
            }
        }
        if !improved {
            return current;
        }
    }
}

/// Repeated restarts with perturbation, keeping the best assignment seen.
///
/// `rounds` counts the perturbations tried from each restart. The result is
/// deterministic for a given seed and cross.
pub fn search(
    t: &Tables,
    restarts: usize,
    rounds: usize,
    seed: u64,
) -> (f64, Vec<usize>, Vec<usize>) {
    let mut rng = Rng::new(seed);
    let mut best = f64::NEG_INFINITY;
    let mut best_a: Vec<usize> = vec![0; t.markers];
    let mut best_b: Vec<usize> = vec![0; t.markers];

    for restart in 0..restarts {
        let mut xa: Vec<usize> = (0..t.markers).map(|m| rng.below(t.contrib[0][m].len())).collect();
        let mut xb: Vec<usize> = (0..t.markers).map(|m| rng.below(t.contrib[1][m].len())).collect();
        if restart == 0 {
            xa = vec![0; t.markers];
            xb = vec![0; t.markers];
        }
        let mut scratch: Vec<Vec<usize>> = Vec::new();
        let mut current = local_search(t, &mut xa, &mut xb, &mut scratch);
        for round in 0..rounds {
            if current > best + 1e-11 {
                best = current;
                best_a = xa.clone();
                best_b = xb.clone();
            }
            if round + 1 == rounds {
                break;
            }
            // Perturb a few markers at random and descend again.
            let kicks = 1 + rng.below(3);
            let mut trial_a = xa.clone();
            let mut trial_b = xb.clone();
            for _ in 0..kicks {
                let m = rng.below(t.markers);
                trial_a[m] = rng.below(t.contrib[0][m].len());
                trial_b[m] = rng.below(t.contrib[1][m].len());
            }
            let value = local_search(t, &mut trial_a, &mut trial_b, &mut scratch);
            if value > current - 1e-11 {
                current = value;
                xa = trial_a;
                xb = trial_b;
            }
        }
    }

    // Polish the best assignment found with single marker steepest ascent.
    let mut final_a = best_a.clone();
    let mut final_b = best_b.clone();
    let polished = sweep_markers(t, &mut final_a, &mut final_b);
    if polished > best {
        best = polished;
        best_a = final_a;
        best_b = final_b;
    }
    (best, best_a, best_b)
}
