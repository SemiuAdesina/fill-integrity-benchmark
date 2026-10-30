
//! Picking the canonical representative of an optimal answer.
//!
//! Given the parental haplotypes, the progeny are independent: the best total is
//! the sum of their individual best totals, so an optimal answer is a choice of
//! one optimal path per progeny. Several usually tie, and
//! `spec/result-format-v1.md` fixes which one is required: the smallest under
//! the interleaved key, progeny by progeny, parent `A`'s states before parent
//! `B`'s.
//!
//! Two things make that more than a left to right walk over markers.
//!
//! * The choice is coupled along the chain. Taking a smaller state at one marker
//!   can force a larger one later, because the whole path must still be optimal.
//!   So a value is only committed to when some completion of it is still
//!   optimal, which the forward and backward vectors answer exactly.
//! * The answer has to be canonical. Relabelling two homologs that carry
//!   identical alleles leaves the haplotypes untouched and only changes the
//!   paths, and the canonical form is the smallest relabelling of the paths.
//!   While a permutation is still tied with the identity, every state assigned
//!   has to be one the identity wins or draws on; a state the permutation would
//!   win on is rejected. That is the same first difference rule the sorted row
//!   order uses, so it costs one bit per permutation still in play.

use crate::cross_input::Cross;
use crate::cross_model::{
    Haplotypes, Parent, ProgenyPath, State, ALL_STATES, HOMOLOGS, STATES,
};
use crate::solver::tables::{Tables, JOINTS};
use crate::solver::viterbi::{maxplus, Chain};

/// How far below its own optimum a progeny's path may fall and still count as
/// optimal. Only floating point noise is meant to fit under this.
pub const PATH_TOL: f64 = 1e-9;

/// Every permutation of the four homologs, as image tuples in ascending order.
fn all_permutations() -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::new();
    for a in 0..HOMOLOGS as u8 {
        for b in 0..HOMOLOGS as u8 {
            for c in 0..HOMOLOGS as u8 {
                for d in 0..HOMOLOGS as u8 {
                    let p = [a, b, c, d];
                    let mut seen = [false; HOMOLOGS];
                    if p.iter().all(|x| !std::mem::replace(&mut seen[*x as usize], true)) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

/// The haplotypes a column assignment describes for one parent.
/// The column this parent was assigned at a marker.
fn pick_of(pair: (usize, usize), parent: Parent) -> usize {
    match parent {
        Parent::A => pair.0,
        Parent::B => pair.1,
    }
}

pub fn haplotypes_of(tables: &Tables, picks: &[(usize, usize)], parent: Parent) -> Haplotypes {
    let p = parent.index();
    let mut haps = Haplotypes::zeros(tables.markers);
    for m in 0..tables.markers {
        let mask = tables.cols[p][m][pick_of(picks[m], parent)];
        for h in 0..HOMOLOGS {
            haps.set(h, m, (mask >> h) & 1);
        }
    }
    haps
}

/// The same assignment with both parents' rows in ascending order.
///
/// Relabelling the homologs of a parent permutes the four bits of every column
/// that parent holds, and every permutation of a column is itself an admissible
/// column of the same dosage, so relabelling turns one assignment into another
/// assignment with an identical score. The canonical form asks for the rows in
/// ascending order, and the local search does not keep them that way, so an
/// assignment is put in order before it competes with the ones the exact search
/// produces.
pub fn sorted_picks(tables: &Tables, picks: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut out = picks.to_vec();
    for parent in Parent::BOTH {
        let p = parent.index();
        let haps = haplotypes_of(tables, picks, parent);
        let mut to: Vec<usize> = (0..HOMOLOGS).collect();
        // Stable, so rows that are identical keep their relative order.
        to.sort_by(|left, right| haps.row(*left).cmp(haps.row(*right)));
        if to.iter().enumerate().all(|(i, old)| i == *old) {
            continue;
        }
        for m in 0..tables.markers {
            let held = tables.cols[p][m][pick_of(picks[m], parent)];
            let mut moved = 0u8;
            for i in 0..HOMOLOGS {
                moved |= ((held >> to[i]) & 1) << i;
            }
            let index = tables.cols[p][m]
                .iter()
                .position(|mask| *mask == moved)
                .expect("relabelling a column gives another admissible column");
            if p == 0 {
                out[m].0 = index;
            } else {
                out[m].1 = index;
            }
        }
    }
    out
}

/// The rows of `haps` flattened row major, the first half of the order the
/// specification uses to choose between distinct optima.
pub fn flat_rows(haps: &Haplotypes) -> Vec<u8> {
    let mut out = Vec::with_capacity(HOMOLOGS * haps.markers());
    for h in 0..HOMOLOGS {
        out.extend_from_slice(haps.row(h));
    }
    out
}

/// Which homologs carry identical alleles at every marker. The canonical form is
/// only ambiguous inside these groups.
fn row_groups(haps: &Haplotypes) -> [u8; HOMOLOGS] {
    let mut group = [0u8; HOMOLOGS];
    let mut next = 0u8;
    for h in 0..HOMOLOGS {
        if h > 0 && haps.row(h) != haps.row(h - 1) {
            next += 1;
        }
        group[h] = next;
    }
    group
}

/// Chooses the canonical optimal paths for a fixed pair of haplotypes.
pub struct Picker<'a> {
    tables: &'a Tables,
    markers: usize,
    offspring: usize,

    /// `[progeny][marker][joint]` log term of holding a joint state at a marker:
    /// the emission plus both parents' double reduction terms.
    em: Vec<Vec<[f64; JOINTS]>>,
    /// `[progeny][marker][joint]` most the markers after `marker` can contribute,
    /// nothing pinned.
    out: Vec<Vec<[f64; JOINTS]>>,
    /// `[progeny]` best total for that progeny.
    opt: Vec<f64>,

    /// Permutations that leave this parent's rows untouched, `[parent]`.
    group: [Vec<[u8; HOMOLOGS]>; 2],
    /// `[parent][permutation][state]` where that state lands under it.
    remap: [Vec<[u8; STATES]>; 2],

    /// `[parent][progeny][marker]` chosen states, as indices into `ALL_STATES`.
    chosen: [Vec<Vec<usize>>; 2],
    /// Scratch: the current progeny's pinned parent `A` states.
    pin_a: Vec<usize>,
    /// Scratch: backward vectors for the current progeny's parent `B` chain,
    /// with parent `A` pinned and parent `B` free.
    back_b: Vec<[f64; STATES]>,
}

impl<'a> Picker<'a> {
    pub fn new(cross: &'a Cross, tables: &'a Tables, picks: &'a [(usize, usize)]) -> Picker<'a> {
        let chain = Chain::new(cross, tables);
        let markers = cross.markers;
        let offspring = cross.offspring;

        let mut em = Vec::with_capacity(offspring);
        let mut out = Vec::with_capacity(offspring);
        let mut opt = Vec::with_capacity(offspring);
        for c in 0..offspring {
            let mut rows = vec![[f64::NEG_INFINITY; JOINTS]; markers];
            for m in 0..markers {
                let mask_a = tables.cols[0][m][picks[m].0];
                let mask_b = tables.cols[1][m][picks[m].1];
                chain.emission(c, m, mask_a, mask_b, &mut rows[m]);
                for j in 0..JOINTS {
                    rows[m][j] += tables.drj[m][j];
                }
            }

            let mut fwd = rows[0];
            for m in 1..markers {
                let mut next = [f64::NEG_INFINITY; JOINTS];
                maxplus(&fwd, &tables.tmat[0][m - 1], &tables.tmat[1][m - 1], &mut next);
                for j in 0..JOINTS {
                    next[j] += rows[m][j];
                }
                fwd = next;
            }
            let mut best = f64::NEG_INFINITY;
            for v in fwd.iter() {
                if *v > best {
                    best = *v;
                }
            }
            opt.push(best);

            // Backward: what the markers after `m` can still add, nothing
            // pinned. `out[markers - 1]` is a flat zero, there is nothing after
            // the last marker.
            let mut suffix = vec![[0.0f64; JOINTS]; markers];
            for m in (0..markers - 1).rev() {
                let mut rest = [f64::NEG_INFINITY; JOINTS];
                for j in 0..JOINTS {
                    rest[j] = rows[m + 1][j] + suffix[m + 1][j];
                }
                let mut here = [f64::NEG_INFINITY; JOINTS];
                maxplus(&rest, &tables.tmat[0][m], &tables.tmat[1][m], &mut here);
                suffix[m] = here;
            }
            em.push(rows);
            out.push(suffix);
        }

        let mut group = [Vec::new(), Vec::new()];
        let mut remap = [Vec::new(), Vec::new()];
        for parent in Parent::BOTH {
            let p = parent.index();
            let haps = haplotypes_of(tables, picks, parent);
            let rows = row_groups(&haps);
            let mut perms = Vec::new();
            let mut maps = Vec::new();
            for perm in all_permutations() {
                if (0..HOMOLOGS).all(|h| rows[usize::from(perm[h])] == rows[h]) {
                    let mut row = [0u8; STATES];
                    for (s, state) in ALL_STATES.iter().enumerate() {
                        row[s] = state.remap(&perm).canonical_index() as u8;
                    }
                    perms.push(perm);
                    maps.push(row);
                }
            }
            group[p] = perms;
            remap[p] = maps;
        }

        Picker {
            tables,
            markers,
            offspring,
            em,
            out,
            opt,
            group,
            remap,
            chosen: [vec![vec![0usize; markers]; offspring], vec![vec![0usize; markers]; offspring]],
            pin_a: vec![0usize; markers],
            back_b: vec![[0.0f64; STATES]; markers],
        }
    }

    /// Runs the choice. Always succeeds: some optimal path always exists.
    pub fn run(&mut self) {
        let tied_a = (1u32 << self.group[0].len()) - 1;
        let tied_b = (1u32 << self.group[1].len()) - 1;
        let done = self.progeny(0, tied_a, tied_b);
        debug_assert!(done, "no canonical optimal path was found");
    }

    /// The paths that were chosen, as a `ProgenyPath` per progeny.
    pub fn paths(&self) -> Vec<ProgenyPath> {
        let mut out = Vec::with_capacity(self.offspring);
        for c in 0..self.offspring {
            let from_a = self.chosen[0][c].iter().map(|s| ALL_STATES[*s]).collect();
            let from_b = self.chosen[1][c].iter().map(|s| ALL_STATES[*s]).collect();
            out.push(ProgenyPath { from_a, from_b });
        }
        out
    }

    /// Best total of the assignment the paths were chosen against.
    pub fn total(&self) -> f64 {
        self.opt.iter().sum()
    }

    /// Decides progeny `c` onwards. `tied_a` and `tied_b` are the permutations
    /// that have not yet lost to the identity.
    fn progeny(&mut self, c: usize, tied_a: u32, tied_b: u32) -> bool {
        if c == self.offspring {
            return true;
        }
        self.pick_a(c, 0, None, tied_a, tied_b)
    }

    /// Whether assigning state `value` to `parent` keeps the identity the
    /// smallest relabelling, and which permutations it settles.
    fn advance(&self, parent: usize, value: usize, tied: u32) -> Option<u32> {
        let mut next = tied;
        for k in 0..self.group[parent].len() {
            let bit = 1u32 << k;
            if next & bit == 0 {
                continue;
            }
            let image = usize::from(self.remap[parent][k][value]);
            if image < value {
                // This permutation would produce a smaller signature.
                return None;
            }
            if image > value {
                next &= !bit;
            }
        }
        Some(next)
    }

    /// Chooses parent `A`'s state at marker `m` of progeny `c`, then the rest.
    ///
    /// `prev` is the forward vector at `m - 1` with every parent `A` state
    /// before `m` pinned, or `None` at the first marker.
    fn pick_a(
        &mut self,
        c: usize,
        m: usize,
        prev: Option<&[f64; JOINTS]>,
        tied_a: u32,
        tied_b: u32,
    ) -> bool {
        let markers = self.markers;
        let mut stepped = [f64::NEG_INFINITY; JOINTS];
        match prev {
            None => stepped = self.em[c][0],
            Some(before) => {
                maxplus(
                    before,
                    &self.tables.tmat[0][m - 1],
                    &self.tables.tmat[1][m - 1],
                    &mut stepped,
                );
                for j in 0..JOINTS {
                    stepped[j] += self.em[c][m][j];
                }
            }
        }

        for value in 0..STATES {
            let next_a = match self.advance(0, value, tied_a) {
                Some(t) => t,
                None => continue,
            };
            let mut cur = [f64::NEG_INFINITY; JOINTS];
            for sb in 0..STATES {
                let j = value * STATES + sb;
                cur[j] = stepped[j];
            }
            // Parent `A` is only pinned up to `m` here, so this is optimistic:
            // the later markers are still free. A prefix that passes can still
            // fail once every marker is pinned, and then it is retried.
            let mut best = f64::NEG_INFINITY;
            for j in 0..JOINTS {
                let v = cur[j] + self.out[c][m][j];
                if v > best {
                    best = v;
                }
            }
            if !(best >= self.opt[c] - PATH_TOL) {
                continue;
            }

            self.pin_a[m] = value;
            if m + 1 == markers {
                let mut total = f64::NEG_INFINITY;
                for v in cur.iter() {
                    if *v > total {
                        total = *v;
                    }
                }
                if total >= self.opt[c] - PATH_TOL && self.pick_b(c, 0, None, next_a, tied_b) {
                    return true;
                }
                continue;
            }
            if self.pick_a(c, m + 1, Some(&cur), next_a, tied_b) {
                return true;
            }
        }
        false
    }

    /// Chooses parent `B`'s state at marker `m` of progeny `c`, parent `A`
    /// already pinned at every marker.
    ///
    /// `prev` is parent `B`'s forward vector at `m - 1`, or `None` at the first
    /// marker.
    fn pick_b(
        &mut self,
        c: usize,
        m: usize,
        prev: Option<&[f64; STATES]>,
        tied_a: u32,
        tied_b: u32,
    ) -> bool {
        let markers = self.markers;
        if m == 0 {
            self.prepare_b(c);
        }

        let jump_a = if m > 0 {
            self.tables.tmat[0][m - 1][self.pin_a[m - 1] * STATES + self.pin_a[m]]
        } else {
            0.0
        };
        let row_b = &self.tables.tmat[1][m.saturating_sub(1)];

        for value in 0..STATES {
            let next_b = match self.advance(1, value, tied_b) {
                Some(t) => t,
                None => continue,
            };
            let mut base = self.em[c][m][self.pin_a[m] * STATES + value];
            if let Some(before) = prev {
                let mut reached = f64::NEG_INFINITY;
                for sp in 0..STATES {
                    if before[sp] == f64::NEG_INFINITY {
                        continue;
                    }
                    let v = before[sp] + jump_a + row_b[sp * STATES + value];
                    if v > reached {
                        reached = v;
                    }
                }
                base += reached;
            }
            // Parent `A` is pinned throughout and every later parent `B` state
            // is free, so this is exact, not optimistic: passing it means a
            // completion exists.
            if !(base + self.back_b[m][value] >= self.opt[c] - PATH_TOL) {
                continue;
            }

            if m + 1 == markers {
                for m2 in 0..markers {
                    self.chosen[0][c][m2] = self.pin_a[m2];
                }
                self.chosen[1][c][m] = value;
                if self.progeny(c + 1, tied_a, next_b) {
                    return true;
                }
                continue;
            }
            self.chosen[1][c][m] = value;
            let mut next = [f64::NEG_INFINITY; STATES];
            next[value] = base;
            if self.pick_b(c, m + 1, Some(&next), tied_a, next_b) {
                return true;
            }
        }
        false
    }

    /// Backward vectors for the current progeny's parent `B` chain: parent `A`
    /// pinned at every marker, parent `B` free.
    fn prepare_b(&mut self, c: usize) {
        let markers = self.markers;
        for sb in 0..STATES {
            self.back_b[markers - 1][sb] = 0.0;
        }
        for m in (0..markers - 1).rev() {
            let jump_a = self.tables.tmat[0][m][self.pin_a[m] * STATES + self.pin_a[m + 1]];
            let row_b = &self.tables.tmat[1][m];
            let mut here = [f64::NEG_INFINITY; STATES];
            for sb in 0..STATES {
                let mut best = f64::NEG_INFINITY;
                for next in 0..STATES {
                    let v = jump_a
                        + row_b[sb * STATES + next]
                        + self.em[c][m + 1][self.pin_a[m + 1] * STATES + next]
                        + self.back_b[m + 1][next];
                    if v > best {
                        best = v;
                    }
                }
                here[sb] = best;
            }
            self.back_b[m] = here;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_permutation_is_a_permutation() {
        let perms = all_permutations();
        assert_eq!(perms.len(), 24);
        for p in &perms {
            let mut seen = [false; HOMOLOGS];
            assert!(p.iter().all(|x| !std::mem::replace(&mut seen[*x as usize], true)));
        }
    }

    #[test]
    fn row_groups_split_identical_neighbours() {
        let mut haps = Haplotypes::zeros(3);
        haps.set(0, 0, 1);
        haps.set(1, 0, 1);
        haps.set(2, 1, 1);
        haps.set(3, 2, 1);
        // Rows 0 and 1 are both [1,0,0]; rows 2 and 3 are all different.
        assert_eq!(row_groups(&haps), [0, 0, 1, 2]);
    }
}
