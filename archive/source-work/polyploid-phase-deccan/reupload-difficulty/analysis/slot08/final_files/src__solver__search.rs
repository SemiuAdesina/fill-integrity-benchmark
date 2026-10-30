// Branch and bound over the parents' allele columns.
//
// The columns are chosen marker by marker. At every node each progeny carries
// the exact forward Viterbi vector for the markers already chosen, and the
// remaining markers are bounded by letting every progeny pick columns of its
// own there. No reconstruction may do that, so the bound is never below what a
// completion can reach, and it is exact once the last marker is chosen.
//
// Homolog labels are arbitrary, so only canonical assignments are visited: the
// four rows of a parent are held in ascending lexicographic order, which every
// assignment can be put into by relabelling and which removes the twenty four
// fold symmetry. The state that tracks this is where the rows currently stand
// apart, three bits between the four rows.

use crate::cross_model::STATES;
use crate::solver::core::{Core, NJ, NEG};

/// One candidate assignment of columns to both parents.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub score: f64,
    pub columns_a: Vec<u8>,
    pub columns_b: Vec<u8>,
}

/// Allele columns open to one parent at one marker, given how far its rows have
/// already been told apart, paired with the new state of that split.
type Options = Vec<Vec<Vec<(u8, u8)>>>;

fn build_options(core: &Core, parent: usize) -> Options {
    let mut out = Vec::with_capacity(core.markers);
    for m in 0..core.markers {
        let mut per_cut = Vec::with_capacity(8);
        for cut in 0u8..8 {
            let mut choices = Vec::new();
            for &column in &core.cols[parent][m] {
                let mut valid = true;
                for h in 0..3u8 {
                    if cut & (1 << h) == 0 {
                        let low = (column >> h) & 1;
                        let high = (column >> (h + 1)) & 1;
                        if low > high {
                            valid = false;
                            break;
                        }
                    }
                }
                if !valid {
                    continue;
                }
                let mut next_cut = cut;
                for h in 0..3u8 {
                    if ((column >> h) & 1) == 0 && ((column >> (h + 1)) & 1) == 1 {
                        next_cut |= 1 << h;
                    }
                }
                choices.push((column, next_cut));
            }
            per_cut.push(choices);
        }
        out.push(per_cut);
    }
    out
}

pub struct Search<'a> {
    core: &'a Core,
    options: [Options; 2],
    /// Forward vectors at each marker for the branch being explored.
    stack: Vec<Vec<[f64; NJ]>>,
    scratch: [f64; NJ],
    probe: Vec<[f64; NJ]>,
    chosen_a: Vec<u8>,
    chosen_b: Vec<u8>,
    best: f64,
    best_a: Vec<u8>,
    best_b: Vec<u8>,
    /// Prune anything that cannot reach this.
    floor: f64,
    collecting: bool,
    found: Vec<Candidate>,
    pub nodes: u64,
    pub leaves: u64,
}

impl<'a> Search<'a> {
    pub fn new(core: &'a Core) -> Search<'a> {
        let stack = vec![vec![[NEG; NJ]; core.offspring]; core.markers];
        Search {
            core,
            options: [build_options(core, 0), build_options(core, 1)],
            stack,
            scratch: [NEG; NJ],
            probe: vec![[NEG; NJ]; core.offspring],
            chosen_a: vec![0; core.markers],
            chosen_b: vec![0; core.markers],
            best: NEG,
            best_a: vec![0; core.markers],
            best_b: vec![0; core.markers],
            floor: NEG,
            collecting: false,
            found: Vec::new(),
            nodes: 0,
            leaves: 0,
        }
    }

    /// Seeds the search with a known assignment, so it starts by pruning
    /// against a real reconstruction rather than against nothing.
    pub fn seed(&mut self, score: f64, columns_a: &[u8], columns_b: &[u8]) {
        if score > self.best {
            self.best = score;
            self.best_a = columns_a.to_vec();
            self.best_b = columns_b.to_vec();
        }
    }

    pub fn best(&self) -> f64 {
        self.best
    }

    pub fn best_columns(&self) -> (Vec<u8>, Vec<u8>) {
        (self.best_a.clone(), self.best_b.clone())
    }

    /// Runs one pass. Below `floor` nothing is kept; with `collecting` every
    /// assignment that reaches the floor is recorded rather than only the best.
    pub fn run(&mut self, floor: f64, collecting: bool) -> Vec<Candidate> {
        self.floor = floor;
        self.collecting = collecting;
        self.found.clear();
        let initial = self.options[0][0][0].clone();
        let partner = self.options[1][0][0].clone();
        let mut children = Vec::new();
        for &(va, cut_a) in &initial {
            for &(vb, cut_b) in &partner {
                self.chosen_a[0] = va;
                self.chosen_b[0] = vb;
                for c in 0..self.core.offspring {
                    self.core.start(c, va, vb, &mut self.stack[0][c]);
                }
                children.push((self.bound(0), va, vb, cut_a, cut_b));
            }
        }
        children.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap());
        for &(bound, va, vb, cut_a, cut_b) in &children {
            if !self.collecting && bound < self.best {
                continue;
            }
            if bound < self.floor {
                continue;
            }
            self.chosen_a[0] = va;
            self.chosen_b[0] = vb;
            for c in 0..self.core.offspring {
                self.core.start(c, va, vb, &mut self.stack[0][c]);
            }
            self.descend(0, cut_a, cut_b);
        }
        std::mem::take(&mut self.found)
    }

    fn bound(&self, marker: usize) -> f64 {
        let mut total = 0.0;
        for c in 0..self.core.offspring {
            let forward = &self.stack[marker][c];
            let back = &self.core.back[c][marker];
            let mut best = NEG;
            for s in 0..NJ {
                let value = forward[s] + back[s];
                if value > best {
                    best = value;
                }
            }
            total += best;
        }
        total
    }

    fn descend(&mut self, marker: usize, cut_a: u8, cut_b: u8) {
        self.nodes += 1;
        let bound = self.bound(marker);

        if marker + 1 == self.core.markers {
            self.leaves += 1;
            if bound >= self.floor {
                self.found.push(Candidate {
                    score: bound,
                    columns_a: self.chosen_a.clone(),
                    columns_b: self.chosen_b.clone(),
                });
            }
            if bound > self.best {
                self.best = bound;
                self.best_a = self.chosen_a.clone();
                self.best_b = self.chosen_b.clone();
            }
            return;
        }

        if !self.collecting && bound < self.best {
            return;
        }
        if bound < self.floor {
            return;
        }

        let next = marker + 1;
        let mut children: Vec<(f64, u8, u8, u8, u8)> = Vec::new();
        for &(va, next_a) in &self.options[0][next][usize::from(cut_a)] {
            for &(vb, next_b) in &self.options[1][next][usize::from(cut_b)] {
                self.chosen_a[next] = va;
                self.chosen_b[next] = vb;
                for c in 0..self.core.offspring {
                    self.core.advance(
                        c,
                        next,
                        va,
                        vb,
                        &self.stack[marker][c],
                        &mut self.probe[c],
                        &mut self.scratch,
                    );
                }
                let probe = std::mem::replace(&mut self.probe, vec![[NEG; NJ]; self.core.offspring]);
                let stack_row =
                    std::mem::replace(&mut self.stack[next], probe);
                children.push((self.bound(next), va, vb, next_a, next_b));
                self.probe = std::mem::replace(&mut self.stack[next], stack_row);
            }
        }
        children.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap());

        for &(child_bound, va, vb, next_a, next_b) in &children {
            if !self.collecting && child_bound < self.best {
                continue;
            }
            if child_bound < self.floor {
                continue;
            }
            self.chosen_a[next] = va;
            self.chosen_b[next] = vb;
            for c in 0..self.core.offspring {
                self.core.advance(
                    c,
                    next,
                    va,
                    vb,
                    &self.stack[marker][c],
                    &mut self.stack[next][c],
                    &mut self.scratch,
                );
            }
            self.descend(next, next_a, next_b);
        }
    }

    /// Bounded value of letting every progeny pick its own columns throughout.
    pub fn root_bound(&self) -> f64 {
        let mut total = 0.0;
        for c in 0..self.core.offspring {
            let mut best = NEG;
            for s in 0..NJ {
                let sa = s / STATES;
                let sb = s % STATES;
                if !self.core.legal[0][0][sa] || !self.core.legal[1][0][sb] {
                    continue;
                }
                let value = self.core.pers[c][0][s]
                    + self.core.dr[0][0][usize::from(sa / STATES == sb % STATES)]
                    + self.core.back[c][0][s];
                if value > best {
                    best = value;
                }
            }
            total += best;
        }
        total
    }
}
