//! Regression probe for the stdout-authority property.
//!
//! The same harness as `probe`, but linked against a verifier-owned,
//! deliberately incorrect implementation that also floods stdout and stderr with
//! success-looking text. The judge must still fail it, which demonstrates that
//! emitted output carries no grading authority.
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
