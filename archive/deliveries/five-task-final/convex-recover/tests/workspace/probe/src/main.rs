//! Verifier-owned probe.
//!
//! Links exactly one allowlisted candidate file (`/app/src/lib.rs`) as the
//! `candidate` module and records the raw functional result of `recover` for one
//! verifier-named case.
//!
//! The probe deliberately has no grading role. It asserts nothing, compares
//! nothing, prints no verdict and emits no marker, and its exit status is logged
//! but never consulted. Grading happens afterwards in the separate `judge`
//! binary, which never links candidate code. Anything this process writes to
//! stdout or stderr is diagnostics only.
//!
//! Note the translation across the trust boundary: the case input is built from
//! the verifier's own frozen `convex_model` types and converted into the
//! candidate's declared types on the way in, and the candidate's returned value
//! is converted straight back into the verifier's raw record shape on the way
//! out. The candidate never supplies a type, a comparison or a conclusion.
#[allow(dead_code)]
mod candidate;

use convex_model::{cases, record, Rat, RawFace, RawPlane, RawResult, RawVertex};

/// Verifier input -> candidate input. Pure field translation.
fn to_candidate(i: &convex_model::Input) -> candidate::Input {
    candidate::Input {
        families: i
            .families
            .iter()
            .map(|f| candidate::Family {
                normal: f.normal,
                offsets: f
                    .offsets
                    .iter()
                    .map(|o| candidate::Rat { n: o.n, d: o.d })
                    .collect(),
            })
            .collect(),
        supports: i
            .supports
            .iter()
            .map(|s| candidate::Support {
                direction: s.direction,
                value: candidate::Rat {
                    n: s.value.n,
                    d: s.value.d,
                },
            })
            .collect(),
    }
}

fn rat(q: candidate::Rat) -> Rat {
    Rat { n: q.n, d: q.d }
}

/// Candidate output -> raw record. Pure field translation; nothing is judged,
/// normalised, repaired or rejected here.
fn to_raw(r: &candidate::Result) -> RawResult {
    RawResult {
        planes: r
            .planes
            .iter()
            .map(|p| RawPlane {
                family: p.family as u64,
                normal: p.normal,
                offset: rat(p.offset),
            })
            .collect(),
        vertices: r
            .vertices
            .iter()
            .map(|v| RawVertex {
                xyz: [rat(v.xyz[0]), rat(v.xyz[1]), rat(v.xyz[2])],
            })
            .collect(),
        faces: r
            .faces
            .iter()
            .map(|f| RawFace {
                plane: f.plane as u64,
                vertices: f.vertices.iter().map(|&i| i as u64).collect(),
            })
            .collect(),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 3 {
        std::process::exit(2)
    }
    let Some(input) = cases::case(&a[1]) else {
        std::process::exit(2)
    };
    let observed = candidate::recover(&to_candidate(&input));
    let raw = observed.as_ref().map(to_raw);
    if std::fs::write(&a[2], record::encode(raw.as_ref())).is_err() {
        std::process::exit(2)
    }
}
