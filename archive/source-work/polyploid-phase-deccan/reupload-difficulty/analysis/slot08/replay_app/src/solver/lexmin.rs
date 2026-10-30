// The smallest optimal path, in the order the result format compares them.
//
// When several inheritance paths score alike, `spec/result-format-v1.md` asks
// for the one whose state sequence is smallest: progeny in index order, and
// within a progeny parent A's states before parent B's. That is not what an
// ordinary Viterbi tie break gives, because a state that looks best at one
// marker can force a larger one later.
//
// The two parents' paths are independent apart from the emission, which is what
// makes this a two stage sweep. A backward pass fixes how much score is still
// reachable; then parent A's states are chosen one marker at a time, each time
// taking the smallest one that still leaves the optimum within reach, and only
// then parent B's, in the same way against A's now fixed sequence.

use crate::cross_model::STATES;
use crate::solver::core::{Core, NJ, NEG};

/// Cost of every joint state at every marker, for one progeny.
fn nodes(core: &Core, progeny: usize, columns_a: &[u8], columns_b: &[u8]) -> Vec<[f64; NJ]> {
    let mut out = vec![[NEG; NJ]; core.markers];
    for m in 0..core.markers {
        core.node(progeny, m, columns_a[m], columns_b[m], &mut out[m]);
    }
    out
}

/// Most score still reachable from marker `marker` onwards, per joint state.
fn backward(core: &Core, node: &[[f64; NJ]]) -> Vec<[f64; NJ]> {
    let markers = core.markers;
    let mut out = vec![[NEG; NJ]; markers];
    out[markers - 1] = node[markers - 1];
    for m in (0..markers - 1).rev() {
        let ta = &core.trans[0][m];
        let tb = &core.trans[1][m];
        let next = out[m + 1].clone();
        for sa in 0..STATES {
            for sb in 0..STATES {
                let target = sa * STATES + sb;
                if node[m][target] == NEG {
                    continue;
                }
                let mut best = NEG;
                for na in 0..STATES {
                    for nb in 0..STATES {
                        let value = next[na * STATES + nb];
                        if value == NEG {
                            continue;
                        }
                        let step = ta[sa * STATES + na] + tb[sb * STATES + nb] + value;
                        if step > best {
                            best = step;
                        }
                    }
                }
                out[m][target] = best + node[m][target];
            }
        }
    }
    out
}

/// The states of one parent, ordered by how `perm` relabels them, so that
/// taking the first admissible state minimises the written signature.
pub fn state_order(perm: &[u8; crate::cross_model::HOMOLOGS]) -> [usize; STATES] {
    use crate::solver::core::{SA, SB};
    let mut order: Vec<(u8, u8, usize)> = (0..STATES)
        .map(|s| {
            let (x, y) = (perm[SA[s]], perm[SB[s]]);
            let (low, high) = if x <= y { (x, y) } else { (y, x) };
            (low, high, s)
        })
        .collect();
    order.sort();
    let mut out = [0usize; STATES];
    for (slot, (_, _, state)) in out.iter_mut().zip(order) {
        *slot = state;
    }
    out
}

/// The best total for one progeny, and the smallest path that reaches it.
///
/// `order_a` and `order_b` give, for each parent, the states in the order the
/// result format compares them. `slack` is how far below the best total a path
/// may fall and still count as tied.
pub fn lexmin_path(
    core: &Core,
    progeny: usize,
    columns_a: &[u8],
    columns_b: &[u8],
    order_a: &[usize; STATES],
    order_b: &[usize; STATES],
    slack: f64,
) -> (f64, Vec<usize>) {
    let markers = core.markers;
    let node = nodes(core, progeny, columns_a, columns_b);
    let back = backward(core, &node);

    let mut optimum = NEG;
    for s in 0..NJ {
        if back[0][s] > optimum {
            optimum = back[0][s];
        }
    }

    // Stage one: parent A's states, smallest first, keeping the optimum open.
    let mut chosen_a = vec![0usize; markers];
    let mut prefix = [NEG; STATES];
    for m in 0..markers {
        let mut accepted = None;
        for &ca in order_a.iter() {
            let mut candidate = [NEG; STATES];
            for sb in 0..STATES {
                let here = node[m][ca * STATES + sb];
                if here == NEG {
                    continue;
                }
                candidate[sb] = if m == 0 {
                    here
                } else {
                    let ta = &core.trans[0][m - 1];
                    let tb = &core.trans[1][m - 1];
                    let mut best = NEG;
                    for prev_b in 0..STATES {
                        let value = prefix[prev_b];
                        if value == NEG {
                            continue;
                        }
                        let step = value
                            + ta[chosen_a[m - 1] * STATES + ca]
                            + tb[prev_b * STATES + sb]
                            + here;
                        if step > best {
                            best = step;
                        }
                    }
                    best
                };
            }

            let mut reachable = NEG;
            for sb in 0..STATES {
                if candidate[sb] == NEG {
                    continue;
                }
                let rest = if m + 1 == markers {
                    0.0
                } else {
                    // How much the markers after this one can still add, with
                    // parent A free again but parent B starting from `sb`.
                    let ta = &core.trans[0][m];
                    let tb = &core.trans[1][m];
                    let mut through = [NEG; STATES];
                    for nb in 0..STATES {
                        let mut best = NEG;
                        for na in 0..STATES {
                            let value = back[m + 1][na * STATES + nb];
                            if value == NEG {
                                continue;
                            }
                            let step = ta[ca * STATES + na] + value;
                            if step > best {
                                best = step;
                            }
                        }
                        through[nb] = best;
                    }
                    let mut best = NEG;
                    for nb in 0..STATES {
                        let value = through[nb];
                        if value == NEG {
                            continue;
                        }
                        let step = tb[sb * STATES + nb] + value;
                        if step > best {
                            best = step;
                        }
                    }
                    best
                };
                let total = candidate[sb] + rest;
                if total > reachable {
                    reachable = total;
                }
            }

            if reachable >= optimum - slack {
                accepted = Some((ca, candidate));
                break;
            }
        }
        let (ca, candidate) = accepted.expect("the optimal path keeps the optimum reachable");
        chosen_a[m] = ca;
        prefix = candidate;
    }

    // Stage two: parent B's states, now that parent A's sequence is fixed.
    let mut suffix = vec![[0.0f64; STATES]; markers];
    for m in (0..markers - 1).rev() {
        let ta = &core.trans[0][m];
        let tb = &core.trans[1][m];
        for sb in 0..STATES {
            let mut best = NEG;
            for nb in 0..STATES {
                let here = node[m + 1][chosen_a[m + 1] * STATES + nb];
                if here == NEG {
                    continue;
                }
                let step = ta[chosen_a[m] * STATES + chosen_a[m + 1]]
                    + tb[sb * STATES + nb]
                    + here
                    + suffix[m + 1][nb];
                if step > best {
                    best = step;
                }
            }
            suffix[m][sb] = best;
        }
    }

    let mut chosen_b = vec![0usize; markers];
    let mut value = NEG;
    for m in 0..markers {
        let mut accepted = None;
        for &sb in order_b.iter() {
            let here = node[m][chosen_a[m] * STATES + sb];
            if here == NEG {
                continue;
            }
            let total = if m == 0 {
                here
            } else {
                let ta = &core.trans[0][m - 1];
                let tb = &core.trans[1][m - 1];
                if value == NEG {
                    continue;
                }
                value
                    + ta[chosen_a[m - 1] * STATES + chosen_a[m]]
                    + tb[chosen_b[m - 1] * STATES + sb]
                    + here
            };
            let rest = if m + 1 == markers { 0.0 } else { suffix[m][sb] };
            if total + rest >= optimum - slack {
                accepted = Some((sb, total));
                break;
            }
        }
        let (sb, total) = accepted.expect("the optimal path keeps the optimum reachable");
        chosen_b[m] = sb;
        value = total;
    }

    let mut states = Vec::with_capacity(markers);
    for m in 0..markers {
        states.push(chosen_a[m] * STATES + chosen_b[m]);
    }
    (value, states)
}
