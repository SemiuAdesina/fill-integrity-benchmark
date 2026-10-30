// Scratch harness used while developing the search. Not part of the deliverable.

use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::solver::search::{local_search, Rng};
use polyploid_phase::solver::{beam, search};
use polyploid_phase::solver::tables::{Ctx, NJ, NS};

/// Branch and bound over the column chain.
///
/// The bound relaxes only the markers that are not yet fixed: every progeny
/// still runs a real Viterbi path with the real transition terms, but at a free
/// marker it may pick whichever column pair suits it. Carried as suffix values
/// `suffix[k][c][u]`, the best a progeny can still do from marker `k` on once it
/// stands in joint state `u` at marker `k - 1`.
struct Bnb<'a> {
    ctx: &'a Ctx,
    relax: Vec<Vec<[f64; NJ]>>,
    suffix: Vec<Vec<[f64; NJ]>>,
    best: f64,
    ca: Vec<u8>,
    cb: Vec<u8>,
    best_ca: Vec<u8>,
    best_cb: Vec<u8>,
    nodes: u64,
    limit: u64,
    /// Per marker, the best a marker can contribute when its column pair is
    /// shared by every progeny and their states are free.
    free_max: Vec<f64>,
    free_suffix: Vec<f64>,
    /// `N` times the largest transition term of every interval.
    trans_total: f64,
    /// Order state of every pair of homolog rows, per parent: 0 equal so far,
    /// 1 first below second, 2 first above second. Only the first two survive,
    /// which keeps one representative of each relabelling.
    order: [[u8; 6]; 2],
}

impl<'a> Bnb<'a> {
    fn new(ctx: &'a Ctx, limit: u64) -> Bnb<'a> {
        // Every dosage a joint state can reach at a marker under some column
        // pair, as a five bit mask.
        let mut reach: Vec<[u8; NJ]> = vec![[0u8; NJ]; ctx.m];
        for mm in 0..ctx.m {
            for row in ctx.dose[mm].iter() {
                for j in 0..NJ {
                    reach[mm][j] |= 1u8 << row[j];
                }
            }
        }
        let mut relax: Vec<Vec<[f64; NJ]>> = Vec::with_capacity(ctx.n);
        for c in 0..ctx.n {
            let mut per_marker = Vec::with_capacity(ctx.m);
            for mm in 0..ctx.m {
                let mut row = [f64::NEG_INFINITY; NJ];
                for j in 0..NJ {
                    let mut best = f64::NEG_INFINITY;
                    for d in 0..5u8 {
                        if reach[mm][j] & (1 << d) != 0 && ctx.gl[c][mm][d as usize] > best {
                            best = ctx.gl[c][mm][d as usize];
                        }
                    }
                    row[j] = best + ctx.drv[0][mm][j / NS] + ctx.drv[1][mm][j % NS];
                }
                per_marker.push(row);
            }
            relax.push(per_marker);
        }

        // suffix[k][c][u], for k in 1..=markers, and slot 0 holds the fully
        // relaxed total for the whole chain.
        let mut suffix: Vec<Vec<[f64; NJ]>> = vec![vec![[0.0f64; NJ]; ctx.n]; ctx.m + 1];
        for c in 0..ctx.n {
            for k in (1..=ctx.m - 1).rev() {
                let later = suffix[k + 1][c];
                let ta = &ctx.tr[0][k - 1];
                let tb = &ctx.tr[1][k - 1];
                let node = relax[c][k];
                let mut row = [0.0f64; NJ];
                for u in 0..NJ {
                    let sa = u / NS;
                    let sb = u % NS;
                    let mut best = f64::NEG_INFINITY;
                    for sa2 in 0..NS {
                        for sb2 in 0..NS {
                            let v = ta[sa * NS + sa2] + tb[sb * NS + sb2] + node[sa2 * NS + sb2] + later[sa2 * NS + sb2];
                            if v > best {
                                best = v;
                            }
                        }
                    }
                    row[u] = best;
                }
                suffix[k][c] = row;
            }
            let mut total = f64::NEG_INFINITY;
            for u in 0..NJ {
                let v = relax[c][0][u] + suffix[1][c][u];
                if v > total {
                    total = v;
                }
            }
            suffix[0][c] = [total; NJ];
        }

        Bnb {
            ctx,
            relax,
            suffix,
            best: f64::NEG_INFINITY,
            ca: vec![0u8; ctx.m],
            cb: vec![0u8; ctx.m],
            best_ca: vec![0u8; ctx.m],
            best_cb: vec![0u8; ctx.m],
            nodes: 0,
            limit,
            free_max: Vec::new(),
            free_suffix: Vec::new(),
            trans_total: 0.0,
            order: [[0u8; 6]; 2],
        }
    }

    fn prepare_ub1(&mut self) {
        let ctx = self.ctx;
        let mut free_max = Vec::with_capacity(ctx.m);
        for mm in 0..ctx.m {
            let na = ctx.ncol[0][mm];
            let nb = ctx.ncol[1][mm];
            let mut best = f64::NEG_INFINITY;
            for ia in 0..na {
                for ib in 0..nb {
                    let row = &ctx.dose[mm][ia * nb + ib];
                    let mut sum = 0.0f64;
                    for c in 0..ctx.n {
                        let mut best_s = f64::NEG_INFINITY;
                        for j in 0..NJ {
                            let v = ctx.gl[c][mm][row[j] as usize]
                                + ctx.drv[0][mm][j / NS]
                                + ctx.drv[1][mm][j % NS];
                            if v > best_s {
                                best_s = v;
                            }
                        }
                        sum += best_s;
                    }
                    if sum > best {
                        best = sum;
                    }
                }
            }
            free_max.push(best);
        }
        let mut trans = 0.0f64;
        for iv in 0..ctx.m - 1 {
            for pi in 0..2 {
                let mut bt = f64::NEG_INFINITY;
                for v in ctx.tr[pi][iv].iter() {
                    if *v > bt {
                        bt = *v;
                    }
                }
                trans += bt;
            }
        }
        self.free_max = free_max;
        self.trans_total = trans * ctx.n as f64;
    }

    fn ub1_root(&self) -> f64 {
        self.free_max.iter().sum::<f64>() + self.trans_total
    }

    /// Extends the partial rows with one column, rejecting the branch when two
    /// rows have already come out the wrong way round.
    fn extend(&self, parent: usize, mask: u8) -> Option<[u8; 6]> {
        let bits = [
            (mask >> 0) & 1,
            (mask >> 1) & 1,
            (mask >> 2) & 1,
            (mask >> 3) & 1,
        ];
        let mut out = self.order[parent];
        let mut slot = 0usize;
        for i in 0..4 {
            for j in i + 1..4 {
                if out[slot] == 0 {
                    if bits[i] > bits[j] {
                        return None;
                    }
                    if bits[i] < bits[j] {
                        out[slot] = 1;
                    }
                }
                slot += 1;
            }
        }
        Some(out)
    }

    fn root_bound(&self) -> f64 {
        (0..self.ctx.n).map(|c| self.suffix[0][c][0]).sum()
    }

    fn run(&mut self) -> bool {
        self.prepare_ub1();
        self.free_suffix = vec![0.0f64; self.ctx.m + 1];
        for mm in (0..self.ctx.m).rev() {
            self.free_suffix[mm] = self.free_suffix[mm + 1] + self.free_max[mm];
        }
        let start: Vec<[f64; NJ]> = (0..self.ctx.n).map(|_| [f64::NEG_INFINITY; NJ]).collect();
        println!("ub1 root = {:.6}", self.ub1_root());
        self.dfs(0, &start, 0.0)
    }

    fn dfs(&mut self, k: usize, fwd: &[[f64; NJ]], fixed_sum: f64) -> bool {
        if k == self.ctx.m {
            let mut total = 0.0f64;
            for row in fwd.iter() {
                let mut best = f64::NEG_INFINITY;
                for v in row.iter() {
                    if *v > best {
                        best = *v;
                    }
                }
                total += best;
            }
            if total > self.best {
                self.best = total;
                self.best_ca.copy_from_slice(&self.ca);
                self.best_cb.copy_from_slice(&self.cb);
            }
            return true;
        }

        let ctx = self.ctx;
        let na = ctx.ncol[0][k];
        let nb = ctx.ncol[1][k];
        let mut children: Vec<(usize, usize, f64, f64, Vec<[f64; NJ]>, [u8; 6], [u8; 6])> =
            Vec::with_capacity(na * nb);
        for ia in 0..na {
            let order_a = match self.extend(0, ctx.cols[0][k][ia]) {
                Some(order) => order,
                None => continue,
            };
            for ib in 0..nb {
                let order_b = match self.extend(1, ctx.cols[1][k][ib]) {
                    Some(order) => order,
                    None => continue,
                };
                let row = &ctx.dose[k][ia * nb + ib];
                let mut next: Vec<[f64; NJ]> = Vec::with_capacity(ctx.n);
                let mut bound = 0.0f64;
                let mut free_part = 0.0f64;
                let mut feasible = true;
                for c in 0..ctx.n {
                    let prev = fwd[c];
                    let mut cur = [f64::NEG_INFINITY; NJ];
                    if k == 0 {
                        for j in 0..NJ {
                            cur[j] = ctx.gl[c][0][row[j] as usize]
                                + ctx.drv[0][0][j / NS]
                                + ctx.drv[1][0][j % NS];
                        }
                    } else {
                        let ta = &ctx.tr[0][k - 1];
                        let tb = &ctx.tr[1][k - 1];
                        let mut mid = [f64::NEG_INFINITY; NJ];
                        for sa in 0..NS {
                            for sb in 0..NS {
                                let mut best = f64::NEG_INFINITY;
                                for p in 0..NS {
                                    let v = prev[sa * NS + p] + tb[p * NS + sb];
                                    if v > best {
                                        best = v;
                                    }
                                }
                                mid[sa * NS + sb] = best;
                            }
                        }
                        for sb in 0..NS {
                            for sa in 0..NS {
                                let mut best = f64::NEG_INFINITY;
                                for p in 0..NS {
                                    let v = mid[p * NS + sb] + ta[p * NS + sa];
                                    if v > best {
                                        best = v;
                                    }
                                }
                                let j = sa * NS + sb;
                                cur[j] = best
                                    + ctx.gl[c][k][row[j] as usize]
                                    + ctx.drv[0][k][sa]
                                    + ctx.drv[1][k][sb];
                            }
                        }
                    }
                    let tail = self.suffix[k + 1][c];
                    let mut best = f64::NEG_INFINITY;
                    for u in 0..NJ {
                        let v = cur[u] + tail[u];
                        if v > best {
                            best = v;
                        }
                    }
                    if best == f64::NEG_INFINITY {
                        feasible = false;
                        break;
                    }
                    bound += best;
                    let mut best_free = f64::NEG_INFINITY;
                    for j in 0..NJ {
                        let v = ctx.gl[c][k][row[j] as usize]
                            + ctx.drv[0][k][j / NS]
                            + ctx.drv[1][k][j % NS];
                        if v > best_free {
                            best_free = v;
                        }
                    }
                    free_part += best_free;
                    next.push(cur);
                }
                if feasible {
                    let ub1 = fixed_sum + free_part + self.free_suffix[k + 1] + self.trans_total;
                    let bound = if ub1 < bound { ub1 } else { bound };
                    children.push((ia, ib, bound, free_part, next, order_a, order_b));
                }
            }
        }
        children.sort_by(|x, y| y.2.partial_cmp(&x.2).unwrap_or(std::cmp::Ordering::Equal));

        for (ia, ib, bound, free_part, next, order_a, order_b) in children {
            self.nodes += 1;
            if self.nodes > self.limit {
                return false;
            }
            if bound <= self.best - 1e-9 {
                continue;
            }
            let held_a = self.ca[k];
            let held_b = self.cb[k];
            let held_order = self.order;
            self.ca[k] = ia as u8;
            self.cb[k] = ib as u8;
            self.order = [order_a, order_b];
            if !self.dfs(k + 1, &next, fixed_sum + free_part) {
                self.ca[k] = held_a;
                self.cb[k] = held_b;
                self.order = held_order;
                return false;
            }
            self.ca[k] = held_a;
            self.cb[k] = held_b;
            self.order = held_order;
        }
        true
    }
}

fn exact_nodes(e: &polyploid_phase::solver::exact::Exact) -> u64 {
    e.nodes()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().map(String::as_str).unwrap_or("/app/cross.json");
    let mode = args.get(1).map(String::as_str).unwrap_or("best");
    let text = std::fs::read_to_string(path).unwrap();
    let cross = parse_cross(&text).expect("valid cross");
    let ctx = Ctx::new(&cross);

    match mode {
        "best" => {
            let restarts: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(500);
            let kicks: usize = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(40);
            let mut rng = Rng::new(0x2545F4914F6CDD1D);
            let mut best = f64::NEG_INFINITY;
            let mut ca_best = vec![0u8; ctx.m];
            let mut cb_best = vec![0u8; ctx.m];
            for round in 0..restarts {
                let mut ca = vec![0u8; ctx.m];
                let mut cb = vec![0u8; ctx.m];
                if round > 0 {
                    for mm in 0..ctx.m {
                        ca[mm] = rng.below(ctx.ncol[0][mm]) as u8;
                        cb[mm] = rng.below(ctx.ncol[1][mm]) as u8;
                    }
                }
                let mut value = local_search(&ctx, &mut ca, &mut cb);
                if value > best {
                    best = value;
                    ca_best.copy_from_slice(&ca);
                    cb_best.copy_from_slice(&cb);
                }
                let mut held_a = ca.clone();
                let mut held_b = cb.clone();
                for _ in 0..kicks {
                    let mut ta = held_a.clone();
                    let mut tb = held_b.clone();
                    let width = 1 + rng.below(4);
                    for _ in 0..width {
                        let mm = rng.below(ctx.m);
                        if rng.below(2) == 0 {
                            ta[mm] = rng.below(ctx.ncol[0][mm]) as u8;
                        }
                        if rng.below(2) == 0 {
                            tb[mm] = rng.below(ctx.ncol[1][mm]) as u8;
                        }
                    }
                    let trial = local_search(&ctx, &mut ta, &mut tb);
                    if trial > best {
                        best = trial;
                        ca_best.copy_from_slice(&ta);
                        cb_best.copy_from_slice(&tb);
                    }
                    if trial > value {
                        value = trial;
                        held_a.copy_from_slice(&ta);
                        held_b.copy_from_slice(&tb);
                    }
                }
            }
            println!("stochastic best = {best:.10}");
            println!("ca = {ca_best:?}");
            println!("cb = {cb_best:?}");
        }
        "bnb" => {
            let limit: u64 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(20_000_000);
            let seed: Option<usize> = args.get(3).and_then(|a| a.parse().ok());
            let mut bnb = Bnb::new(&ctx, limit);
            println!("root bound = {:.6}", bnb.root_bound());
            if let Some(restarts) = seed {
                let mut rng = Rng::new(0x2545F4914F6CDD1D);
                let mut best = f64::NEG_INFINITY;
                for round in 0..restarts {
                    let mut ca = vec![0u8; ctx.m];
                    let mut cb = vec![0u8; ctx.m];
                    if round > 0 {
                        for mm in 0..ctx.m {
                            ca[mm] = rng.below(ctx.ncol[0][mm]) as u8;
                            cb[mm] = rng.below(ctx.ncol[1][mm]) as u8;
                        }
                    }
                    let value = local_search(&ctx, &mut ca, &mut cb);
                    if value > best {
                        best = value;
                    }
                }
                println!("incumbent = {best:.10}");
                bnb.best = best;
            }
            let start = std::time::Instant::now();
            let complete = bnb.run();
            println!(
                "bnb best = {:.10} nodes = {} complete = {} elapsed = {:?}",
                bnb.best,
                bnb.nodes,
                complete,
                start.elapsed()
            );
            println!("best ca = {:?}", bnb.best_ca);
            println!("best cb = {:?}", bnb.best_cb);
        }
        "cmp" => {
            let limit: u64 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(4_000_000);
            let outcome = search::run(&ctx);
            let mut bnb = Bnb::new(&ctx, limit);
            bnb.best = outcome.value;
            let start = std::time::Instant::now();
            let complete = bnb.run();
            println!(
                "m={} n={} run={:.10} bnb={:.10} complete={} nodes={} elapsed={:?} gap={:.6}",
                ctx.m, ctx.n, outcome.value, bnb.best, complete, bnb.nodes,
                start.elapsed(), bnb.best - outcome.value
            );
            if complete && bnb.best > outcome.value + 1e-9 {
                println!("  MISSED OPTIMUM by {:.10}", bnb.best - outcome.value);
                println!("  ca = {:?}", bnb.best_ca);
                println!("  cb = {:?}", bnb.best_cb);
            }
        }
        "beam" => {
            let relaxed = polyploid_phase::solver::relaxed::Relaxed::new(&ctx);
            let w = polyploid_phase::solver::beam::width(&ctx);
            let t0 = std::time::Instant::now();
            let found = polyploid_phase::solver::beam::beam(&ctx, &relaxed, w, 8);
            let tb = t0.elapsed();
            println!("beam width={w} time={tb:?} best_raw={:.10}", found[0].0);
            let mut best = f64::NEG_INFINITY;
            for (_, ca, cb) in &found {
                let mut a = ca.clone();
                let mut b = cb.clone();
                let v = local_search(&ctx, &mut a, &mut b);
                if v > best {
                    best = v;
                }
            }
            println!("after local search = {best:.10}");
            let outcome = search::run(&ctx);
            println!("full run = {:.10}", outcome.value);
        }
        "tune" => {
            let relaxed = polyploid_phase::solver::relaxed::Relaxed::new(&ctx);
            let starts =
                polyploid_phase::solver::beam::beam(&ctx, &relaxed, beam::width(&ctx), 8);
            let kicks: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(40);
            let restarts: usize = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(25);
            let t0 = std::time::Instant::now();
            let outcome = search::stochastic(&ctx, &starts, restarts, kicks);
            println!(
                "tune r={restarts} k={kicks} value={:.10} time={:?}",
                outcome.value,
                t0.elapsed()
            );
        }
        "check" => {
            use polyploid_phase::cross_model::Parent;
            use polyploid_phase::scoring::score;
            let answer = polyploid_phase::solver::solve(&cross);
            let value = score(&cross, &answer).expect("scored");
            // Independent recomputation: Viterbi every progeny against the
            // reported haplotypes and add the paths up.
            let mut again = 0.0f64;
            let ha = answer.haplotypes(Parent::A);
            let hb = answer.haplotypes(Parent::B);
            let mut ca = vec![0u8; ctx.m];
            let mut cb = vec![0u8; ctx.m];
            for mm in 0..ctx.m {
                let mask_a = (0..4).fold(0u8, |acc, h| acc | ((ha.rows()[h][mm] as u8) << h));
                let mask_b = (0..4).fold(0u8, |acc, h| acc | ((hb.rows()[h][mm] as u8) << h));
                ca[mm] = ctx.cols[0][mm].iter().position(|m| *m == mask_a).unwrap() as u8;
                cb[mm] = ctx.cols[1][mm].iter().position(|m| *m == mask_b).unwrap() as u8;
            }
            for c in 0..ctx.n {
                again += ctx.viterbi(c, &ca, &cb).val;
            }
            let reported = ctx.viterbi(0, &ca, &cb).val;
            let _ = reported;
            let diff = (value - again).abs();
            let mut worst = 0.0f64;
            for c in 0..ctx.n {
                let prog = ctx.viterbi(c, &ca, &cb);
                let own: f64 = prog.val;
                let _ = own;
            }
            println!("check value={value:.10} viterbi sum={again:.10} diff={diff:.3e}");
        }
        "inc" => {
            use polyploid_phase::solver::beam;
            use polyploid_phase::solver::exact::Exact;
            use polyploid_phase::solver::relaxed::Relaxed;
            let limit: u64 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(4_000_000);
            let relaxed = Relaxed::new(&ctx);
            let mut starts = beam::beam(&ctx, &relaxed, beam::width(&ctx), 8);
            let mut incumbent = f64::NEG_INFINITY;
            for (value, ca, cb) in starts.iter_mut() {
                let mut a = ca.clone();
                let mut b = cb.clone();
                *value = local_search(&ctx, &mut a, &mut b);
                *ca = a;
                *cb = b;
                if *value > incumbent {
                    incumbent = *value;
                }
            }
            let mut exact = Exact::new(&ctx, limit);
            exact.seed(incumbent);
            let complete = exact.run();
            println!(
                "inc m={} n={} beam={:.6} bnb={:.6} complete={} nodes={}",
                ctx.m, ctx.n, incumbent, exact.best(), complete,
                exact_nodes(&exact)
            );
        }
        other => panic!("unknown mode {other}"),
    }
}
