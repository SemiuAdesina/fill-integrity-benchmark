//! Verifier-owned expected values.
//!
//! This module never links candidate code and is never installed into the
//! sandbox the graded process can read. It holds two separate things:
//!
//! * [`grade`] — the asserted properties, transcribed one-for-one from the V1
//!   graded harness. These, and only these, decide correctness.
//! * [`canonical`] — a known-correct record set used *solely* to stage the
//!   negative control. It plays no part in grading a candidate.
use convex_model::{Rat, RawFace, RawPlane, RawResult, RawVertex};

fn r(n: i64) -> Rat {
    Rat { n, d: 1 }
}

/// The canonical correct reconstruction of the frozen cube evidence set:
/// the six axis halfspaces at offset one, eight lexicographically sorted
/// integer vertices, and six faces of four sorted vertex indices each.
///
/// Used only by the negative control to stage correct records.
pub fn canonical_cube() -> RawResult {
    let normals = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ];
    let coords = [
        [-1, -1, -1],
        [-1, -1, 1],
        [-1, 1, -1],
        [-1, 1, 1],
        [1, -1, -1],
        [1, -1, 1],
        [1, 1, -1],
        [1, 1, 1],
    ];
    let incidence: [[u64; 4]; 6] = [
        [4, 5, 6, 7],
        [0, 1, 2, 3],
        [2, 3, 6, 7],
        [0, 1, 4, 5],
        [1, 3, 5, 7],
        [0, 2, 4, 6],
    ];
    RawResult {
        planes: normals
            .iter()
            .enumerate()
            .map(|(family, &normal)| RawPlane {
                family: family as u64,
                normal,
                offset: r(1),
            })
            .collect(),
        vertices: coords
            .iter()
            .map(|c| RawVertex {
                xyz: [r(c[0]), r(c[1]), r(c[2])],
            })
            .collect(),
        faces: incidence
            .iter()
            .enumerate()
            .map(|(plane, ids)| RawFace {
                plane: plane as u64,
                vertices: ids.to_vec(),
            })
            .collect(),
    }
}

/// The canonical correct reconstruction of the compound-tie evidence set: the
/// six axis halfspaces at offset one plus the corner cut `x+y+z <= 2`, ten
/// lexicographically sorted integer vertices, and seven faces.
///
/// The rival selection that swaps the cut for family seven reaches the same
/// facet count, so `(6, 2)` beating `(7, 1)` in `(family, offset)` order is
/// what makes this the answer.
pub fn canonical_cut_cube() -> RawResult {
    let planes: [([i64; 3], u64, i64); 7] = [
        ([1, 0, 0], 0, 1),
        ([-1, 0, 0], 1, 1),
        ([0, 1, 0], 2, 1),
        ([0, -1, 0], 3, 1),
        ([0, 0, 1], 4, 1),
        ([0, 0, -1], 5, 1),
        ([1, 1, 1], 6, 2),
    ];
    let coords: [[i64; 3]; 10] = [
        [-1, -1, -1],
        [-1, -1, 1],
        [-1, 1, -1],
        [-1, 1, 1],
        [0, 1, 1],
        [1, -1, -1],
        [1, -1, 1],
        [1, 0, 1],
        [1, 1, -1],
        [1, 1, 0],
    ];
    let incidence: [&[u64]; 7] = [
        &[5, 6, 7, 8, 9],
        &[0, 1, 2, 3],
        &[2, 3, 4, 8, 9],
        &[0, 1, 5, 6],
        &[1, 3, 4, 6, 7],
        &[0, 2, 5, 8],
        &[4, 7, 9],
    ];
    RawResult {
        planes: planes
            .iter()
            .map(|&(normal, family, offset)| RawPlane {
                family,
                normal,
                offset: r(offset),
            })
            .collect(),
        vertices: coords
            .iter()
            .map(|c| RawVertex {
                xyz: [r(c[0]), r(c[1]), r(c[2])],
            })
            .collect(),
        faces: incidence
            .iter()
            .enumerate()
            .map(|(plane, ids)| RawFace {
                plane: plane as u64,
                vertices: ids.to_vec(),
            })
            .collect(),
    }
}

/// The canonical correct reconstruction of the half-step evidence set: the four
/// side halfspaces, the doubled upper normal and the lower bound, all at offset
/// one; eight lexicographically sorted vertices whose height is a true fraction;
/// and six faces of four sorted vertex indices each.
///
/// The upper vertices are what this fixture exists for. Each is the intersection
/// of two side planes with the doubled upper plane, so its determinant is two:
/// the two side coordinates arrive as an unreduced pair whose canonical form is a
/// whole number, and the height arrives as a fraction that stays one.
pub fn canonical_half_step() -> RawResult {
    let half = Rat { n: 1, d: 2 };
    let normals = [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 2],
        [0, 0, -1],
    ];
    let coords = [
        [r(-1), r(-1), r(-1)],
        [r(-1), r(-1), half],
        [r(-1), r(1), r(-1)],
        [r(-1), r(1), half],
        [r(1), r(-1), r(-1)],
        [r(1), r(-1), half],
        [r(1), r(1), r(-1)],
        [r(1), r(1), half],
    ];
    let incidence: [[u64; 4]; 6] = [
        [4, 5, 6, 7],
        [0, 1, 2, 3],
        [2, 3, 6, 7],
        [0, 1, 4, 5],
        [1, 3, 5, 7],
        [0, 2, 4, 6],
    ];
    RawResult {
        planes: normals
            .iter()
            .enumerate()
            .map(|(family, &normal)| RawPlane {
                family: family as u64,
                normal,
                offset: r(1),
            })
            .collect(),
        vertices: coords.iter().map(|&xyz| RawVertex { xyz }).collect(),
        faces: incidence
            .iter()
            .enumerate()
            .map(|(plane, ids)| RawFace {
                plane: plane as u64,
                vertices: ids.to_vec(),
            })
            .collect(),
    }
}

/// The correct record content for one case, for negative-control staging only.
pub fn canonical(id: &str) -> Option<RawResult> {
    match id {
        "boundedness" => None,
        "compound_tie" => Some(canonical_cut_cube()),
        "rational_offset_forces_reduced_coordinates" => Some(canonical_half_step()),
        _ => Some(canonical_cube()),
    }
}

/// Grade one case from the raw values recorded for it.
///
/// Each arm is the V1 assertion for that case, unchanged in what it demands:
/// only the place the decision is made has moved. `Ok(())` means the observed
/// values satisfy the property; `Err` names the property that did not hold.
pub fn grade(id: &str, got: Option<&RawResult>) -> Result<(), String> {
    match id {
        // V1: assert_eq!(x.planes.len(),6); assert!(x.planes.iter().all(|p|p.family<6));
        // Public requirement: the winner is the minimum nonredundant facet set.
        // Semantic behavior: a looser evidence-compatible support plane is excluded.
        // Rejects: keeping every plane the visible supports allow.
        "minimum_facets" => {
            let x = some(id, got)?;
            if x.planes.len() != 6 {
                return Err(format!("plane count {} != 6", x.planes.len()));
            }
            if !x.planes.iter().all(|p| p.family < 6) {
                return Err("a plane outside families 0..5 was selected".into());
            }
            Ok(())
        }
        // V1: assert_eq!(x.vertices.len(),8);
        //     assert_eq!(x.vertices[0].xyz,[r(-1),r(-1),r(-1)]);
        //     assert_eq!(x.vertices[7].xyz,[r(1),r(1),r(1)]);
        // Public requirement: triple intersections use reduced exact rationals.
        // Semantic behavior: the cube has eight unique canonical vertices.
        // Rejects: epsilon or floating-point intersection, and unreduced or
        // duplicated coordinates from enumerating triples without deduplication.
        "exact_vertices" => {
            let x = some(id, got)?;
            if x.vertices.len() != 8 {
                return Err(format!("vertex count {} != 8", x.vertices.len()));
            }
            if x.vertices[0].xyz != [r(-1), r(-1), r(-1)] {
                return Err("first canonical vertex is not (-1,-1,-1)".into());
            }
            if x.vertices[7].xyz != [r(1), r(1), r(1)] {
                return Err("last canonical vertex is not (1,1,1)".into());
            }
            Ok(())
        }
        // V1: assert_eq!(x.faces.len(),6);
        //     for (k,f): assert_eq!(f.plane,k); assert_eq!(f.vertices.len(),4);
        //                assert!(f.vertices.windows(2).all(|w|w[0]<w[1]));
        // Public requirement: face incidence is canonical and complete.
        // Semantic behavior: six sorted faces each contain four sorted vertex indices.
        // Rejects: incidence computed with a strict inequality, which drops the
        // on-plane vertices, and faces emitted in discovery order.
        "face_incidence" => {
            let x = some(id, got)?;
            if x.faces.len() != 6 {
                return Err(format!("face count {} != 6", x.faces.len()));
            }
            for (k, f) in x.faces.iter().enumerate() {
                if f.plane != k as u64 {
                    return Err(format!("face {k} keyed to plane {}", f.plane));
                }
                if f.vertices.len() != 4 {
                    return Err(format!("face {k} has {} vertices", f.vertices.len()));
                }
                if !f.vertices.windows(2).all(|w| w[0] < w[1]) {
                    return Err(format!("face {k} vertex indices are not strictly sorted"));
                }
            }
            Ok(())
        }
        // V1: let best = x.vertices.iter().map(|v|v.xyz.iter().map(|q|q.n/q.d).sum::<i64>()).max().unwrap();
        //     assert_eq!(best,3);
        // Public requirement: support equality is inclusive and exact.
        // Semantic behavior: withheld diagonal support is derived from returned vertices.
        // Rejects: fitting the supplied supports rather than reconstructing the
        // body. A solver that treats support as a loose inequality, or that stops
        // once the visible evidence is satisfied, lands on a body whose unseen
        // diagonal extent is wrong.
        "withheld_support" => {
            let x = some(id, got)?;
            let best = x
                .vertices
                .iter()
                .map(|v| {
                    v.xyz
                        .iter()
                        .map(|q| if q.d == 0 { 0 } else { q.n / q.d })
                        .sum::<i64>()
                })
                .max()
                .ok_or_else(|| "no vertices from which to derive support".to_string())?;
            if best != 3 {
                return Err(format!("withheld diagonal support {best} != 3"));
            }
            Ok(())
        }
        // V1: assert!(recover(&i).is_none());
        // Public requirement: an incomplete halfspace selection is rejected as unbounded.
        // Semantic behavior: support evidence cannot make an open slab legal.
        // Rejects: omitting recession detection entirely. This fixture is chosen
        // so no other rule can substitute for it -- every plane owns a genuine
        // positive-area facet and the triple intersections are numerous and
        // distinct -- so a solver without the boundedness test answers with a
        // reconstruction where the contract requires nothing.
        "boundedness" => match got {
            None => Ok(()),
            Some(_) => Err("an unbounded selection was accepted".into()),
        },
        // V1: assert_eq!((x.planes.len(),x.vertices.len(),x.faces.len()),(6,8,6));
        //     assert!(x.vertices.iter().all(|v|v.xyz.iter().all(|q|q.d==1)));
        // Public requirement: equality, coincident triples, redundancy and canonical ordering compose.
        // Semantic behavior: many triples deduplicate to eight vertices while the seventh plane is absent.
        // Rejects: a solver correct on any one rule but not on their conjunction
        // -- enumerating triples without deduplication inflates the vertex count,
        // keeping the support-compatible looser plane inflates the plane count,
        // and epsilon comparison leaves coordinates that are not reduced integers.
        "compound" => {
            let x = some(id, got)?;
            let shape = (x.planes.len(), x.vertices.len(), x.faces.len());
            if shape != (6, 8, 6) {
                return Err(format!("shape {shape:?} != (6, 8, 6)"));
            }
            if !x
                .vertices
                .iter()
                .all(|v| v.xyz.iter().all(|q| q.d == 1))
            {
                return Err("a vertex coordinate is not a reduced integer".into());
            }
            Ok(())
        }
        // Public requirement: minimum facet count, then lexicographic
        // (family, offset), over selections that are bounded, full-dimensional
        // and all-facet-owning, with supports met exactly.
        // Semantic behavior: four rules have to interact before one answer
        // survives, and this arm compares the whole reconstruction rather than
        // its shape, so a solution that gets the selection right but the
        // geometry wrong (or the reverse) still fails.
        // Rejects: returning the first legal selection; treating support as an
        // inequality instead of attained equality; dropping both the recession
        // test and the positive-area facet test, since the cheaper
        // support-consistent selection here is caught by either one alone; and
        // breaking the minimum-count tie by anything other than lexicographic
        // order.
        "compound_tie" => {
            let x = some(id, got)?;
            let want = canonical_cut_cube();
            if x.planes != want.planes {
                return Err(format!(
                    "selected planes {:?} are not the required minimum lexicographic set",
                    x.planes
                        .iter()
                        .map(|p| (p.family, p.offset.n, p.offset.d))
                        .collect::<Vec<_>>()
                ));
            }
            if x.vertices != want.vertices {
                return Err("vertex set is not the exact reduced sorted reconstruction".into());
            }
            if x.faces != want.faces {
                return Err("face incidence is not canonical and complete".into());
            }
            Ok(())
        }
        // Public requirement: offsets, vertices and supports are exact reduced
        // rationals with positive denominators.
        // Semantic behavior: the upper halfspace carries a normal of two, so the
        // body is bounded at a height no integer expresses, every upper vertex
        // divides by a determinant of two, and the withheld-direction support is
        // itself a fraction. The whole reconstruction is compared.
        // Rejects: integer-only arithmetic, which cannot represent this body;
        // dividing the intersection out without reducing, which returns a
        // coordinate as an unreduced pair whose canonical form is a whole number;
        // and leaving the determinant sign in the denominator.
        "rational_offset_forces_reduced_coordinates" => {
            let x = some(id, got)?;
            let want = canonical_half_step();
            if x.planes != want.planes {
                return Err("selected planes are not the six supplied families at their offsets"
                    .into());
            }
            if x.vertices != want.vertices {
                return Err("vertex set is not the exact reduced sorted reconstruction".into());
            }
            if x.faces != want.faces {
                return Err("face incidence is not canonical and complete".into());
            }
            Ok(())
        }
        _ => Err("unknown case".into()),
    }
}

/// V1 reached these cases through `recover(..).unwrap()`, which panicked on
/// `None`; the equivalent here is an explicit failure.
fn some<'a>(_id: &str, got: Option<&'a RawResult>) -> Result<&'a RawResult, String> {
    got.ok_or_else(|| "no reconstruction returned".to_string())
}
