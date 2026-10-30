// Scratch harness used while developing the search. Not part of the deliverable.

use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::solver::search::{local_search, Rng};
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
    nodes: u64,
    limit: u64,
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
            nodes: 0,
            limit,
        }
    }

    fn root_bound(&self) -> f64 {
        (0..self.ctx.n).map(|c| self.suffix[0][c][0]).sum()
    }

    fn run(&mut self) -> bool {
        let start: Vec<[f64; NJ]> = (0..self.ctx.n).map(|_| [f64::NEG_INFINITY; NJ]).collect();
        self.dfs(0, &start)
    }

    fn dfs(&mut self, k: usize, fwd: &[[f64; NJ]]) -> bool {
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
        let mut children: Vec<(usize, usize, f64, Vec<[f64; NJ]>)> = Vec::with_capacity(na * nb);
        for ia in 0..na {
            for ib in 0..nb {
                let row = &ctx.dose[k][ia * nb + ib];
                let mut next: Vec<[f64; NJ]> = Vec::with_capacity(ctx.n);
                let mut bound = 0.0f64;
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
                    next.push(cur);
                }
                if feasible {
                    children.push((ia, ib, bound, next));
                }
            }
        }
        children.sort_by(|x, y| y.2.partial_cmp(&x.2).unwrap_or(std::cmp::Ordering::Equal));

        for (ia, ib, bound, next) in children {
            self.nodes += 1;
            if self.nodes > self.limit {
                return false;
            }
            if bound <= self.best - 1e-9 {
                continue;
            }
            let held_a = self.ca[k];
            let held_b = self.cb[k];
            self.ca[k] = ia as u8;
            self.cb[k] = ib as u8;
            if !self.dfs(k + 1, &next) {
                self.ca[k] = held_a;
                self.cb[k] = held_b;
                return false;
            }
            self.ca[k] = held_a;
            self.cb[k] = held_b;
        }
        true
    }
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
        }
        other => panic!("unknown mode {other}"),
    }
}
