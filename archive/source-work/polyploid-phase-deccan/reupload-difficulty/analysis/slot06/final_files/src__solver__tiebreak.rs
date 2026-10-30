
//! Choosing among equally scoring reconstructions.
//!
//! Finding the optimum is not the whole job: several reconstructions can attain
//! it, and `spec/result-format-v1.md` names the one required — the smallest in
//! the order parent A's rows, parent B's rows, then the full path signature.
//!
//! Given a column assignment and a homolog relabelling for each parent, that
//! order is separable: the progeny are independent, so each one's path can be
//! chosen on its own, and within one progeny parent A's states all sort before
//! parent B's. What is left is a lexicographic shortest path problem, solved
//! here by two forward passes over the joint states that lie on some optimal
//! path: the first fixes the smallest possible parent A sequence, the second
//! picks the smallest parent B sequence among the paths that keep it.

use crate::cross_model::{Haplotypes, Parent, ProgenyPath, Reconstruction, State, ALL_STATES};
use crate::solver::canon;
use crate::solver::tables::{Ctx, NS};

/// How close a partial total must be to the optimum to count as tied with it.
const TIE: f64 = 1e-9;

/// Largest number of joint states tolerated at one marker before the exact
/// lexicographic pass is abandoned for the plain Viterbi path.
const LIVE_LIMIT: usize = 96;

/// Builds one parent's four homolog rows from its column choices.
pub fn haplotypes(ctx: &Ctx, ca: &[u8], parent: Parent) -> Haplotypes {
    let mut rows = Haplotypes::zeros(ctx.m);
    let pi = parent.index();
    for mm in 0..ctx.m {
        let mask = ctx.cols[pi][mm][ca[mm] as usize];
        for h in 0..4 {
            rows.set(h, mm, (mask >> h) & 1);
        }
    }
    rows
}

/// Assembles a reconstruction from columns and one joint state per marker.
pub fn reconstruction(
    ctx: &Ctx,
    ca: &[u8],
    cb: &[u8],
    paths: &[Vec<usize>],
) -> Reconstruction {
    let mut answer = Reconstruction {
        haplotypes_a: haplotypes(ctx, ca, Parent::A),
        haplotypes_b: haplotypes(ctx, cb, Parent::B),
        paths: Vec::with_capacity(paths.len()),
    };
    for path in paths {
        let mut from_a = Vec::with_capacity(ctx.m);
        let mut from_b = Vec::with_capacity(ctx.m);
        for &j in path {
            from_a.push(ALL_STATES[j / NS]);
            from_b.push(ALL_STATES[j % NS]);
        }
        answer.paths.push(ProgenyPath { from_a, from_b });
    }
    answer
}

/// The remapped two byte spelling of every state, for one relabelling.
fn remapped(perm: &[u8; 4]) -> [[u8; 2]; NS] {
    let mut out = [[0u8; 2]; NS];
    for (si, state) in ALL_STATES.iter().enumerate() {
        let moved = state.remap(perm);
        out[si] = [moved.low(), moved.high()];
    }
    out
}

/// One progeny's path, chosen to be the smallest in the order the result format
/// compares: parent A's whole remapped sequence first, then parent B's.
pub fn min_signature_path(
    ctx: &Ctx,
    c: usize,
    ca: &[u8],
    cb: &[u8],
    perm_a: &[u8; 4],
    perm_b: &[u8; 4],
) -> Vec<usize> {
    let m = ctx.m;
    let prog = ctx.viterbi(c, ca, cb);
    let plain = prog.path();
    let asig = remapped(perm_a);
    let bsig = remapped(perm_b);

    // Node potentials, needed to ask whether a state sits on an optimal path.
    let mut node: Vec<[f64; NS * NS]> = Vec::with_capacity(m);
    for mm in 0..m {
        let d = &ctx.dose[mm][ca[mm] as usize * ctx.ncol[1][mm] + cb[mm] as usize];
        let da = ctx.drv[0][mm];
        let db = ctx.drv[1][mm];
        let mut row = [0.0f64; NS * NS];
        for j in 0..NS * NS {
            row[j] = ctx.gl[c][mm][d[j] as usize] + da[j / NS] + db[j % NS];
        }
        node.push(row);
    }

    // Joint states that lie on at least one optimal path.
    let mut live: Vec<Vec<usize>> = Vec::with_capacity(m);
    for mm in 0..m {
        let mut set = Vec::new();
        for j in 0..NS * NS {
            if prog.fwd[mm][j].is_finite()
                && prog.bwd[mm][j].is_finite()
                && (prog.fwd[mm][j] + prog.bwd[mm][j] - node[mm][j] - prog.val).abs() <= TIE
            {
                set.push(j);
            }
        }
        if set.len() > LIVE_LIMIT {
            return plain;
        }
        live.push(set);
    }

    let ahead = |j: usize| [asig[j / NS][0], asig[j / NS][1]];
    let bhead = |j: usize| [bsig[j % NS][0], bsig[j % NS][1]];
    let step = |mm: usize, from: usize, to: usize| -> f64 {
        ctx.tr[0][mm - 1][(from / NS) * NS + to / NS]
            + ctx.tr[1][mm - 1][(from % NS) * NS + to % NS]
    };

    // First pass: smallest parent A sequence over optimal paths.
    let mut best_a: Vec<Vec<Option<Vec<u8>>>> = vec![vec![None; NS * NS]; m];
    for &j in &live[0] {
        if (prog.fwd[0][j] + prog.bwd[0][j] - node[0][j] - prog.val).abs() <= TIE {
            let pair = ahead(j);
            best_a[0][j] = Some(vec![pair[0], pair[1]]);
        }
    }
    for mm in 1..m {
        for &j in &live[mm] {
            let target = prog.fwd[mm][j];
            let mut best: Option<Vec<u8>> = None;
            for &prev in &live[mm - 1] {
                if (prog.fwd[mm - 1][prev] + step(mm, prev, j) + node[mm][j] - target).abs() > TIE {
                    continue;
                }
                if let Some(held) = &best_a[mm - 1][prev] {
                    let pair = ahead(j);
                    let mut candidate = Vec::with_capacity(held.len() + 2);
                    candidate.extend_from_slice(held);
                    candidate.push(pair[0]);
                    candidate.push(pair[1]);
                    if best.as_ref().is_none_or(|b| candidate < *b) {
                        best = Some(candidate);
                    }
                }
            }
            best_a[mm][j] = best;
        }
    }

    let mut chosen_a: Option<Vec<u8>> = None;
    for &j in &live[m - 1] {
        if (prog.fwd[m - 1][j] - prog.val).abs() <= TIE {
            if let Some(held) = &best_a[m - 1][j] {
                if chosen_a.as_ref().is_none_or(|b| held < b) {
                    chosen_a = Some(held.clone());
                }
            }
        }
    }
    let Some(target_a) = chosen_a else { return plain };

    // Second pass: smallest parent B sequence among the paths that keep it.
    let keeps = |mm: usize, j: usize| -> bool {
        match &best_a[mm][j] {
            Some(held) => held.len() == 2 * (mm + 1) && held.as_slice() == &target_a[..2 * (mm + 1)],
            None => false,
        }
    };
    let mut best_b: Vec<Vec<Option<Vec<u8>>>> = vec![vec![None; NS * NS]; m];
    let mut from: Vec<Vec<Option<usize>>> = vec![vec![None; NS * NS]; m];
    for &j in &live[0] {
        if keeps(0, j) {
            let pair = bhead(j);
            best_b[0][j] = Some(vec![pair[0], pair[1]]);
        }
    }
    for mm in 1..m {
        for &j in &live[mm] {
            if !keeps(mm, j) {
                continue;
            }
            let target = prog.fwd[mm][j];
            let mut best: Option<Vec<u8>> = None;
            let mut arg: Option<usize> = None;
            for &prev in &live[mm - 1] {
                if !keeps(mm - 1, prev) {
                    continue;
                }
                if (prog.fwd[mm - 1][prev] + step(mm, prev, j) + node[mm][j] - target).abs() > TIE {
                    continue;
                }
                if let Some(held) = &best_b[mm - 1][prev] {
                    let pair = bhead(j);
                    let mut candidate = Vec::with_capacity(held.len() + 2);
                    candidate.extend_from_slice(held);
                    candidate.push(pair[0]);
                    candidate.push(pair[1]);
                    if best.as_ref().is_none_or(|b| candidate < *b) {
                        best = Some(candidate);
                        arg = Some(prev);
                    }
                }
            }
            best_b[mm][j] = best;
            from[mm][j] = arg;
        }
    }

    let mut end: Option<usize> = None;
    for &j in &live[m - 1] {
        if (prog.fwd[m - 1][j] - prog.val).abs() > TIE || !keeps(m - 1, j) {
            continue;
        }
        if let Some(held) = &best_b[m - 1][j] {
            let better = match end {
                None => true,
                Some(other) => best_b[m - 1][other].as_ref().is_none_or(|b| held < b),
            };
            if better {
                end = Some(j);
            }
        }
    }
    let Some(mut cursor) = end else { return plain };

    let mut out = vec![0usize; m];
    out[m - 1] = cursor;
    for mm in (1..m).rev() {
        match from[mm][cursor] {
            Some(prev) => {
                out[mm - 1] = prev;
                cursor = prev;
            }
            None => return plain,
        }
    }
    out
}

/// The reconstructions worth comparing for one column assignment.
///
/// The plain Viterbi paths, then the smallest signature paths under each
/// relabelling that keeps the rows sorted, and then the same again under
/// whatever relabelling canonical form picks for the result — because the
/// relabelling the specification chooses depends on the paths, so the two have
/// to be brought into agreement.
pub fn candidates(ctx: &Ctx, ca: &[u8], cb: &[u8]) -> Vec<Vec<Vec<usize>>> {
    let hap_a = haplotypes(ctx, ca, Parent::A);
    let hap_b = haplotypes(ctx, cb, Parent::B);
    let perms_a = canon::sorting_permutations(&hap_a);
    let perms_b = canon::sorting_permutations(&hap_b);

    let mut out: Vec<Vec<Vec<usize>>> = Vec::new();
    let mut plain: Vec<Vec<usize>> = Vec::with_capacity(ctx.n);
    for c in 0..ctx.n {
        plain.push(ctx.viterbi(c, ca, cb).path());
    }
    out.push(plain);

    let mut queue: Vec<([u8; 4], [u8; 4])> = Vec::new();
    for pa in &perms_a {
        for pb in &perms_b {
            queue.push((*pa, *pb));
        }
    }
    let mut tried: Vec<([u8; 4], [u8; 4])> = Vec::new();

    while let Some((pa, pb)) = queue.pop() {
        if tried.contains(&(pa, pb)) {
            continue;
        }
        tried.push((pa, pb));
        let mut paths = Vec::with_capacity(ctx.n);
        for c in 0..ctx.n {
            paths.push(min_signature_path(ctx, c, ca, cb, &pa, &pb));
        }
        let mut answer = reconstruction(ctx, ca, cb, &paths);
        canon::canonicalise(&mut answer);
        let next_a = canon::best_permutation(
            answer.haplotypes(Parent::A),
            &answer.paths.iter().map(|p| p.states(Parent::A).to_vec()).collect::<Vec<_>>(),
        );
        let next_b = canon::best_permutation(
            answer.haplotypes(Parent::B),
            &answer.paths.iter().map(|p| p.states(Parent::B).to_vec()).collect::<Vec<_>>(),
        );
        if !tried.contains(&(next_a, next_b)) && queue.len() < 64 {
            queue.push((next_a, next_b));
        }
        let rebuilt: Vec<Vec<usize>> = answer
            .paths
            .iter()
            .map(|p| {
                (0..ctx.m)
                    .map(|mm| {
                        p.states(Parent::A)[mm].canonical_index() * NS
                            + p.states(Parent::B)[mm].canonical_index()
                    })
                    .collect()
            })
            .collect();
        out.push(rebuilt);
    }
    out
}

/// The relabelling the specification would choose for these states.
pub fn chosen_perm(hap: &Haplotypes, states: &[Vec<State>]) -> [u8; 4] {
    canon::best_permutation(hap, states)
}
