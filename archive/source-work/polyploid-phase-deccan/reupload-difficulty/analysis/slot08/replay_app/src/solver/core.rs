// Shared tables and the two dynamic programs the search is built from.
//
// Everything here is a function of the cross alone: the admissible allele
// columns, which inheritance states are legal where, the double reduction and
// transition tables, and the two per progeny recursions.
//
// The forward recursion is the Viterbi of `spec/scoring-model.md` for one
// progeny against a fixed pair of allele columns. The backward recursion is an
// upper bound on what the rest of the markers can still contribute when the
// columns past the current one are unknown: it lets every progeny choose its
// own columns there, which no legal reconstruction may do, so the value it
// returns is never below what a real completion can reach.

use crate::cross_input::Cross;
use crate::cross_model::{Parent, STATES};

/// Joint states of one progeny: ten states from parent A times ten from B.
pub const NJ: usize = STATES * STATES;

/// First homolog of each canonical state index.
pub const SA: [usize; STATES] = [0, 0, 0, 0, 1, 1, 1, 2, 2, 3];
/// Second homolog of each canonical state index, with `SA[i] <= SB[i]`.
pub const SB: [usize; STATES] = [0, 1, 2, 3, 1, 2, 3, 2, 3, 3];

pub const NEG: f64 = f64::NEG_INFINITY;

pub struct Core {
    pub markers: usize,
    pub offspring: usize,
    /// Admissible allele columns, as a bitmask over the four homologs, for
    /// `[parent][marker]`. Bit `h` is the allele homolog `h` carries.
    pub cols: [Vec<Vec<u8>>; 2],
    /// Whether each state is legal for `[parent][marker]`.
    pub legal: [Vec<[bool; STATES]>; 2],
    /// Double reduction term for `[parent][marker][is_double_reduction]`.
    pub dr: [Vec<[f64; 2]>; 2],
    /// Transition term for `[parent][interval][from * STATES + to]`.
    pub trans: [Vec<[f64; NJ]>; 2],
    /// Alternate alleles contributed by a state, `[parent][marker][column][state]`.
    pub gval: [Vec<[[u8; STATES]; 16]>; 2],
    /// The read evidence, `[progeny][marker][dosage]`.
    pub gl: Vec<Vec<[f64; 5]>>,
    /// Emission each progeny could reach at a marker if it alone picked the
    /// columns: `[progeny][marker][joint state]`.
    pub pers: Vec<Vec<[f64; NJ]>>,
    /// Backward bound: `[progeny][marker][joint state]`, the most markers
    /// `marker + 1 ..` can still contribute when their columns are unknown.
    pub back: Vec<Vec<[f64; NJ]>>,
}

impl Core {
    pub fn new(cross: &Cross) -> Core {
        let markers = cross.markers;
        let offspring = cross.offspring;

        let mut cols: [Vec<Vec<u8>>; 2] = [Vec::new(), Vec::new()];
        let mut legal: [Vec<[bool; STATES]>; 2] = [Vec::new(), Vec::new()];
        let mut dr: [Vec<[f64; 2]>; 2] = [Vec::new(), Vec::new()];
        let mut trans: [Vec<[f64; NJ]>; 2] = [Vec::new(), Vec::new()];
        let mut gval: [Vec<[[u8; STATES]; 16]>; 2] = [Vec::new(), Vec::new()];

        for (p, parent) in Parent::BOTH.iter().enumerate() {
            let params = cross.params(*parent);
            for m in 0..markers {
                let mut admissible = Vec::new();
                for bits in 0u8..16 {
                    if bits.count_ones() as u8 == params.dosage[m] {
                        admissible.push(bits);
                    }
                }
                cols[p].push(admissible);

                let mut mask = [false; STATES];
                for (s, state) in crate::cross_model::ALL_STATES.iter().enumerate() {
                    mask[s] = !state.is_double_reduction() || params.dr_allowed[m];
                }
                legal[p].push(mask);

                let rate = params.dr_rate[m];
                dr[p].push([(1.0 - rate).ln(), if rate > 0.0 { rate.ln() } else { NEG }]);

                let mut per_column = [[0u8; STATES]; 16];
                for bits in 0u8..16 {
                    for s in 0..STATES {
                        per_column[usize::from(bits)][s] = ((bits >> SA[s]) & 1) + ((bits >> SB[s]) & 1);
                    }
                }
                gval[p].push(per_column);
            }

            for m in 0..markers.saturating_sub(1) {
                let r = params.recombination[m];
                let ln_r = r.ln();
                let ln_c = (1.0 - r).ln();
                let mut table = [NEG; NJ];
                for (from, a) in crate::cross_model::ALL_STATES.iter().enumerate() {
                    for (to, b) in crate::cross_model::ALL_STATES.iter().enumerate() {
                        let k = f64::from(a.switch_count(*b));
                        table[from * STATES + to] = k * ln_r + (2.0 - k) * ln_c;
                    }
                }
                trans[p].push(table);
            }
        }

        let gl = cross.child_gl.clone();

        // Achievable contribution from one parent's state at one marker, as a
        // mask over the three possible values, so the personalised emission can
        // be maximised over columns without touching the columns themselves.
        let mut achievable: [Vec<[u8; STATES]>; 2] = [Vec::new(), Vec::new()];
        for p in 0..2 {
            for m in 0..markers {
                let mut mask = [0u8; STATES];
                for s in 0..STATES {
                    let mut bits = 0u8;
                    for &column in &cols[p][m] {
                        bits |= 1 << gval[p][m][usize::from(column)][s];
                    }
                    mask[s] = bits;
                }
                achievable[p].push(mask);
            }
        }

        let mut pers = Vec::with_capacity(offspring);
        for c in 0..offspring {
            let mut rows = Vec::with_capacity(markers);
            for m in 0..markers {
                let mut row = [NEG; NJ];
                let gls = &gl[c][m];
                for sa in 0..STATES {
                    if !legal[0][m][sa] {
                        continue;
                    }
                    for sb in 0..STATES {
                        if !legal[1][m][sb] {
                            continue;
                        }
                        let mut best = NEG;
                        for g in 0..3 {
                            if achievable[0][m][sa] & (1 << g) == 0 {
                                continue;
                            }
                            for h in 0..3 {
                                if achievable[1][m][sb] & (1 << h) == 0 {
                                    continue;
                                }
                                let value = gls[g + h];
                                if value > best {
                                    best = value;
                                }
                            }
                        }
                        row[sa * STATES + sb] = best;
                    }
                }
                rows.push(row);
            }
            pers.push(rows);
        }

        let mut core = Core {
            markers,
            offspring,
            cols,
            legal,
            dr,
            trans,
            gval,
            gl,
            pers,
            back: Vec::new(),
        };
        core.rebuild_back();
        core
    }

    /// Recomputes the backward bound from the current personalised emissions.
    pub fn rebuild_back(&mut self) {
        let (markers, offspring) = (self.markers, self.offspring);
        let mut back = Vec::with_capacity(offspring);
        for c in 0..offspring {
            let mut rows = vec![[NEG; NJ]; markers];
            for s in 0..NJ {
                let sa = s / STATES;
                let sb = s % STATES;
                if self.legal[0][markers - 1][sa] && self.legal[1][markers - 1][sb] {
                    rows[markers - 1][s] = 0.0;
                }
            }
            for m in (0..markers - 1).rev() {
                let ta = &self.trans[0][m];
                let tb = &self.trans[1][m];
                let (above, below) = rows.split_at_mut(m + 1);
                let row = &mut above[m];
                let next: &[f64; NJ] = &below[0];
                for s in 0..NJ {
                    let sa = s / STATES;
                    let sb = s % STATES;
                    if !self.legal[0][m][sa] || !self.legal[1][m][sb] {
                        continue;
                    }
                    let mut best = NEG;
                    for t in 0..NJ {
                        let value = next[t];
                        if value == NEG {
                            continue;
                        }
                        let na = t / STATES;
                        let nb = t % STATES;
                        let step = ta[sa * STATES + na]
                            + tb[sb * STATES + nb]
                            + self.pers[c][m + 1][t]
                            + self.dr[0][m + 1][usize::from(SA[na] == SB[na])]
                            + self.dr[1][m + 1][usize::from(SA[nb] == SB[nb])]
                            + value;
                        if step > best {
                            best = step;
                        }
                    }
                    row[s] = best;
                }
            }
            back.push(rows);
        }
        self.back = back;
    }

    /// Carries a forward vector across one interval, without the emission at
    /// the far end. What reaches every state once the transitions are paid.
    pub fn shift(
        &self,
        interval: usize,
        prev: &[f64; NJ],
        out: &mut [f64; NJ],
        scratch: &mut [f64; NJ],
    ) {
        let ta = &self.trans[0][interval];
        let tb = &self.trans[1][interval];
        for to_b in 0..STATES {
            for from_a in 0..STATES {
                let mut best = NEG;
                for from_b in 0..STATES {
                    let value = prev[from_a * STATES + from_b] + tb[from_b * STATES + to_b];
                    if value > best {
                        best = value;
                    }
                }
                scratch[to_b * STATES + from_a] = best;
            }
        }
        for to_a in 0..STATES {
            for to_b in 0..STATES {
                let mut best = NEG;
                for from_a in 0..STATES {
                    let value = scratch[to_b * STATES + from_a] + ta[from_a * STATES + to_a];
                    if value > best {
                        best = value;
                    }
                }
                out[to_a * STATES + to_b] = best;
            }
        }
    }

    /// Emission plus double reduction terms at one marker, for every joint
    /// state. `NEG` where the state is illegal.
    pub fn node(&self, progeny: usize, marker: usize, column_a: u8, column_b: u8, out: &mut [f64; NJ]) {
        let legal_a = &self.legal[0][marker];
        let legal_b = &self.legal[1][marker];
        let gl = &self.gl[progeny][marker];
        let ga = &self.gval[0][marker][usize::from(column_a)];
        let gb = &self.gval[1][marker][usize::from(column_b)];
        let dr_a = &self.dr[0][marker];
        let dr_b = &self.dr[1][marker];
        for sa in 0..STATES {
            if !legal_a[sa] {
                for sb in 0..STATES {
                    out[sa * STATES + sb] = NEG;
                }
                continue;
            }
            let term_a = dr_a[usize::from(SA[sa] == SB[sa])];
            for sb in 0..STATES {
                out[sa * STATES + sb] = if legal_b[sb] {
                    gl[usize::from(ga[sa] + gb[sb])]
                        + term_a
                        + dr_b[usize::from(SA[sb] == SB[sb])]
                } else {
                    NEG
                };
            }
        }
    }

    /// Forward vector at marker `0` for one progeny.
    pub fn start(&self, progeny: usize, column_a: u8, column_b: u8, out: &mut [f64; NJ]) {
        self.node(progeny, 0, column_a, column_b, out);
    }

    /// Advances one progeny's forward vector from marker `marker - 1` to
    /// `marker`, adding the transition terms of the interval between them.
    pub fn advance(
        &self,
        progeny: usize,
        marker: usize,
        column_a: u8,
        column_b: u8,
        prev: &[f64; NJ],
        out: &mut [f64; NJ],
        scratch: &mut [f64; NJ],
    ) {
        let mut node = [NEG; NJ];
        self.node(progeny, marker, column_a, column_b, &mut node);
        let ta = &self.trans[0][marker - 1];
        let tb = &self.trans[1][marker - 1];

        // `scratch[to_b * STATES + from_a]` holds the best value reaching
        // `(from_a, to_b)`, so the second pass sweeps parent A's transition
        // alone. That turns a hundred by hundred step into two hundred.
        for to_b in 0..STATES {
            for from_a in 0..STATES {
                let mut best = NEG;
                for from_b in 0..STATES {
                    let value = prev[from_a * STATES + from_b] + tb[from_b * STATES + to_b];
                    if value > best {
                        best = value;
                    }
                }
                scratch[to_b * STATES + from_a] = best;
            }
        }
        for to_a in 0..STATES {
            for to_b in 0..STATES {
                let mut best = NEG;
                for from_a in 0..STATES {
                    let value = scratch[to_b * STATES + from_a] + ta[from_a * STATES + to_a];
                    if value > best {
                        best = value;
                    }
                }
                let target = to_a * STATES + to_b;
                out[target] = if best == NEG || node[target] == NEG {
                    NEG
                } else {
                    best + node[target]
                };
            }
        }
    }

    /// Best total for one progeny over every legal path, given both parents'
    /// columns at every marker.
    pub fn best_total(&self, progeny: usize, columns_a: &[u8], columns_b: &[u8]) -> f64 {
        let mut current = [NEG; NJ];
        let mut next = [NEG; NJ];
        let mut scratch = [NEG; NJ];
        self.start(progeny, columns_a[0], columns_b[0], &mut current);
        for m in 1..self.markers {
            self.advance(
                progeny,
                m,
                columns_a[m],
                columns_b[m],
                &current,
                &mut next,
                &mut scratch,
            );
            std::mem::swap(&mut current, &mut next);
        }
        current.iter().fold(NEG, |a, b| a.max(*b))
    }
}
