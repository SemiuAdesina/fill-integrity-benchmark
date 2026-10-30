
//! Exact branch and bound over markers.
//!
//! The search assigns one column choice per marker, in marker order. What it
//! keeps at each node is, for every progeny, the vector of best scores so far
//! ending in each joint state: a partial Viterbi, shared by the whole subtree.
//! Descending one marker costs one transition step per progeny, and the value
//! of a complete assignment is read straight off the vectors at the leaf.
//!
//! Three things keep the tree small.
//!
//! * A bound. `r[m][c][j]` is the most progeny `c` can still add from marker
//!   `m` onwards if it may pick every remaining column for itself. Adding it to
//!   the partial scores bounds the whole subtree, because a real assignment is
//!   one where every progeny happens to pick the same columns. The bound is
//!   charged per child rather than only per node, which is a second large gain:
//!   the transition step is paid once per progeny, and then each surviving
//!   child is capped one progeny at a time and dropped as soon as the progeny
//!   still to come cannot make up the difference.
//! * Symmetry. Relabelling the four homologs of a parent maps an assignment to
//!   one with the same score, so only the smallest member of each orbit is
//!   expanded. Carried down the tree is the subgroup still fixing the chosen
//!   prefix; it collapses to the identity after a couple of markers, which is
//!   when the saving ends, and also when the tree is deepest.
//! * Skipping states that cannot occur. A joint state that calls for a double
//!   reduction where none is allowed is illegal at that marker, and every
//!   inner loop runs over the legal states only, which is a third of them when
//!   double reduction is mostly disallowed.
//!
//! Assignments are collected rather than stopped at: when two of them tie, the
//! caller has to choose between them, so every assignment within `tol` of the
//! best is kept.

use std::time::Instant;

use crate::solver::model::{Model, NEG, NJ, NS};

/// Most assignments kept as tying at the optimum.
const TIE_CAP: usize = 4096;
/// How often the clock is consulted.
const TICK: u64 = 1024;

/// One relabelling: where the four homologs of each parent move.
type Element = ([u8; 4], [u8; 4]);

pub struct Search<'a> {
    model: &'a Model,
    zmax: usize,
    /// Every element of `S4 x S4`.
    group: Vec<Element>,
    /// `zact[(m * g + gi) * zmax + z]`: where `z` at marker `m` moves to.
    zact: Vec<u8>,
    /// `r[m][c][j]`: most progeny `c` can add from marker `m` on, given state
    /// `j` at marker `m - 1`. Index `markers` is zero.
    r: Vec<Vec<[f64; NJ]>>,
    /// `stack[m][c][j]`: best score of markers `0..m` ending in state `j`.
    stack: Vec<Vec<[f64; NJ]>>,
    /// `adv[m][c][j]`: `stack[m][c]` pushed through the transition to marker `m`.
    adv: Vec<Vec<[f64; NJ]>>,
    /// `w[m][c][j]`: `adv` plus the double reduction term, at legal `j`.
    w: Vec<Vec<[f64; NJ]>>,
    /// `u[m][c][j]`: `w` plus `r[m + 1]`, the whole per state child bound.
    u: Vec<Vec<[f64; NJ]>>,
    /// `sub[m]`: group elements still fixing the chosen prefix.
    sub: Vec<Vec<usize>>,
    /// `cand[m]`: children worth expanding, best bound first.
    cand: Vec<Vec<(f64, usize)>>,
    /// `nb[m][c]`: what progeny `c` can still add, columns unconstrained.
    nb: Vec<Vec<f64>>,
    /// Scratch, one slot per level.
    partial: Vec<Vec<f64>>,
    alive: Vec<Vec<usize>>,
    /// The assignment along the current path.
    z: Vec<usize>,
    pub best: f64,
    pub best_z: Vec<Vec<usize>>,
    tol: f64,
    pub nodes: u64,
    budget: u64,
    deadline: Instant,
    pub complete: bool,
}

impl<'a> Search<'a> {
    pub fn new(model: &'a Model, tol: f64) -> Search<'a> {
        let markers = model.markers;
        let offspring = model.offspring;
        let zmax = model.zcount.iter().copied().max().unwrap_or(1);

        let perms = permutations4();
        let mut group: Vec<Element> = Vec::with_capacity(perms.len() * perms.len());
        for pa in perms.iter() {
            for pb in perms.iter() {
                group.push((*pa, *pb));
            }
        }
        let g = group.len();

        let mut zact = vec![0u8; markers * g * zmax];
        for m in 0..markers {
            let nb = model.ncol[1][m];
            for (gi, element) in group.iter().enumerate() {
                for z in 0..model.zcount[m] {
                    let (ia, ib) = (z / nb, z % nb);
                    let ma = mask_of(&model.columns[0][m][ia]);
                    let mb = mask_of(&model.columns[1][m][ib]);
                    let ia2 = index_of_mask(&model.columns[0][m], shuffle(&element.0, ma));
                    let ib2 = index_of_mask(&model.columns[1][m], shuffle(&element.1, mb));
                    zact[(m * g + gi) * zmax + z] = (ia2 * nb + ib2) as u8;
                }
            }
        }

        // `r`, from the last marker back to the first.
        let mut r = vec![vec![[NEG; NJ]; offspring]; markers + 1];
        for c in 0..offspring {
            for j in 0..NJ {
                r[markers][c][j] = 0.0;
            }
        }
        let mut tmp = [0.0f64; NJ];
        let mut mid = [0.0f64; NJ];
        let mut out = [0.0f64; NJ];
        for m in (0..markers).rev() {
            let src_a = &model.legal_a[m];
            let src_b = &model.legal_b[m];
            let dst = if m == 0 { 0 } else { m - 1 };
            for c in 0..offspring {
                let gl = &model.gl[c][m];
                let base = &model.base[m];
                let next = &r[m + 1][c];
                let mut acc = [NEG; NJ];
                for z in 0..model.zcount[m] {
                    let dose = &model.dose[m][z];
                    for &j in model.legal[m].iter() {
                        tmp[j] = gl[usize::from(dose[j])] + base[j] + next[j];
                    }
                    if m == 0 {
                        out = tmp;
                    } else {
                        // `tmp` is indexed by the state at marker `m`; `r[m]`
                        // by the state at marker `m - 1`.
                        transpose(
                            &model.ta[m - 1],
                            &model.tb[m - 1],
                            &tmp,
                            &mut mid,
                            &mut out,
                            src_a,
                            src_b,
                            &model.legal_a[m - 1],
                            &model.legal_b[m - 1],
                        );
                    }
                    for &j in model.legal[dst].iter() {
                        if out[j] > acc[j] {
                            acc[j] = out[j];
                        }
                    }
                }
                for &j in model.legal[dst].iter() {
                    r[m][c][j] = acc[j];
                }
            }
        }

        let levels = markers + 1;
        let table = || vec![[0.0f64; NJ]; offspring];
        Search {
            model,
            zmax,
            group,
            zact,
            r,
            stack: vec![table(); levels],
            adv: vec![table(); levels],
            w: vec![table(); levels],
            u: vec![table(); levels],
            sub: vec![Vec::new(); levels],
            cand: vec![Vec::new(); levels],
            nb: vec![vec![0.0f64; offspring]; levels],
            partial: vec![vec![0.0f64; zmax]; levels],
            alive: vec![Vec::new(); levels],
            z: vec![0usize; markers],
            best: f64::NEG_INFINITY,
            best_z: Vec::new(),
            tol,
            nodes: 0,
            budget: u64::MAX,
            deadline: Instant::now(),
            complete: true,
        }
    }

    /// Searches, starting from an assignment already known to score `seed`.
    ///
    /// Returns whether every branch was expanded or bounded away, so that
    /// `best` is the true optimum. Stops early once `budget` nodes have been
    /// expanded or `deadline` has passed, and reports that through the result:
    /// `best` is then only the best assignment the search reached.
    pub fn run(
        &mut self,
        seed: f64,
        seed_z: Vec<usize>,
        budget: u64,
        deadline: Instant,
    ) -> bool {
        self.best = seed;
        self.best_z = vec![seed_z];
        self.budget = budget;
        self.deadline = deadline;
        self.nodes = 0;
        self.complete = true;
        self.sub[0] = (0..self.group.len()).collect();
        self.descend(0);
        self.complete
    }

    /// Expands marker `m`, having fixed markers `0..m - 1`.
    fn descend(&mut self, m: usize) {
        let markers = self.model.markers;
        let offspring = self.model.offspring;
        self.nodes += 1;
        if self.nodes > self.budget || (self.nodes % TICK == 0 && Instant::now() >= self.deadline)
        {
            self.complete = false;
        }
        if !self.complete {
            return;
        }

        if m == markers {
            let mut total = 0.0f64;
            for c in 0..offspring {
                let f = &self.stack[m][c];
                let mut best = NEG;
                for &j in self.model.legal[markers - 1].iter() {
                    if f[j] > best {
                        best = f[j];
                    }
                }
                total += best;
            }
            self.record(total);
            return;
        }

        // The state that indexes the vectors held here is the one at marker
        // `m - 1`, or at marker `0` for the root, where nothing is fixed yet.
        let here = if m == 0 { 0 } else { m - 1 };
        let la = &self.model.legal_a[m];
        let lb = &self.model.legal_b[m];

        // Bound the whole node before paying for any transition.
        let mut node_bound = 0.0f64;
        for c in 0..offspring {
            let f = &self.stack[m][c];
            let rr = &self.r[m][c];
            let mut best = NEG;
            for &j in self.model.legal[here].iter() {
                let v = f[j] + rr[j];
                if v > best {
                    best = v;
                }
            }
            self.nb[m][c] = best;
            node_bound += best;
        }
        if !(node_bound > self.best - self.tol) {
            return;
        }

        // What the progeny after `c` can add at most, whatever the columns.
        let mut tail = 0.0f64;
        for c in (0..offspring).rev() {
            let before = tail;
            tail += self.nb[m][c];
            self.nb[m][c] = before;
        }

        // Children that are smallest in their orbit under the surviving
        // subgroup, and so are the only ones that need expanding.
        let zcount = self.model.zcount[m];
        let g = self.group.len();
        let floor = self.best - self.tol;
        let mut alive = std::mem::take(&mut self.alive[m]);
        let mut partial = std::mem::take(&mut self.partial[m]);
        alive.clear();
        for z in 0..zcount {
            let mut smallest = true;
            for gi in self.sub[m].iter() {
                if usize::from(self.zact[(m * g + gi) * self.zmax + z]) < z {
                    smallest = false;
                    break;
                }
            }
            if smallest {
                partial[z] = 0.0;
                alive.push(z);
            }
        }
        if alive.is_empty() {
            self.alive[m] = alive;
            self.partial[m] = partial;
            return;
        }

        // Push every progeny through the transition once, then charge the
        // surviving children for it, dropping a child as soon as the progeny
        // still to come cannot make up the difference.
        let mut mid = [0.0f64; NJ];
        for c in 0..offspring {
            if m == 0 {
                for j in 0..NJ {
                    self.adv[m][c][j] = 0.0;
                }
            } else {
                let (adv, cur) = (&mut self.adv[m][c], &self.stack[m][c]);
                transpose(
                    &self.model.ta[m - 1],
                    &self.model.tb[m - 1],
                    cur,
                    &mut mid,
                    adv,
                    &self.model.legal_a[here],
                    &self.model.legal_b[here],
                    la,
                    lb,
                );
            }
            let gl = &self.model.gl[c][m];
            let base = &self.model.base[m];
            let next = &self.r[m + 1][c];
            let (adv, w) = (&self.adv[m][c], &mut self.w[m][c]);
            let u = &mut self.u[m][c];
            for &sa in la.iter() {
                for &sb in lb.iter() {
                    let j = usize::from(sa) * NS + usize::from(sb);
                    let value = adv[j] + base[j];
                    w[j] = value;
                    u[j] = value + next[j];
                }
            }
            let u = &self.u[m][c];
            let mut keep = 0usize;
            for slot in 0..alive.len() {
                let z = alive[slot];
                let dose = &self.model.dose[m][z];
                let mut best = NEG;
                for &sa in la.iter() {
                    for &sb in lb.iter() {
                        let j = usize::from(sa) * NS + usize::from(sb);
                        let v = u[j] + gl[usize::from(dose[j])];
                        if v > best {
                            best = v;
                        }
                    }
                }
                partial[z] += best;
                if partial[z] + self.nb[m][c] > floor {
                    alive[keep] = z;
                    keep += 1;
                }
            }
            alive.truncate(keep);
            if alive.is_empty() {
                break;
            }
        }

        let mut cand = std::mem::take(&mut self.cand[m]);
        cand.clear();
        for z in alive.iter() {
            cand.push((partial[*z], *z));
        }
        cand.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.1.cmp(&b.1))
        });
        self.alive[m] = alive;
        self.partial[m] = partial;

        for (_, z) in cand.iter() {
            let z = *z;
            let mut sub = Vec::new();
            for gi in self.sub[m].iter() {
                if usize::from(self.zact[(m * g + gi) * self.zmax + z]) == z {
                    sub.push(*gi);
                }
            }
            let gl_all = &self.model.gl;
            for c in 0..offspring {
                let gl = &gl_all[c][m];
                let dose = &self.model.dose[m][z];
                let (w, row) = (&self.w[m][c], &mut self.stack[m + 1][c]);
                for &sa in la.iter() {
                    for &sb in lb.iter() {
                        let j = usize::from(sa) * NS + usize::from(sb);
                        row[j] = w[j] + gl[usize::from(dose[j])];
                    }
                }
            }
            self.z[m] = z;
            self.sub[m + 1] = sub;
            self.descend(m + 1);
            if !self.complete {
                break;
            }
        }
        self.cand[m] = cand;
    }

    fn record(&mut self, value: f64) {
        if value > self.best + self.tol {
            self.best = value;
            self.best_z.clear();
        }
        if value >= self.best - self.tol && self.best_z.len() < TIE_CAP {
            self.best_z.push(self.z.clone());
        }
    }
}

/// `out[j'] = max over j of (cur[j] + T[j][j'])`, with `T` separable per parent.
///
/// `src` names the legal states of the marker being left, `dst` those of the
/// marker being entered.
fn transpose(
    ta: &[f64; NS * NS],
    tb: &[f64; NS * NS],
    cur: &[f64; NJ],
    mid: &mut [f64; NJ],
    out: &mut [f64; NJ],
    src_a: &[u8],
    src_b: &[u8],
    dst_a: &[u8],
    dst_b: &[u8],
) {
    for &a in src_a.iter() {
        let a = usize::from(a);
        for &bp in dst_b.iter() {
            let bp = usize::from(bp);
            let mut best = NEG;
            for &b in src_b.iter() {
                let v = cur[a * NS + usize::from(b)] + tb[usize::from(b) * NS + bp];
                if v > best {
                    best = v;
                }
            }
            mid[a * NS + bp] = best;
        }
    }
    for &ap in dst_a.iter() {
        let ap = usize::from(ap);
        for &bp in dst_b.iter() {
            let bp = usize::from(bp);
            let mut best = NEG;
            for &a in src_a.iter() {
                let v = mid[usize::from(a) * NS + bp] + ta[usize::from(a) * NS + ap];
                if v > best {
                    best = v;
                }
            }
            out[ap * NS + bp] = best;
        }
    }
}

/// All 24 permutations of four homologs, in ascending image order.
fn permutations4() -> Vec<[u8; 4]> {
    let mut out = Vec::with_capacity(24);
    for a in 0..4u8 {
        for b in 0..4u8 {
            for c in 0..4u8 {
                for d in 0..4u8 {
                    let perm = [a, b, c, d];
                    let mut seen = [false; 4];
                    if perm.iter().all(|x| {
                        let fresh = !seen[*x as usize];
                        seen[*x as usize] = true;
                        fresh
                    }) {
                        out.push(perm);
                    }
                }
            }
        }
    }
    out
}

fn mask_of(column: &[u8; 4]) -> u8 {
    let mut bits = 0u8;
    for h in 0..4 {
        if column[h] != 0 {
            bits |= 1 << h;
        }
    }
    bits
}

/// Where the set `bits` of homologs lands under `perm`.
fn shuffle(perm: &[u8; 4], bits: u8) -> u8 {
    let mut out = 0u8;
    for h in 0..4 {
        if bits & (1 << h) != 0 {
            out |= 1 << perm[h];
        }
    }
    out
}

fn index_of_mask(columns: &[[u8; 4]], bits: u8) -> usize {
    for (i, column) in columns.iter().enumerate() {
        if mask_of(column) == bits {
            return i;
        }
    }
    0
}
