// scratch prototype: branch and bound measurement. deleted before finish.
use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::cross_model::{Parent, ALL_STATES};

const NS: usize = 10;
const NJ: usize = 100;
const NEG: f64 = f64::NEG_INFINITY;

fn lo(i: usize) -> usize {
    ALL_STATES[i].low() as usize
}
fn hi(i: usize) -> usize {
    ALL_STATES[i].high() as usize
}

fn admissible(dosage: u8) -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for bits in 0u8..16 {
        if bits.count_ones() as u8 == dosage {
            out.push([bits & 1, (bits >> 1) & 1, (bits >> 2) & 1, (bits >> 3) & 1]);
        }
    }
    out
}

fn perms4() -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for a in 0..4u8 {
        for b in 0..4u8 {
            for c in 0..4u8 {
                for d in 0..4u8 {
                    let p = [a, b, c, d];
                    let mut seen = [false; 4];
                    if p.iter().all(|x| !std::mem::replace(&mut seen[*x as usize], true)) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

/// new[p[h]] = old[h]
fn apply(p: &[u8; 4], c: &[u8; 4]) -> [u8; 4] {
    let mut out = [0u8; 4];
    for h in 0..4 {
        out[p[h] as usize] = c[h];
    }
    out
}

fn key4(c: &[u8; 4]) -> u8 {
    c[0] | (c[1] << 1) | (c[2] << 2) | (c[3] << 3)
}

struct Ctx {
    m: usize,
    n: usize,
    gl: Vec<Vec<[f64; 5]>>,
    cols_a: Vec<Vec<[u8; 4]>>,
    cols_b: Vec<Vec<[u8; 4]>>,
    legal: Vec<[bool; NJ]>,
    drj: Vec<[f64; NJ]>,
    ta: Vec<[[f64; NS]; NS]>,
    tb: Vec<[[f64; NS]; NS]>,
    dose: Vec<Vec<[u8; NJ]>>,
    dmask: Vec<[u8; NJ]>,
    na: Vec<usize>,
    nb: Vec<usize>,
}

impl Ctx {
    fn new(text: &str) -> Ctx {
        let cross = parse_cross(text).unwrap();
        let m = cross.markers;
        let n = cross.offspring;
        let mut cols_a = Vec::new();
        let mut cols_b = Vec::new();
        let mut legal = Vec::new();
        let mut drj = Vec::new();
        for mm in 0..m {
            let mut ca = admissible(cross.params(Parent::A).dosage[mm]);
            let mut cb = admissible(cross.params(Parent::B).dosage[mm]);
            ca.sort_by_key(key4);
            cb.sort_by_key(key4);
            cols_a.push(ca);
            cols_b.push(cb);
            let mut l = [true; NJ];
            for sa in 0..NS {
                if ALL_STATES[sa].is_double_reduction() && !cross.params(Parent::A).dr_allowed[mm] {
                    for sb in 0..NS {
                        l[sa * NS + sb] = false;
                    }
                }
            }
            for sb in 0..NS {
                if ALL_STATES[sb].is_double_reduction() && !cross.params(Parent::B).dr_allowed[mm] {
                    for sa in 0..NS {
                        l[sa * NS + sb] = false;
                    }
                }
            }
            legal.push(l);
            let mut d = [0.0f64; NJ];
            let ra = cross.params(Parent::A).dr_rate[mm];
            let rb = cross.params(Parent::B).dr_rate[mm];
            for sa in 0..NS {
                let va =
                    if ALL_STATES[sa].is_double_reduction() { ra.ln() } else { (1.0 - ra).ln() };
                for sb in 0..NS {
                    let vb = if ALL_STATES[sb].is_double_reduction() {
                        rb.ln()
                    } else {
                        (1.0 - rb).ln()
                    };
                    d[sa * NS + sb] = va + vb;
                }
            }
            drj.push(d);
        }
        let mut ta = Vec::new();
        let mut tb = Vec::new();
        for mm in 0..m - 1 {
            let mut a = [[0.0f64; NS]; NS];
            let mut b = [[0.0f64; NS]; NS];
            let ra = cross.params(Parent::A).recombination[mm];
            let rb = cross.params(Parent::B).recombination[mm];
            for s in 0..NS {
                for t in 0..NS {
                    let k = ALL_STATES[s].switch_count(ALL_STATES[t]) as f64;
                    a[s][t] = k * ra.ln() + (2.0 - k) * (1.0 - ra).ln();
                    b[s][t] = k * rb.ln() + (2.0 - k) * (1.0 - rb).ln();
                }
            }
            ta.push(a);
            tb.push(b);
        }
        let mut dose = Vec::new();
        let mut dmvec = Vec::new();
        let mut na = Vec::new();
        let mut nb = Vec::new();
        for mm in 0..m {
            let mut tabs = Vec::new();
            for ia in 0..cols_a[mm].len() {
                for ib in 0..cols_b[mm].len() {
                    let ca = &cols_a[mm][ia];
                    let cb = &cols_b[mm][ib];
                    let mut t = [0u8; NJ];
                    for sa in 0..NS {
                        let va = ca[lo(sa)] + ca[hi(sa)];
                        for sb in 0..NS {
                            let vb = cb[lo(sb)] + cb[hi(sb)];
                            t[sa * NS + sb] = va + vb;
                        }
                    }
                    tabs.push(t);
                }
            }
            let mut dmask = [0u8; NJ];
            for t in &tabs {
                for j in 0..NJ {
                    dmask[j] |= 1u8 << t[j];
                }
            }
            dose.push(tabs);
            dmvec.push(dmask);
            na.push(cols_a[mm].len());
            nb.push(cols_b[mm].len());
        }
        Ctx {
            m,
            n,
            gl: cross.child_gl.clone(),
            cols_a,
            cols_b,
            legal,
            drj,
            ta,
            tb,
            dose,
            dmask: dmvec,
            na,
            nb,
        }
    }

    fn h(&self, mm: usize, ia: usize, ib: usize) -> usize {
        ia * self.nb[mm] + ib
    }
}

fn viterbi_path(ctx: &Ctx, nodes: &[[f64; NJ]]) -> (f64, Vec<usize>) {
    let mut f = [NEG; NJ];
    for j in 0..NJ {
        if ctx.legal[0][j] {
            f[j] = nodes[0][j];
        }
    }
    let mut back: Vec<[usize; NJ]> = Vec::with_capacity(ctx.m);
    back.push([0usize; NJ]);
    for mm in 1..ctx.m {
        let mut g = [NEG; NJ];
        let mut ga = [[0usize; NS]; NS];
        let ta = &ctx.ta[mm - 1];
        for sb in 0..NS {
            for sat in 0..NS {
                let mut best = NEG;
                let mut arg = 0usize;
                for sa in 0..NS {
                    let v = f[sa * NS + sb] + ta[sa][sat];
                    if v > best {
                        best = v;
                        arg = sa;
                    }
                }
                g[sat * NS + sb] = best;
                ga[sat][sb] = arg;
            }
        }
        let tb = &ctx.tb[mm - 1];
        let mut nf = [NEG; NJ];
        let mut bp = [0usize; NJ];
        for sat in 0..NS {
            for sbt in 0..NS {
                let mut best = NEG;
                let mut arg = 0usize;
                for sb in 0..NS {
                    let v = g[sat * NS + sb] + tb[sb][sbt];
                    if v > best {
                        best = v;
                        arg = sb;
                    }
                }
                let j = sat * NS + sbt;
                if ctx.legal[mm][j] {
                    nf[j] = best + nodes[mm][j];
                    bp[j] = ga[sat][arg] * NS + arg;
                }
            }
        }
        back.push(bp);
        f = nf;
    }
    let mut best = NEG;
    let mut arg = 0usize;
    for j in 0..NJ {
        if f[j] > best {
            best = f[j];
            arg = j;
        }
    }
    let mut path = vec![0usize; ctx.m];
    let mut cur = arg;
    for mm in (0..ctx.m).rev() {
        path[mm] = cur;
        if mm > 0 {
            cur = back[mm][cur];
        }
    }
    (best, path)
}

struct Search<'a> {
    ctx: &'a Ctx,
    opt: Vec<Vec<[f64; NJ]>>,
    back: Vec<Vec<[f64; NJ]>>,
    best: f64,
    best_assign: Vec<(usize, usize)>,
    nodes: u64,
    leaves: u64,
    stack: Vec<(usize, usize)>,
    fwd: Vec<Vec<[f64; NJ]>>,
    eps: f64,
}

impl<'a> Search<'a> {
    fn new(ctx: &'a Ctx) -> Search<'a> {
        let mut opt = Vec::with_capacity(ctx.n);
        for c in 0..ctx.n {
            let mut rows = Vec::with_capacity(ctx.m);
            for mm in 0..ctx.m {
                let mut o = [NEG; NJ];
                let g = &ctx.gl[c][mm];
                for j in 0..NJ {
                    if !ctx.legal[mm][j] {
                        continue;
                    }
                    let mut best = NEG;
                    let dmask = ctx.dmask[mm][j];
                    for d in 0..5u8 {
                        if dmask & (1 << d) != 0 && g[d as usize] > best {
                            best = g[d as usize];
                        }
                    }
                    o[j] = best + ctx.drj[mm][j];
                }
                rows.push(o);
            }
            opt.push(rows);
        }
        let mut back: Vec<Vec<[f64; NJ]>> = Vec::with_capacity(ctx.n);
        for c in 0..ctx.n {
            let mut rows = vec![[0.0f64; NJ]; ctx.m];
            for mm in (0..ctx.m - 1).rev() {
                let mut r = [NEG; NJ];
                for sat in 0..NS {
                    for sbt in 0..NS {
                        let j = sat * NS + sbt;
                        if !ctx.legal[mm + 1][j] {
                            continue;
                        }
                        let nxt = opt[c][mm + 1][j] + rows[mm + 1][j];
                        for sa in 0..NS {
                            for sb in 0..NS {
                                let jj = sa * NS + sb;
                                let v = ctx.ta[mm][sa][sat] + ctx.tb[mm][sb][sbt] + nxt;
                                if v > r[jj] {
                                    r[jj] = v;
                                }
                            }
                        }
                    }
                }
                rows[mm] = r;
            }
            back.push(rows);
        }
        Search {
            ctx,
            opt,
            back,
            best: NEG,
            best_assign: Vec::new(),
            nodes: 0,
            leaves: 0,
            stack: Vec::new(),
            fwd: Vec::new(),
            eps: 1e-7,
        }
    }

    fn run(&mut self) {
        let n = self.ctx.n;
        let m = self.ctx.m;
        self.fwd = vec![vec![[NEG; NJ]; n]; m];
        let ga = perms4();
        let gb = perms4();
        self.dfs(0, &ga, &gb);
    }

    fn dfs(&mut self, mm: usize, ga: &[[u8; 4]], gb: &[[u8; 4]]) {
        let ctx = self.ctx;
        self.nodes += 1;
        if mm == ctx.m {
            self.leaves += 1;
            let mut total = 0.0f64;
            for c in 0..ctx.n {
                let f = &self.fwd[mm - 1][c];
                let mut best = NEG;
                for j in 0..NJ {
                    if f[j] > best {
                        best = f[j];
                    }
                }
                total += best;
            }
            if total > self.best {
                self.best = total;
                self.best_assign = self.stack.clone();
            }
            return;
        }
        let mut cand_a: Vec<usize> = Vec::new();
        for ia in 0..ctx.na[mm] {
            let col = ctx.cols_a[mm][ia];
            if ga.iter().all(|p| key4(&apply(p, &col)) >= key4(&col)) {
                cand_a.push(ia);
            }
        }
        let mut cand_b: Vec<usize> = Vec::new();
        for ib in 0..ctx.nb[mm] {
            let col = ctx.cols_b[mm][ib];
            if gb.iter().all(|p| key4(&apply(p, &col)) >= key4(&col)) {
                cand_b.push(ib);
            }
        }
        for ia in &cand_a {
            let cola = ctx.cols_a[mm][*ia];
            let nga: Vec<[u8; 4]> =
                ga.iter().filter(|p| apply(p, &cola) == cola).cloned().collect();
            for ib in &cand_b {
                let colb = ctx.cols_b[mm][*ib];
                let ngb: Vec<[u8; 4]> =
                    gb.iter().filter(|p| apply(p, &colb) == colb).cloned().collect();
                let hh = ctx.h(mm, *ia, *ib);
                let dose = &ctx.dose[mm][hh];
                let mut ub = 0.0f64;
                let mut pruned = false;
                for c in 0..ctx.n {
                    let g = &ctx.gl[c][mm];
                    let dr = &ctx.drj[mm];
                    let mut nf = [NEG; NJ];
                    if mm == 0 {
                        for j in 0..NJ {
                            if ctx.legal[0][j] {
                                nf[j] = g[dose[j] as usize] + dr[j];
                            }
                        }
                    } else {
                        let prev = self.fwd[mm - 1][c];
                        let mut tmp = [NEG; NJ];
                        let ta = &ctx.ta[mm - 1];
                        for sb in 0..NS {
                            for sat in 0..NS {
                                let mut best = NEG;
                                for sa in 0..NS {
                                    let v = prev[sa * NS + sb] + ta[sa][sat];
                                    if v > best {
                                        best = v;
                                    }
                                }
                                tmp[sat * NS + sb] = best;
                            }
                        }
                        let tb = &ctx.tb[mm - 1];
                        for sat in 0..NS {
                            for sbt in 0..NS {
                                let j = sat * NS + sbt;
                                if !ctx.legal[mm][j] {
                                    continue;
                                }
                                let mut best = NEG;
                                for sb in 0..NS {
                                    let v = tmp[sat * NS + sb] + tb[sb][sbt];
                                    if v > best {
                                        best = v;
                                    }
                                }
                                nf[j] = best + g[dose[j] as usize] + dr[j];
                            }
                        }
                    }
                    self.fwd[mm][c] = nf;
                    let mut best = NEG;
                    let bk = &self.back[c][mm];
                    for j in 0..NJ {
                        let v = nf[j] + bk[j];
                        if v > best {
                            best = v;
                        }
                    }
                    ub += best;
                    if ub < self.best - self.eps {
                        pruned = true;
                        break;
                    }
                }
                if !pruned {
                    self.stack.push((*ia, *ib));
                    self.dfs(mm + 1, &nga, &ngb);
                    self.stack.pop();
                }
            }
        }
    }
}

fn local_search(ctx: &Ctx, restarts: usize) -> f64 {
    let mut rng = 987654321u64;
    let mut rand = move || {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (rng >> 33) as usize
    };
    let mut best = NEG;
    for _ in 0..restarts {
        let mut ia: Vec<usize> = (0..ctx.m).map(|mm| rand() % ctx.na[mm]).collect();
        let mut ib: Vec<usize> = (0..ctx.m).map(|mm| rand() % ctx.nb[mm]).collect();
        let mut prev = NEG;
        for _ in 0..30 {
            let mut total = 0.0f64;
            let mut paths: Vec<Vec<usize>> = Vec::new();
            for c in 0..ctx.n {
                let mut nodes = Vec::with_capacity(ctx.m);
                for mm in 0..ctx.m {
                    let hh = ctx.h(mm, ia[mm], ib[mm]);
                    let dose = &ctx.dose[mm][hh];
                    let mut nd = [0.0f64; NJ];
                    let g = &ctx.gl[c][mm];
                    for j in 0..NJ {
                        nd[j] = g[dose[j] as usize] + ctx.drj[mm][j];
                    }
                    nodes.push(nd);
                }
                let (v, p) = viterbi_path(ctx, &nodes);
                total += v;
                paths.push(p);
            }
            if total > best {
                best = total;
            }
            if (total - prev).abs() < 1e-9 {
                break;
            }
            prev = total;
            for mm in 0..ctx.m {
                let mut bestv = NEG;
                let mut bi = (0usize, 0usize);
                for a in 0..ctx.na[mm] {
                    for b in 0..ctx.nb[mm] {
                        let dose = &ctx.dose[mm][ctx.h(mm, a, b)];
                        let mut s = 0.0f64;
                        for c in 0..ctx.n {
                            s += ctx.gl[c][mm][dose[paths[c][mm]] as usize];
                        }
                        if s > bestv {
                            bestv = s;
                            bi = (a, b);
                        }
                    }
                }
                ia[mm] = bi.0;
                ib[mm] = bi.1;
            }
        }
    }
    best
}

#[test]
fn measure_bnb() {
    let text = std::fs::read_to_string("/app/cross.json").unwrap();
    let ctx = Ctx::new(&text);
    let mut s = Search::new(&ctx);
    s.best = local_search(&ctx, 40);
    println!("initial LB = {:.6}", s.best);
    let t0 = std::time::Instant::now();
    s.run();
    println!(
        "nodes={} leaves={} best={:.6} time={:?}",
        s.nodes,
        s.leaves,
        s.best,
        t0.elapsed()
    );
    println!("assign = {:?}", s.best_assign);
}
