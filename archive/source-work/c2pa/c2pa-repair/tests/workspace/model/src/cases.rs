//! Verifier-owned case inputs, transcribed unchanged from the V1 harness.
//!
//! Only the *inputs* live here, because the candidate-tainted probe has to
//! receive them in order to make the call. What the answer should be is held
//! exclusively by the `judge` crate, which the candidate never links and whose
//! sources the graded account cannot read at run time. Each case comment below
//! therefore describes the fixture only; the V1 assertion each case transcribes
//! is recorded in `judge/src/expected.rs`, above the arm that encodes it.
use crate::{Asset,Assertion,Edit,Input,Kind,Manifest,Plan,PriorManifest};

/// Every case the verifier grades. The names, the count and the fixtures are
/// exactly the V1 population for the first sixteen entries: the three `frozen`
/// cases followed by the thirteen `graded` cases. V1's opaque `case_6f38a41d9c72`
/// negative control is not a case here; it is replaced by the verifier-owned
/// `judge --selftest`. The four entries after them close validator clauses the
/// V1 fixtures documented but never made load-bearing.
///
/// Each entry records the public behaviour it makes load-bearing and the
/// plausible wrong implementation it exists to reject. Expected values stay in
/// the judge; nothing below states what any answer is.
pub const CASES:&[&str]=&[
 // Behaviour: `Edit::key` exposes one fixed order over all five edit kinds.
 // Rejects: deriving the ordering key from input position or a hash, which
 // silently changes every equal-cardinality tie downstream.
 "public_edit_keys_have_fixed_kind_order",
 // Behaviour: a legal universe holding the same edit twice is invalid input.
 // Rejects: quietly deduplicating it, or enumerating subsets of a multiset so
 // one repair is reachable at two different cardinalities.
 "duplicate_legal_edits_are_invalid",
 // Behaviour: a universe past the published bound is refused before any search.
 // Rejects: attempting the subset enumeration anyway and timing out or
 // truncating the universe to the bound.
 "edit_universe_bound_is_enforced",
 // Behaviour: a one-edit repair is found and the manifest it returns is the
 // post-edit manifest, which itself validates.
 // Rejects: returning the chosen edits without applying them, and validating
 // against the malformed manifest rather than the repaired one.
 "one_action_replacement_repairs_manifest",
 // Behaviour: the repair is the global minimum-cardinality subset, so edits
 // that are individually legal but unnecessary stay out.
 // Rejects: accept-any-valid-repair, which applies the whole universe, and
 // greedy first-error repair, which stops at the first quiet validator.
 "minimum_subset_ignores_valid_decoys",
 // Behaviour: two repairs of equal size are separated by the canonical
 // `Edit::key` vector.
 // Rejects: a wrong equal-cardinality tie -- returning the first equal-size
 // subset the enumeration reached, or the one matching input order.
 "equal_cardinality_repairs_use_edit_key",
 // Behaviour: a parent reference must resolve into the prior store *for the
 // same asset*.
 // Rejects: resolving parents against the whole prior store, ignoring the
 // asset binding that makes the reference meaningful.
 "parent_must_resolve_in_prior_store_for_same_asset",
 // Behaviour: cardinality is validated only after the complete ordered edit
 // sequence has been applied.
 // Rejects: validating each edit independently, and requiring every
 // intermediate state to validate -- either one refuses a pair of edits whose
 // combined effect is legal.
 "relationship_cardinality_checked_after_all_edits",
 // Behaviour: a hashed URI resolves backward only, to an earlier assertion.
 // Rejects: resolving references in either direction, or scanning the whole
 // assertion list without regard to position.
 "forward_hashed_uri_target_is_invalid",
 // Behaviour: a delete removes only its stable-id target and leaves the
 // relative order of everything else intact.
 // Rejects: index-based delete handling, which removes the wrong assertion
 // once an earlier edit has moved things.
 "delete_preserves_relative_assertion_order",
 // Behaviour: an edit targeting an already deleted assertion fails the whole
 // sequence.
 // Rejects: index-based application that silently retargets the slot, and
 // skipping edits whose target has gone.
 "delete_then_edit_same_stable_id_fails_sequence",
 // Behaviour: a label replacement, a relationship change, a complete
 // hashed-URI rewrite and assertion ordering all interact in one repair, with
 // a destructive delete offered as a decoy.
 // Rejects: greedy first-error repair; accept-any-valid-repair; and taking the
 // decoy delete, which quiets the validator without being the legal repair.
 "compound_label_parent_hash_and_order_repair",
 // Behaviour: with more than one hashed URI present, satisfying backward
 // resolution for the later reference changes the size of the minimum edit
 // set, after which the canonical key decides between two repairs of that size.
 // Rejects: checking only the first hashed URI; ignoring minimum cardinality;
 // and a wrong equal-cardinality tie.
 "compound_second_uri_order_cardinality_and_tie",
 // Behaviour: the returned reference graph is derived from the post-edit
 // manifest, so it tracks a label the repair itself created.
 // Rejects: deriving edges from the malformed manifest, which resolves the
 // reference against the pre-edit label and reports the wrong graph.
 "derived_reference_graph_tracks_post_edit_label",
 // Behaviour: assertion labels must be unique even when every reference still
 // resolves.
 // Rejects: validating only that references resolve, treating the duplicate as
 // harmless because nothing dangles.
 "duplicate_labels_are_rejected_even_if_reference_resolves",
 // Behaviour: a malformed manifest with no legal edit available fails closed.
 // Rejects: returning the empty edit set as a successful repair, which is what
 // skipping the final global validation produces.
 "no_legal_repair_fails_closed",
 // Behaviour: edits apply in canonical key order, whatever order they arrive in.
 // Rejects: applying edits in input order, which reaches a different manifest
 // whenever two edits touch related state.
 "canonical_edit_application_rejects_input_order",
 // Behaviour: the manifest under validation must name the asset it is validated
 // against.
 // Rejects: validating the assertions in isolation, so a manifest bound to a
 // different asset passes on the strength of its internal consistency.
 "manifest_asset_id_must_match_asset",
 // Behaviour: stable assertion ids are unique, because an id is what every edit
 // targets and a repeated one makes that targeting ambiguous.
 // Rejects: checking labels for uniqueness but not ids, which is the natural
 // omission once labels are the thing references resolve against.
 "duplicate_assertion_ids_are_invalid",
 // Behaviour: a stable assertion id is nonempty.
 // Rejects: accepting the empty id because nothing in this manifest happens to
 // reference it.
 "empty_assertion_id_is_invalid",
 // Behaviour: an assertion label is nonempty.
 // Rejects: accepting the empty label because no hashed URI in this manifest
 // resolves against it.
 "empty_assertion_label_is_invalid",
 // Behaviour: each edit resolves its target in the manifest as it stands when
 // that edit runs, so a second deletion still names its own assertion after the
 // first deletion has moved everything below it up one slot.
 // Rejects: resolving targets to positions in the arriving manifest and then
 // applying them to the mutated one, which silently deletes the wrong
 // assertion as soon as two deletions land in the same sequence.
 "stable_ids_survive_earlier_deletion",
];

// ---- shared fixture builders, transcribed from the V1 harness --------------
fn asset()->Asset{Asset{id:"asset-7".into(),mime:"image/x-synthetic".into(),hash:"hash-7".into()}}
fn prior()->Vec<PriorManifest>{vec![
 PriorManifest{claim_id:"old-7".into(),asset_id:"asset-7".into()},
 PriorManifest{claim_id:"other".into(),asset_id:"other-asset".into()},
]}
fn a(id:&str,label:&str,kind:Kind)->Assertion{Assertion{id:id.into(),label:label.into(),kind}}
fn action(id:&str,value:&str)->Assertion{a(id,"c2pa.actions",Kind::Action{value:value.into()})}
fn ingredient(id:&str,rel:&str,claim:&str)->Assertion{a(id,"c2pa.ingredient",Kind::Ingredient{relationship:rel.into(),manifest_ref:claim.into()})}
fn metadata(id:&str,label:&str)->Assertion{a(id,label,Kind::Metadata{asset_id:"asset-7".into(),mime:"image/x-synthetic".into(),hash:"hash-7".into()})}
fn uri(id:&str,target:&str)->Assertion{a(id,"c2pa.hash",Kind::HashedUri{url:"asset://asset-7".into(),hash:"hash-7".into(),target_label:target.into()})}
fn manifest(assertions:Vec<Assertion>)->Manifest{Manifest{claim_id:"new-7".into(),asset_id:"asset-7".into(),assertions}}
fn input(m:Manifest,edits:Vec<Edit>)->Input{Input{asset:asset(),prior:prior(),malformed:m,legal:edits}}

fn del(t:&str)->Edit{Edit::Delete{target:t.into()}}
fn relabel(t:&str,l:&str)->Edit{Edit::ReplaceLabel{target:t.into(),label:l.into()}}
fn reaction(t:&str,v:&str)->Edit{Edit::ReplaceAction{target:t.into(),value:v.into()}}
fn rerel(t:&str,r:&str)->Edit{Edit::ReplaceRelationship{target:t.into(),relationship:r.into()}}
fn reuri(t:&str,u:&str,h:&str,l:&str)->Edit{Edit::ReplaceHashedUri{target:t.into(),url:u.into(),hash:h.into(),target_label:l.into()}}

/// Inputs for one named case. Returns `None` for an unknown id so the probe
/// cannot be steered onto an undefined problem.
pub fn plan(id:&str)->Option<Plan>{
 Some(match id{

  // -- frozen suite ----------------------------------------------------------

  // Input: one edit of each kind, in declaration order.
  // Behaviour: one fixed kind order over all five edit variants.
  // Rejects: keying on input position or a hash, which changes every tie.
  "public_edit_keys_have_fixed_kind_order"=>Plan::Keys(vec![
   del("x"),relabel("x","l"),reaction("x","v"),rerel("x","r"),reuri("x","u","h","t"),
  ]),

  // Input: the same legal edit supplied twice; fixture uses its own small asset.
  // Behaviour: a repeated legal edit makes the universe invalid input.
  // Rejects: quietly deduplicating, or enumerating over a multiset.
  "duplicate_legal_edits_are_invalid"=>{
   let e=relabel("meta","unused");
   let m=Manifest{claim_id:"new".into(),asset_id:"a".into(),assertions:vec![
    a("act","action",Kind::Action{value:"edited".into()}),
    a("ing","ingredient",Kind::Ingredient{relationship:"parent".into(),manifest_ref:"old".into()}),
    a("meta","metadata",Kind::Metadata{asset_id:"a".into(),mime:"m".into(),hash:"h".into()}),
    a("ref","hash",Kind::HashedUri{url:"asset://a".into(),hash:"h".into(),target_label:"metadata".into()}),
   ]};
   Plan::Repair(Input{
    asset:Asset{id:"a".into(),mime:"m".into(),hash:"h".into()},
    prior:vec![PriorManifest{claim_id:"old".into(),asset_id:"a".into()}],
    malformed:m,legal:vec![e.clone(),e],
   })
  }

  // Input: twenty-one legal edits, one past the published universe bound.
  // Behaviour: an oversized universe is refused before any search.
  // Rejects: enumerating anyway, or truncating the universe to the bound.
  "edit_universe_bound_is_enforced"=>Plan::Repair(Input{
   asset:Asset{id:"a".into(),mime:"m".into(),hash:"h".into()},
   prior:vec![],
   malformed:Manifest{claim_id:"c".into(),asset_id:"a".into(),assertions:vec![]},
   legal:vec![del("x");21],
  }),

  // -- graded suite ----------------------------------------------------------

  // Input: a manifest whose action value is stale, with one action replacement
  // available. Calls `repair`, then `validate` on the manifest it returned.
  // Behaviour: the repair applies its edits and the returned manifest validates.
  // Rejects: returning chosen edits unapplied, or validating the malformed
  // manifest instead of the repaired one.
  "one_action_replacement_repairs_manifest"=>Plan::RepairThenValidate(input(
   manifest(vec![action("act","opened"),ingredient("ing","parent","old-7"),metadata("meta","c2pa.metadata"),uri("ref","c2pa.metadata")]),
   vec![reaction("act","edited")],
  )),

  // Input: two valid decoy edits alongside the action replacement.
  // Behaviour: the global minimum-cardinality subset, so legal-but-unneeded
  // edits stay out.
  // Rejects: accept-any-valid-repair, and greedy first-error repair.
  "minimum_subset_ignores_valid_decoys"=>Plan::Repair(input(
   manifest(vec![action("act","opened"),ingredient("ing","parent","old-7"),metadata("meta","c2pa.metadata"),uri("ref","c2pa.metadata")]),
   vec![relabel("meta","unneeded"),reaction("act","edited"),del("ref")],
  )),

  // Input: two duplicate actions and a delete for each, so more than one repair
  // of equal cardinality exists.
  // Behaviour: equal-size repairs are separated by the canonical key vector.
  // Rejects: the first equal-size subset found, or input order.
  "equal_cardinality_repairs_use_edit_key"=>Plan::Repair(input(
   manifest(vec![action("a","edited"),action("b","edited"),ingredient("ing","parent","old-7"),metadata("meta","c2pa.metadata"),uri("ref","c2pa.metadata")]),
   vec![del("b"),del("a")],
  )),

  // Input: the parent ingredient names a claim held for a different asset.
  // Behaviour: a parent must resolve in the prior store for this asset.
  // Rejects: resolving against the whole prior store, ignoring the asset.
  "parent_must_resolve_in_prior_store_for_same_asset"=>Plan::Validate{
   asset:asset(),prior:prior(),
   manifest:manifest(vec![action("act","edited"),ingredient("ing","parent","other"),metadata("meta","c2pa.metadata"),uri("ref","c2pa.metadata")]),
  },

  // Input: an already valid manifest plus two relationship swaps, each of which
  // alone breaks parent cardinality while the pair is a no-op.
  // Behaviour: cardinality is checked only after the full ordered sequence.
  // Rejects: independent per-edit validation, and requiring every intermediate
  // state to validate; both refuse a pair whose combined effect is legal.
  "relationship_cardinality_checked_after_all_edits"=>{
   let component=a("i1","c2pa.ingredient.component",Kind::Ingredient{relationship:"component".into(),manifest_ref:"old-7".into()});
   Plan::Repair(input(
    manifest(vec![action("act","edited"),component,ingredient("i2","parent","old-7"),metadata("meta","c2pa.metadata"),uri("ref","c2pa.metadata")]),
    vec![rerel("i1","parent"),rerel("i2","component")],
   ))
  }

  // Input: the hashed URI is placed before the assertion it names.
  // Behaviour: a hashed URI resolves backward only.
  // Rejects: resolving in either direction, or scanning without position.
  "forward_hashed_uri_target_is_invalid"=>Plan::Validate{
   asset:asset(),prior:prior(),
   manifest:manifest(vec![action("act","edited"),ingredient("ing","parent","old-7"),uri("ref","c2pa.metadata"),metadata("meta","c2pa.metadata")]),
  },

  // Input: a junk assertion sits between the others, with a delete for it.
  // Behaviour: delete removes its stable-id target and preserves order.
  // Rejects: index-based delete handling.
  "delete_preserves_relative_assertion_order"=>Plan::Repair(input(
   manifest(vec![action("act","edited"),a("junk","junk",Kind::Action{value:"opened".into()}),ingredient("ing","parent","old-7"),metadata("meta","c2pa.metadata"),uri("ref","c2pa.metadata")]),
   vec![del("junk")],
  )),

  // Input: a delete followed by a relabel of the same stable id.
  // Behaviour: editing an already deleted assertion fails the sequence.
  // Rejects: index-based retargeting, and skipping edits whose target is gone.
  "delete_then_edit_same_stable_id_fails_sequence"=>Plan::Apply{
   manifest:manifest(vec![action("act","edited"),ingredient("ing","parent","old-7"),metadata("meta","c2pa.metadata"),uri("ref","c2pa.metadata")]),
   edits:vec![del("meta"),relabel("meta","x")],
  },

  // Input: a manifest with a wrong label, a wrong relationship and a wrong
  // hashed URI, offered a label, relationship and URI edit plus a destructive
  // delete decoy.
  // Behaviour: label, relationship, hashed-URI rewrite and ordering interact
  // in one repair, with a destructive delete offered as a decoy.
  // Rejects: greedy first-error repair, accept-any-valid-repair, and taking
  // the decoy delete that quiets the validator without being the repair.
  "compound_label_parent_hash_and_order_repair"=>{
   let bad_uri=a("ref","c2pa.hash",Kind::HashedUri{url:"asset://wrong".into(),hash:"wrong".into(),target_label:"c2pa.metadata".into()});
   Plan::Repair(input(
    manifest(vec![action("act","edited"),ingredient("ing","component","old-7"),metadata("meta","bad.metadata"),bad_uri]),
    vec![
     reuri("ref","asset://asset-7","hash-7","c2pa.metadata"),
     rerel("ing","parent"),
     relabel("meta","c2pa.metadata"),
     del("meta"),
    ],
   ))
  }

  // Input: two hashed URIs, the second naming a label that appears only later
  // in the assertion list, with both a deletion and a rewrite offered for it.
  // Behaviour: the later hashed URI's backward resolution changes the minimum
  // edit-set size, then the canonical key decides between two of that size.
  // Rejects: checking only the first hashed URI, ignoring minimum cardinality,
  // and a wrong equal-cardinality tie.
  "compound_second_uri_order_cardinality_and_tie"=>{
   let aux=a("r2","c2pa.hash.aux",Kind::HashedUri{url:"asset://asset-7".into(),hash:"hash-7".into(),target_label:"c2pa.thumbnail".into()});
   let tail=a("tail","c2pa.thumbnail",Kind::Ingredient{relationship:"component".into(),manifest_ref:"old-7".into()});
   Plan::Repair(input(
    manifest(vec![
     action("act","opened"),ingredient("ing","component","old-7"),
     metadata("meta","c2pa.metadata"),uri("r1","c2pa.metadata"),aux,tail,
    ]),
    vec![
     reaction("act","edited"),
     rerel("ing","parent"),
     del("r2"),
     reuri("r2","asset://asset-7","hash-7","c2pa.metadata"),
     del("tail"),
    ],
   ))
  }

  // Input: the hashed URI names a label that only exists after the relabel.
  // Behaviour: the reference graph is derived from the post-edit manifest.
  // Rejects: deriving edges from the malformed manifest's labels.
  "derived_reference_graph_tracks_post_edit_label"=>Plan::Repair(input(
   manifest(vec![action("act","edited"),ingredient("ing","parent","old-7"),metadata("meta","wrong"),uri("ref","c2pa.metadata")]),
   vec![relabel("meta","c2pa.metadata")],
  )),

  // Input: two assertions share a label while the hashed URI still resolves.
  // Behaviour: labels must be unique even when every reference resolves.
  // Rejects: validating only that references resolve.
  "duplicate_labels_are_rejected_even_if_reference_resolves"=>Plan::Validate{
   asset:asset(),prior:prior(),
   manifest:manifest(vec![
    action("act","edited"),ingredient("ing","parent","old-7"),metadata("m1","c2pa.metadata"),
    a("m2","c2pa.metadata",Kind::Metadata{asset_id:"x".into(),mime:"x".into(),hash:"x".into()}),
    uri("ref","c2pa.metadata"),
   ]),
  },

  // Input: a malformed manifest with an empty legal edit universe.
  // Behaviour: a malformed manifest with no legal edit fails closed.
  // Rejects: returning the empty edit set as a successful repair.
  "no_legal_repair_fails_closed"=>Plan::Repair(input(manifest(vec![action("act","opened")]),vec![])),

  // Input: edits supplied out of canonical key order.
  // Behaviour: edits apply in canonical key order, not arrival order.
  // Rejects: applying in input order.
  "canonical_edit_application_rejects_input_order"=>Plan::Apply{
   manifest:manifest(vec![action("act","opened"),ingredient("ing","component","old-7")]),
   edits:vec![rerel("ing","parent"),reaction("act","edited")],
  },

  // -- validator clauses the V1 fixtures documented but never exercised -------
  //
  // Each of the four manifests below is otherwise valid for this asset and
  // prior store, and carries exactly one defect, so the clause named in its
  // comment is the only ground on which it can be refused.

  // Input: a manifest whose own asset id is a different asset, with assertions
  // that are all correct for the asset it is validated against.
  // Behaviour: the manifest has to name the asset it is validated against.
  // Rejects: validating the assertions in isolation, so internal consistency
  // carries a manifest bound to another asset.
  "manifest_asset_id_must_match_asset"=>Plan::Validate{
   asset:asset(),prior:prior(),
   manifest:Manifest{claim_id:"new-7".into(),asset_id:"other-asset".into(),assertions:vec![
    action("act","edited"),ingredient("ing","parent","old-7"),
    metadata("meta","c2pa.metadata"),uri("ref","c2pa.metadata"),
   ]},
  },

  // Input: the action and the metadata assertion share one stable id; every
  // label is still distinct.
  // Behaviour: stable ids are unique, since an id is what every edit targets.
  // Rejects: checking label uniqueness only, the natural omission once labels
  // are the thing references resolve against.
  "duplicate_assertion_ids_are_invalid"=>Plan::Validate{
   asset:asset(),prior:prior(),
   manifest:manifest(vec![
    action("dup","edited"),ingredient("ing","parent","old-7"),
    metadata("dup","c2pa.metadata"),uri("ref","c2pa.metadata"),
   ]),
  },

  // Input: the parent ingredient has an empty stable id and nothing references
  // it.
  // Behaviour: a stable id is nonempty.
  // Rejects: accepting it because no edit or reference in this manifest names it.
  "empty_assertion_id_is_invalid"=>Plan::Validate{
   asset:asset(),prior:prior(),
   manifest:manifest(vec![
    action("act","edited"),ingredient("","parent","old-7"),
    metadata("meta","c2pa.metadata"),uri("ref","c2pa.metadata"),
   ]),
  },

  // Input: the parent ingredient has an empty label; the hashed URI resolves
  // against a different assertion, so no reference depends on it.
  // Behaviour: a label is nonempty.
  // Rejects: accepting it because no hashed URI here resolves against it.
  "empty_assertion_label_is_invalid"=>Plan::Validate{
   asset:asset(),prior:prior(),
   manifest:manifest(vec![
    action("act","edited"),
    a("ing","",Kind::Ingredient{relationship:"parent".into(),manifest_ref:"old-7".into()}),
    metadata("meta","c2pa.metadata"),uri("ref","c2pa.metadata"),
   ]),
  },

  // Input: two throwaway assertions sitting immediately below each other, with
  // a deletion for each. Both deletions apply cleanly, so the sequence succeeds
  // and the surviving order is what separates the readings.
  // Behaviour: the second deletion resolves its own stable id in the manifest as
  // it stands after the first, not in the manifest that arrived.
  // Rejects: capturing positions from the arriving manifest and applying them to
  // the mutated one, which removes the assertion that moved into the vacated
  // slot instead of the one the edit names.
  "stable_ids_survive_earlier_deletion"=>Plan::Apply{
   manifest:manifest(vec![
    action("act","edited"),
    a("d1","junk-one",Kind::Action{value:"opened".into()}),
    a("d2","junk-two",Kind::Action{value:"opened".into()}),
    ingredient("ing","parent","old-7"),
    metadata("meta","c2pa.metadata"),uri("ref","c2pa.metadata"),
   ]),
   edits:vec![del("d1"),del("d2")],
  },

  _=>return None,
 })
}
