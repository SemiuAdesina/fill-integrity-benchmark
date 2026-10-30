use c2pa_repair::*;

fn fixture() -> (Asset, Vec<PriorManifest>, UpdateManifest) {
    let asset = Asset { id: "clip".into(), mime: "image/x-demo".into(), hash: "hash-7".into() };
    let prior = vec![PriorManifest { claim_id: "prior-claim".into(), asset_id: "clip".into() }];
    let manifest = UpdateManifest {
        claim_id: "update-claim".into(),
        asset_id: "clip".into(),
        assertions: vec![
            Assertion { id: "act".into(), label: "action".into(), kind: AssertionKind::Action { value: "edited".into() } },
            Assertion { id: "ing".into(), label: "ingredient".into(), kind: AssertionKind::Ingredient { relationship: "parent".into(), manifest_ref: "prior-claim".into() } },
            Assertion { id: "meta".into(), label: "metadata".into(), kind: AssertionKind::Metadata { asset_id: "clip".into(), mime: "image/x-demo".into(), hash: "hash-7".into() } },
            Assertion { id: "uri".into(), label: "hashed-uri".into(), kind: AssertionKind::HashedUri { url: "asset://clip".into(), hash: "hash-7".into(), target_label: "metadata".into() } },
        ],
    };
    (asset, prior, manifest)
}

#[test]
fn edit_application_requires_unique_canonical_key_order() {
    let (_, _, manifest) = fixture();
    let edits = vec![
        Edit::ReplaceAction { target: "act".into(), value: "edited".into() },
        Edit::ReplaceLabel { target: "meta".into(), label: "metadata-2".into() },
    ];
    assert!(apply_edits(&manifest, &edits).is_err(),
        "FAILED: chosen edits must be unique and sorted by Edit::key before application");
}

#[test]
fn validation_derives_parent_and_hashed_uri_edges() {
    let (asset, prior, manifest) = fixture();
    let summary = validate(&asset, &prior, &manifest)
        .expect("FAILED: the complete synthetic manifest should validate");
    assert_eq!(summary.edges, vec![
        ReferenceEdge { from_id: "ing".into(), to: "prior-claim".into() },
        ReferenceEdge { from_id: "uri".into(), to: "meta".into() },
    ], "FAILED: reference edges must be derived from the post-edit manifest");
}

#[test]
fn validation_checks_every_hashed_uri_and_backward_order() {
    let (asset, prior, mut manifest) = fixture();
    manifest.assertions.insert(0, Assertion {
        id: "early-uri".into(), label: "early-hashed-uri".into(),
        kind: AssertionKind::HashedUri {
            url: "asset://clip".into(), hash: "hash-7".into(), target_label: "metadata".into(),
        },
    });
    assert!(validate(&asset, &prior, &manifest).is_err(),
        "FAILED: every hashed URI must resolve to exactly one earlier label");
}

#[test]
fn validation_requires_exact_metadata_cardinality() {
    let (asset, prior, mut manifest) = fixture();
    manifest.assertions.push(Assertion {
        id: "meta-extra".into(), label: "metadata-extra".into(),
        kind: AssertionKind::Metadata { asset_id: "other".into(), mime: "other/type".into(), hash: "other-hash".into() },
    });
    assert!(validate(&asset, &prior, &manifest).is_err(),
        "FAILED: metadata cardinality is exact, not merely at-least-one matching assertion");
}

#[test]
fn repair_finds_a_global_minimum_subset_without_a_decoy() {
    let (asset, prior_store, mut malformed) = fixture();
    if let AssertionKind::Action { value } = &mut malformed.assertions[0].kind { *value = "draft".into(); }
    if let AssertionKind::Ingredient { relationship, .. } = &mut malformed.assertions[1].kind { *relationship = "component".into(); }
    let action = Edit::ReplaceAction { target: "act".into(), value: "edited".into() };
    let parent = Edit::ReplaceRelationship { target: "ing".into(), relationship: "parent".into() };
    let decoy = Edit::ReplaceLabel { target: "meta".into(), label: "action".into() };
    let result = repair(&RepairInput { asset, prior_store, malformed, legal_edits: vec![parent.clone(), decoy, action.clone()] })
        .expect("FAILED: a two-edit repair exists inside the supplied legal universe");
    assert_eq!(result.edits, vec![action, parent],
        "FAILED: choose the minimum valid subset; do not append a destructive legal decoy");
}

#[test]
fn equal_cardinality_repairs_use_the_lexicographically_smallest_key_vector() {
    let (asset, prior_store, mut malformed) = fixture();
    malformed.assertions.remove(1);
    malformed.assertions.insert(1, Assertion { id: "b-parent".into(), label: "parent-b".into(), kind: AssertionKind::Ingredient { relationship: "parent".into(), manifest_ref: "prior-claim".into() } });
    malformed.assertions.insert(1, Assertion { id: "a-parent".into(), label: "parent-a".into(), kind: AssertionKind::Ingredient { relationship: "parent".into(), manifest_ref: "prior-claim".into() } });
    let delete_a = Edit::Delete { target: "a-parent".into() };
    let delete_b = Edit::Delete { target: "b-parent".into() };
    let result = repair(&RepairInput { asset, prior_store, malformed, legal_edits: vec![delete_b, delete_a.clone()] })
        .expect("FAILED: deleting either extra parent produces a valid one-edit repair");
    assert_eq!(result.edits, vec![delete_a],
        "FAILED: equal-cardinality repairs use the lexicographically smallest Edit::key vector");
}

#[test]
fn legal_edit_universe_is_unique_and_bounded() {
    let (asset, prior_store, malformed) = fixture();
    let same = Edit::ReplaceAction { target: "act".into(), value: "edited".into() };
    let duplicate = RepairInput { asset: asset.clone(), prior_store: prior_store.clone(), malformed: malformed.clone(), legal_edits: vec![same.clone(), same.clone()] };
    assert!(repair(&duplicate).is_err(), "FAILED: duplicate legal edit keys must be rejected");
    let oversized = RepairInput { asset, prior_store, malformed, legal_edits: vec![same; 21] };
    assert!(repair(&oversized).is_err(), "FAILED: the legal edit universe is bounded at twenty edits");
}

#[test]
fn deletion_uses_stable_ids_and_preserves_relative_order() {
    let (_, _, manifest) = fixture();
    let edited = apply_edits(&manifest, &[
        Edit::Delete { target: "ing".into() },
        Edit::ReplaceLabel { target: "uri".into(), label: "uri-after-delete".into() },
    ]).expect("FAILED: a later edit targets a stable ID, not a shifted numeric index");
    let ids: Vec<&str> = edited.assertions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec!["act", "meta", "uri"],
        "FAILED: deletion must preserve the relative order of remaining assertions");
}
