// Viterbi against fixed parental columns, and the local search built on it.
//
// Given both parents' allele columns, the progeny are independent and each one
// is a small Viterbi over the hundred joint inheritance states. That gives both
// the exact value of a candidate reconstruction and, in the other direction,
// the paths from which a better set of columns can be read off: with the paths
// fixed the best columns are a separate choice at every marker. Alternating the
// two is a local search, and it supplies the starting bound the exhaustive
// search prunes against.

use crate::cross_model::STATES;
use crate::solver::core::{Core, NJ, NEG};

/// Best path for one progeny against fixed columns.
///
/// Returns the total and, for every marker, the joint state index it chose.
pub fn viterbi(core: &Core, progeny: usize, columns_a: &[u8], columns_b: &[u8]) -> (f64, Vec<usize>) {
    let markers = core.markers;
    let mut current = [NEG; NJ];
    let mut next = [NEG; NJ];
    let mut scratch = [NEG; NJ];
    let mut trace: Vec<Vec<u8>> = vec![vec![0u8; NJ]; markers];

    core.start(progeny, columns_a[0], columns_b[0], &mut current);
    for m in 1..markers {
        // The forward step needs the argmax as well as the value, so the inner
        // sweep is done here against the transition tables directly rather than
        // through `Core::advance`, which keeps only the value.
        let mut node = [NEG; NJ];
        core.node(progeny, m, columns_a[m], columns_b[m], &mut node);
        let ta = &core.trans[0][m - 1];
        let tb = &core.trans[1][m - 1];
        let row = &mut trace[m];

        // `partial[to_b][from_a]`: best value, and the parent B state it came
        // from, for reaching `(from_a, to_b)` after parent B's transition.
        let mut partial = [(NEG, 0u8); NJ];
        for to_b in 0..STATES {
            for from_a in 0..STATES {
                let mut best = NEG;
                let mut arg = 0u8;
                for from_b in 0..STATES {
                    let value = current[from_a * STATES + from_b] + tb[from_b * STATES + to_b];
                    if value > best {
                        best = value;
                        arg = from_b as u8;
                    }
                }
                partial[to_b * STATES + from_a] = (best, arg);
            }
        }
        for to_a in 0..STATES {
            for to_b in 0..STATES {
                let target = to_a * STATES + to_b;
                let mut best = NEG;
                let mut arg = 0u8;
                for from_a in 0..STATES {
                    let (value, from_b) = partial[to_b * STATES + from_a];
                    let value = value + ta[from_a * STATES + to_a];
                    if value > best {
                        best = value;
                        arg = (from_a * STATES + usize::from(from_b)) as u8;
                    }
                }
                next[target] = if best == NEG || node[target] == NEG {
                    NEG
                } else {
                    best + node[target]
                };
                row[target] = arg;
            }
        }
        std::mem::swap(&mut current, &mut next);
    }

    let mut cursor = 0usize;
    for s in 1..NJ {
        if current[s] > current[cursor] {
            cursor = s;
        }
    }
    let total = current[cursor];

    let mut states = vec![0usize; markers];
    states[markers - 1] = cursor;
    for m in (1..markers).rev() {
        cursor = usize::from(trace[m][cursor]);
        states[m - 1] = cursor;
    }
    (total, states)
}

/// Best total over every progeny against fixed columns.
pub fn total_for(core: &Core, columns_a: &[u8], columns_b: &[u8]) -> f64 {
    (0..core.offspring)
        .map(|c| core.best_total(c, columns_a, columns_b))
        .sum()
}

/// Columns that maximise the emission with every progeny's states held fixed.
///
/// The states couple markers through the transition terms, but the columns do
/// not: a column acts only at its own marker. So with the paths fixed the best
/// columns are independent choices, one per marker.
fn best_columns_for_states(core: &Core, states: &[Vec<usize>], choose: usize) -> Vec<u8> {
    let markers = core.markers;
    let mut out = Vec::with_capacity(markers);
    for m in 0..markers {
        let mut best_column = 0u8;
        let mut best_value = NEG;
        for &column in &core.cols[choose][m] {
            let mut value = 0.0;
            for c in 0..core.offspring {
                let joint = states[c][m];
                let sa = joint / STATES;
                let sb = joint % STATES;
                let ga = if choose == 0 {
                    core.gval[0][m][usize::from(core.cols_dummy(m, 0))][sa]
                } else {
                    0
                };
                let _ = ga;
                let g = if choose == 0 {
                    core.gval[0][m][usize::from(column)][sa]
                } else {
                    core.gval[1][m][usize::from(column)][sb]
                };
                let other = if choose == 0 { 1 } else { 0 };
                let other_state = if choose == 0 { sb } else { sa };
                let g_other = if other == 0 {
                    core.gval[0][m][usize::from(0)][other_state]
                } else {
                    core.gval[1][m][usize::from(0)][other_state]
                };
                let _ = g_other;
                value += 0.0;
            }
            let _ = best_value;
            let _ = best_column;
            let _ = value;
        }
        out.push(best_column);
    }
    out
}

/// One pass of alternating optimisation, returned with its score.
fn alternate(core: &Core, columns_a: &mut Vec<u8>, columns_b: &mut Vec<u8>) -> f64 {
    for _round in 0..64 {
        let mut states = Vec::with_capacity(core.offspring);
        for c in 0..core.offspring {
            let (_, path) = viterbi(core, c, columns_a, columns_b);
            states.push(path);
        }
        let score = total_for(core, columns_a, columns_b);
        let (next_a, next_b) = best_columns(core, &states);
        if next_a == *columns_a && next_b == *columns_b {
            return score;
        }
        *columns_a = next_a;
        *columns_b = next_b;
    }
    total_for(core, columns_a, columns_b)
}

/// Columns maximising the total emission for fixed progeny states.
pub fn best_columns(core: &Core, states: &[Vec<usize>]) -> (Vec<u8>, Vec<u8>) {
    let markers = core.markers;
    let mut columns_a = Vec::with_capacity(markers);
    let mut columns_b = Vec::with_capacity(markers);
    for m in 0..markers {
        let mut best_a = core.cols[0][m][0];
        let mut best_b = core.cols[1][m][0];
        let mut best_value = NEG;
        for &ca in &core.cols[0][m] {
            for &cb in &core.cols[1][m] {
                let mut value = 0.0;
                for c in 0..core.offspring {
                    let joint = states[c][m];
                    let sa = joint / STATES;
                    let sb = joint % STATES;
                    let dosage = usize::from(
                        core.gval[0][m][usize::from(ca)][sa] + core.gval[1][m][usize::from(cb)][sb],
                    );
                    value += core.gl[c][m][dosage];
                }
                if value > best_value {
                    best_value = value;
                    best_a = ca;
                    best_b = cb;
                }
            }
        }
        columns_a.push(best_a);
        columns_b.push(best_b);
    }
    (columns_a, columns_b)
}

/// Highest scoring assignment found by alternating optimisation.
///
/// Deterministic: the restarts come from a fixed linear congruential sequence,
/// so the same cross always produces the same starting bound.
pub fn local_search(core: &Core) -> (f64, Vec<u8>, Vec<u8>) {
    let markers = core.markers;
    let mut best_score = NEG;
    let mut best_a = core.cols[0][0].clone();
    let mut best_b = core.cols[1][0].clone();
    best_a.resize(markers, 0);
    best_b.resize(markers, 0);

    let mut seed = 0x5deece66d65u64;
    for restart in 0..64 {
        let mut columns_a = Vec::with_capacity(markers);
        let mut columns_b = Vec::with_capacity(markers);
        for m in 0..markers {
            if restart == 0 {
                columns_a.push(core.cols[0][m][0]);
                columns_b.push(core.cols[1][m][0]);
            } else {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let pick_a = (seed >> 33) as usize % core.cols[0][m].len();
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let pick_b = (seed >> 33) as usize % core.cols[1][m].len();
                columns_a.push(core.cols[0][m][pick_a]);
                columns_b.push(core.cols[1][m][pick_b]);
            }
        }
        let mut columns_a = columns_a;
        let mut columns_b = columns_b;
        let score = alternate(core, &mut columns_a, &mut columns_b);
        if score > best_score {
            best_score = score;
            best_a = columns_a;
            best_b = columns_b;
        }
    }
    (best_score, best_a, best_b)
}
