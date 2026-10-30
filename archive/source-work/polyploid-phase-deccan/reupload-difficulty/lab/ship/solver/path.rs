
//! Choosing the required path once the columns are fixed.
//!
//! Optimality alone does not determine the answer. Several paths can attain the
//! same optimal score, and the result format then asks for the one with the
//! smallest signature, comparing parent `A`'s whole state sequence before parent
//! `B`'s. A backward traceback cannot produce that, because it settles the last
//! marker first and so minimises from the wrong end.
//!
//! This walks forward instead. At each marker it tries the candidate states in
//! ascending canonical order and takes the first that can still be completed to
//! an optimum, which is the standard greedy for a lexicographic minimum. Parent
//! `A` is settled across every marker first, then parent `B` is settled against
//! the sequence parent `A` ended up with.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, ProgenyPath, State, ALL_STATES, STATES};
use crate::scoring::{double_reduction_term, transition_term};

use super::columns::Column;

/// Slack allowed when asking whether a candidate can still reach the optimum.
///
/// The tables here sum in a different order from the scorer, so exact equality
/// would reject states that do reach it. This sits far below any real score gap
/// and far above the accumulated rounding.
const REACH: f64 = 1e-7;

/// Tables for one progeny that depend only on the fixed columns.
struct Layers {
    /// `node[m][a][b]`: emission plus both double reduction terms, or negative
    /// infinity where either state is an illegal double reduction form.
    node: Vec<Vec<Vec<f64>>>,
    /// `back[m][a][b]`: best score obtainable strictly after marker `m`.
    back: Vec<Vec<Vec<f64>>>,
    optimum: f64,
}

/// Builds the required path for one progeny.
pub fn required_path(
    cross: &Cross,
    progeny: usize,
    columns_a: &[Column],
    columns_b: &[Column],
) -> ProgenyPath {
    let layers = Layers::build(cross, progeny, columns_a, columns_b);
    let from_a = smallest_parent_a(cross, &layers);
    let from_b = smallest_parent_b(cross, &layers, &from_a);
    ProgenyPath { from_a, from_b }
}

impl Layers {
    fn build(cross: &Cross, progeny: usize, columns_a: &[Column], columns_b: &[Column]) -> Layers {
        let markers = cross.markers;

        let mut node = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut layer = vec![vec![f64::NEG_INFINITY; STATES]; STATES];
            for (ia, sa) in ALL_STATES.iter().enumerate() {
                if sa.is_double_reduction() && !cross.params(Parent::A).dr_allowed[m] {
                    continue;
                }
                for (ib, sb) in ALL_STATES.iter().enumerate() {
                    if sb.is_double_reduction() && !cross.params(Parent::B).dr_allowed[m] {
                        continue;
                    }
                    let dosage = carried(*sa, &columns_a[m]) + carried(*sb, &columns_b[m]);
                    layer[ia][ib] = cross.child_gl[progeny][m][usize::from(dosage)]
                        + double_reduction_term(cross, Parent::A, *sa, m)
                        + double_reduction_term(cross, Parent::B, *sb, m);
                }
            }
            node.push(layer);
        }

        let mut back = vec![vec![vec![0.0f64; STATES]; STATES]; markers];
        for m in (0..markers.saturating_sub(1)).rev() {
            for ia in 0..STATES {
                for ib in 0..STATES {
                    let mut best = f64::NEG_INFINITY;
                    for na in 0..STATES {
                        for nb in 0..STATES {
                            if node[m + 1][na][nb] == f64::NEG_INFINITY {
                                continue;
                            }
                            let value = interval(cross, ia, ib, na, nb, m)
                                + node[m + 1][na][nb]
                                + back[m + 1][na][nb];
                            if value > best {
                                best = value;
                            }
                        }
                    }
                    back[m][ia][ib] = best;
                }
            }
        }

        let mut optimum = f64::NEG_INFINITY;
        for ia in 0..STATES {
            for ib in 0..STATES {
                if node[0][ia][ib] == f64::NEG_INFINITY {
                    continue;
                }
                let value = node[0][ia][ib] + back[0][ia][ib];
                if value > optimum {
                    optimum = value;
                }
            }
        }

        Layers { node, back, optimum }
    }
}

fn carried(state: State, column: &Column) -> u8 {
    column[state.low() as usize] + column[state.high() as usize]
}

fn interval(cross: &Cross, ia: usize, ib: usize, na: usize, nb: usize, marker: usize) -> f64 {
    transition_term(cross, Parent::A, ALL_STATES[ia], ALL_STATES[na], marker)
        + transition_term(cross, Parent::B, ALL_STATES[ib], ALL_STATES[nb], marker)
}

/// Smallest parent `A` sequence appearing in some optimal path.
fn smallest_parent_a(cross: &Cross, layers: &Layers) -> Vec<State> {
    let mut chosen: Vec<usize> = Vec::with_capacity(cross.markers);
    // Best prefix score so far, indexed by parent B's state at the last marker.
    let mut reach = vec![f64::NEG_INFINITY; STATES];

    for m in 0..cross.markers {
        let mut settled = None;
        for candidate in 0..STATES {
            let next = extend(cross, layers, &chosen, &reach, candidate, m);
            let reaches = (0..STATES).any(|ib| {
                next[ib] != f64::NEG_INFINITY
                    && next[ib] + layers.back[m][candidate][ib] >= layers.optimum - REACH
            });
            if reaches {
                settled = Some((candidate, next));
                break;
            }
        }
        let (candidate, next) =
            settled.expect("some state at every marker lies on an optimal path");
        chosen.push(candidate);
        reach = next;
    }

    chosen.into_iter().map(|i| ALL_STATES[i]).collect()
}

/// Extends the prefix by pinning parent `A` to `candidate` at `marker`.
fn extend(
    cross: &Cross,
    layers: &Layers,
    chosen: &[usize],
    reach: &[f64],
    candidate: usize,
    marker: usize,
) -> Vec<f64> {
    let mut next = vec![f64::NEG_INFINITY; STATES];
    for ib in 0..STATES {
        if layers.node[marker][candidate][ib] == f64::NEG_INFINITY {
            continue;
        }
        if marker == 0 {
            next[ib] = layers.node[marker][candidate][ib];
            continue;
        }
        let previous = chosen[marker - 1];
        let mut best = f64::NEG_INFINITY;
        for pb in 0..STATES {
            if reach[pb] == f64::NEG_INFINITY {
                continue;
            }
            let value = reach[pb] + interval(cross, previous, pb, candidate, ib, marker - 1);
            if value > best {
                best = value;
            }
        }
        if best != f64::NEG_INFINITY {
            next[ib] = best + layers.node[marker][candidate][ib];
        }
    }
    next
}

/// Smallest parent `B` sequence, given the sequence parent `A` settled on.
fn smallest_parent_b(cross: &Cross, layers: &Layers, from_a: &[State]) -> Vec<State> {
    let markers = cross.markers;
    let a: Vec<usize> = from_a.iter().map(|s| s.canonical_index()).collect();

    let mut back = vec![vec![0.0f64; STATES]; markers];
    for m in (0..markers.saturating_sub(1)).rev() {
        for ib in 0..STATES {
            let mut best = f64::NEG_INFINITY;
            for nb in 0..STATES {
                if layers.node[m + 1][a[m + 1]][nb] == f64::NEG_INFINITY {
                    continue;
                }
                let value = interval(cross, a[m], ib, a[m + 1], nb, m)
                    + layers.node[m + 1][a[m + 1]][nb]
                    + back[m + 1][nb];
                if value > best {
                    best = value;
                }
            }
            back[m][ib] = best;
        }
    }

    let mut optimum = f64::NEG_INFINITY;
    for ib in 0..STATES {
        if layers.node[0][a[0]][ib] == f64::NEG_INFINITY {
            continue;
        }
        let value = layers.node[0][a[0]][ib] + back[0][ib];
        if value > optimum {
            optimum = value;
        }
    }

    let mut chosen: Vec<usize> = Vec::with_capacity(markers);
    let mut running = 0.0f64;
    for m in 0..markers {
        let mut settled = None;
        for candidate in 0..STATES {
            if layers.node[m][a[m]][candidate] == f64::NEG_INFINITY {
                continue;
            }
            let value = if m == 0 {
                layers.node[0][a[0]][candidate]
            } else {
                running
                    + interval(cross, a[m - 1], chosen[m - 1], a[m], candidate, m - 1)
                    + layers.node[m][a[m]][candidate]
            };
            if value + back[m][candidate] >= optimum - REACH {
                settled = Some((candidate, value));
                break;
            }
        }
        let (candidate, value) =
            settled.expect("some state at every marker lies on an optimal path");
        chosen.push(candidate);
        running = value;
    }

    chosen.into_iter().map(|i| ALL_STATES[i]).collect()
}
