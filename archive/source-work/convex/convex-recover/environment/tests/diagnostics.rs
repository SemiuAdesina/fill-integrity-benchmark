use convex_recover::*;

fn q(n: i64, d: i64) -> Rat { Rat { n, d } }
fn family(normal: [i64; 3], offsets: &[(i64, i64)]) -> Family {
    Family { normal, offsets: offsets.iter().map(|&(n, d)| q(n, d)).collect() }
}
fn cube() -> Vec<Family> {
    vec![
        family([1, 0, 0], &[(1, 1)]), family([-1, 0, 0], &[(1, 1)]),
        family([0, 1, 0], &[(1, 1)]), family([0, -1, 0], &[(1, 1)]),
        family([0, 0, 1], &[(1, 1)]), family([0, 0, -1], &[(1, 1)]),
    ]
}

#[test]
fn bounded_box_reconstructs_exact_vertices() {
    let result = recover(&Input { families: cube(), supports: vec![] })
        .expect("FAILED: six opposing axis planes bound a full-dimensional body");
    assert_eq!(result.vertices.len(), 8,
        "FAILED: triple intersections must be retained, reduced and deduplicated");
    assert!(result.vertices.iter().all(|v| v.xyz.iter().all(|r| r.d == 1)),
        "FAILED: integral cube corners must be emitted as reduced rationals");
}

#[test]
fn every_selected_plane_owns_a_canonical_face() {
    let result = recover(&Input { families: cube(), supports: vec![] })
        .expect("FAILED: the synthetic cube is recoverable");
    assert_eq!(result.faces.len(), 6, "FAILED: every selected nonredundant plane owns one face");
    assert!(result.faces.iter().all(|f| f.vertices.len() == 4 && f.vertices.windows(2).all(|w| w[0] < w[1])),
        "FAILED: cube faces contain four sorted exact on-plane vertex indices");
}

#[test]
fn an_open_prism_is_rejected_on_boundedness() {
    let mut families = cube();
    families.truncate(4);
    assert!(recover(&Input { families, supports: vec![] }).is_none(),
        "FAILED: a body with an unbounded z recession direction is not legal");
}

#[test]
fn support_is_an_exact_attained_maximum() {
    let mut families = cube();
    families[0] = family([1, 0, 0], &[(2, 1), (1, 2)]);
    let input = Input { families, supports: vec![Support { direction: [1, 0, 0], value: q(1, 2) }] };
    let result = recover(&input).expect("FAILED: the supplied half-unit support has an exact reconstruction");
    assert_eq!(result.planes.iter().find(|p| p.family == 0).map(|p| p.offset), Some(q(1, 2)),
        "FAILED: support equality must be attained exactly, not treated as a loose inequality");
}

#[test]
fn a_redundant_loose_plane_is_not_a_facet() {
    let mut families = cube();
    families.push(family([1, 1, 0], &[(3, 1)]));
    let supports = vec![
        Support { direction: [1, 0, 0], value: q(1, 1) },
        Support { direction: [-1, 0, 0], value: q(1, 1) },
        Support { direction: [0, 1, 0], value: q(1, 1) },
        Support { direction: [0, -1, 0], value: q(1, 1) },
        Support { direction: [0, 0, 1], value: q(1, 1) },
        Support { direction: [0, 0, -1], value: q(1, 1) },
    ];
    let result = recover(&Input { families, supports })
        .expect("FAILED: the six cube facets remain a legal reconstruction");
    assert_eq!(result.planes.len(), 6,
        "FAILED: a loose plane with no positive-area face must not be selected");
}

#[test]
fn equal_minimum_selections_use_family_order() {
    let mut families = cube();
    families.insert(1, family([1, 0, 0], &[(1, 1)]));
    let result = recover(&Input { families, supports: vec![] })
        .expect("FAILED: either duplicate x-bound family can close the same body");
    assert_eq!(result.planes.len(), 6, "FAILED: minimum facet count excludes one duplicate family");
    assert!(result.planes.iter().any(|p| p.family == 0) && !result.planes.iter().any(|p| p.family == 1),
        "FAILED: equal minimum selections use lexicographic family/offset order");
}

#[test]
fn rational_height_is_reduced_in_every_upper_vertex() {
    let mut families = cube();
    families[4] = family([0, 0, 1], &[(2, 4)]);
    let result = recover(&Input { families, supports: vec![] })
        .expect("FAILED: a box with rational height is bounded and full-dimensional");
    let upper: Vec<_> = result.vertices.iter().filter(|v| v.xyz[2] == q(1, 2)).collect();
    assert_eq!(upper.len(), 4,
        "FAILED: rational intersections must be reduced and deduplicated at z=1/2");
}
