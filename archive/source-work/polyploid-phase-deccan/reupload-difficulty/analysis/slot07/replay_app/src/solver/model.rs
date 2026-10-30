
//! Precomputed tables and the per progeny Viterbi.
//!
//! Everything the search needs about a cross is reduced here to flat numeric
//! tables, so that the inner loops of the search touch nothing but `f64` and
//! `u8` slices.
//!
//! Two indexings recur everywhere:
//!
//! * A parent state index `s` in `0..10`, in the canonical order of
//!   `spec/scoring-model.md`. `LOW[s]` and `HIGH[s]` are its two homologs.
//! * A joint state index `j = sa * 10 + sb` in `0..100`, the pair of parent
//!   states a progeny carries at one marker.
//!
//! A parental haplotype assignment is held as one column choice per parent per
//! marker, `z[m] = iA * nB + iB`, where `iA` and `iB` index the admissible
//! allele columns at that marker.

use crate::cross_input::Cross;
use crate::cross_model::Parent;
use crate::scoring::{double_reduction_term, transition_term};

/// States per parent, `C(4,2) + 4`.
pub const NS: usize = 10;
/// Joint states per progeny, `NS * NS`.
pub const NJ: usize = 100;
/// Used wherever a joint state cannot occur: an illegal double reduction, or a
/// state that no path has reached yet.
pub const NEG: f64 = f64::NEG_INFINITY;

/// Lower homolog of each canonical parent state.
pub const LOW: [u8; NS] = [0, 0, 0, 0, 1, 1, 1, 2, 2, 3];
/// Upper homolog of each canonical parent state.
pub const HIGH: [u8; NS] = [0, 1, 2, 3, 1, 2, 3, 2, 3, 3];

#[inline(always)]
pub fn split(j: usize) -> (usize, usize) {
    (j / NS, j % NS)
}

/// A cross reduced to tables.
pub struct Model {
    pub markers: usize,
    pub offspring: usize,
    /// `columns[parent][marker][choice]` is the allele of each of the four
    /// homologs, for every column with the dosage that marker requires.
    pub columns: [Vec<Vec<[u8; 4]>>; 2],
    /// Number of admissible columns per parent per marker.
    pub ncol: [Vec<usize>; 2],
    /// Number of joint column choices per marker, `ncol[0][m] * ncol[1][m]`.
    pub zcount: Vec<usize>,
    /// `dose[m][z][j]` is the progeny dosage for joint state `j` under `z`.
    pub dose: Vec<Vec<[u8; NJ]>>,
    /// `base[m][j]` is the double reduction contribution of joint state `j` at
    /// marker `m`, or `NEG` when the state is illegal there.
    pub base: Vec<[f64; NJ]>,
    /// `legal_a[m]` and `legal_b[m]`: the parent states that can occur at marker
    /// `m`. A joint state is legal exactly when both its halves are, so the
    /// legal joint states are the product of the two, and every inner loop can
    /// run over that product instead of all hundred.
    pub legal_a: Vec<Vec<u8>>,
    pub legal_b: Vec<Vec<u8>>,
    /// `legal[m]`: every legal joint state `sa * NS + sb` at marker `m`.
    pub legal: Vec<Vec<usize>>,
    /// `ta[interval][s * NS + s']`, parent A's transition log likelihood.
    pub ta: Vec<[f64; NS * NS]>,
    /// As `ta`, for parent B.
    pub tb: Vec<[f64; NS * NS]>,
    /// `gl[c][m][d]`, copied from the cross.
    pub gl: Vec<Vec<[f64; 5]>>,
    /// Scratch space, one row per thread of the search.
    node_scratch: Vec<[f64; NJ]>,
}

impl Model {
    pub fn new(cross: &Cross) -> Model {
        let markers = cross.markers;
        let offspring = cross.offspring;

        let mut columns: [Vec<Vec<[u8; 4]>>; 2] = [Vec::new(), Vec::new()];
        let mut ncol: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
        for (p, parent) in Parent::BOTH.iter().enumerate() {
            let dosage = &cross.params(*parent).dosage;
            let mut per_marker = Vec::with_capacity(markers);
            let mut counts = Vec::with_capacity(markers);
            for m in 0..markers {
                let want = dosage[m];
                let mut list: Vec<[u8; 4]> = Vec::new();
                for bits in 0u8..16 {
                    if bits.count_ones() as u8 == want {
                        list.push([bits & 1, (bits >> 1) & 1, (bits >> 2) & 1, (bits >> 3) & 1]);
                    }
                }
                counts.push(list.len());
                per_marker.push(list);
            }
            columns[p] = per_marker;
            ncol[p] = counts;
        }

        let mut zcount = Vec::with_capacity(markers);
        let mut dose: Vec<Vec<[u8; NJ]>> = Vec::with_capacity(markers);
        for m in 0..markers {
            let na = ncol[0][m];
            let nb = ncol[1][m];
            zcount.push(na * nb);
            let mut per_z: Vec<[u8; NJ]> = Vec::with_capacity(na * nb);
            for ia in 0..na {
                let ca = columns[0][m][ia];
                for ib in 0..nb {
                    let cb = columns[1][m][ib];
                    let mut row = [0u8; NJ];
                    for j in 0..NJ {
                        let (sa, sb) = split(j);
                        let from_a = ca[usize::from(LOW[sa])] + ca[usize::from(HIGH[sa])];
                        let from_b = cb[usize::from(LOW[sb])] + cb[usize::from(HIGH[sb])];
                        row[j] = from_a + from_b;
                    }
                    per_z.push(row);
                }
            }
            dose.push(per_z);
        }

        let mut base: Vec<[f64; NJ]> = Vec::with_capacity(markers);
        let mut legal_a: Vec<Vec<u8>> = Vec::with_capacity(markers);
        let mut legal_b: Vec<Vec<u8>> = Vec::with_capacity(markers);
        let mut legal: Vec<Vec<usize>> = Vec::with_capacity(markers);
        for m in 0..markers {
            let mut row = [0.0f64; NJ];
            let allowed_a = &cross.params(Parent::A).dr_allowed;
            let allowed_b = &cross.params(Parent::B).dr_allowed;
            let mut la = Vec::with_capacity(NS);
            let mut lb = Vec::with_capacity(NS);
            for sa in 0..NS {
                let ok_a = LOW[sa] != HIGH[sa] || allowed_a[m];
                let term_a = double_reduction_term(cross, Parent::A, state_of(sa), m);
                if ok_a {
                    la.push(sa as u8);
                }
                for sb in 0..NS {
                    let ok_b = LOW[sb] != HIGH[sb] || allowed_b[m];
                    let term_b = double_reduction_term(cross, Parent::B, state_of(sb), m);
                    row[sa * NS + sb] = if ok_a && ok_b { term_a + term_b } else { NEG };
                }
            }
            for sb in 0..NS {
                let ok_b = LOW[sb] != HIGH[sb] || allowed_b[m];
                if ok_b {
                    lb.push(sb as u8);
                }
            }
            let mut joint = Vec::with_capacity(la.len() * lb.len());
            for sa in la.iter() {
                for sb in lb.iter() {
                    joint.push(usize::from(*sa) * NS + usize::from(*sb));
                }
            }
            base.push(row);
            legal_a.push(la);
            legal_b.push(lb);
            legal.push(joint);
        }

        let mut ta: Vec<[f64; NS * NS]> = Vec::with_capacity(markers.saturating_sub(1));
        let mut tb: Vec<[f64; NS * NS]> = Vec::with_capacity(markers.saturating_sub(1));
        for m in 0..markers.saturating_sub(1) {
            let mut ra = [0.0f64; NS * NS];
            let mut rb = [0.0f64; NS * NS];
            for s in 0..NS {
                for t in 0..NS {
                    ra[s * NS + t] = transition_term(cross, Parent::A, state_of(s), state_of(t), m);
                    rb[s * NS + t] = transition_term(cross, Parent::B, state_of(s), state_of(t), m);
                }
            }
            ta.push(ra);
            tb.push(rb);
        }

        Model {
            markers,
            offspring,
            columns,
            ncol,
            zcount,
            dose,
            base,
            legal_a,
            legal_b,
            legal,
            ta,
            tb,
            gl: cross.child_gl.clone(),
            node_scratch: Vec::new(),
        }
    }

    /// Node contribution of joint state `j` for progeny `c` at marker `m`.
    ///
    /// Emission plus both double reduction terms, or `NEG` when the state is
    /// illegal at that marker.
    #[inline(always)]
    pub fn node_cost(&self, c: usize, m: usize, z: usize, j: usize) -> f64 {
        self.gl[c][m][usize::from(self.dose[m][z][j])] + self.base[m][j]
    }

    /// Fills `out` with the node contribution of every joint state.
    pub fn node_costs(&self, c: usize, m: usize, z: usize, out: &mut [f64; NJ]) {
        let gl = &self.gl[c][m];
        let dose = &self.dose[m][z];
        let base = &self.base[m];
        for j in 0..NJ {
            out[j] = gl[usize::from(dose[j])] + base[j];
        }
    }

    /// Total log likelihood of the best path for one progeny under `z`.
    pub fn viterbi(&self, c: usize, z: &[usize]) -> f64 {
        let mut cur = [0.0f64; NJ];
        let mut next = [0.0f64; NJ];
        let mut mid = [0.0f64; NJ];
        let mut node = [0.0f64; NJ];
        for m in 0..self.markers {
            self.node_costs(c, m, z[m], &mut node);
            if m == 0 {
                cur.copy_from_slice(&node);
                continue;
            }
            self.advance(&self.ta[m - 1], &self.tb[m - 1], &cur, &mut mid, &mut next, &node);
            cur.copy_from_slice(&next);
        }
        best_of(&cur)
    }

    /// Advances one marker: `next[j'] = max_j (cur[j] + T[j][j']) + node[j']`.
    ///
    /// The transition is a sum of one term per parent, so the maximisation over
    /// the hundred joint states factors into two passes over ten states each
    /// rather than a hundred by hundred product. `mid` is scratch.
    pub fn advance(
        &self,
        ta: &[f64; NS * NS],
        tb: &[f64; NS * NS],
        cur: &[f64; NJ],
        mid: &mut [f64; NJ],
        next: &mut [f64; NJ],
        node: &[f64; NJ],
    ) {
        for sp in 0..NS {
            let row = &ta[sp * NS..sp * NS + NS];
            for b in 0..NS {
                let mut best = NEG;
                for s in 0..NS {
                    let v = cur[s * NS + b] + row[s];
                    if v > best {
                        best = v;
                    }
                }
                mid[sp * NS + b] = best;
            }
        }
        for sp in 0..NS {
            for bp in 0..NS {
                let mut best = NEG;
                let row = &tb[bp * NS..bp * NS + NS];
                for b in 0..NS {
                    let v = mid[sp * NS + b] + row[b];
                    if v > best {
                        best = v;
                    }
                }
                let j = sp * NS + bp;
                next[j] = best + node[j];
            }
        }
    }

    /// Best path for one progeny, as joint state indices, and its score.
    pub fn viterbi_path(&self, c: usize, z: &[usize]) -> (f64, Vec<usize>) {
        let markers = self.markers;
        let mut cur = [0.0f64; NJ];
        let mut next = [0.0f64; NJ];
        let mut mid = [0.0f64; NJ];
        let mut node = [0.0f64; NJ];
        let mut back_a: Vec<[u8; NJ]> = Vec::with_capacity(markers);
        let mut back_b: Vec<[u8; NJ]> = Vec::with_capacity(markers);
        for m in 0..markers {
            self.node_costs(c, m, z[m], &mut node);
            if m == 0 {
                cur.copy_from_slice(&node);
                back_a.push([0u8; NJ]);
                back_b.push([0u8; NJ]);
                continue;
            }
            let ta = &self.ta[m - 1];
            let tb = &self.tb[m - 1];
            let mut ba = [0u8; NJ];
            let mut bb = [0u8; NJ];
            for sp in 0..NS {
                for b in 0..NS {
                    let mut best = NEG;
                    let mut arg = 0usize;
                    for s in 0..NS {
                        let v = cur[s * NS + b] + ta[s * NS + sp];
                        if v > best {
                            best = v;
                            arg = s;
                        }
                    }
                    mid[sp * NS + b] = best;
                    ba[sp * NS + b] = arg as u8;
                }
            }
            for sp in 0..NS {
                for bp in 0..NS {
                    let mut best = NEG;
                    let mut arg = 0usize;
                    for b in 0..NS {
                        let v = mid[sp * NS + b] + tb[b * NS + bp];
                        if v > best {
                            best = v;
                            arg = b;
                        }
                    }
                    let j = sp * NS + bp;
                    next[j] = best + node[j];
                    bb[j] = arg as u8;
                }
            }
            cur.copy_from_slice(&next);
            back_a.push(ba);
            back_b.push(bb);
        }

        let mut cursor = 0usize;
        let mut best = NEG;
        for j in 0..NJ {
            if cur[j] > best {
                best = cur[j];
                cursor = j;
            }
        }
        let mut path = vec![0usize; markers];
        for m in (0..markers).rev() {
            path[m] = cursor;
            if m > 0 {
                let (sa, _sb) = split(cursor);
                let prev_b = usize::from(back_b[m][cursor]);
                let prev_a = usize::from(back_a[m][sa * NS + prev_b]);
                cursor = prev_a * NS + prev_b;
            }
        }
        (best, path)
    }

    /// Total for every progeny under `z`: the value of the assignment.
    pub fn value(&self, z: &[usize]) -> f64 {
        let mut total = 0.0f64;
        for c in 0..self.offspring {
            total += self.viterbi(c, z);
        }
        total
    }

    /// Scratch buffers for one search thread.
    pub fn scratch(&mut self, count: usize) -> &mut Vec<[f64; NJ]> {
        if self.node_scratch.len() < count {
            self.node_scratch.resize(count, [0.0f64; NJ]);
        }
        &mut self.node_scratch
    }
}

#[inline(always)]
pub fn best_of(row: &[f64; NJ]) -> f64 {
    let mut best = NEG;
    for v in row.iter() {
        if *v > best {
            best = *v;
        }
    }
    best
}

fn state_of(index: usize) -> crate::cross_model::State {
    crate::cross_model::State::new(LOW[index], HIGH[index])
        .expect("canonical state indices name homologs in range")
}
