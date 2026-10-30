
//! The global search.
//!
//! Two facts shape it. Progeny are conditionally independent given the parental
//! haplotypes, so a progeny's best path is a small Viterbi problem; and with
//! every path fixed, the best allele column at each marker is independent of
//! every other marker. Those two give an alternating maximisation that is exact
//! in each block, and it converges in a handful of rounds from any start, which
//! makes a single evaluation of an assignment cheap.
//!
//! The alternating step on its own is nowhere near enough. The space is rugged:
//! two thousand random starts on the fixture cross produce two thousand
//! different fixed points, and the best of them is well below the optimum. So
//! the descent is embedded in a population search. A pool of good assignments
//! is kept, and each round either perturbs a pool member or crosses two of them
//! marker by marker, descends, and offers the result back to the pool. The
//! crossover is what makes the search work: two assignments that are each stuck
//! can share the markers they respectively got right.
//!
//! Where the admissible space is small enough to list, two exact stages are
//! added: a full enumeration over both parents at once when the joint space is
//! small, and otherwise an alternation that optimises one whole parent at a
//! time by enumerating it, which is exact in a far larger block than a single
//! marker.

use super::canon;
use super::dp;
use super::tables::{Ctx, NS};

/// Slack for treating two totals as the same score. Ties in the data are exact,
/// so this only has to absorb floating point noise; it is far below the `1e-8`
/// tolerance the specification allows, which keeps a run of decisions from
/// accumulating into a real loss.
const TIE: f64 = 1e-9;

/// How many distinct assignments the population holds.
const POOL: usize = 16;

/// How many markers one sweep of the hill climb may change before the tables
/// are rebuilt. The tables go stale as markers change, so a long sweep turns
/// optimistic and has to be rolled back; a short one stays accurate and costs
/// one rebuild per sweep instead of one per change.
const MAX_MOVES: usize = 3;

/// A reconstruction in the solver's own representation: allele columns per
/// marker and, per progeny, the canonical index of each inheritance state.
#[derive(Clone)]
pub struct Solution {
    pub cols_a: Vec<u8>,
    pub cols_b: Vec<u8>,
    pub pa: Vec<Vec<u8>>,
    pub pb: Vec<Vec<u8>>,
    pub score: f64,
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, limit: usize) -> usize {
        if limit <= 1 {
            0
        } else {
            (self.next_u64() % limit as u64) as usize
        }
    }
}

/// Best total for fixed parental columns, with the paths that attain it.
pub fn evaluate(ctx: &Ctx, cols_a: &[u8], cols_b: &[u8]) -> (f64, Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut total = 0.0;
    let mut pa = Vec::with_capacity(ctx.offspring);
    let mut pb = Vec::with_capacity(ctx.offspring);
    for c in 0..ctx.offspring {
        let (value, path) = dp::viterbi_path(ctx, cols_a, cols_b, c);
        total += value;
        pa.push(path[0].clone());
        pb.push(path[1].clone());
    }
    (total, pa, pb)
}

/// Best allele columns for fixed paths.
///
/// With every path fixed the total separates over markers, so each marker's
/// pair of columns can be chosen on its own and this step is exact.
pub fn optimize_columns(ctx: &Ctx, pa: &[Vec<u8>], pb: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>) {
    let m = ctx.markers;
    let n = ctx.offspring;
    let mut out_a = vec![0u8; m];
    let mut out_b = vec![0u8; m];
    for k in 0..m {
        let mut best = f64::NEG_INFINITY;
        let mut best_a = ctx.opts[0][k][0];
        let mut best_b = ctx.opts[1][k][0];
        for &ca in &ctx.opts[0][k] {
            let va = ctx.contrib_a(ca);
            for &cb in &ctx.opts[1][k] {
                let vb = ctx.contrib_a(cb);
                let mut sum = 0.0;
                for c in 0..n {
                    let dose = va[usize::from(pa[c][k])] + vb[usize::from(pb[c][k])];
                    sum += ctx.gl[c][k][usize::from(dose)];
                }
                if sum > best {
                    best = sum;
                    best_a = ca;
                    best_b = cb;
                }
            }
        }
        out_a[k] = best_a;
        out_b[k] = best_b;
    }
    (out_a, out_b)
}

/// Alternating maximisation: paths against the columns, then columns against
/// the paths, until neither moves.
pub fn descend(
    ctx: &Ctx,
    cols_a: &mut Vec<u8>,
    cols_b: &mut Vec<u8>,
    rounds: usize,
) -> (f64, Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut last = f64::NEG_INFINITY;
    for _ in 0..rounds {
        let (score, pa, pb) = evaluate(ctx, cols_a, cols_b);
        if !(score > last + 1e-12) {
            return (score, pa, pb);
        }
        last = score;
        let (next_a, next_b) = optimize_columns(ctx, &pa, &pb);
        if next_a == *cols_a && next_b == *cols_b {
            return (score, pa, pb);
        }
        *cols_a = next_a;
        *cols_b = next_b;
    }
    let (score, pa, pb) = evaluate(ctx, cols_a, cols_b);
    (score, pa, pb)
}

/// Total with the columns at one marker replaced, paths re-optimised.
///
/// `g[m]` and `r[m]` bracket marker `m`: they cover everything before it and
/// everything after it, and neither depends on the columns at `m`. So the best
/// total achievable with any given replacement is a max over the hundred joint
/// states at that marker alone, and no other marker's work has to be redone to
/// try a replacement.
fn marker_total(
    ctx: &Ctx,
    m: usize,
    fw: &[Vec<[f64; NS * NS]>],
    bw: &[dp::Backward],
    column_a: u8,
    column_b: u8,
) -> f64 {
    let ca = ctx.contrib_a(column_a);
    let cb = ctx.contrib_a(column_b);
    let tail = m + 1 < ctx.markers;
    let mut total = 0.0;
    for c in 0..ctx.offspring {
        let g = &fw[c][m];
        let r = if tail { Some(&bw[c].r[m]) } else { None };
        let gl = &ctx.gl[c][m];
        let dr = &ctx.dr_sum[m];
        let mut best = f64::NEG_INFINITY;
        for i in 0..NS {
            if !ctx.legal[0][m][i] {
                continue;
            }
            for j in 0..NS {
                if !ctx.legal[1][m][j] {
                    continue;
                }
                let s = i * NS + j;
                let mut value = g[s] + gl[usize::from(ca[i] + cb[j])] + dr[s];
                if let Some(rr) = r {
                    value += rr[s];
                }
                if value > best {
                    best = value;
                }
            }
        }
        total += best;
    }
    total
}

/// The same hill climb, one accepted change at a time.
///
/// Rebuilding the tables after every change makes each step exact, so this
/// never overshoots, but it costs a rebuild per change rather than per sweep.
/// It is the fallback for when a batch sweep turns out to have been too
/// optimistic.
fn polish_exact(ctx: &Ctx, cols_a: &mut Vec<u8>, cols_b: &mut Vec<u8>, mut current: f64) -> f64 {
    loop {
        let fw: Vec<Vec<[f64; NS * NS]>> =
            (0..ctx.offspring).map(|c| dp::forward(ctx, cols_a, cols_b, c)).collect();
        let bw: Vec<dp::Backward> =
            (0..ctx.offspring).map(|c| dp::backward(ctx, cols_a, cols_b, c)).collect();

        let mut moved = false;
        for m in 0..ctx.markers {
            let mut best = current;
            let mut pick = None;
            for &a in &ctx.opts[0][m] {
                for &b in &ctx.opts[1][m] {
                    if a == cols_a[m] && b == cols_b[m] {
                        continue;
                    }
                    let value = marker_total(ctx, m, &fw, &bw, a, b);
                    if value > best + 1e-12 {
                        best = value;
                        pick = Some((a, b));
                    }
                }
            }
            if let Some((a, b)) = pick {
                cols_a[m] = a;
                cols_b[m] = b;
                current = best;
                moved = true;
                break;
            }
        }
        if !moved {
            break;
        }
    }
    current
}

/// Hill climbs one marker at a time against the true objective.
///
/// This is a strictly different neighbourhood from the alternating step: the
/// alternating step re-optimises every marker's columns against fixed paths,
/// while this re-optimises every path against a change at one marker. Neither
/// contains the other, so [`refine`] runs them in turn.
pub fn polish(ctx: &Ctx, cols_a: &mut Vec<u8>, cols_b: &mut Vec<u8>) -> f64 {
    let mut current = total_of(ctx, cols_a, cols_b);
    loop {
        let fw: Vec<Vec<[f64; NS * NS]>> =
            (0..ctx.offspring).map(|c| dp::forward(ctx, cols_a, cols_b, c)).collect();
        let bw: Vec<dp::Backward> =
            (0..ctx.offspring).map(|c| dp::backward(ctx, cols_a, cols_b, c)).collect();

        // One sweep against a single pair of tables. The tables go stale as
        // soon as a marker changes, so the estimate is optimistic; the true
        // total is measured afterwards and the sweep is rolled back if it did
        // not deliver.
        let held_a = cols_a.clone();
        let held_b = cols_b.clone();
        let mut estimate = current;
        let mut moved = 0usize;
        for m in 0..ctx.markers {
            if moved >= MAX_MOVES {
                break;
            }
            let mut best = estimate;
            let mut pick = None;
            for &a in &ctx.opts[0][m] {
                for &b in &ctx.opts[1][m] {
                    if a == cols_a[m] && b == cols_b[m] {
                        continue;
                    }
                    let value = marker_total(ctx, m, &fw, &bw, a, b);
                    if value > best + 1e-12 {
                        best = value;
                        pick = Some((a, b));
                    }
                }
            }
            if let Some((a, b)) = pick {
                cols_a[m] = a;
                cols_b[m] = b;
                estimate = best;
                moved += 1;
            }
        }
        if moved == 0 {
            break;
        }
        let measured = total_of(ctx, cols_a, cols_b);
        if measured > current + 1e-12 {
            current = measured;
        } else {
            *cols_a = held_a;
            *cols_b = held_b;
            // The sweep was too optimistic to trust, so finish one change at a
            // time, which never overshoots.
            return polish_exact(ctx, cols_a, cols_b, current);
        }
    }
    current
}

/// Alternating step and one marker hill climb to a point neither can move.
pub fn refine(ctx: &Ctx, cols_a: &mut Vec<u8>, cols_b: &mut Vec<u8>, limit: usize) -> f64 {
    let mut score = descend(ctx, cols_a, cols_b, 64).0;
    for _ in 0..limit {
        let polished = polish(ctx, cols_a, cols_b);
        let next = descend(ctx, cols_a, cols_b, 64).0;
        if !(next > score + 1e-12 || polished > score + 1e-12) {
            score = next.max(polished);
            break;
        }
        score = next.max(polished);
    }
    score
}

/// Number of admissible assignments for one parent, saturating.
fn space_size(ctx: &Ctx, parent: usize) -> u128 {
    let mut total: u128 = 1;
    for k in 0..ctx.markers {
        total = total.saturating_mul(ctx.opts[parent][k].len() as u128);
    }
    total
}

/// Flattened row major haplotype values, the first part of the ordering that
/// `spec/result-format-v1.md` uses between distinct optima.
fn flat_rows(cols: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 * cols.len());
    for h in 0..4 {
        for column in cols {
            out.push((column >> h) & 1);
        }
    }
    out
}

fn walk(ctx: &Ctx, parent: usize, marker: usize, current: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
    if marker == ctx.markers {
        out.push(current.clone());
        return;
    }
    for &column in &ctx.opts[parent][marker] {
        current[marker] = column;
        walk(ctx, parent, marker + 1, current, out);
    }
}

fn canonical_assignments(ctx: &Ctx, parent: usize) -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    let mut current = vec![0u8; ctx.markers];
    walk(ctx, parent, 0, &mut current, &mut all);
    let mut canonical: Vec<Vec<u8>> = all.iter().map(|c| canon::canon_cols(c)).collect();
    canonical.sort_unstable();
    canonical.dedup();
    canonical
}

/// One member of the population: a total and the two column vectors.
type Member = (f64, Vec<u8>, Vec<u8>);

struct Engine<'a> {
    ctx: &'a Ctx,
    rng: Rng,
    /// Highest total seen.
    best_score: f64,
    /// The tied assignment that sorts first by haplotype rows.
    best_pair: Option<(Vec<u8>, Vec<u8>)>,
    /// Population, best total first.
    pool: Vec<Member>,
    /// Whether candidates are taken to a point the one marker hill climb also
    /// cannot move, which costs far more per candidate than the plain
    /// alternating step.
    strong: bool,
}

impl<'a> Engine<'a> {
    fn new(ctx: &'a Ctx, seed: u64) -> Engine<'a> {
        Engine {
            ctx,
            rng: Rng::new(seed),
            best_score: f64::NEG_INFINITY,
            best_pair: None,
            pool: Vec::with_capacity(POOL + 1),
            strong: false,
        }
    }

    /// Offers a candidate to the search: it is scored against the optimum, kept
    /// if it ties it, and offered to the population.
    fn submit(&mut self, score: f64, cols_a: &[u8], cols_b: &[u8]) {
        let promoted = score > self.best_score + TIE;
        if promoted {
            self.best_score = score;
        }
        if !(score > self.best_score - TIE) {
            return;
        }
        let key = (canon::canon_cols(cols_a), canon::canon_cols(cols_b));

        let rank = (flat_rows(&key.0), flat_rows(&key.1));
        let better = match &self.best_pair {
            None => true,
            Some(chosen) => {
                if promoted {
                    true
                } else {
                    let chosen_rank = (flat_rows(&chosen.0), flat_rows(&chosen.1));
                    rank < chosen_rank
                }
            }
        };
        if better {
            self.best_pair = Some(key.clone());
        }

        if self.pool.iter().any(|member| member.1 == key.0 && member.2 == key.1) {
            return;
        }
        let slot = self.pool.partition_point(|member| member.0 > score);
        if slot >= POOL {
            return;
        }
        self.pool.insert(slot, (score, key.0, key.1));
        if self.pool.len() > POOL {
            self.pool.truncate(POOL);
        }
    }

    fn random_columns(&mut self) -> (Vec<u8>, Vec<u8>) {
        let mut a = Vec::with_capacity(self.ctx.markers);
        let mut b = Vec::with_capacity(self.ctx.markers);
        for k in 0..self.ctx.markers {
            let oa = &self.ctx.opts[0][k];
            let ob = &self.ctx.opts[1][k];
            a.push(oa[self.rng.below(oa.len())]);
            b.push(ob[self.rng.below(ob.len())]);
        }
        (a, b)
    }

    /// Descends from a start and offers the result to the search.
    fn develop(&mut self, mut cols_a: Vec<u8>, mut cols_b: Vec<u8>) -> f64 {
        let score = if self.strong {
            refine(self.ctx, &mut cols_a, &mut cols_b, 8)
        } else {
            descend(self.ctx, &mut cols_a, &mut cols_b, 64).0
        };
        self.submit(score, &cols_a, &cols_b);
        score
    }

    /// Randomises columns so the next descent lands in another basin.
    fn perturb(&mut self, cols_a: &mut Vec<u8>, cols_b: &mut Vec<u8>) {
        let m = self.ctx.markers;
        match self.rng.below(6) {
            0 => {
                let k = self.rng.below(m);
                let parent = self.rng.below(2);
                let opts = &self.ctx.opts[parent][k];
                let choice = opts[self.rng.below(opts.len())];
                if parent == 0 {
                    cols_a[k] = choice;
                } else {
                    cols_b[k] = choice;
                }
            }
            1 => {
                let count = 2 + self.rng.below(4);
                for _ in 0..count {
                    let k = self.rng.below(m);
                    let parent = self.rng.below(2);
                    let opts = &self.ctx.opts[parent][k];
                    let choice = opts[self.rng.below(opts.len())];
                    if parent == 0 {
                        cols_a[k] = choice;
                    } else {
                        cols_b[k] = choice;
                    }
                }
            }
            2 => {
                let parent = self.rng.below(2);
                let start = self.rng.below(m);
                let width = 1 + self.rng.below(4);
                for k in start..m.min(start + width) {
                    let opts = &self.ctx.opts[parent][k];
                    let choice = opts[self.rng.below(opts.len())];
                    if parent == 0 {
                        cols_a[k] = choice;
                    } else {
                        cols_b[k] = choice;
                    }
                }
            }
            3 => {
                // Resample a scattered third of the markers in one parent.
                let parent = self.rng.below(2);
                for k in 0..m {
                    if self.rng.below(3) != 0 {
                        continue;
                    }
                    let opts = &self.ctx.opts[parent][k];
                    let choice = opts[self.rng.below(opts.len())];
                    if parent == 0 {
                        cols_a[k] = choice;
                    } else {
                        cols_b[k] = choice;
                    }
                }
            }
            4 => {
                let parent = self.rng.below(2);
                for k in 0..m {
                    let opts = &self.ctx.opts[parent][k];
                    let choice = opts[self.rng.below(opts.len())];
                    if parent == 0 {
                        cols_a[k] = choice;
                    } else {
                        cols_b[k] = choice;
                    }
                }
            }
            _ => {
                for k in 0..m {
                    let oa = &self.ctx.opts[0][k];
                    cols_a[k] = oa[self.rng.below(oa.len())];
                    let ob = &self.ctx.opts[1][k];
                    cols_b[k] = ob[self.rng.below(ob.len())];
                }
            }
        }
    }

    /// Mixes two assignments, taking each marker's columns from either.
    fn crossover(&mut self, left: &Member, right: &Member) -> (Vec<u8>, Vec<u8>) {
        let m = self.ctx.markers;
        let mut a = left.1.clone();
        let mut b = left.2.clone();
        if self.rng.below(2) == 0 {
            // Uniform: every marker independently from either parent.
            for k in 0..m {
                if self.rng.below(2) == 0 {
                    a[k] = right.1[k];
                }
                if self.rng.below(2) == 0 {
                    b[k] = right.2[k];
                }
            }
        } else {
            // One crossover point in each parent.
            let cut = 1 + self.rng.below(m - 1);
            for k in cut..m {
                a[k] = right.1[k];
            }
            let cut = 1 + self.rng.below(m - 1);
            for k in cut..m {
                b[k] = right.2[k];
            }
        }
        (a, b)
    }

    /// One round of the population search.
    fn step(&mut self) {
        let count = self.pool.len();
        let (cols_a, cols_b) = match count {
            0 => self.random_columns(),
            1 => {
                let mut a = self.pool[0].1.clone();
                let mut b = self.pool[0].2.clone();
                self.perturb(&mut a, &mut b);
                (a, b)
            }
            _ => {
                if self.rng.below(100) < 55 {
                    let i = self.rng.below(count);
                    let j = self.rng.below(count);
                    if i == j {
                        let mut a = self.pool[i].1.clone();
                        let mut b = self.pool[i].2.clone();
                        self.perturb(&mut a, &mut b);
                        (a, b)
                    } else {
                        self.crossover(&self.pool[i].clone(), &self.pool[j].clone())
                    }
                } else {
                    // Perturb a pool member, usually a good one but sometimes
                    // any, so the search does not only intensify.
                    let i = if self.rng.below(4) == 0 {
                        self.rng.below(count)
                    } else {
                        self.rng.below(count.min(4))
                    };
                    let mut a = self.pool[i].1.clone();
                    let mut b = self.pool[i].2.clone();
                    self.perturb(&mut a, &mut b);
                    (a, b)
                }
            }
        };
        self.develop(cols_a, cols_b);
    }
}

/// Rebuilds the answer from one assignment: the relabelling and paths that the
/// result format requires among the ones that tie.
fn finalise(ctx: &Ctx, cols_a: &[u8], cols_b: &[u8]) -> Solution {
    let perms_a = canon::sorting_permutations(cols_a);
    let perms_b = canon::sorting_permutations(cols_b);

    let backward: Vec<dp::Backward> =
        (0..ctx.offspring).map(|c| dp::backward(ctx, cols_a, cols_b, c)).collect();

    let mut best: Option<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<Vec<u8>>, Vec<Vec<u8>>)> = None;
    for pa in &perms_a {
        for pb in &perms_b {
            let ca = canon::remap_cols(cols_a, pa);
            let cb = canon::remap_cols(cols_b, pb);
            let mut out_a = Vec::with_capacity(ctx.offspring);
            let mut out_b = Vec::with_capacity(ctx.offspring);
            for c in 0..ctx.offspring {
                let (b, r) = dp::permute_backward(&backward[c].b, &backward[c].r, pa, pb);
                let (from_a, from_b) =
                    dp::min_lex_path(ctx, &b, &r, backward[c].vstar, &ca, &cb, c, TIE);
                out_a.push(from_a);
                out_b.push(from_b);
            }
            let key = canon::distinct_key(&ca, &cb, &out_a, &out_b);
            let better = match &best {
                None => true,
                Some((best_key, _, _, _, _)) => key < *best_key,
            };
            if better {
                best = Some((key, ca, cb, out_a, out_b));
            }
        }
    }

    let (_, ca, cb, out_a, out_b) = best.expect("at least one relabelling exists");
    let mut total = 0.0;
    for c in 0..ctx.offspring {
        total += path_value(ctx, &ca, &cb, &out_a[c], &out_b[c], c);
    }
    Solution { cols_a: ca, cols_b: cb, pa: out_a, pb: out_b, score: total }
}

/// The score of one progeny's written path, term by term.
///
/// The reported total has to be the scorer's value for the document that is
/// written out, which is the value of the paths in it. That is the Viterbi
/// optimum only because the paths chosen are optimal ones.
fn path_value(ctx: &Ctx, ca: &[u8], cb: &[u8], pa: &[u8], pb: &[u8], c: usize) -> f64 {
    let m = ctx.markers;
    let mut total = 0.0;
    for k in 0..m {
        let va = ctx.contrib_a(ca[k]);
        let vb = ctx.contrib_a(cb[k]);
        let i = usize::from(pa[k]);
        let j = usize::from(pb[k]);
        total += ctx.gl[c][k][usize::from(va[i] + vb[j])] + ctx.dr_sum[k][i * NS + j];
        if k + 1 < m {
            total += ctx.trans[0][k][i * NS + usize::from(pa[k + 1])]
                + ctx.trans[1][k][j * NS + usize::from(pb[k + 1])];
        }
    }
    total
}

/// Exhaustive search over both parents' admissible assignments, up to
/// relabelling of each parent independently.
fn exhaustive(ctx: &Ctx, engine: &mut Engine, a_list: &[Vec<u8>], b_list: &[Vec<u8>]) {
    for ca in a_list {
        for cb in b_list {
            let mut total = 0.0;
            for c in 0..ctx.offspring {
                total += dp::viterbi_value(ctx, ca, cb, c);
            }
            engine.submit(total, ca, cb);
        }
    }
}

/// Alternating maximisation in which one whole parent is optimised at a time by
/// enumerating that parent's assignments, the other parent's columns held
/// fixed. Each half sweep is exact in the block it touches, which is a much
/// larger neighbourhood than the alternating step reaches.
fn block_scan(ctx: &Ctx, engine: &mut Engine, a_list: &[Vec<u8>], b_list: &[Vec<u8>], sweeps: usize) {
    let mut current = engine
        .best_pair
        .clone()
        .expect("a block scan needs a starting assignment");
    for _ in 0..sweeps {
        let mut best_score = f64::NEG_INFINITY;
        let mut best_a = current.0.clone();
        for ca in a_list {
            let mut total = 0.0;
            for c in 0..ctx.offspring {
                total += dp::viterbi_value(ctx, ca, &current.1, c);
            }
            engine.submit(total, ca, &current.1);
            if total > best_score {
                best_score = total;
                best_a = ca.clone();
            }
        }

        let mut best_b = current.1.clone();
        let mut best_score_b = f64::NEG_INFINITY;
        for cb in b_list {
            let mut total = 0.0;
            for c in 0..ctx.offspring {
                total += dp::viterbi_value(ctx, &best_a, cb, c);
            }
            engine.submit(total, &best_a, cb);
            if total > best_score_b {
                best_score_b = total;
                best_b = cb.clone();
            }
        }

        let start_score = total_of(ctx, &current.0, &current.1);
        let improved = best_score_b > start_score + 1e-12;
        current = (best_a, best_b);
        if !improved {
            break;
        }
    }

    // Offer the end point, which no single parent change can improve.
    engine.submit(total_of(ctx, &current.0, &current.1), &current.0, &current.1);
}

fn total_of(ctx: &Ctx, cols_a: &[u8], cols_b: &[u8]) -> f64 {
    let mut total = 0.0;
    for c in 0..ctx.offspring {
        total += dp::viterbi_value(ctx, cols_a, cols_b, c);
    }
    total
}

/// How much work the search does, and with which random stream.
///
/// The defaults are what `solve` uses. The fields exist so the search can be
/// driven with a different budget, which is how its behaviour was tuned.
#[derive(Clone, Copy)]
pub struct Budget {
    pub seed: u64,
    pub restarts: usize,
    pub rounds: usize,
    pub strong: usize,
    pub block_sweeps: usize,
}

impl Budget {
    /// A budget matched to the size of the cross.
    ///
    /// Measuring one candidate costs work proportional to `offspring *
    /// markers`, so the number of candidates worth trying goes as its inverse.
    /// The dearer local search costs more again the more markers there are,
    /// because a hill climb over more markers accepts more changes. The
    /// constants put the small end of the declared size range at tens of
    /// thousands of candidates and keep the large end to a comparable time.
    pub fn for_size(ctx: &Ctx) -> Budget {
        let work = ctx.offspring.max(1) * ctx.markers.max(1);
        let cheap = (3_600_000 / work).clamp(2_000, 40_000);
        let strong = (4_000_000 / (work * ctx.markers.max(1))).clamp(150, 4_000);
        Budget {
            seed: 0x5068_6C79_7068_6173,
            restarts: (cheap / 40).clamp(128, 1_024),
            rounds: cheap,
            strong,
            block_sweeps: 4,
        }
    }
}

impl Default for Budget {
    fn default() -> Budget {
        Budget {
            seed: 0x5068_6C79_7068_6173,
            restarts: 512,
            rounds: 40_000,
            strong: 2_000,
            block_sweeps: 4,
        }
    }
}

/// Searches for the best reconstruction.
pub fn run(ctx: &Ctx) -> Solution {
    run_with(ctx, Budget::for_size(ctx))
}

/// Searches with an explicit budget.
pub fn run_with(ctx: &Ctx, budget: Budget) -> Solution {
    let mut engine = Engine::new(ctx, budget.seed);

    // Enumeration is only possible while the assignments can be listed. Above
    // that the search falls back to the population search.
    let listable = 400_000u128;
    let (a_list, b_list) = if space_size(ctx, 0) <= listable && space_size(ctx, 1) <= listable {
        (canonical_assignments(ctx, 0), canonical_assignments(ctx, 1))
    } else {
        (Vec::new(), Vec::new())
    };

    if !a_list.is_empty()
        && (a_list.len() as u128) * (b_list.len() as u128) <= 60_000
    {
        exhaustive(ctx, &mut engine, &a_list, &b_list);
    }

    if engine.best_pair.is_none() {
        // Structured start: the first admissible column at every marker.
        let mut low_a = Vec::with_capacity(ctx.markers);
        let mut low_b = Vec::with_capacity(ctx.markers);
        for k in 0..ctx.markers {
            low_a.push(ctx.opts[0][k][0]);
            low_b.push(ctx.opts[1][k][0]);
        }
        engine.develop(low_a, low_b);

        for _ in 0..budget.restarts {
            let (a, b) = engine.random_columns();
            engine.develop(a, b);
        }

        for _ in 0..budget.rounds {
            engine.step();
        }

        // Second pass with the dearer local search, seeded from what the first
        // pass found. It sees far fewer candidates but takes each of them to a
        // point the cheap pass cannot reach.
        engine.strong = true;
        for _ in 0..budget.strong {
            engine.step();
        }
    }

    if !a_list.is_empty() && budget.block_sweeps > 0 {
        let joint = (a_list.len() as u128) * (b_list.len() as u128);
        if joint > 60_000 {
            // One sweep costs both lists' worth of evaluations, so take as many
            // sweeps as that leaves room for and no more.
            let per_sweep = (a_list.len() as u128 + b_list.len() as u128)
                * (ctx.offspring as u128 * ctx.markers as u128 + 1);
            let sweeps = (4_000_000u128 / per_sweep).min(budget.block_sweeps as u128) as usize;
            if sweeps > 0 {
                block_scan(ctx, &mut engine, &a_list, &b_list, sweeps);
            }
        }
    }

    let winner = engine.best_pair.clone().expect("the search recorded no assignment");
    finalise(ctx, &winner.0, &winner.1)
}
