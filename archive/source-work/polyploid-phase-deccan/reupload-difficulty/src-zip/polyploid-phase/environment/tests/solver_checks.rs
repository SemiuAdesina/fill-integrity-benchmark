
//! Solver diagnostics on enumerable crosses.
//!
//! Every cross below sits at the twelve-marker, eight-progeny floor of the input
//! format but pins `parent_dosage` to 0 or 4 at all but a few markers. A marker
//! with dosage 0 or 4 admits exactly one allele column, so the admissible
//! parental assignments for these crosses can be enumerated in full and the
//! optimum found with no search at all. Each check runs `solve` on such a cross
//! and holds the answer to something the specification requires: agreement with
//! the enumerated optimum, legality, canonical form, the documented
//! representative among equally scoring answers, and reproducibility.
//!
//! These are diagnostics, not the graded suite. Agreeing with enumeration where
//! enumeration is possible says nothing about instances where it is not, and
//! the graded run uses crosses of its own.
//!
//! Run with:
//!
//!     cargo test --offline --locked --release --test solver_checks

use polyploid_phase::cross_input::{parse_cross, Cross};
use polyploid_phase::cross_model::{
    Haplotypes, Parent, Reconstruction, State, ALL_STATES, HOMOLOGS,
};
use polyploid_phase::digest::canonical_digest;
use polyploid_phase::result_output::{decode, encode};
use polyploid_phase::scoring::{
    double_reduction_term, score, scores_equal, transition_term,
};
use polyploid_phase::solver::solve;

const M: usize = 12;
const N: usize = 8;

// -------------------------------------------------------------------------
// Building a cross
// -------------------------------------------------------------------------

/// One parent's declared parameters for a diagnostic cross.
struct ParentSpec {
    dosage: [u8; M],
    recombination: [f64; M - 1],
    dr_allowed: [bool; M],
    dr_rate: [f64; M],
}

/// A cross assembled from a planted truth. The likelihoods are peaked at the
/// dosage each progeny would carry under the planted haplotypes and paths, so
/// the enumerated optimum is well defined; how sharply they are peaked is set
/// per cross.
struct CrossSpec {
    a: ParentSpec,
    b: ParentSpec,
    child_gl: Vec<Vec<[f64; 5]>>,
}

fn json_list<T: std::fmt::Display>(items: &[T]) -> String {
    let parts: Vec<String> = items.iter().map(|x| x.to_string()).collect();
    format!("[{}]", parts.join(","))
}

fn json_f64_list(items: &[f64]) -> String {
    let parts: Vec<String> = items.iter().map(|x| format!("{x:?}")).collect();
    format!("[{}]", parts.join(","))
}

fn json_bool_list(items: &[bool]) -> String {
    let parts: Vec<String> = items.iter().map(|x| x.to_string()).collect();
    format!("[{}]", parts.join(","))
}

impl CrossSpec {
    fn to_json(&self) -> String {
        let gl: Vec<String> = self
            .child_gl
            .iter()
            .map(|progeny| {
                let rows: Vec<String> = progeny.iter().map(|row| json_f64_list(row)).collect();
                format!("[{}]", rows.join(","))
            })
            .collect();
        format!(
            concat!(
                "{{\"format_version\":1,\"markers\":{},\"offspring\":{},",
                "\"parent_dosage\":{{\"A\":{},\"B\":{}}},",
                "\"child_gl\":[{}],",
                "\"recombination\":{{\"A\":{},\"B\":{}}},",
                "\"double_reduction\":{{",
                "\"A\":{{\"allowed\":{},\"rate\":{}}},",
                "\"B\":{{\"allowed\":{},\"rate\":{}}}}}}}"
            ),
            M,
            N,
            json_list(&self.a.dosage),
            json_list(&self.b.dosage),
            gl.join(","),
            json_f64_list(&self.a.recombination),
            json_f64_list(&self.b.recombination),
            json_bool_list(&self.a.dr_allowed),
            json_f64_list(&self.a.dr_rate),
            json_bool_list(&self.b.dr_allowed),
            json_f64_list(&self.b.dr_rate),
        )
    }

    fn parse(&self) -> Cross {
        parse_cross(&self.to_json()).expect("diagnostic cross must satisfy the input format")
    }
}

/// Likelihoods peaked at `truth`, falling off by `slope` per unit of dosage
/// distance. A small deterministic tilt keeps distinct dosages from tying
/// unless a cross wants them to.
fn peaked(truth: u8, slope: f64, tilt: f64) -> [f64; 5] {
    let mut out = [0.0f64; 5];
    for (d, slot) in out.iter_mut().enumerate() {
        let distance = (d as f64 - f64::from(truth)).abs();
        *slot = -slope * distance - tilt * d as f64;
    }
    out
}

fn uniform_rec(value: f64) -> [f64; M - 1] {
    [value; M - 1]
}

fn parent_spec(dosage: [u8; M], recombination: f64, dr_allowed: [bool; M], dr_rate: f64) -> ParentSpec {
    let mut rate = [0.0f64; M];
    for m in 0..M {
        if dr_allowed[m] {
            rate[m] = dr_rate;
        }
    }
    ParentSpec { dosage, recombination: uniform_rec(recombination), dr_allowed, dr_rate: rate }
}

/// Dosage a progeny carries at `m` under planted haplotypes and states.
fn planted_dosage(hap_a: &Haplotypes, hap_b: &Haplotypes, sa: State, sb: State, m: usize) -> u8 {
    hap_a.row(sa.low() as usize)[m]
        + hap_a.row(sa.high() as usize)[m]
        + hap_b.row(sb.low() as usize)[m]
        + hap_b.row(sb.high() as usize)[m]
}

fn haplotypes_from_columns(columns: &[[u8; HOMOLOGS]; M]) -> Haplotypes {
    let mut haps = Haplotypes::zeros(M);
    for (m, column) in columns.iter().enumerate() {
        for h in 0..HOMOLOGS {
            haps.set(h, m, column[h]);
        }
    }
    haps
}

fn column_dosage(columns: &[[u8; HOMOLOGS]; M]) -> [u8; M] {
    let mut out = [0u8; M];
    for (m, column) in columns.iter().enumerate() {
        out[m] = column.iter().sum();
    }
    out
}

fn state(a: u8, b: u8) -> State {
    State::new(a, b).expect("diagnostic states are in range")
}

// -------------------------------------------------------------------------
// Exhaustive optimum
// -------------------------------------------------------------------------

/// Every allele column with exactly `dosage` alternate alleles, ascending.
fn admissible_columns(dosage: u8) -> Vec<[u8; HOMOLOGS]> {
    let mut out = Vec::new();
    for bits in 0u8..16 {
        if bits.count_ones() as u8 == dosage {
            out.push([bits & 1, (bits >> 1) & 1, (bits >> 2) & 1, (bits >> 3) & 1]);
        }
    }
    out
}

/// Best total for one progeny against fixed haplotypes, over all legal paths.
fn best_path_total(cross: &Cross, hap_a: &Haplotypes, hap_b: &Haplotypes, c: usize) -> f64 {
    let joint: Vec<(State, State)> = ALL_STATES
        .iter()
        .flat_map(|a| ALL_STATES.iter().map(move |b| (*a, *b)))
        .collect();
    let legal = |sa: State, sb: State, m: usize| {
        (!sa.is_double_reduction() || cross.params(Parent::A).dr_allowed[m])
            && (!sb.is_double_reduction() || cross.params(Parent::B).dr_allowed[m])
    };
    let node = |sa: State, sb: State, m: usize| {
        let dosage = planted_dosage(hap_a, hap_b, sa, sb, m);
        cross.child_gl[c][m][usize::from(dosage)]
            + double_reduction_term(cross, Parent::A, sa, m)
            + double_reduction_term(cross, Parent::B, sb, m)
    };
    let mut best: Vec<f64> = joint
        .iter()
        .map(|(sa, sb)| if legal(*sa, *sb, 0) { node(*sa, *sb, 0) } else { f64::NEG_INFINITY })
        .collect();
    for m in 1..cross.markers {
        let mut next = vec![f64::NEG_INFINITY; joint.len()];
        for (i, (sa, sb)) in joint.iter().enumerate() {
            if !legal(*sa, *sb, m) {
                continue;
            }
            let here = node(*sa, *sb, m);
            for (j, (pa, pb)) in joint.iter().enumerate() {
                if best[j] == f64::NEG_INFINITY {
                    continue;
                }
                let total = best[j]
                    + transition_term(cross, Parent::A, *pa, *sa, m - 1)
                    + transition_term(cross, Parent::B, *pb, *sb, m - 1)
                    + here;
                if total > next[i] {
                    next[i] = total;
                }
            }
        }
        best = next;
    }
    best.into_iter().fold(f64::NEG_INFINITY, f64::max)
}

fn all_assignments(dosage: &[u8]) -> Vec<Haplotypes> {
    let per_marker: Vec<Vec<[u8; HOMOLOGS]>> = dosage.iter().map(|d| admissible_columns(*d)).collect();
    let mut out = Vec::new();
    let mut current = [[0u8; HOMOLOGS]; M];
    fn walk(
        m: usize,
        per_marker: &[Vec<[u8; HOMOLOGS]>],
        current: &mut [[u8; HOMOLOGS]; M],
        out: &mut Vec<Haplotypes>,
    ) {
        if m == M {
            out.push(haplotypes_from_columns(current));
            return;
        }
        for column in &per_marker[m] {
            current[m] = *column;
            walk(m + 1, per_marker, current, out);
        }
    }
    walk(0, &per_marker, &mut current, &mut out);
    out
}

/// The maximum total over every admissible haplotype assignment and every legal
/// path, found by enumeration. Only usable where the assignments are few.
fn exhaustive_optimum(cross: &Cross) -> f64 {
    let assignments_a = all_assignments(&cross.params(Parent::A).dosage);
    let assignments_b = all_assignments(&cross.params(Parent::B).dosage);
    let mut best = f64::NEG_INFINITY;
    for hap_a in &assignments_a {
        for hap_b in &assignments_b {
            let total: f64 = (0..cross.offspring)
                .map(|c| best_path_total(cross, hap_a, hap_b, c))
                .sum();
            if total > best {
                best = total;
            }
        }
    }
    best
}

// -------------------------------------------------------------------------
// Canonical form and the order among distinct optima
// -------------------------------------------------------------------------

fn permutations() -> Vec<[u8; HOMOLOGS]> {
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

/// The key `spec/result-format-v1.md` minimises when choosing a relabelling
/// for one parent: the reordered rows, then that parent's remapped path
/// signature, then the image tuple.
fn relabel_key(answer: &Reconstruction, parent: Parent, p: &[u8; HOMOLOGS]) -> (Vec<Vec<u8>>, Vec<u8>, [u8; HOMOLOGS]) {
    let haps = answer.haplotypes(parent);
    let mut rows = vec![Vec::new(); HOMOLOGS];
    for h in 0..HOMOLOGS {
        rows[p[h] as usize] = haps.row(h).to_vec();
    }
    let mut signature = Vec::new();
    for path in &answer.paths {
        for s in path.states(parent) {
            let r = s.remap(p);
            signature.push(r.low());
            signature.push(r.high());
        }
    }
    (rows, signature, *p)
}

/// Why the reconstruction is not in canonical form, if it is not.
fn canonical_defect(answer: &Reconstruction) -> Option<String> {
    let identity = [0u8, 1, 2, 3];
    for parent in Parent::BOTH {
        let own = relabel_key(answer, parent, &identity);
        let rows_sorted = own.0.windows(2).all(|w| w[0] <= w[1]);
        if !rows_sorted {
            return Some(format!("parent {} rows are not in ascending order", parent.label()));
        }
        for p in permutations() {
            let other = relabel_key(answer, parent, &p);
            if other < own {
                return Some(format!(
                    "parent {} has a relabelling that sorts earlier than the answer",
                    parent.label()
                ));
            }
        }
    }
    None
}

/// The order among distinct optima: parent A's rows, parent B's rows, then the
/// full path signature with each progeny's parent A states before parent B's.
fn distinct_key(answer: &Reconstruction) -> Vec<u8> {
    let mut key = Vec::new();
    for parent in Parent::BOTH {
        for h in 0..HOMOLOGS {
            key.extend_from_slice(answer.haplotypes(parent).row(h));
        }
    }
    for path in &answer.paths {
        for parent in Parent::BOTH {
            for s in path.states(parent) {
                key.push(s.low());
                key.push(s.high());
            }
        }
    }
    key
}

fn solve_and_score(cross: &Cross) -> (Reconstruction, f64) {
    let answer = solve(cross);
    let total = score(cross, &answer).unwrap_or_else(|e| panic!("the scorer rejects the answer: {e}"));
    (answer, total)
}

// -------------------------------------------------------------------------
// The crosses
// -------------------------------------------------------------------------

const OPEN: [bool; M] = [true; M];
const CLOSED: [bool; M] = [false; M];

/// Parent A has one free marker, parent B has two, and the two free markers
/// of B are far enough apart that a progeny's path between them costs
/// recombination. The planted paths hold homologs across the gap.
fn coupled_cross() -> CrossSpec {
    let mut cols_a = [[1u8; HOMOLOGS]; M];
    cols_a[5] = [0, 0, 1, 1];
    for m in [0, 3, 8, 11] {
        cols_a[m] = [0, 0, 0, 0];
    }
    let mut cols_b = [[0u8; HOMOLOGS]; M];
    cols_b[3] = [0, 1, 0, 1];
    cols_b[9] = [1, 0, 1, 0];
    for m in [1, 6, 7] {
        cols_b[m] = [1, 1, 1, 1];
    }
    let hap_a = haplotypes_from_columns(&cols_a);
    let hap_b = haplotypes_from_columns(&cols_b);
    let paths_a = [state(0, 2), state(1, 3), state(2, 3), state(0, 1), state(0, 3), state(1, 2), state(2, 3), state(0, 2)];
    let paths_b = [state(1, 3), state(0, 2), state(1, 3), state(0, 2), state(1, 2), state(0, 3), state(1, 3), state(0, 2)];
    let child_gl = (0..N)
        .map(|c| {
            (0..M)
                .map(|m| peaked(planted_dosage(&hap_a, &hap_b, paths_a[c], paths_b[c], m), 1.1, 0.01))
                .collect()
        })
        .collect();
    CrossSpec {
        a: parent_spec(column_dosage(&cols_a), 0.08, CLOSED, 0.0),
        b: parent_spec(column_dosage(&cols_b), 0.08, CLOSED, 0.0),
        child_gl,
    }
}

/// Double reduction is legal for parent A at the first half of the markers only
/// and never for parent B. At three consecutive markers where it is legal, one
/// homolog of parent A alone carries the alternate allele, and the planted
/// truth has half the progeny receive that homolog twice, so the likelihoods
/// there favour a dosage only a repeated homolog can reach.
fn double_reduction_cross() -> CrossSpec {
    let mut cols_a = [[0u8; HOMOLOGS]; M];
    for m in [3, 4, 5] {
        cols_a[m] = [0, 0, 1, 0];
    }
    for m in [1, 9] {
        cols_a[m] = [1, 1, 1, 1];
    }
    let mut cols_b = [[0u8; HOMOLOGS]; M];
    cols_b[7] = [1, 1, 0, 0];
    for m in [2, 10] {
        cols_b[m] = [1, 1, 1, 1];
    }
    let hap_a = haplotypes_from_columns(&cols_a);
    let hap_b = haplotypes_from_columns(&cols_b);
    let mut allowed_a = CLOSED;
    for slot in allowed_a.iter_mut().take(6) {
        *slot = true;
    }
    let mut paths_a = Vec::with_capacity(N);
    for c in 0..N {
        let mut a = Vec::with_capacity(M);
        for m in 0..M {
            a.push(match c % 4 {
                0 | 2 => if m < 6 { state(2, 2) } else { state(1, 2) },
                1 => state(0, 2),
                _ => state(1, 3),
            });
        }
        paths_a.push(a);
    }
    let paths_b = [state(0, 1), state(0, 2), state(1, 3), state(0, 1), state(2, 3), state(0, 1), state(1, 2), state(0, 3)];
    let child_gl = (0..N)
        .map(|c| {
            (0..M)
                .map(|m| peaked(planted_dosage(&hap_a, &hap_b, paths_a[c][m], paths_b[c], m), 2.0, 0.01))
                .collect()
        })
        .collect();
    CrossSpec {
        a: parent_spec(column_dosage(&cols_a), 0.3, allowed_a, 0.45),
        b: parent_spec(column_dosage(&cols_b), 0.2, CLOSED, 0.0),
        child_gl,
    }
}

/// The planted paths carry a repeated homolog through the first markers and
/// then keep one copy of it while the other changes, so the cheapest legal
/// path through the true dosages crosses exactly one interval with a single
/// changed homolog out of a repeated pair.
fn repeated_homolog_cross() -> CrossSpec {
    let mut cols_a = [[0u8; HOMOLOGS]; M];
    cols_a[2] = [0, 1, 0, 0];
    cols_a[8] = [0, 1, 1, 0];
    for m in [5, 11] {
        cols_a[m] = [1, 1, 1, 1];
    }
    let mut cols_b = [[1u8; HOMOLOGS]; M];
    cols_b[6] = [1, 0, 0, 1];
    for m in [0, 3, 9] {
        cols_b[m] = [0, 0, 0, 0];
    }
    let hap_a = haplotypes_from_columns(&cols_a);
    let hap_b = haplotypes_from_columns(&cols_b);
    let mut paths_a = Vec::with_capacity(N);
    let mut paths_b = Vec::with_capacity(N);
    for c in 0..N {
        let mut a = Vec::with_capacity(M);
        let mut b = Vec::with_capacity(M);
        for m in 0..M {
            let before = m < 6;
            a.push(match c % 4 {
                0 => if before { state(1, 1) } else { state(1, 2) },
                1 => if before { state(1, 1) } else { state(1, 3) },
                2 => state(0, 1),
                _ => state(1, 2),
            });
            b.push(match c % 3 {
                0 => state(0, 3),
                1 => state(1, 2),
                _ => state(0, 2),
            });
        }
        paths_a.push(a);
        paths_b.push(b);
    }
    let child_gl = (0..N)
        .map(|c| {
            (0..M)
                .map(|m| peaked(planted_dosage(&hap_a, &hap_b, paths_a[c][m], paths_b[c][m], m), 1.2, 0.01))
                .collect()
        })
        .collect();
    CrossSpec {
        a: parent_spec(column_dosage(&cols_a), 0.12, OPEN, 0.3),
        b: parent_spec(column_dosage(&cols_b), 0.12, CLOSED, 0.0),
        child_gl,
    }
}

/// Parent A's free column gives two homologs identical alleles at every marker,
/// so relabelling them leaves the haplotypes untouched and only the paths can
/// decide the canonical form. Likelihoods are flat between two dosages for some
/// progeny, so equally scoring answers exist.
fn symmetric_cross() -> CrossSpec {
    let mut cols_a = [[0u8; HOMOLOGS]; M];
    cols_a[6] = [1, 1, 0, 0];
    for m in [1, 4, 10] {
        cols_a[m] = [1, 1, 1, 1];
    }
    let mut cols_b = [[1u8; HOMOLOGS]; M];
    cols_b[3] = [0, 1, 1, 0];
    for m in [0, 8] {
        cols_b[m] = [0, 0, 0, 0];
    }
    let hap_a = haplotypes_from_columns(&cols_a);
    let hap_b = haplotypes_from_columns(&cols_b);
    let paths_a = [state(0, 2), state(1, 3), state(0, 1), state(2, 3), state(0, 3), state(1, 2), state(0, 2), state(1, 3)];
    let paths_b = [state(0, 1), state(2, 3), state(1, 2), state(0, 3), state(0, 2), state(1, 3), state(0, 1), state(2, 3)];
    let child_gl = (0..N)
        .map(|c| {
            (0..M)
                .map(|m| {
                    let truth = planted_dosage(&hap_a, &hap_b, paths_a[c], paths_b[c], m);
                    let mut gl = peaked(truth, 0.9, 0.0);
                    // Every other progeny cannot tell the true dosage from the
                    // one above it at the free markers.
                    if c % 2 == 1 && (m == 6 || m == 3) && truth < 4 {
                        gl[usize::from(truth) + 1] = gl[usize::from(truth)];
                    }
                    gl
                })
                .collect()
        })
        .collect();
    CrossSpec {
        a: parent_spec(column_dosage(&cols_a), 0.1, CLOSED, 0.0),
        b: parent_spec(column_dosage(&cols_b), 0.1, CLOSED, 0.0),
        child_gl,
    }
}


/// Equally likely dosages at many markers, so several inheritance paths reach
/// the optimum and the choice among them is coupled across markers: taking the
/// smaller state at one marker can force a larger one later. Which of those
/// paths is required is fixed by the result format, and it has to be settled
/// across the whole sequence rather than one marker at a time.
fn coupled_ties_cross() -> CrossSpec {
    let child_gl: Vec<Vec<[f64; 5]>> = vec![
            vec![[-1.0, -0.0, -1.0, -2.0, -3.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-2.0, -0.0, -0.0, -0.0, -2.0], [-3.0, -2.0, -1.0, -0.0, -0.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-0.0, -1.0, -2.0, -3.0, -4.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-4.0, -3.0, -2.0, -0.0, -0.0], [-3.0, -2.0, -1.0, -0.0, -0.0], [-3.0, -2.0, -1.0, -0.0, -0.0], [-0.0, -1.0, -2.0, -3.0, -4.0]],
            vec![[-3.0, -2.0, -1.0, -0.0, -1.0], [-0.0, -1.0, -2.0, -3.0, -4.0], [-3.0, -2.0, -0.0, -0.0, -1.0], [-2.0, -1.0, -0.0, -1.0, -2.0], [-1.0, -0.0, -0.0, -2.0, -3.0], [-2.0, -1.0, -0.0, -1.0, -2.0], [-2.0, -1.0, -0.0, -1.0, -2.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-3.0, -2.0, -0.0, -0.0, -1.0], [-3.0, -2.0, -1.0, -0.0, -1.0], [-3.0, -2.0, -1.0, -0.0, -1.0], [-3.0, -2.0, -1.0, -0.0, -0.0]],
            vec![[-4.0, -3.0, -2.0, -1.0, -0.0], [-2.0, -1.0, -0.0, -1.0, -2.0], [-0.0, -0.0, -1.0, -2.0, -3.0], [-0.0, -1.0, -2.0, -3.0, -4.0], [-1.0, -0.0, -0.0, -2.0, -3.0], [-0.0, -1.0, -2.0, -3.0, -4.0], [-3.0, -2.0, -0.0, -0.0, -1.0], [-0.0, -0.0, -0.0, -2.0, -3.0], [-0.0, -1.0, -2.0, -3.0, -4.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-0.0, -0.0, -2.0, -3.0, -4.0], [-0.0, -0.0, -1.0, -2.0, -3.0]],
            vec![[-0.0, -1.0, -2.0, -3.0, -4.0], [-3.0, -2.0, -1.0, -0.0, -1.0], [-0.0, -1.0, -2.0, -3.0, -4.0], [-0.0, -0.0, -2.0, -3.0, -4.0], [-2.0, -1.0, -0.0, -1.0, -2.0], [-3.0, -2.0, -0.0, -0.0, -1.0], [-0.0, -0.0, -1.0, -2.0, -3.0], [-3.0, -2.0, -1.0, -0.0, -1.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-0.0, -1.0, -2.0, -3.0, -4.0], [-2.0, -1.0, -0.0, -0.0, -2.0], [-0.0, -1.0, -2.0, -3.0, -4.0]],
            vec![[-2.0, -1.0, -0.0, -1.0, -2.0], [-2.0, -0.0, -0.0, -0.0, -2.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-0.0, -1.0, -2.0, -3.0, -4.0], [-0.0, -0.0, -2.0, -3.0, -4.0], [-1.0, -0.0, -1.0, -2.0, -3.0], [-0.0, -0.0, -2.0, -3.0, -4.0], [-3.0, -2.0, -1.0, -0.0, -1.0], [-3.0, -2.0, -0.0, -0.0, -1.0], [-2.0, -0.0, -0.0, -1.0, -2.0], [-4.0, -3.0, -2.0, -0.0, -0.0], [-1.0, -0.0, -1.0, -2.0, -3.0]],
            vec![[-1.0, -0.0, -1.0, -2.0, -3.0], [-0.0, -0.0, -1.0, -2.0, -3.0], [-2.0, -1.0, -0.0, -1.0, -2.0], [-0.0, -1.0, -2.0, -3.0, -4.0], [-1.0, -0.0, -1.0, -2.0, -3.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-1.0, -0.0, -1.0, -2.0, -3.0], [-2.0, -0.0, -0.0, -0.0, -2.0], [-1.0, -0.0, -0.0, -2.0, -3.0], [-3.0, -2.0, -0.0, -0.0, -0.0], [-3.0, -2.0, -0.0, -0.0, -1.0]],
            vec![[-4.0, -3.0, -2.0, -1.0, -0.0], [-0.0, -0.0, -2.0, -3.0, -4.0], [-3.0, -2.0, -1.0, -0.0, -0.0], [-2.0, -0.0, -0.0, -0.0, -2.0], [-4.0, -3.0, -2.0, -0.0, -0.0], [-3.0, -2.0, -0.0, -0.0, -0.0], [-1.0, -0.0, -0.0, -2.0, -3.0], [-3.0, -2.0, -0.0, -0.0, -1.0], [-3.0, -2.0, -1.0, -0.0, -0.0], [-4.0, -3.0, -2.0, -0.0, -0.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-2.0, -0.0, -0.0, -1.0, -2.0]],
            vec![[-4.0, -3.0, -2.0, -1.0, -0.0], [-0.0, -0.0, -2.0, -3.0, -4.0], [-2.0, -1.0, -0.0, -1.0, -2.0], [-3.0, -2.0, -1.0, -0.0, -1.0], [-2.0, -0.0, -0.0, -0.0, -2.0], [-0.0, -1.0, -2.0, -3.0, -4.0], [-2.0, -1.0, -0.0, -0.0, -2.0], [-2.0, -0.0, -0.0, -0.0, -2.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-3.0, -2.0, -1.0, -0.0, -1.0], [-4.0, -3.0, -2.0, -1.0, -0.0], [-2.0, -0.0, -0.0, -0.0, -2.0]],
    ];
    CrossSpec {
        a: parent_spec([0, 2, 0, 4, 4, 4, 0, 4, 4, 1, 4, 0], 0.1, [false, false, false, true, true, true, true, true, true, false, false, false], 0.3),
        b: parent_spec([0, 4, 0, 4, 1, 0, 4, 2, 4, 4, 0, 0], 0.1, [false, false, false, false, false, false, false, false, false, false, false, false], 0.0),
        child_gl,
    }
}

fn every_cross() -> Vec<(&'static str, CrossSpec)> {
    vec![
        ("coupled", coupled_cross()),
        ("double_reduction", double_reduction_cross()),
        ("repeated_homolog", repeated_homolog_cross()),
        ("symmetric", symmetric_cross()),
        ("coupled_ties", coupled_ties_cross()),
    ]
}

// -------------------------------------------------------------------------
// Checks
// -------------------------------------------------------------------------

#[test]
fn diagnostic_crosses_are_valid_inputs() {
    for (name, spec) in every_cross() {
        let cross = spec.parse();
        assert_eq!(cross.markers, M, "{name}");
        assert_eq!(cross.offspring, N, "{name}");
    }
}

#[test]
fn search_reaches_the_enumerated_optimum() {
    for (name, spec) in every_cross() {
        let cross = spec.parse();
        let (_, found) = solve_and_score(&cross);
        let optimum = exhaustive_optimum(&cross);
        assert!(
            scores_equal(found, optimum),
            "{name}: the search reports {found:.10} where enumeration finds {optimum:.10}"
        );
    }
}

#[test]
fn double_reduction_appears_only_where_allowed() {
    let cross = double_reduction_cross().parse();
    let (answer, _) = solve_and_score(&cross);
    for (c, path) in answer.paths.iter().enumerate() {
        for parent in Parent::BOTH {
            let allowed = &cross.params(parent).dr_allowed;
            for (m, s) in path.states(parent).iter().enumerate() {
                assert!(
                    !s.is_double_reduction() || allowed[m],
                    "progeny {c} parent {} double reduces at marker {m}, where it is forbidden",
                    parent.label()
                );
            }
        }
    }
    let uses_dr = answer
        .paths
        .iter()
        .any(|p| p.states(Parent::A).iter().any(|s| s.is_double_reduction()));
    assert!(uses_dr, "no progeny double reduces in parent A, where the evidence calls for it");
}

#[test]
fn answers_are_in_canonical_form() {
    for (name, spec) in every_cross() {
        let cross = spec.parse();
        let (answer, _) = solve_and_score(&cross);
        if let Some(defect) = canonical_defect(&answer) {
            panic!("{name}: {defect}");
        }
    }
}

#[test]
fn equally_scoring_answers_resolve_to_the_documented_representative() {
    for (name, spec) in [("symmetric", symmetric_cross()), ("coupled_ties", coupled_ties_cross())] {
    let cross = spec.parse();
    let (answer, total) = solve_and_score(&cross);
    // A tie among optima is only meaningful once the answer is an optimum: a
    // suboptimal answer rarely has an equally scoring neighbour, and would pass
    // the scan below without saying anything.
    let optimum = exhaustive_optimum(&cross);
    assert!(
        scores_equal(total, optimum),
        "{name}: the search reports {total:.10} where enumeration finds {optimum:.10}"
    );
    let own_key = distinct_key(&answer);
    // Pairs of adjacent markers, so a tie that is coupled across an interval is
    // seen as well as one confined to a single marker.
    for c in 0..cross.offspring {
        for parent in Parent::BOTH {
            for m in 0..cross.markers - 1 {
                for first in ALL_STATES {
                    for second in ALL_STATES {
                        let mut variant = answer.clone();
                        let states = match parent {
                            Parent::A => &mut variant.paths[c].from_a,
                            Parent::B => &mut variant.paths[c].from_b,
                        };
                        if states[m] == first && states[m + 1] == second {
                            continue;
                        }
                        states[m] = first;
                        states[m + 1] = second;
                        let Ok(other) = score(&cross, &variant) else { continue };
                        if !scores_equal(other, total) || canonical_defect(&variant).is_some() {
                            continue;
                        }
                        assert!(
                            distinct_key(&variant) >= own_key,
                            "{name}: progeny {c} parent {} markers {m}-{}: an equally scoring canonical answer sorts before the one reported",
                            parent.label(), m + 1
                        );
                    }
                }
            }
        }
    }
    for c in 0..cross.offspring {
        for parent in Parent::BOTH {
            for m in 0..cross.markers {
                for alternative in ALL_STATES {
                    let mut variant = answer.clone();
                    let states = match parent {
                        Parent::A => &mut variant.paths[c].from_a,
                        Parent::B => &mut variant.paths[c].from_b,
                    };
                    if states[m] == alternative {
                        continue;
                    }
                    states[m] = alternative;
                    let Ok(other) = score(&cross, &variant) else { continue };
                    if !scores_equal(other, total) || canonical_defect(&variant).is_some() {
                        continue;
                    }
                    assert!(
                        distinct_key(&variant) >= own_key,
                        "{name}: progeny {c} parent {} marker {m}: an equally scoring canonical answer sorts before the one reported",
                        parent.label()
                    );
                }
            }
        }
    }
    }
}

#[test]
fn repeated_runs_agree_and_the_document_is_self_consistent() {
    for (name, spec) in every_cross() {
        let cross = spec.parse();
        let (first, total_first) = solve_and_score(&cross);
        let (second, total_second) = solve_and_score(&cross);
        assert_eq!(first, second, "{name}: two runs returned different answers");
        assert!(scores_equal(total_first, total_second), "{name}: two runs returned different totals");

        let text = encode(&first, total_first);
        let decoded = decode(&text, cross.markers, cross.offspring)
            .unwrap_or_else(|e| panic!("{name}: the written document does not decode: {e}"));
        assert_eq!(decoded.answer, first, "{name}: the document does not round-trip");
        assert!(
            scores_equal(decoded.total_log_likelihood, total_first),
            "{name}: the document's total differs from the scorer's"
        );
        assert_eq!(
            decoded.canonical_digest,
            canonical_digest(&first),
            "{name}: the document's digest is not the digest of its reconstruction"
        );
    }
}
