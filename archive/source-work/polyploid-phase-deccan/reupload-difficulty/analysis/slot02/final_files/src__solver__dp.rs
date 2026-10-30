
//! Dynamic programming over one progeny's inheritance path.
//!
//! Given both parents' allele columns, a progeny's best path is a Viterbi pass
//! over the hundred joint states. Three variants are needed:
//!
//! - [`viterbi_path`] recovers an optimal path, used while searching.
//! - [`viterbi_value`] returns only its value, used when a candidate is being
//!   measured and thrown away.
//! - [`min_lex_path`] returns the lexicographically smallest path among the
//!   optimal ones, which is what the result format requires when several paths
//!   tie.

use super::tables::{Ctx, NS};

pub const NEG: f64 = f64::NEG_INFINITY;

/// Emission plus both double reduction terms for one joint state.
#[inline]
fn node(ctx: &Ctx, c: usize, m: usize, ca: &[u8; NS], cb: &[u8; NS], i: usize, j: usize) -> f64 {
    if !ctx.legal[0][m][i] || !ctx.legal[1][m][j] {
        NEG
    } else {
        ctx.gl[c][m][usize::from(ca[i] + cb[j])] + ctx.dr_sum[m][i * NS + j]
    }
}

/// Best path value for one progeny, discarding the path.
pub fn viterbi_value(ctx: &Ctx, cols_a: &[u8], cols_b: &[u8], c: usize) -> f64 {
    let m = ctx.markers;
    let mut prev = [NEG; NS * NS];
    {
        let ca = ctx.contrib_a(cols_a[0]);
        let cb = ctx.contrib_a(cols_b[0]);
        for i in 0..NS {
            for j in 0..NS {
                prev[i * NS + j] = node(ctx, c, 0, &ca, &cb, i, j);
            }
        }
    }
    for mm in 1..m {
        let ca = ctx.contrib_a(cols_a[mm]);
        let cb = ctx.contrib_a(cols_b[mm]);
        let ta = &ctx.trans[0][mm - 1];
        let tb = &ctx.trans[1][mm - 1];
        let mut tmp = [NEG; NS * NS];
        for ip in 0..NS {
            for j in 0..NS {
                let mut best = NEG;
                for i in 0..NS {
                    let v = prev[i * NS + j] + ta[i * NS + ip];
                    if v > best {
                        best = v;
                    }
                }
                tmp[ip * NS + j] = best;
            }
        }
        let mut next = [NEG; NS * NS];
        for ip in 0..NS {
            for jp in 0..NS {
                let mut best = NEG;
                for j in 0..NS {
                    let v = tmp[ip * NS + j] + tb[j * NS + jp];
                    if v > best {
                        best = v;
                    }
                }
                next[ip * NS + jp] = best + node(ctx, c, mm, &ca, &cb, ip, jp);
            }
        }
        prev = next;
    }
    let mut best = NEG;
    for value in prev.iter() {
        if *value > best {
            best = *value;
        }
    }
    best
}

/// Best path for one progeny, with the path itself.
pub fn viterbi_path(
    ctx: &Ctx,
    cols_a: &[u8],
    cols_b: &[u8],
    c: usize,
) -> (f64, [Vec<u8>; 2]) {
    let m = ctx.markers;
    let mut prev = [NEG; NS * NS];
    let mut back_i = vec![[0u8; NS * NS]; m];
    let mut back_j = vec![[0u8; NS * NS]; m];
    {
        let ca = ctx.contrib_a(cols_a[0]);
        let cb = ctx.contrib_a(cols_b[0]);
        for i in 0..NS {
            for j in 0..NS {
                prev[i * NS + j] = node(ctx, c, 0, &ca, &cb, i, j);
            }
        }
    }
    for mm in 1..m {
        let ca = ctx.contrib_a(cols_a[mm]);
        let cb = ctx.contrib_a(cols_b[mm]);
        let ta = &ctx.trans[0][mm - 1];
        let tb = &ctx.trans[1][mm - 1];
        let mut tmp = [NEG; NS * NS];
        for ip in 0..NS {
            for j in 0..NS {
                let mut best = NEG;
                let mut arg = 0usize;
                for i in 0..NS {
                    let v = prev[i * NS + j] + ta[i * NS + ip];
                    if v > best {
                        best = v;
                        arg = i;
                    }
                }
                tmp[ip * NS + j] = best;
                back_i[mm][ip * NS + j] = arg as u8;
            }
        }
        let mut next = [NEG; NS * NS];
        for ip in 0..NS {
            for jp in 0..NS {
                let mut best = NEG;
                let mut arg = 0usize;
                for j in 0..NS {
                    let v = tmp[ip * NS + j] + tb[j * NS + jp];
                    if v > best {
                        best = v;
                        arg = j;
                    }
                }
                let index = ip * NS + jp;
                next[index] = best + node(ctx, c, mm, &ca, &cb, ip, jp);
                back_j[mm][index] = arg as u8;
            }
        }
        prev = next;
    }

    let mut cursor = 0usize;
    for index in 1..NS * NS {
        if prev[index] > prev[cursor] {
            cursor = index;
        }
    }
    let mut from_a = vec![0u8; m];
    let mut from_b = vec![0u8; m];
    let mut index = cursor;
    for mm in (0..m).rev() {
        from_a[mm] = (index / NS) as u8;
        from_b[mm] = (index % NS) as u8;
        if mm > 0 {
            let j = usize::from(back_j[mm][index]);
            let i = usize::from(back_i[mm][(index / NS) * NS + j]);
            index = i * NS + j;
        }
    }
    (prev[cursor], [from_a, from_b])
}

/// Suffix maxima of the joint Viterbi recursion for one progeny.
///
/// `b[m][i * NS + j]` is the best value of markers `m..M` given the state at
/// `m`; `r[interval][i * NS + j]` is the best value of markers `interval+1..M`
/// reachable from state `(i, j)` at `interval`, transitions included.
pub struct Backward {
    pub b: Vec<[f64; NS * NS]>,
    pub r: Vec<[f64; NS * NS]>,
    pub vstar: f64,
}

pub fn backward(ctx: &Ctx, cols_a: &[u8], cols_b: &[u8], c: usize) -> Backward {
    let m = ctx.markers;
    let mut b = vec![[NEG; NS * NS]; m];
    {
        let ca = ctx.contrib_a(cols_a[m - 1]);
        let cb = ctx.contrib_a(cols_b[m - 1]);
        for i in 0..NS {
            for j in 0..NS {
                b[m - 1][i * NS + j] = node(ctx, c, m - 1, &ca, &cb, i, j);
            }
        }
    }
    let mut r: Vec<[f64; NS * NS]> = Vec::with_capacity(m.saturating_sub(1));
    for _ in 0..m.saturating_sub(1) {
        r.push([NEG; NS * NS]);
    }
    for mm in (0..m - 1).rev() {
        let ta = &ctx.trans[0][mm];
        let tb = &ctx.trans[1][mm];
        let next_b = &b[mm + 1];
        let mut u = [NEG; NS * NS];
        for ip in 0..NS {
            for j in 0..NS {
                let mut best = NEG;
                for jp in 0..NS {
                    let v = tb[j * NS + jp] + next_b[ip * NS + jp];
                    if v > best {
                        best = v;
                    }
                }
                u[ip * NS + j] = best;
            }
        }
        let mut rr = [NEG; NS * NS];
        for i in 0..NS {
            for j in 0..NS {
                let mut best = NEG;
                for ip in 0..NS {
                    let v = ta[i * NS + ip] + u[ip * NS + j];
                    if v > best {
                        best = v;
                    }
                }
                rr[i * NS + j] = best;
            }
        }
        r[mm] = rr;
        let ca = ctx.contrib_a(cols_a[mm]);
        let cb = ctx.contrib_a(cols_b[mm]);
        for i in 0..NS {
            for j in 0..NS {
                let here = node(ctx, c, mm, &ca, &cb, i, j);
                b[mm][i * NS + j] = if here == NEG { NEG } else { here + rr[i * NS + j] };
            }
        }
    }
    let mut vstar = NEG;
    for value in b[0].iter() {
        if *value > vstar {
            vstar = *value;
        }
    }
    Backward { b, r, vstar }
}

/// Prefix maxima of the joint Viterbi recursion for one progeny.
///
/// `g[m][i * NS + j]` is the best value of markers `0..m` up to and including
/// the transition into state `(i, j)` at `m`, but excluding the weight of `m`
/// itself, so `g[0]` is zero everywhere. Together with [`Backward::r`] it gives
/// the value of any path through marker `m` as `g[m][s] + weight + r[m][s]`.
pub fn forward(ctx: &Ctx, cols_a: &[u8], cols_b: &[u8], c: usize) -> Vec<[f64; NS * NS]> {
    let m = ctx.markers;
    let mut out: Vec<[f64; NS * NS]> = Vec::with_capacity(m);
    let mut prev = [NEG; NS * NS];
    {
        let ca = ctx.contrib_a(cols_a[0]);
        let cb = ctx.contrib_a(cols_b[0]);
        for i in 0..NS {
            for j in 0..NS {
                prev[i * NS + j] = node(ctx, c, 0, &ca, &cb, i, j);
            }
        }
    }
    out.push([0.0; NS * NS]);
    for mm in 1..m {
        let ta = &ctx.trans[0][mm - 1];
        let tb = &ctx.trans[1][mm - 1];
        let mut tmp = [NEG; NS * NS];
        for ip in 0..NS {
            for j in 0..NS {
                let mut best = NEG;
                for i in 0..NS {
                    let v = prev[i * NS + j] + ta[i * NS + ip];
                    if v > best {
                        best = v;
                    }
                }
                tmp[ip * NS + j] = best;
            }
        }
        let mut g = [NEG; NS * NS];
        for ip in 0..NS {
            for jp in 0..NS {
                let mut best = NEG;
                for j in 0..NS {
                    let v = tmp[ip * NS + j] + tb[j * NS + jp];
                    if v > best {
                        best = v;
                    }
                }
                g[ip * NS + jp] = best;
            }
        }
        out.push(g);

        let ca = ctx.contrib_a(cols_a[mm]);
        let cb = ctx.contrib_a(cols_b[mm]);
        let mut next = [NEG; NS * NS];
        for ip in 0..NS {
            for jp in 0..NS {
                let here = node(ctx, c, mm, &ca, &cb, ip, jp);
                next[ip * NS + jp] = if here == NEG { NEG } else { g[ip * NS + jp] + here };
            }
        }
        prev = next;
    }
    out
}

/// Relabels the joint state index of a backward table through `pa` and `pb`.
///
/// The value of a path is unchanged by relabelling, so a table computed once
/// can be reused for every candidate relabelling instead of being rebuilt.
pub fn permute_backward(
    b: &[[f64; NS * NS]],
    r: &[[f64; NS * NS]],
    pa: &[u8; 4],
    pb: &[u8; 4],
) -> (Vec<[f64; NS * NS]>, Vec<[f64; NS * NS]>) {
    use super::tables::{SA, SB, STATE_INDEX};
    let remap = |index: usize| -> usize {
        let i = index / NS;
        let j = index % NS;
        let ni = STATE_INDEX[usize::from(pa[usize::from(SA[i])])]
            [usize::from(pa[usize::from(SB[i])])];
        let nj = STATE_INDEX[usize::from(pb[usize::from(SA[j])])]
            [usize::from(pb[usize::from(SB[j])])];
        usize::from(ni) * NS + usize::from(nj)
    };
    let out_b: Vec<[f64; NS * NS]> = b
        .iter()
        .map(|row| {
            let mut out = [NEG; NS * NS];
            for index in 0..NS * NS {
                out[remap(index)] = row[index];
            }
            out
        })
        .collect();
    let out_r: Vec<[f64; NS * NS]> = r
        .iter()
        .map(|row| {
            let mut out = [NEG; NS * NS];
            for index in 0..NS * NS {
                out[remap(index)] = row[index];
            }
            out
        })
        .collect();
    (out_b, out_r)
}

/// Lexicographically smallest optimal path for one progeny.
///
/// The ordering is the one `spec/result-format-v1.md` minimises: parent A's
/// states for markers `0..M`, then parent B's, each state compared by its
/// canonical index. `eps` absorbs the floating point difference between the
/// two recurrences that compute the optimum, and a prefix is accepted when it
/// still admits a completion within `eps` of the optimum, so the whole path
/// ends up within `eps` of it too.
pub fn min_lex_path(
    ctx: &Ctx,
    b: &[[f64; NS * NS]],
    r: &[[f64; NS * NS]],
    vstar: f64,
    cols_a: &[u8],
    cols_b: &[u8],
    c: usize,
    eps: f64,
) -> (Vec<u8>, Vec<u8>) {
    let m = ctx.markers;
    let target = vstar - eps;

    // Phase one: choose parent A's states, markers ascending. `w[j]` is the
    // best value of the prefix `0..=k` over the free parent B states, with the
    // chosen A states fixed and parent B ending at state `j`.
    let mut chosen_a = vec![0u8; m];
    let mut w = [NEG; NS];
    for k in 0..m {
        let ca = ctx.contrib_a(cols_a[k]);
        let cb = ctx.contrib_a(cols_b[k]);
        let mut accepted = None;
        for a in 0..NS {
            if !ctx.legal[0][k][a] {
                continue;
            }
            let mut candidate = [NEG; NS];
            if k == 0 {
                for j in 0..NS {
                    if !ctx.legal[1][k][j] {
                        continue;
                    }
                    candidate[j] =
                        ctx.gl[c][k][usize::from(ca[a] + cb[j])] + ctx.dr_sum[k][a * NS + j];
                }
            } else {
                let offset = ctx.trans[0][k - 1][usize::from(chosen_a[k - 1]) * NS + a];
                let tb = &ctx.trans[1][k - 1];
                for j in 0..NS {
                    if !ctx.legal[1][k][j] {
                        continue;
                    }
                    let mut best = NEG;
                    for jp in 0..NS {
                        if w[jp] == NEG {
                            continue;
                        }
                        let v = w[jp] + offset + tb[jp * NS + j];
                        if v > best {
                            best = v;
                        }
                    }
                    if best == NEG {
                        continue;
                    }
                    candidate[j] =
                        best + ctx.gl[c][k][usize::from(ca[a] + cb[j])] + ctx.dr_sum[k][a * NS + j];
                }
            }
            let mut best = NEG;
            if k + 1 < m {
                for j in 0..NS {
                    if candidate[j] == NEG {
                        continue;
                    }
                    let v = candidate[j] + r[k][a * NS + j];
                    if v > best {
                        best = v;
                    }
                }
            } else {
                for j in 0..NS {
                    if candidate[j] > best {
                        best = candidate[j];
                    }
                }
            }
            if best >= target {
                accepted = Some((a as u8, candidate));
                break;
            }
        }
        let (a, candidate) = accepted.expect("every marker admits a legal state");
        chosen_a[k] = a;
        w = candidate;
    }

    // Phase two: parent A's states are fixed, so the prefix value is a scalar
    // and parent B's states are chosen the same greedy way.
    let mut chosen_b = vec![0u8; m];
    let mut accumulated = NEG;
    for k in 0..m {
        let ca = ctx.contrib_a(cols_a[k]);
        let cb = ctx.contrib_a(cols_b[k]);
        let mut suffix = [NEG; NS];
        if k + 1 < m {
            let ta = &ctx.trans[0][k];
            let tb = &ctx.trans[1][k];
            let next_b = &b[k + 1];
            // p[jp] folds parent A's now fixed move into the suffix table.
            let mut p = [NEG; NS];
            for jp in 0..NS {
                let mut best = NEG;
                for ip in 0..NS {
                    let v = ta[usize::from(chosen_a[k]) * NS + ip] + next_b[ip * NS + jp];
                    if v > best {
                        best = v;
                    }
                }
                p[jp] = best;
            }
            for j in 0..NS {
                let mut best = NEG;
                for jp in 0..NS {
                    if p[jp] == NEG {
                        continue;
                    }
                    let v = tb[j * NS + jp] + p[jp];
                    if v > best {
                        best = v;
                    }
                }
                suffix[j] = best;
            }
        }
        let mut accepted = None;
        for j in 0..NS {
            if !ctx.legal[1][k][j] {
                continue;
            }
            let here = ctx.gl[c][k][usize::from(ca[usize::from(chosen_a[k])] + cb[j])]
                + ctx.dr_sum[k][usize::from(chosen_a[k]) * NS + j];
            let value = if k == 0 {
                here
            } else {
                let offset = ctx.trans[1][k - 1][usize::from(chosen_b[k - 1]) * NS + j]
                    + ctx.trans[0][k - 1][usize::from(chosen_a[k - 1]) * NS + usize::from(chosen_a[k])];
                accumulated + offset + here
            };
            let reachable = if k + 1 < m { value + suffix[j] } else { value };
            if reachable >= target {
                accepted = Some((j as u8, value));
                break;
            }
        }
        let (j, value) = accepted.expect("every marker admits a legal state");
        chosen_b[k] = j;
        accumulated = value;
    }

    (chosen_a, chosen_b)
}
