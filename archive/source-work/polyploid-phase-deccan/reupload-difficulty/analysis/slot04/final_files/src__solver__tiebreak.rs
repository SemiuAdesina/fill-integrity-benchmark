
//! The smallest optimal paths, and the smallest optimal reconstruction.
//!
//! Several path sets can reach the same optimum. `spec/result-format-v1.md`
//! asks for the smallest one: progeny in index order, and for each progeny
//! parent `A`'s whole state sequence before parent `B`'s, each state compared
//! as its remapped pair.
//!
//! The choice is made greedily. A state is kept when the optimum is still
//! reachable from it, which is decided with a suffix table: the best score of
//! the markers after the current one, given the state here. Parent `A`'s
//! sequence is fixed first with parent `B` free, then parent `B`'s is fixed
//! against the suffix table that parent `A`'s fixed sequence leaves.

use crate::cross_model::{Haplotypes, HOMOLOGS};
use crate::solver::core::{Tables, NJ, NS, PA, PB};

/// How far a partial choice may fall short of the optimum and still count as
/// optimal. Far above the rounding noise of a sum of a few dozen terms, far
/// below the `1e-8` that makes two totals the same score.
const TOL: f64 = 1e-9;

/// State indices in the order their remapped pairs compare under `perm`.
pub fn frame_order(perm: &[u8; HOMOLOGS]) -> [usize; NS] {
    let mut keys: Vec<(u8, u8, usize)> = Vec::with_capacity(NS);
    for (s, state) in crate::cross_model::ALL_STATES.iter().enumerate() {
        let moved = state.remap(perm);
        keys.push((moved.low(), moved.high(), s));
    }
    keys.sort_unstable();
    let mut out = [0usize; NS];
    for (slot, (_, _, s)) in keys.iter().enumerate() {
        out[slot] = *s;
    }
    out
}

/// Permutations that leave the rows in ascending order.
///
/// Canonical form is one of these; which one depends on the paths, so the
/// search offers each of them a chance to produce the smallest signature.
pub fn sorting_permutations(rows: &Haplotypes) -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::new();
    for perm in crate::solver::canonical::homolog_permutations() {
        let mut moved: Vec<Vec<u8>> = vec![Vec::new(); HOMOLOGS];
        for h in 0..HOMOLOGS {
            moved[perm[h] as usize] = rows.row(h).to_vec();
        }
        if moved.windows(2).all(|w| w[0] <= w[1]) {
            out.push(perm);
        }
    }
    out
}

/// The smallest optimal joint path for one progeny.
fn minimal_path(t: &Tables, c: usize, xa: &[usize], xb: &[usize], order_a: &[usize; NS], order_b: &[usize; NS]) -> Vec<usize> {
    let markers = t.markers;
    let target = t.viterbi(c, xa, xb, None);

    let mut em: Vec<[f64; NJ]> = vec![[0.0f64; NJ]; markers];
    for m in 0..markers {
        t.emission_row(c, m, xa[m], xb[m], &mut em[m]);
    }

    // `suf[m][j]`: the best score of the markers after `m`, both parents free,
    // given the state at `m` is `j`.
    let mut suf: Vec<[f64; NJ]> = vec![[0.0f64; NJ]; markers];
    for m in (0..markers.saturating_sub(1)).rev() {
        let ta = &t.trans[PA][m];
        let tb = &t.trans[PB][m];
        let mut rest = [f64::NEG_INFINITY; NJ];
        for k in 0..NJ {
            if t.legalj[m + 1][k] {
                rest[k] = em[m + 1][k] + t.drj[m + 1][k] + suf[m + 1][k];
            }
        }
        let mut x = [f64::NEG_INFINITY; NJ];
        for pa in 0..NS {
            for sb in 0..NS {
                let mut best = f64::NEG_INFINITY;
                for pb in 0..NS {
                    let value = rest[pa * NS + pb] + tb[pb * NS + sb];
                    if value > best {
                        best = value;
                    }
                }
                x[pa * NS + sb] = best;
            }
        }
        for sa in 0..NS {
            for sb in 0..NS {
                let mut best = f64::NEG_INFINITY;
                for pa in 0..NS {
                    let value = x[pa * NS + sb] + ta[pa * NS + sa];
                    if value > best {
                        best = value;
                    }
                }
                suf[m][sa * NS + sb] = best;
            }
        }
    }

    // Parent `A`'s states, smallest first, each kept only when the optimum is
    // still reachable. `fwd_b[sb]` is the best score of the markers up to here
    // with the chosen prefix of `A` and any prefix of `B` ending at `sb`.
    let mut path_a = vec![0usize; markers];
    let mut fwd_b = [f64::NEG_INFINITY; NS];
    for m in 0..markers {
        let mut chosen = order_a[0];
        let mut chosen_fwd = [f64::NEG_INFINITY; NS];
        let mut fallback = f64::NEG_INFINITY;
        for &sa in order_a.iter() {
            let mut fwd = [f64::NEG_INFINITY; NS];
            if m == 0 {
                for sb in 0..NS {
                    let j = sa * NS + sb;
                    if t.legalj[0][j] {
                        fwd[sb] = em[0][j] + t.drj[0][j];
                    }
                }
            } else {
                let ta = &t.trans[PA][m - 1];
                let tb = &t.trans[PB][m - 1];
                let step = ta[path_a[m - 1] * NS + sa];
                for sb in 0..NS {
                    let j = sa * NS + sb;
                    if !t.legalj[m][j] {
                        continue;
                    }
                    let mut best = f64::NEG_INFINITY;
                    for pb in 0..NS {
                        let value = fwd_b[pb] + tb[pb * NS + sb] + step;
                        if value > best {
                            best = value;
                        }
                    }
                    fwd[sb] = best + em[m][j] + t.drj[m][j];
                }
            }
            let mut reach = f64::NEG_INFINITY;
            for sb in 0..NS {
                let value = fwd[sb] + suf[m][sa * NS + sb];
                if value > reach {
                    reach = value;
                }
            }
            if reach >= target - TOL {
                chosen = sa;
                chosen_fwd = fwd;
                break;
            }
            if reach > fallback {
                fallback = reach;
                chosen = sa;
                chosen_fwd = fwd;
            }
        }
        path_a[m] = chosen;
        fwd_b = chosen_fwd;
    }

    // `suf_b[m][sb]`: the best score of the markers after `m` with parent `A`'s
    // whole sequence fixed and parent `B` leaving `m` in state `sb`.
    let mut suf_b: Vec<[f64; NS]> = vec![[0.0f64; NS]; markers];
    for m in (0..markers.saturating_sub(1)).rev() {
        let ta = &t.trans[PA][m];
        let tb = &t.trans[PB][m];
        let step = ta[path_a[m] * NS + path_a[m + 1]];
        for sb in 0..NS {
            let mut best = f64::NEG_INFINITY;
            for nb in 0..NS {
                let j = path_a[m + 1] * NS + nb;
                if !t.legalj[m + 1][j] {
                    continue;
                }
                let value =
                    step + tb[sb * NS + nb] + em[m + 1][j] + t.drj[m + 1][j] + suf_b[m + 1][nb];
                if value > best {
                    best = value;
                }
            }
            suf_b[m][sb] = best;
        }
    }

    let mut path_b = vec![0usize; markers];
    let mut carried = 0.0f64;
    for m in 0..markers {
        let mut chosen = order_b[0];
        let mut chosen_value = f64::NEG_INFINITY;
        let mut fallback = f64::NEG_INFINITY;
        for &sb in order_b.iter() {
            let j = path_a[m] * NS + sb;
            if !t.legalj[m][j] {
                continue;
            }
            let mut value = em[m][j] + t.drj[m][j];
            if m > 0 {
                value += carried
                    + t.trans[PA][m - 1][path_a[m - 1] * NS + path_a[m]]
                    + t.trans[PB][m - 1][path_b[m - 1] * NS + sb];
            }
            let reach = if m + 1 < markers { value + suf_b[m][sb] } else { value };
            if reach >= target - TOL {
                chosen = sb;
                chosen_value = value;
                break;
            }
            if reach > fallback {
                fallback = reach;
                chosen = sb;
                chosen_value = value;
            }
        }
        path_b[m] = chosen;
        carried = chosen_value;
    }

    (0..markers).map(|m| path_a[m] * NS + path_b[m]).collect()
}

/// The smallest optimal joint paths for every progeny, in the relabelling given
/// by `perm_a` and `perm_b`.
///
/// Returns the total and the paths. Should a rounding artefact ever leave the
/// greedy choice a hair below the optimum, that progeny falls back to the plain
/// Viterbi path rather than reporting a suboptimal score.
pub fn minimal_paths(
    t: &Tables,
    xa: &[usize],
    xb: &[usize],
    perm_a: &[u8; HOMOLOGS],
    perm_b: &[u8; HOMOLOGS],
) -> (f64, Vec<Vec<usize>>) {
    let order_a = frame_order(perm_a);
    let order_b = frame_order(perm_b);
    let (optimum, plain) = t.best_paths(xa, xb);
    let mut paths = Vec::with_capacity(t.offspring);
    for c in 0..t.offspring {
        let picked = minimal_path(t, c, xa, xb, &order_a, &order_b);
        let value = t.score_paths(&[picked.clone()], xa, xb);
        let reference = t.score_paths(&[plain[c].clone()], xa, xb);
        if value >= reference - TOL {
            paths.push(picked);
        } else {
            paths.push(plain[c].clone());
        }
    }
    let total = t.score_paths(&paths, xa, xb);
    if total < optimum - 1e-7 {
        return (optimum, plain);
    }
    (total, paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cross_input::parse_cross;
    use crate::solver::core::Tables;
    use crate::solver::search;

    #[test]
    fn minimal_paths_reach_the_optimum() {
        let text = std::fs::read_to_string("/app/cross.json").expect("cross.json");
        let cross = parse_cross(&text).expect("valid cross");
        let t = Tables::build(&cross);
        let (value, xa, xb) = search::search(&t, 24, 24, 0x9e3779b97f4a7c15);
        let identity = [0u8, 1, 2, 3];
        let (total, paths) = minimal_paths(&t, &xa, &xb, &identity, &identity);
        let (optimum, _) = t.best_paths(&xa, &xb);
        println!("search {value:.9} minimal {total:.9} optimum {optimum:.9}");
        assert!(total >= optimum - 1e-9, "minimal paths lost {:.12}", optimum - total);
        assert_eq!(paths.len(), t.offspring);
        for path in &paths {
            assert_eq!(path.len(), t.markers);
        }
    }
}
