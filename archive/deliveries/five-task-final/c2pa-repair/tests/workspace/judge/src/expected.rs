//! Verifier-owned expected values.
//!
//! One arm per V1 case, in the V1 order, each quoting the V1 assertion it
//! transcribes. Nothing here reads candidate stdout, stderr or exit status: the
//! only input is the decoded raw functional values the probe recorded, and the
//! only output is this file's own verdict.
use c2pa_model::{Edge,Edit,Manifest,Value};

fn edit_action(target:&str,value:&str)->Edit{Edit::ReplaceAction{target:target.into(),value:value.into()}}
fn edit_delete(target:&str)->Edit{Edit::Delete{target:target.into()}}
fn edge(from_id:&str,to:&str)->Edge{Edge{from_id:from_id.into(),to:to.into()}}
fn edit_rel(target:&str,relationship:&str)->Edit{Edit::ReplaceRelationship{target:target.into(),relationship:relationship.into()}}
fn edit_label(target:&str,label:&str)->Edit{Edit::ReplaceLabel{target:target.into(),label:label.into()}}
fn edit_uri(target:&str,url:&str,hash:&str,target_label:&str)->Edit{Edit::ReplaceHashedUri{target:target.into(),url:url.into(),hash:hash.into(),target_label:target_label.into()}}

fn one<'a>(obs:&'a [Value])->Result<&'a Value,String>{
 match obs{[v]=>Ok(v),_=>Err(format!("expected exactly 1 recorded observation, found {}",obs.len()))}
}
fn err_msg(obs:&[Value])->Result<&str,String>{
 match one(obs)?{Value::Err(m)=>Ok(m),other=>Err(format!("expected a rejection, recorded {}",shape(other)))}
}
fn repair_of(obs:&[Value])->Result<(&Vec<Edit>,&Manifest,&Vec<Edge>),String>{
 match one(obs)?{Value::Repair{edits,manifest,edges}=>Ok((edits,manifest,edges)),other=>Err(format!("expected a repair, recorded {}",shape(other)))}
}
fn manifest_of(obs:&[Value])->Result<&Manifest,String>{
 match one(obs)?{Value::Manifest(m)=>Ok(m),other=>Err(format!("expected an applied manifest, recorded {}",shape(other)))}
}
fn shape(v:&Value)->&'static str{
 match v{Value::Err(_)=>"a rejection",Value::Keys(_)=>"edit keys",Value::Manifest(_)=>"an applied manifest",Value::Edges(_)=>"validator edges",Value::Repair{..}=>"a repair"}
}
fn ids(m:&Manifest)->Vec<&str>{m.assertions.iter().map(|a|a.id.as_str()).collect()}

/// Grade one case's recorded observations against the verifier's own
/// expectation. `Ok(())` is a pass; `Err(reason)` is a failure.
pub fn check(id:&str,obs:&[Value])->Result<(),String>{
 match id{

  // V1: let keys:Vec<_>=xs.iter().map(Edit::key).collect();
  //     assert!(keys.windows(2).all(|w|w[0]<w[1]));
  // Public requirement: one fixed kind order over all five edit variants.
  // Rejects: an ordering key derived from input position or a hash, which
  // silently changes every equal-cardinality tie downstream.
  "public_edit_keys_have_fixed_kind_order"=>{
   let Value::Keys(k)=one(obs)? else{return Err(format!("expected edit keys, recorded {}",shape(one(obs)?)))};
   if k.len()!=5{return Err(format!("expected 5 edit keys, recorded {}",k.len()))}
   if !k.windows(2).all(|w|w[0]<w[1]){return Err("edit keys are not strictly increasing across the kinds".into())}
   Ok(())
  }

  // V1: assert!(repair(&i).is_err());   (the same legal edit supplied twice)
  // Public requirement: a repeated legal edit makes the universe invalid.
  // Rejects: quietly deduplicating, or enumerating subsets of a multiset so one
  // repair becomes reachable at two cardinalities.
  "duplicate_legal_edits_are_invalid"=>{err_msg(obs)?;Ok(())}

  // V1: assert!(repair(&i).is_err());   (twenty-one legal edits)
  // Public requirement: an oversized universe is refused before any search.
  // Rejects: enumerating anyway, or truncating the universe to the bound.
  "edit_universe_bound_is_enforced"=>{err_msg(obs)?;Ok(())}

  // V1: let x=repair(&i).unwrap();
  //     assert_eq!(x.edits.len(),1);
  //     assert!(validate(&i.asset,&i.prior_store,&x.manifest).is_ok());
  // Public requirement: the repair applies its edits, and the manifest it
  // returns is the post-edit manifest, which itself validates.
  // Rejects: returning chosen edits unapplied, and validating the malformed
  // manifest rather than the repaired one.
  "one_action_replacement_repairs_manifest"=>{
   let [first,second]=obs else{return Err(format!("expected 2 recorded observations, found {}",obs.len()))};
   let Value::Repair{edits,..}=first else{return Err(format!("expected a repair, recorded {}",shape(first)))};
   if edits.len()!=1{return Err(format!("repair used {} edits, expected 1",edits.len()))}
   match second{
    Value::Edges(_)=>Ok(()),
    Value::Err(m)=>Err(format!("the repaired manifest did not validate: {m}")),
    other=>Err(format!("expected validator edges, recorded {}",shape(other))),
   }
  }

  // V1: assert_eq!(x.edits,vec![Edit::ReplaceAction{target:"act",value:"edited"}]);
  // Public requirement: the global minimum-cardinality subset.
  // Rejects: accept-any-valid-repair, which applies the whole universe, and
  // greedy first-error repair, which stops at the first quiet validator.
  "minimum_subset_ignores_valid_decoys"=>{
   let (edits,_,_)=repair_of(obs)?;
   if *edits!=vec![edit_action("act","edited")]{return Err(format!("repair chose {edits:?}, not the single action replacement"))}
   Ok(())
  }

  // V1: assert_eq!(x.edits,vec![Edit::Delete{target:"a"}]);
  //     assert_eq!(x.manifest.assertions[0].id,"b");
  // Public requirement: equal-size repairs are separated by the canonical key.
  // Rejects: the first equal-size subset the enumeration reached, and the one
  // matching input order.
  "equal_cardinality_repairs_use_edit_key"=>{
   let (edits,manifest,_)=repair_of(obs)?;
   if *edits!=vec![edit_delete("a")]{return Err(format!("repair chose {edits:?}, not the lexically smallest tie"))}
   match manifest.assertions.first(){
    Some(a) if a.id=="b"=>Ok(()),
    Some(a)=>Err(format!("first surviving assertion is {:?}, expected \"b\"",a.id)),
    None=>Err("repaired manifest has no assertions".into()),
   }
  }

  // V1: assert!(validate(&asset(),&prior(),&m).is_err());
  // Public requirement: a parent resolves in the prior store for this asset.
  // Rejects: resolving against the whole prior store, ignoring the asset bind.
  "parent_must_resolve_in_prior_store_for_same_asset"=>{err_msg(obs)?;Ok(())}

  // V1: assert_eq!(x.edits.len(),0);
  //     assert_eq!(x.edges.iter().filter(|e|e.to=="old-7").count(),1);
  // Public requirement: cardinality is checked only after the full ordered
  // sequence, and the derived edges come from that final state.
  // Rejects: independent per-edit validation, and requiring every intermediate
  // state to validate; both refuse a pair whose combined effect is legal.
  "relationship_cardinality_checked_after_all_edits"=>{
   let (edits,_,edges)=repair_of(obs)?;
   if !edits.is_empty(){return Err(format!("repair used {} edits, expected 0",edits.len()))}
   let n=edges.iter().filter(|e|e.to=="old-7").count();
   if n!=1{return Err(format!("{n} derived edges reach \"old-7\", expected 1"))}
   Ok(())
  }

  // V1: assert_eq!(validate(&asset(),&prior(),&m).unwrap_err(),"hashed URI target/order");
  // Public requirement: a hashed URI resolves backward only, by exact message.
  // Rejects: resolving in either direction, or scanning the assertion list
  // without regard to position.
  "forward_hashed_uri_target_is_invalid"=>{
   let m=err_msg(obs)?;
   if m!="hashed URI target/order"{return Err(format!("rejected with {m:?}, expected \"hashed URI target/order\""))}
   Ok(())
  }

  // V1: assert_eq!(ids,vec!["act","ing","meta","ref"]);
  // Public requirement: delete removes its stable-id target and preserves the
  // relative order of the survivors.
  // Rejects: index-based delete handling, which drops the wrong assertion.
  "delete_preserves_relative_assertion_order"=>{
   let (_,manifest,_)=repair_of(obs)?;
   if ids(manifest)!=vec!["act","ing","meta","ref"]{return Err(format!("surviving assertion order is {:?}",ids(manifest)))}
   Ok(())
  }

  // V1: assert!(apply_edits(&m,&edits).is_err());
  // Public requirement: editing an already deleted assertion fails the sequence.
  // Rejects: index-based retargeting of the vacated slot, and skipping edits
  // whose target has gone.
  "delete_then_edit_same_stable_id_fails_sequence"=>{err_msg(obs)?;Ok(())}

  // Combines a metadata-label repair, a parent relationship change and a full
  // hashed-URI replacement, with a competing deletion offered for the same
  // label. Pinning the edit vector by identity, not just its size, is what
  // rejects an implementation that reaches the right cardinality with the
  // wrong three edits — e.g. one that deletes the metadata assertion instead
  // of relabelling it, or that leaves the URI's stale hash in place.
  // Rejects: greedy first-error repair; accept-any-valid-repair; and taking the
  // destructive delete decoy, which quiets the validator at the same size.
  "compound_label_parent_hash_and_order_repair"=>{
   let (edits,_,edges)=repair_of(obs)?;
   let want=vec![
    edit_label("meta","c2pa.metadata"),
    edit_rel("ing","parent"),
    edit_uri("ref","asset://asset-7","hash-7","c2pa.metadata"),
   ];
   if *edits!=want{return Err(format!("minimum repair is {edits:?}"))}
   if *edges!=vec![edge("ing","old-7"),edge("ref","meta")]{return Err(format!("derived reference graph is {edges:?}"))}
   Ok(())
  }

  // New in this revision. Every hashed URI is checked, not just the first, so
  // the later one's forward target has to be repaired too; that third edit is
  // reachable by either a deletion or a rewrite, and the canonical key decides.
  // A solver that stops at the first resolving URI answers with two edits.
  // Rejects: checking only the first hashed URI; ignoring minimum cardinality;
  // and a wrong equal-cardinality tie.
  "compound_second_uri_order_cardinality_and_tie"=>{
   let (edits,_,edges)=repair_of(obs)?;
   let want=vec![edit_delete("r2"),edit_action("act","edited"),edit_rel("ing","parent")];
   if *edits!=want{return Err(format!("minimum repair is {edits:?}"))}
   if *edges!=vec![edge("ing","old-7"),edge("r1","meta")]{return Err(format!("derived reference graph is {edges:?}"))}
   Ok(())
  }

  // V1: assert_eq!(x.edges[1].to,"meta");
  // Public requirement: the reference graph is derived from the post-edit
  // manifest, so it tracks a label the repair itself created.
  // Rejects: deriving edges from the malformed manifest's labels.
  "derived_reference_graph_tracks_post_edit_label"=>{
   let (_,_,edges)=repair_of(obs)?;
   match edges.get(1){
    Some(e) if e.to=="meta"=>Ok(()),
    Some(e)=>Err(format!("second derived edge reaches {:?}, expected \"meta\"",e.to)),
    None=>Err(format!("only {} derived edges, expected at least 2",edges.len())),
   }
  }

  // V1: assert!(validate(&asset(),&prior(),&m).is_err());
  // Public requirement: labels are unique even when every reference resolves.
  // Rejects: validating only that references resolve.
  "duplicate_labels_are_rejected_even_if_reference_resolves"=>{err_msg(obs)?;Ok(())}

  // V1: assert_eq!(repair(&i).unwrap_err(),"no legal repair");
  // Public requirement: no legal repair fails closed, by exact message.
  // Rejects: returning the empty edit set as a successful repair, which is what
  // skipping the final global validation produces.
  "no_legal_repair_fails_closed"=>{
   let m=err_msg(obs)?;
   if m!="no legal repair"{return Err(format!("failed with {m:?}, expected \"no legal repair\""))}
   Ok(())
  }

  // V1: assert!(apply_edits(&m,&edits).is_err());
  // Public requirement: edits apply in canonical key order, not arrival order.
  // Rejects: applying edits in input order.
  "canonical_edit_application_rejects_input_order"=>{err_msg(obs)?;Ok(())}

  // Public requirement: the manifest names the asset it is validated against.
  // Semantic behavior: the manifest below is internally consistent and correct
  // for this asset in every other respect, so the asset binding is the only
  // ground on which it can be refused.
  // Rejects: validating the assertion set in isolation, which lets a manifest
  // bound to another asset pass on internal consistency alone.
  "manifest_asset_id_must_match_asset"=>{err_msg(obs)?;Ok(())}

  // Public requirement: stable assertion ids are unique.
  // Semantic behavior: every label here is distinct and every reference
  // resolves, so only the repeated id can refuse this manifest -- and an id has
  // to be unique because an id is what every edit targets.
  // Rejects: checking label uniqueness but not id uniqueness, the natural
  // omission once labels are the thing references resolve against.
  "duplicate_assertion_ids_are_invalid"=>{err_msg(obs)?;Ok(())}

  // Public requirement: a stable assertion id is nonempty.
  // Semantic behavior: nothing in this manifest references the offending
  // assertion, so an implementation that only checks ids it happens to follow
  // sees no problem here.
  // Rejects: treating an unreferenced empty id as harmless.
  "empty_assertion_id_is_invalid"=>{err_msg(obs)?;Ok(())}

  // Public requirement: an assertion label is nonempty.
  // Semantic behavior: the one hashed URI here resolves against a different
  // assertion, so the empty label breaks no reference in this manifest.
  // Rejects: treating an empty label as harmless because nothing resolves
  // against it.
  "empty_assertion_label_is_invalid"=>{err_msg(obs)?;Ok(())}

  // Public requirement: every edit resolves its target against the manifest as
  // it stands when that edit runs, so a stable id keeps naming its own assertion
  // after an earlier deletion has moved it.
  // Semantic behavior: both deletions apply cleanly here, so the sequence
  // succeeds either way and only the surviving assertion order separates a
  // stable-id reading from a positional one.
  // Rejects: capturing target positions from the arriving manifest and applying
  // them to the mutated one, which drops the assertion that moved into the
  // vacated slot rather than the one the second edit names.
  "stable_ids_survive_earlier_deletion"=>{
   let m=manifest_of(obs)?;
   if ids(m)!=vec!["act","ing","meta","ref"]{return Err(format!("surviving assertion order is {:?}",ids(m)))}
   Ok(())
  }

  _=>Err("unknown case".into()),
 }
}
