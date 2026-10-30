
//! Local improvement of a parental assignment.
//!
//! The search over parental columns is what the exact pass has to prove
//! something about, and it goes far faster when it starts from the true optimum.
//! This module runs coordinate ascent: re-optimise one marker's column pair at a
//! time, exactly, against the paths the rest of the assignment induces.
//!
//! A single marker can be re-optimised without rerunning every Viterbi. With
//! forward and backward vectors in hand, replacing the columns at one marker
//! only touches the emission there, so a candidate costs one pass over the
//! hundred joint states per progeny.

use crate::cross_input::Cross;
use crate::cross_model::STATES;
use crate::solver::tables::{Tables, JOINTS};
use crate::solver::viterbi::{maxplus, Chain};

/// Coordinate ascent over the column pair held at each marker.
pub struct Local<'a> {
    cross: &'a Cross,
    tables: &'a Tables,
    chain: Chain<'a>,
    forward: Vec<Vec<[f64; JOINTS]>>,
    backward: Vec<Vec<[f64; JOINTS]>>,
    into: Vec<Vec<[f64; JOINTS]>>,
    out_of: Vec<Vec<[f64; JOINTS]>>,
}

impl<'a> Local<'a> {
    pub fn new(cross: &'a Cross, tables: &'a Tables) -> Local<'a> {
        let offspring = cross.offspring;
        let markers = cross.markers;
        Local {
            cross,
            tables,
            chain: Chain::new(cross, tables),
            forward: vec![Vec::new(); offspring],
            backward: vec![Vec::new(); offspring],
            into: vec![vec![[0.0; JOINTS]; markers]; offspring],
            out_of: vec![vec![[0.0; JOINTS]; markers]; offspring],
        }
    }

    /// Recomputes every forward and backward vector for `picks`.
    fn refresh(&mut self, picks: &[(usize, usize)]) {
        let markers = self.tables.markers;
        for c in 0..self.cross.offspring {
            self.forward[c] = self.chain.forward(c, picks);
            self.backward[c] = self.chain.backward(c, picks);
            // Nothing precedes the first marker, so the entry cost there is a
            // flat zero; same for the exit after the last one.
            self.into[c][0] = [0.0; JOINTS];
            self.out_of[c][markers - 1] = [0.0; JOINTS];
            for m in 1..markers {
                let ta = &self.tables.tmat[0][m - 1];
                let tb = &self.tables.tmat[1][m - 1];
                let mut here = [f64::NEG_INFINITY; JOINTS];
                maxplus(&self.forward[c][m - 1], ta, tb, &mut here);
                self.into[c][m] = here;
            }
            for m in 0..markers - 1 {
                let ta = &self.tables.tmat[0][m];
                let tb = &self.tables.tmat[1][m];
                let mut here = [f64::NEG_INFINITY; JOINTS];
                maxplus(&self.backward[c][m + 1], ta, tb, &mut here);
                self.out_of[c][m] = here;
            }
        }
    }

    /// Total of `picks` with the paths optimised out.
    pub fn total(&self) -> f64 {
        let last = self.tables.markers - 1;
        let mut sum = 0.0;
        for c in 0..self.cross.offspring {
            let mut best = f64::NEG_INFINITY;
            for v in self.forward[c][last].iter() {
                if *v > best {
                    best = *v;
                }
            }
            sum += best;
        }
        sum
    }

    /// Score of putting the column pair `(ia, ib)` at `marker`, everything else
    /// and the paths left optimal.
    fn marker_total(&self, marker: usize, ia: usize, ib: usize) -> f64 {
        let mask_a = self.tables.cols[0][marker][ia];
        let mask_b = self.tables.cols[1][marker][ib];
        let mut buffer = [f64::NEG_INFINITY; JOINTS];
        let drj = &self.tables.drj[marker];
        let mut sum = 0.0;
        for c in 0..self.cross.offspring {
            self.chain.emission(c, marker, mask_a, mask_b, &mut buffer);
            let into = &self.into[c][marker];
            let out_of = &self.out_of[c][marker];
            let mut best = f64::NEG_INFINITY;
            for j in 0..JOINTS {
                let v = into[j] + buffer[j] + drj[j] + out_of[j];
                if v > best {
                    best = v;
                }
            }
            sum += best;
        }
        sum
    }

    /// Runs sweeps until no single marker can be improved.
    pub fn ascend(&mut self, picks: &mut [(usize, usize)]) -> f64 {
        let markers = self.tables.markers;
        self.refresh(picks);
        let mut current = self.total();
        let mut improved = true;
        while improved {
            improved = false;
            for m in 0..markers {
                let na = self.tables.cols[0][m].len();
                let nb = self.tables.cols[1][m].len();
                let mut best_value = current;
                let mut best_pair = picks[m];
                for ia in 0..na {
                    for ib in 0..nb {
                        if (ia, ib) == best_pair {
                            continue;
                        }
                        let v = self.marker_total(m, ia, ib);
                        if v > best_value + 1e-12 {
                            best_value = v;
                            best_pair = (ia, ib);
                        }
                    }
                }
                if best_pair != picks[m] {
                    picks[m] = best_pair;
                    current = best_value;
                    improved = true;
                    self.refresh(picks);
                }
            }
        }
        current
    }
}

/// Deterministic pseudo random source, so restarts are reproducible.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    pub fn next(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % (n as u64)) as usize
    }
}

/// Splits a joint state index into its two parent states.
pub fn split(joint: usize) -> (usize, usize) {
    (joint / STATES, joint % STATES)
}
