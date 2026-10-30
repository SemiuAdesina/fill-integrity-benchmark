
//! Branch and bound over one cloning site and one distribution of
//! insert-region sites.
//!
//! A node is a box: an interval for the insert length, an interval for each
//! insert-region position, and a set of fragment constraints, each saying
//! that one fragment of one digest lies in one observed band's tolerance
//! window or below the detection limit. Constraints are propagated into the
//! intervals. A digest whose cuts the box orders has fragment length
//! intervals; a digest whose visible fragments are also separated in length
//! has a fixed pairing with its bands, and its deviation is then a convex
//! quadratic bounded below by a tangent plane. Branching prefers structure:
//! the widest fragment of an ordered digest that could still pair with
//! several bands is split over those bands; only when every fragment is
//! settled is the widest interval halved.

use std::collections::BTreeMap;

use crate::digest;
use crate::input::{Digest, Problem};
use crate::map_model::{key, predictions, Key, PlasmidMap};
use crate::sites;

const INF: i64 = 1 << 50;

/// Minimum cost of a perfect assignment of rows to columns of a square
/// matrix (Kuhn-Munkres with potentials). Costs of `INF` mark forbidden
/// pairs; a result of at least `INF` means no admissible assignment exists.
fn hungarian(cost: &[Vec<i64>]) -> i64 {
    let n = cost.len();
    if n == 0 {
        return 0;
    }
    let mut u = vec![0i64; n + 1];
    let mut v = vec![0i64; n + 1];
    let mut p = vec![0usize; n + 1];
    let mut way = vec![0usize; n + 1];
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0usize;
        let mut minv = vec![i64::MAX; n + 1];
        let mut used = vec![false; n + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = i64::MAX;
            let mut j1 = 0usize;
            for j in 1..=n {
                if used[j] {
                    continue;
                }
                let cur = cost[i0 - 1][j - 1] - u[i0] - v[j];
                if cur < minv[j] {
                    minv[j] = cur;
                    way[j] = j0;
                }
                if minv[j] < delta {
                    delta = minv[j];
                    j1 = j;
                }
            }
            for j in 0..=n {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut total = 0i64;
    for j in 1..=n {
        if p[j] != 0 {
            total += cost[p[j] - 1][j - 1];
        }
    }
    total
}

/// The best map found so far at the current level, with its key.
pub struct Best {
    pub key: Key,
    pub map: PlasmidMap,
}

/// What the search needs about one cloning site, computed once.
pub struct SiteContext {
    pub c: usize,
    /// Per enzyme: surviving backbone cuts below `c` (fixed) and at or above it (moving).
    pub fixed: Vec<Vec<usize>>,
    pub moving: Vec<Vec<usize>>,
    /// Per enzyme: the least number of insert-region sites its single digest can admit.
    pub minimum: Vec<usize>,
    /// Observed digests with their enzymes as panel indices and expanded bands.
    pub digests: Vec<DigestView>,
}

pub struct DigestView {
    pub enzymes: Vec<usize>,
    pub expanded: Vec<u32>,
    /// Distinct band lengths, ascending.
    pub distinct: Vec<u32>,
}

impl SiteContext {
    pub fn new(problem: &Problem, c: usize) -> Self {
        let v = problem.v();
        let d = problem.detection_limit;
        let mut fixed = Vec::new();
        let mut moving = Vec::new();
        let mut minimum = Vec::new();
        for enzyme in &problem.enzymes {
            let all = sites::scan(&problem.backbone.seq, &enzyme.site, enzyme.cut_offset);
            let mut surviving: Vec<usize> =
                all.iter().filter(|s| !sites::spans_junction(s.start, enzyme.site.len(), c, v)).map(|s| s.cut).collect();
            surviving.sort_unstable();
            surviving.dedup();
            let a: Vec<usize> = surviving.iter().copied().filter(|&p| p < c).collect();
            let b: Vec<usize> = surviving.iter().copied().filter(|&p| p >= c).collect();
            let mut gaps: Vec<usize> = a.windows(2).map(|w| w[1] - w[0]).chain(b.windows(2).map(|w| w[1] - w[0])).collect();
            if !a.is_empty() && !b.is_empty() {
                gaps.push(v - b[b.len() - 1] + a[0]);
            }
            let visible_fixed = gaps.iter().filter(|&&g| g >= d as usize).count();
            let single = problem.single(&enzyme.name).expect("a single digest per enzyme");
            let m = digest::expand(&single.bands).len();
            minimum.push(m.saturating_sub(visible_fixed + 1));
            fixed.push(a);
            moving.push(b);
        }
        let digests = problem
            .observed
            .iter()
            .map(|dg: &Digest| {
                let expanded = digest::expand(&dg.bands);
                let mut distinct = expanded.clone();
                distinct.dedup();
                DigestView {
                    enzymes: dg.enzymes.iter().map(|e| problem.enzymes.iter().position(|x| &x.name == e).expect("panel enzyme")).collect(),
                    expanded,
                    distinct,
                }
            })
            .collect();
        SiteContext { c, fixed, moving, minimum, digests }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Interval {
    lo: i64,
    hi: i64,
}

impl Interval {
    fn point(p: i64) -> Self {
        Interval { lo: p, hi: p }
    }
    fn width(self) -> i64 {
        self.hi - self.lo
    }
}

/// A cut position: a constant, a backbone position that moves with the
/// insert length, or one of the position boxes.
#[derive(Clone, Copy, Debug)]
enum Cut {
    Fixed(i64),
    Moving(i64),
    Box(usize),
}

/// A fragment length as a linear form: `plus - minus + with_l * L + constant`.
#[derive(Clone, Copy, Debug)]
struct Frag {
    plus: Cut,
    minus: Cut,
    /// 1 for the wrap-around fragment (its length includes `V + L`), else 0.
    wrap: bool,
}

/// One fragment constraint: fragment `frag` of digest `digest` has a length
/// in `[lo, hi]`.
#[derive(Clone, Copy, Debug)]
struct Constraint {
    digest: usize,
    frag: usize,
    lo: i64,
    hi: i64,
}

struct Search<'a> {
    problem: &'a Problem,
    ctx: &'a SiteContext,
    composition: &'a [usize],
    /// Offsets of each enzyme's first position box.
    first: Vec<usize>,
    count: usize,
}

/// The variables of a node: index 0 is the insert length, then the boxes,
/// plus the order relations a branch has fixed between boxes of different
/// enzymes: `(a, b, -1)` for `a < b`, `(a, b, 0)` for `a == b`, `(a, b, 1)`
/// for `a > b`.
#[derive(Clone)]
struct State {
    l: Interval,
    boxes: Vec<Interval>,
    orders: Vec<(usize, usize, i8)>,
}

impl<'a> Search<'a> {
    fn enzyme_of(&self, box_index: usize) -> usize {
        let mut e = 0;
        while e + 1 < self.first.len() && self.first[e + 1] <= box_index {
            e += 1;
        }
        e
    }

    fn cut_interval(&self, cut: Cut, s: &State) -> Interval {
        match cut {
            Cut::Fixed(p) => Interval::point(p),
            Cut::Moving(p) => Interval { lo: p + s.l.lo, hi: p + s.l.hi },
            Cut::Box(i) => s.boxes[i],
        }
    }

    fn frag_interval(&self, f: &Frag, s: &State) -> Interval {
        let a = self.cut_interval(f.minus, s);
        let b = self.cut_interval(f.plus, s);
        let v = self.problem.v() as i64;
        if f.wrap {
            Interval { lo: (b.lo + v + s.l.lo - a.hi).max(1), hi: b.hi + v + s.l.hi - a.lo }
        } else {
            Interval { lo: (b.lo - a.hi).max(1), hi: b.hi - a.lo }
        }
    }

    /// The digest's cuts in the order the box fixes, or `None` when two
    /// insert-region cuts of different enzymes could still be either way
    /// round. Surviving backbone sites below the opening precede the insert
    /// region, and those at or above it follow it, whatever the insert length.
    fn ordered_cuts(&self, dg: &DigestView, s: &State) -> Option<Vec<Cut>> {
        let mut fixed: Vec<i64> = Vec::new();
        let mut moving: Vec<i64> = Vec::new();
        let mut boxes: Vec<(Interval, usize)> = Vec::new();
        for &enzyme in &dg.enzymes {
            fixed.extend(self.ctx.fixed[enzyme].iter().map(|&p| p as i64));
            moving.extend(self.ctx.moving[enzyme].iter().map(|&p| p as i64));
            for r in 0..self.composition[enzyme] {
                let idx = self.first[enzyme] + r;
                boxes.push((s.boxes[idx], idx));
            }
        }
        fixed.sort_unstable();
        fixed.dedup();
        moving.sort_unstable();
        moving.dedup();
        boxes.sort_by_key(|(iv, i)| (iv.lo, *i));
        for w in boxes.windows(2) {
            let separated = w[0].0.hi < w[1].0.lo;
            let same_rank = w[1].1 == w[0].1 + 1 && self.enzyme_of(w[0].1) == self.enzyme_of(w[1].1);
            // A branch may have fixed the order of two cuts of different
            // enzymes, or made them equal; equal cuts give the same fragments
            // whichever is listed first.
            let (a, b) = (w[0].1, w[1].1);
            let related = s.orders.iter().any(|&(x, y, rel)| (x == a && y == b && rel <= 0) || (x == b && y == a && rel >= 0));
            if !separated && !same_rank && !related {
                return None;
            }
        }
        let mut cuts: Vec<Cut> = fixed.into_iter().map(Cut::Fixed).collect();
        cuts.extend(boxes.into_iter().map(|(_, i)| Cut::Box(i)));
        cuts.extend(moving.into_iter().map(Cut::Moving));
        Some(cuts)
    }

    fn fragments(cuts: &[Cut]) -> Vec<Frag> {
        let mut out: Vec<Frag> = cuts.windows(2).map(|w| Frag { plus: w[1], minus: w[0], wrap: false }).collect();
        out.push(Frag { plus: cuts[0], minus: cuts[cuts.len() - 1], wrap: true });
        out
    }

    /// Tightens the variables of `f` so that its length lies in `[lo, hi]`.
    /// Returns false when that leaves some interval empty.
    fn tighten(&self, f: &Frag, lo: i64, hi: i64, s: &mut State) -> bool {
        let v = self.problem.v() as i64;
        // length = plus - minus + (wrap ? V + L : 0)
        let plus = self.cut_interval(f.plus, s);
        let minus = self.cut_interval(f.minus, s);
        let (wl, wc) = if f.wrap { (1i64, v) } else { (0, 0) };
        // plus in [lo - (wl*L.hi + wc) + minus.lo, hi - (wl*L.lo + wc) + minus.hi]
        let plus_range = Interval { lo: lo - (wl * s.l.hi + wc) + minus.lo, hi: hi - (wl * s.l.lo + wc) + minus.hi };
        // minus in [plus.lo + wl*L.lo + wc - hi, plus.hi + wl*L.hi + wc - lo]
        let minus_range = Interval { lo: plus.lo + wl * s.l.lo + wc - hi, hi: plus.hi + wl * s.l.hi + wc - lo };
        if !self.restrict(f.plus, plus_range, s) || !self.restrict(f.minus, minus_range, s) {
            return false;
        }
        // L appears through moving cuts (+L on plus, -L on minus) and the wrap term
        let l_coef = (matches!(f.plus, Cut::Moving(_)) as i64) - (matches!(f.minus, Cut::Moving(_)) as i64) + wl;
        if l_coef != 0 {
            // length = base + l_coef * L where base excludes the L parts
            let base_lo = self.base_lo(f.plus, s) - self.base_hi(f.minus, s) + wc;
            let base_hi = self.base_hi(f.plus, s) - self.base_lo(f.minus, s) + wc;
            let (llo, lhi) = if l_coef > 0 { (lo - base_hi, hi - base_lo) } else { (base_lo - hi, base_hi - lo) };
            let llo = llo.max(s.l.lo);
            let lhi = lhi.min(s.l.hi);
            if llo > lhi {
                return false;
            }
            s.l = Interval { lo: llo, hi: lhi };
        }
        true
    }

    fn base_lo(&self, cut: Cut, s: &State) -> i64 {
        match cut {
            Cut::Fixed(p) | Cut::Moving(p) => p,
            Cut::Box(i) => s.boxes[i].lo,
        }
    }

    fn base_hi(&self, cut: Cut, s: &State) -> i64 {
        match cut {
            Cut::Fixed(p) | Cut::Moving(p) => p,
            Cut::Box(i) => s.boxes[i].hi,
        }
    }

    fn restrict(&self, cut: Cut, range: Interval, s: &mut State) -> bool {
        match cut {
            Cut::Fixed(p) => range.lo <= p && p <= range.hi,
            Cut::Moving(p) => {
                let llo = (range.lo - p).max(s.l.lo);
                let lhi = (range.hi - p).min(s.l.hi);
                if llo > lhi {
                    return false;
                }
                s.l = Interval { lo: llo, hi: lhi };
                true
            }
            Cut::Box(i) => {
                let lo = range.lo.max(s.boxes[i].lo);
                let hi = range.hi.min(s.boxes[i].hi);
                if lo > hi {
                    return false;
                }
                s.boxes[i] = Interval { lo, hi };
                true
            }
        }
    }

    /// The variable and constant a cut resolves to: `(var, constant)` with
    /// `var` the insert length (0), a position box (`i + 1`), or the zero
    /// node (`count + 1`) for a constant.
    fn resolve(&self, cut: Cut) -> (usize, i64) {
        match cut {
            Cut::Fixed(p) => (self.count + 1, p),
            Cut::Moving(p) => (0, p),
            Cut::Box(i) => (i + 1, 0),
        }
    }

    /// Every window a fragment is currently held to: the branch constraints,
    /// and for every certainly visible fragment of an ordered digest that can
    /// reach exactly one band, that band's window. `None` when some visible
    /// fragment reaches no band at all.
    fn windows(&self, s: &State, constraints: &[Constraint]) -> Option<Vec<(Frag, i64, i64)>> {
        let d = self.problem.detection_limit as i64;
        let e = self.problem.error_percent;
        let mut out = Vec::new();
        for (di, dg) in self.ctx.digests.iter().enumerate() {
            let Some(cuts) = self.ordered_cuts(dg, s) else {
                continue;
            };
            let frags = Self::fragments(&cuts);
            for k in constraints.iter().filter(|k| k.digest == di) {
                out.push((frags[k.frag], k.lo, k.hi));
            }
            // Visibility counts: the digest shows exactly `m` bands, so when
            // every fragment that could be visible is needed, each borderline
            // fragment must reach the limit, and when the certainly visible
            // ones already number `m`, each borderline fragment must fall
            // short of it.
            let m = dg.expanded.len();
            let intervals: Vec<Interval> = frags.iter().map(|f| self.frag_interval(f, s)).collect();
            let certain = intervals.iter().filter(|iv| iv.lo >= d).count();
            let possible = intervals.iter().filter(|iv| iv.hi >= d).count();
            if certain > m || possible < m {
                return None;
            }
            for (f, iv) in frags.iter().zip(intervals.iter()) {
                if iv.lo < d && iv.hi >= d {
                    if possible == m {
                        out.push((*f, d, i64::MAX / 8));
                    } else if certain == m {
                        out.push((*f, 1, d - 1));
                    }
                }
            }
            for f in &frags {
                let iv = self.frag_interval(f, s);
                if iv.lo < d {
                    continue;
                }
                let mut only: Option<(i64, i64)> = None;
                let mut count = 0;
                for &b in &dg.distinct {
                    let tol = digest::tolerance(b, e) as i64;
                    let b = b as i64;
                    if b + tol >= iv.lo && b - tol <= iv.hi {
                        count += 1;
                        only = Some(((b - tol).max(1), b + tol));
                    }
                }
                if count == 0 {
                    return None;
                }
                if count == 1 {
                    let (lo, hi) = only.expect("one band");
                    out.push((*f, lo, hi));
                }
            }
        }
        Some(out)
    }

    /// Tightest bounds consistent with every difference constraint, by
    /// shortest paths from and to the zero node; a negative cycle means the
    /// box holds no map. Constraints: enzyme rank order, the insert region,
    /// and every window on a fragment that is not the wrap-around one. Wrap
    /// windows are applied by interval arithmetic afterwards, and the whole
    /// step repeats while it keeps tightening.
    fn propagate(&self, s: &mut State, constraints: &[Constraint]) -> bool {
        let nvars = self.count + 2; // L, boxes, zero
        let zero = self.count + 1;
        let c = self.ctx.c as i64;
        for _round in 0..12 {
            let before = s.clone();
            let Some(windows) = self.windows(s, constraints) else {
                return false;
            };
            // edges (u, v, w): x_v - x_u <= w
            let mut edges: Vec<(usize, usize, i64)> = Vec::new();
            edges.push((zero, 0, s.l.hi));
            edges.push((0, zero, -s.l.lo));
            for (i, b) in s.boxes.iter().enumerate() {
                edges.push((zero, i + 1, b.hi));
                edges.push((i + 1, zero, -b.lo));
                edges.push((i + 1, zero, -c)); // box >= c
                edges.push((0, i + 1, c)); // box - L <= c
            }
            for (e, &t) in self.composition.iter().enumerate() {
                for r in 1..t {
                    let a = self.first[e] + r; // box a > box a-1
                    edges.push((a + 1, a, -1));
                }
            }
            for &(a, b, rel) in &s.orders {
                match rel {
                    -1 => edges.push((b + 1, a + 1, -1)), // a - b <= -1
                    1 => edges.push((a + 1, b + 1, -1)),  // b - a <= -1
                    _ => {
                        edges.push((a + 1, b + 1, 0));
                        edges.push((b + 1, a + 1, 0));
                    }
                }
            }
            let v = self.problem.v() as i64;
            for (f, lo, hi) in &windows {
                let (vb, cb) = self.resolve(f.plus);
                let (va, ca) = self.resolve(f.minus);
                // wrap-around: length = plus - minus + V + L; with L replaced by
                // its bounds the constraint is a valid, slightly relaxed,
                // difference constraint (exact once L is a point)
                let (lo, hi) = if f.wrap { (lo - v - s.l.hi, hi - v - s.l.lo) } else { (*lo, *hi) };
                if va == vb {
                    let len = cb - ca;
                    if len < lo || len > hi {
                        return false;
                    }
                    continue;
                }
                edges.push((va, vb, hi - cb + ca));
                edges.push((vb, va, cb - ca - lo));
            }
            // shortest paths from zero (upper bounds) and to zero (lower bounds)
            let mut dist = vec![i64::MAX / 4; nvars];
            dist[zero] = 0;
            let mut changed = true;
            let mut iterations = 0;
            while changed {
                changed = false;
                iterations += 1;
                if iterations > nvars + 1 {
                    return false; // negative cycle
                }
                for &(u, v, w) in &edges {
                    if dist[u] < i64::MAX / 4 && dist[u] + w < dist[v] {
                        dist[v] = dist[u] + w;
                        changed = true;
                    }
                }
            }
            let mut rdist = vec![i64::MAX / 4; nvars];
            rdist[zero] = 0;
            changed = true;
            iterations = 0;
            while changed {
                changed = false;
                iterations += 1;
                if iterations > nvars + 1 {
                    return false;
                }
                for &(u, v, w) in &edges {
                    // reversed: distance to zero
                    if rdist[v] < i64::MAX / 4 && rdist[v] + w < rdist[u] {
                        rdist[u] = rdist[v] + w;
                        changed = true;
                    }
                }
            }
            let upper = |i: usize| dist[i];
            let lower = |i: usize| -rdist[i];
            for &(a, b, rel) in &s.orders {
                if rel == 0 {
                    let lo = lower(a + 1).max(lower(b + 1));
                    let hi = upper(a + 1).min(upper(b + 1));
                    if lo > hi {
                        return false;
                    }
                }
            }
            if lower(0) > upper(0) {
                return false;
            }
            s.l = Interval { lo: lower(0).max(s.l.lo), hi: upper(0).min(s.l.hi) };
            for i in 0..self.count {
                let lo = lower(i + 1).max(s.boxes[i].lo);
                let hi = upper(i + 1).min(s.boxes[i].hi);
                if lo > hi {
                    return false;
                }
                s.boxes[i] = Interval { lo, hi };
            }
            for &(a, b, rel) in &s.orders.clone() {
                if rel == 0 {
                    let lo = s.boxes[a].lo.max(s.boxes[b].lo);
                    let hi = s.boxes[a].hi.min(s.boxes[b].hi);
                    if lo > hi {
                        return false;
                    }
                    s.boxes[a] = Interval { lo, hi };
                    s.boxes[b] = Interval { lo, hi };
                }
            }
            for (f, lo, hi) in &windows {
                if f.wrap && !self.tighten(f, *lo, *hi, s) {
                    return false;
                }
            }
            if s.l.lo > s.l.hi {
                return false;
            }
            if s.l == before.l && s.boxes == before.boxes {
                break;
            }
        }
        true
    }

    /// Lower bound of the total deviation over the box, or `None` when the box
    /// cannot hold a map reproducing every digest.
    ///
    /// Every digest whose cuts the box orders contributes. When its visible
    /// fragments are settled, the pairings the box still admits are
    /// enumerated (each visible fragment to a band whose window its length
    /// interval meets); the true pairing at any map in the box is one of
    /// them, so the least tangent-plane bound over them is a bound for the
    /// digest. Otherwise the digest is bounded by the cheapest
    /// tolerance-respecting assignment of bands to fragments, and by the
    /// order statistics of the fragment intervals. Sums over digests of
    /// per-digest bounds are bounds for the total.
    fn bound(&self, s: &State, warm: &mut Vec<f64>) -> Option<u64> {
        let problem = self.problem;
        let d = problem.detection_limit as i64;
        let e = problem.error_percent;
        let nvars = s.boxes.len() + 1;
        let mut loose = 0u64;
        // per digest: the admissible pairings, each a list of (fragment, band)
        let mut structured: Vec<Vec<Vec<(Frag, f64)>>> = Vec::new();
        for dg in &self.ctx.digests {
            let Some(cuts) = self.ordered_cuts(dg, s) else {
                continue;
            };
            let frags = Self::fragments(&cuts);
            let m = dg.expanded.len();
            let mut certain = 0usize;
            let mut possible = 0usize;
            let mut visible: Vec<(Interval, Frag)> = Vec::new();
            let mut ambiguous: Vec<Interval> = Vec::new();
            for f in &frags {
                let iv = self.frag_interval(f, s);
                if iv.hi < d {
                    continue;
                }
                possible += 1;
                if iv.lo < d {
                    ambiguous.push(iv);
                    continue;
                }
                certain += 1;
                visible.push((iv, *f));
            }
            if certain > m || possible < m {
                return None;
            }
            for (iv, _) in &visible {
                let reachable = dg.distinct.iter().any(|&b| {
                    let b = b as i64;
                    let gap = if b < iv.lo { iv.lo - b } else if b > iv.hi { b - iv.hi } else { 0 };
                    gap as u64 <= digest::tolerance(b as u32, e) as u64
                });
                if !reachable {
                    return None;
                }
            }
            visible.sort_by_key(|(iv, _)| iv.lo);
            let settled = ambiguous.is_empty() && certain == m;
            let mut order_bound = 0u64;
            if settled {
                let lows: Vec<i64> = visible.iter().map(|(iv, _)| iv.lo).collect();
                let mut highs: Vec<i64> = visible.iter().map(|(iv, _)| iv.hi).collect();
                highs.sort_unstable();
                for k in 0..m {
                    let b = dg.expanded[k] as i64;
                    let tol = digest::tolerance(b as u32, e) as i64;
                    if lows[k] > b + tol || highs[k] < b - tol {
                        return None;
                    }
                    let gap = if b < lows[k] { lows[k] - b } else if b > highs[k] { b - highs[k] } else { 0 };
                    order_bound += (gap as u64) * (gap as u64);
                }
                // admissible pairings: backtracking over fragments in order of
                // lower bound, each to a still-free band whose window it meets
                let mut pairings: Vec<Vec<(Frag, f64)>> = Vec::new();
                let mut used = vec![false; m];
                let mut current: Vec<(Frag, f64)> = Vec::with_capacity(m);
                let mut visits = 0usize;
                fn rec(
                    k: usize,
                    visible: &[(Interval, Frag)],
                    bands: &[u32],
                    e: u32,
                    used: &mut Vec<bool>,
                    current: &mut Vec<(Frag, f64)>,
                    out: &mut Vec<Vec<(Frag, f64)>>,
                    visits: &mut usize,
                ) {
                    *visits += 1;
                    if out.len() > 16 || *visits > 400 {
                        return;
                    }
                    if k == visible.len() {
                        out.push(current.clone());
                        return;
                    }
                    let (iv, f) = visible[k];
                    let mut last: Option<u32> = None;
                    for (j, &b) in bands.iter().enumerate() {
                        if used[j] || last == Some(b) {
                            continue;
                        }
                        let tol = digest::tolerance(b, e) as i64;
                        let bi = b as i64;
                        if bi + tol < iv.lo || bi - tol > iv.hi {
                            continue;
                        }
                        last = Some(b);
                        used[j] = true;
                        current.push((f, b as f64));
                        rec(k + 1, visible, bands, e, used, current, out, visits);
                        current.pop();
                        used[j] = false;
                    }
                }
                rec(0, &visible, &dg.expanded, e, &mut used, &mut current, &mut pairings, &mut visits);
                if pairings.is_empty() && visits <= 400 {
                    return None;
                }
                if !pairings.is_empty() && pairings.len() <= 16 && visits <= 400 {
                    structured.push(pairings);
                    continue;
                }
            }
            // The pairing at any map in the box is some bijection between the
            // bands and the visible fragments, so the cheapest
            // tolerance-respecting assignment of bands to fragments that can
            // still be visible bounds the digest from below.
            let mut rows: Vec<(Interval, bool)> = visible.iter().map(|(iv, _)| (*iv, true)).collect();
            for iv in &ambiguous {
                rows.push((*iv, false));
            }
            let n = rows.len();
            let mut cost = vec![vec![INF; n]; n];
            for (r, (iv, certain_row)) in rows.iter().enumerate() {
                for (col, &b) in dg.expanded.iter().enumerate() {
                    let b = b as i64;
                    let gap = if b < iv.lo { iv.lo - b } else if b > iv.hi { b - iv.hi } else { 0 };
                    if gap <= digest::tolerance(b as u32, e) as i64 {
                        cost[r][col] = gap * gap;
                    }
                }
                for col in m..n {
                    cost[r][col] = if *certain_row { INF } else { 0 };
                }
            }
            let assigned = hungarian(&cost);
            if assigned >= INF {
                return None;
            }
            loose += (assigned as u64).max(order_bound);
        }
        if structured.is_empty() {
            return Some(loose);
        }
        let v = problem.v() as f64;
        let lo: Vec<f64> = std::iter::once(s.l.lo as f64).chain(s.boxes.iter().map(|b| b.lo as f64)).collect();
        let hi: Vec<f64> = std::iter::once(s.l.hi as f64).chain(s.boxes.iter().map(|b| b.hi as f64)).collect();
        if warm.len() != nvars {
            warm.clear();
            warm.extend((0..nvars).map(|i| (lo[i] + hi[i]) / 2.0));
        }
        for i in 0..nvars {
            warm[i] = warm[i].clamp(lo[i], hi[i]);
        }
        let coef = |f: &Frag, i: usize| -> f64 {
            let mut a = 0.0;
            let side = |cut: Cut, sign: f64, a: &mut f64| match cut {
                Cut::Box(j) if j + 1 == i => *a += sign,
                Cut::Moving(_) if i == 0 => *a += sign,
                _ => {}
            };
            side(f.plus, 1.0, &mut a);
            side(f.minus, -1.0, &mut a);
            if f.wrap && i == 0 {
                a += 1.0;
            }
            a
        };
        let value = |f: &Frag, y: &[f64]| -> f64 {
            let at = |cut: Cut| match cut {
                Cut::Fixed(p) => p as f64,
                Cut::Moving(p) => p as f64 + y[0],
                Cut::Box(j) => y[j + 1],
            };
            at(f.plus) - at(f.minus) + if f.wrap { v + y[0] } else { 0.0 }
        };
        // descend on the first admissible pairing of every digest
        let primary: Vec<&(Frag, f64)> = structured.iter().flat_map(|p| p[0].iter()).collect();
        let joint: Vec<&(Frag, f64)> = structured.iter().filter(|p| p.len() == 1).flat_map(|p| p[0].iter()).collect();
        for _ in 0..12 {
            for i in 0..nvars {
                let mut num = 0.0;
                let mut den = 0.0;
                for (f, b) in &primary {
                    let a = coef(f, i);
                    if a == 0.0 {
                        continue;
                    }
                    let r = value(f, warm) - b - a * warm[i];
                    num += a * r;
                    den += a * a;
                }
                if den > 0.0 {
                    warm[i] = (-num / den).clamp(lo[i], hi[i]);
                }
            }
        }
        // The tangent plane of a set of terms at `warm`, minimised over the box.
        let tangent_of = |terms: &[&(Frag, f64)], grad: &mut Vec<f64>| -> f64 {
            let mut q = 0.0;
            for g in grad.iter_mut() {
                *g = 0.0;
            }
            for (f, b) in terms {
                let r = value(f, warm) - b;
                q += r * r;
                for (i, g) in grad.iter_mut().enumerate() {
                    let a = coef(f, i);
                    if a != 0.0 {
                        *g += 2.0 * a * r;
                    }
                }
            }
            let mut tangent = q;
            for i in 0..nvars {
                let down = grad[i] * (lo[i] - warm[i]);
                let up = grad[i] * (hi[i] - warm[i]);
                tangent += down.min(up).min(0.0);
            }
            tangent
        };
        let mut grad = vec![0.0; nvars];
        let ambiguous: Vec<&Vec<Vec<(Frag, f64)>>> = structured.iter().filter(|p| p.len() > 1).collect();
        let combinations: usize = ambiguous.iter().map(|p| p.len()).product();
        let total = if combinations <= 64 {
            // Few enough joint pairings: the least joint tangent over all of
            // them bounds the total, since the true pairing is one of them.
            let mut best = f64::INFINITY;
            let mut index = vec![0usize; ambiguous.len()];
            loop {
                let mut terms: Vec<&(Frag, f64)> = joint.clone();
                for (k, p) in ambiguous.iter().enumerate() {
                    terms.extend(p[index[k]].iter());
                }
                let t = tangent_of(&terms, &mut grad);
                if t < best {
                    best = t;
                }
                let mut k = 0;
                while k < index.len() {
                    index[k] += 1;
                    if index[k] < ambiguous[k].len() {
                        break;
                    }
                    index[k] = 0;
                    k += 1;
                }
                if k == index.len() {
                    break;
                }
            }
            if best.is_finite() { best.max(0.0) } else { 0.0 }
        } else {
            // Otherwise the fixed-pairing digests share one plane and every
            // ambiguous digest contributes the least plane over its pairings.
            let mut total = if joint.is_empty() { 0.0 } else { tangent_of(&joint, &mut grad).max(0.0) };
            for pairings in &ambiguous {
                let mut best_digest = f64::INFINITY;
                for pairing in pairings.iter() {
                    let terms: Vec<&(Frag, f64)> = pairing.iter().collect();
                    let t = tangent_of(&terms, &mut grad);
                    if t < best_digest {
                        best_digest = t;
                    }
                }
                total += best_digest.max(0.0);
            }
            total
        };
        let structured_bound = if total > 0.0 { (total - 1e-6).floor().max(0.0) as u64 } else { 0 };
        Some(loose + structured_bound)
    }

    /// The map at the rounded near-minimiser of the box, when it is valid and
    /// reproduces every digest: an early incumbent for the deviation bound.
    fn probe_point(&self, s: &State, warm: &[f64], best: &mut Option<Best>) {
        if warm.len() != s.boxes.len() + 1 {
            return;
        }
        let l = (warm[0].round() as i64).clamp(s.l.lo, s.l.hi);
        let mut point = State { l: Interval::point(l), boxes: Vec::with_capacity(s.boxes.len()), orders: s.orders.clone() };
        for (i, b) in s.boxes.iter().enumerate() {
            let q = (warm[i + 1].round() as i64).clamp(b.lo, b.hi);
            point.boxes.push(Interval::point(q));
        }
        self.leaf(&point, best);
    }

    /// The widest fragment of an ordered digest that could still pair with
    /// more than one band (or with a band and fall below the limit), unless
    /// a constraint already covers it.
    fn unsettled(&self, s: &State, constraints: &[Constraint]) -> Option<(usize, usize, Interval)> {
        let d = self.problem.detection_limit as i64;
        let e = self.problem.error_percent;
        let mut pick: Option<(usize, usize, Interval)> = None;
        for (di, dg) in self.ctx.digests.iter().enumerate() {
            let Some(cuts) = self.ordered_cuts(dg, s) else {
                continue;
            };
            let frags = Self::fragments(&cuts);
            for (fi, f) in frags.iter().enumerate() {
                if constraints.iter().any(|k| k.digest == di && k.frag == fi) {
                    continue;
                }
                let iv = self.frag_interval(f, s);
                if iv.hi < d {
                    continue;
                }
                let options = dg
                    .distinct
                    .iter()
                    .filter(|&&b| {
                        let tol = digest::tolerance(b, e) as i64;
                        let b = b as i64;
                        b + tol >= iv.lo && b - tol <= iv.hi
                    })
                    .count()
                    + usize::from(iv.lo < d);
                if options <= 1 {
                    continue;
                }
                if pick.map_or(true, |(_, _, p)| iv.width() > p.width()) {
                    pick = Some((di, fi, iv));
                }
            }
        }
        pick
    }

    fn leaf(&self, s: &State, best: &mut Option<Best>) {
        let problem = self.problem;
        let ctx = self.ctx;
        let insert_length = s.l.lo as usize;
        let c = ctx.c;
        let mut cuts: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, enzyme) in problem.enzymes.iter().enumerate() {
            let mut positions: Vec<usize> = ctx.fixed[i].clone();
            positions.extend(ctx.moving[i].iter().map(|&p| p + insert_length));
            for r in 0..self.composition[i] {
                let q = s.boxes[self.first[i] + r].lo;
                if q < c as i64 || q > (c + insert_length) as i64 {
                    return;
                }
                positions.push(q as usize);
            }
            positions.sort_unstable();
            for w in positions.windows(2) {
                if w[0] == w[1] {
                    return;
                }
            }
            cuts.insert(enzyme.name.clone(), positions);
        }
        let n = problem.v() + insert_length;
        let mut total = 0u64;
        for dg in &problem.observed {
            let predicted = digest::predicted(&cuts, &dg.enzymes, n, problem.detection_limit);
            match digest::deviation(&predicted, &dg.bands, problem.error_percent) {
                Some(dev) => total += dev,
                None => return,
            }
        }
        let mut held_out = predictions(problem, insert_length, &cuts);
        for h in &problem.held_out { held_out.insert(h.id.clone(), digest::group(&digest::predicted(&cuts, &h.enzymes, n, 1))); }
        let map = PlasmidMap { cloning_site: c, insert_length, cuts, held_out };
        let k = key(&map, total);
        let better = match best {
            Some(b) => k < b.key,
            None => true,
        };
        if better {
            *best = Some(Best { key: k, map });
        }
    }

    fn recurse(&self, mut s: State, constraints: &mut Vec<Constraint>, best: &mut Option<Best>, warm: &mut Vec<f64>) {
        if !self.propagate(&mut s, constraints) {
            return;
        }
        let Some(lb) = self.bound(&s, warm) else {
            return;
        };
        if let Some(b) = &*best {
            if lb > b.key.deviation {
                return;
            }
        }
        if s.l.width() == 0 && s.boxes.iter().all(|b| b.width() == 0) {
            self.leaf(&s, best);
            return;
        }
        if s.l.width() <= 32 && s.boxes.iter().all(|b| b.width() <= 32) {
            self.probe_point(&s, warm, best);
            if let Some(b) = &*best {
                if lb > b.key.deviation {
                    return;
                }
            }
        }
        if let Some((di, fi, iv)) = self.unsettled(&s, constraints) {
            let dg = &self.ctx.digests[di];
            let d = self.problem.detection_limit as i64;
            let e = self.problem.error_percent;
            let mid = (iv.lo + iv.hi) / 2;
            let mut options: Vec<(i64, i64, i64)> = Vec::new(); // (distance from the middle, lo, hi)
            for &b in &dg.distinct {
                let tol = digest::tolerance(b, e) as i64;
                let b = b as i64;
                if b + tol >= iv.lo && b - tol <= iv.hi {
                    options.push(((b - mid).abs(), (b - tol).max(1), b + tol));
                }
            }
            if iv.lo < d {
                options.push((mid.abs(), 1, d - 1));
            }
            options.sort_unstable();
            for (_, lo, hi) in options {
                constraints.push(Constraint { digest: di, frag: fi, lo, hi });
                let mut w = warm.clone();
                self.recurse(s.clone(), constraints, best, &mut w);
                constraints.pop();
            }
            return;
        }
        // Two narrow cuts of different enzymes that could still be either way
        // round (or coincide): branch on their order, which orders every
        // digest containing both at once.
        let mut pair: Option<(usize, usize)> = None;
        'outer: for a in 0..s.boxes.len() {
            for b in (a + 1)..s.boxes.len() {
                if self.enzyme_of(a) == self.enzyme_of(b) {
                    continue;
                }
                let (x, y) = (s.boxes[a], s.boxes[b]);
                let overlap = x.hi >= y.lo && y.hi >= x.lo;
                let related = s.orders.iter().any(|&(p, q, _)| (p == a && q == b) || (p == b && q == a));
                // Wide boxes separate cheaply by halving; ordering pays once
                // both are narrow and the digest would otherwise stay unordered.
                let narrow = x.width() <= 32 && y.width() <= 32;
                if overlap && !related && narrow {
                    pair = Some((a, b));
                    break 'outer;
                }
            }
        }
        if let Some((a, b)) = pair {
            for rel in [-1i8, 1, 0] {
                let mut child = s.clone();
                child.orders.push((a, b, rel));
                if rel == 0 {
                    let lo = s.boxes[a].lo.max(s.boxes[b].lo);
                    let hi = s.boxes[a].hi.min(s.boxes[b].hi);
                    if lo > hi {
                        continue;
                    }
                    child.boxes[a] = Interval { lo, hi };
                    child.boxes[b] = Interval { lo, hi };
                }
                let mut w = warm.clone();
                self.recurse(child, constraints, best, &mut w);
            }
            return;
        }
        // Everything is settled: halve the widest interval.
        let mut widest = s.l.width();
        let mut which: Option<usize> = None;
        for (i, iv) in s.boxes.iter().enumerate() {
            if iv.width() > widest {
                widest = iv.width();
                which = Some(i);
            }
        }
        match which {
            None => {
                let mid = s.l.lo + (s.l.hi - s.l.lo) / 2;
                let mut left = s.clone();
                left.l = Interval { lo: s.l.lo, hi: mid };
                let mut w1 = warm.clone();
                self.recurse(left, constraints, best, &mut w1);
                let mut right = s;
                right.l = Interval { lo: mid + 1, hi: right.l.hi };
                let mut w2 = warm.clone();
                self.recurse(right, constraints, best, &mut w2);
            }
            Some(i) => {
                let mid = s.boxes[i].lo + (s.boxes[i].hi - s.boxes[i].lo) / 2;
                let mut left = s.clone();
                left.boxes[i] = Interval { lo: s.boxes[i].lo, hi: mid };
                let mut w1 = warm.clone();
                self.recurse(left, constraints, best, &mut w1);
                let mut right = s;
                right.boxes[i] = Interval { lo: mid + 1, hi: right.boxes[i].hi };
                let mut w2 = warm.clone();
                self.recurse(right, constraints, best, &mut w2);
            }
        }
    }
}

/// Searches one distribution of insert-region sites at one cloning site,
/// updating `best` with any map whose key is smaller.
pub fn search(problem: &Problem, ctx: &SiteContext, composition: &[usize], best: &mut Option<Best>) {
    let mut first = Vec::new();
    let mut count = 0usize;
    for &t in composition {
        first.push(count);
        count += t;
    }
    let search = Search { problem, ctx, composition, first, count };
    let c = ctx.c as i64;
    let l = Interval { lo: problem.insert_min as i64, hi: problem.insert_max as i64 };
    let state = State { l, boxes: (0..search.count).map(|_| Interval { lo: c, hi: c + l.hi }).collect(), orders: Vec::new() };
    let mut constraints = Vec::new();
    let mut warm: Vec<f64> = Vec::new();
    search.recurse(state, &mut constraints, best, &mut warm);
}
