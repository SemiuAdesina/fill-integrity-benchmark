//! Translation between the verifier's own types and the candidate's types.
//!
//! The candidate file declares the entire published data contract, not just a
//! solver function, so the verifier keeps independent copies of those shapes in
//! `c2pa_model` and converts at the boundary. Inputs go out as candidate
//! values; answers come back as verifier values. A candidate therefore can
//! never hand the judge a value of a type it controls, and a candidate that
//! changes the contract shapes fails to compile — which scores zero, exactly as
//! it did under V1.
//!
//! Nothing here asserts, compares or reports. It only moves data.
#![allow(dead_code)]
use crate::candidate;
use c2pa_model as m;

// ------------------------------------------------------- verifier -> candidate
pub fn asset_out(a:&m::Asset)->candidate::Asset{candidate::Asset{id:a.id.clone(),mime:a.mime.clone(),hash:a.hash.clone()}}
pub fn prior_out(p:&[m::PriorManifest])->Vec<candidate::PriorManifest>{p.iter().map(|x|candidate::PriorManifest{claim_id:x.claim_id.clone(),asset_id:x.asset_id.clone()}).collect()}
pub fn kind_out(k:&m::Kind)->candidate::AssertionKind{
 match k{
  m::Kind::Action{value}=>candidate::AssertionKind::Action{value:value.clone()},
  m::Kind::Ingredient{relationship,manifest_ref}=>candidate::AssertionKind::Ingredient{relationship:relationship.clone(),manifest_ref:manifest_ref.clone()},
  m::Kind::Metadata{asset_id,mime,hash}=>candidate::AssertionKind::Metadata{asset_id:asset_id.clone(),mime:mime.clone(),hash:hash.clone()},
  m::Kind::HashedUri{url,hash,target_label}=>candidate::AssertionKind::HashedUri{url:url.clone(),hash:hash.clone(),target_label:target_label.clone()},
 }
}
pub fn manifest_out(x:&m::Manifest)->candidate::UpdateManifest{
 candidate::UpdateManifest{claim_id:x.claim_id.clone(),asset_id:x.asset_id.clone(),
  assertions:x.assertions.iter().map(|a|candidate::Assertion{id:a.id.clone(),label:a.label.clone(),kind:kind_out(&a.kind)}).collect()}
}
pub fn edit_out(e:&m::Edit)->candidate::Edit{
 match e{
  m::Edit::Delete{target}=>candidate::Edit::Delete{target:target.clone()},
  m::Edit::ReplaceLabel{target,label}=>candidate::Edit::ReplaceLabel{target:target.clone(),label:label.clone()},
  m::Edit::ReplaceAction{target,value}=>candidate::Edit::ReplaceAction{target:target.clone(),value:value.clone()},
  m::Edit::ReplaceRelationship{target,relationship}=>candidate::Edit::ReplaceRelationship{target:target.clone(),relationship:relationship.clone()},
  m::Edit::ReplaceHashedUri{target,url,hash,target_label}=>candidate::Edit::ReplaceHashedUri{target:target.clone(),url:url.clone(),hash:hash.clone(),target_label:target_label.clone()},
 }
}
pub fn input_out(i:&m::Input)->candidate::RepairInput{
 candidate::RepairInput{asset:asset_out(&i.asset),prior_store:prior_out(&i.prior),malformed:manifest_out(&i.malformed),legal_edits:i.legal.iter().map(edit_out).collect()}
}

// ------------------------------------------------------- candidate -> verifier
pub fn kind_in(k:&candidate::AssertionKind)->m::Kind{
 match k{
  candidate::AssertionKind::Action{value}=>m::Kind::Action{value:value.clone()},
  candidate::AssertionKind::Ingredient{relationship,manifest_ref}=>m::Kind::Ingredient{relationship:relationship.clone(),manifest_ref:manifest_ref.clone()},
  candidate::AssertionKind::Metadata{asset_id,mime,hash}=>m::Kind::Metadata{asset_id:asset_id.clone(),mime:mime.clone(),hash:hash.clone()},
  candidate::AssertionKind::HashedUri{url,hash,target_label}=>m::Kind::HashedUri{url:url.clone(),hash:hash.clone(),target_label:target_label.clone()},
 }
}
pub fn manifest_in(x:&candidate::UpdateManifest)->m::Manifest{
 m::Manifest{claim_id:x.claim_id.clone(),asset_id:x.asset_id.clone(),
  assertions:x.assertions.iter().map(|a|m::Assertion{id:a.id.clone(),label:a.label.clone(),kind:kind_in(&a.kind)}).collect()}
}
pub fn edit_in(e:&candidate::Edit)->m::Edit{
 match e{
  candidate::Edit::Delete{target}=>m::Edit::Delete{target:target.clone()},
  candidate::Edit::ReplaceLabel{target,label}=>m::Edit::ReplaceLabel{target:target.clone(),label:label.clone()},
  candidate::Edit::ReplaceAction{target,value}=>m::Edit::ReplaceAction{target:target.clone(),value:value.clone()},
  candidate::Edit::ReplaceRelationship{target,relationship}=>m::Edit::ReplaceRelationship{target:target.clone(),relationship:relationship.clone()},
  candidate::Edit::ReplaceHashedUri{target,url,hash,target_label}=>m::Edit::ReplaceHashedUri{target:target.clone(),url:url.clone(),hash:hash.clone(),target_label:target_label.clone()},
 }
}
pub fn edges_in(v:&[candidate::ReferenceEdge])->Vec<m::Edge>{v.iter().map(|e|m::Edge{from_id:e.from_id.clone(),to:e.to.clone()}).collect()}

// ------------------------------------------------------------- the three calls
pub fn apply(manifest:&m::Manifest,edits:&[m::Edit])->m::Value{
 let e:Vec<candidate::Edit>=edits.iter().map(edit_out).collect();
 match candidate::apply_edits(&manifest_out(manifest),&e){
  Ok(x)=>m::Value::Manifest(manifest_in(&x)),
  Err(msg)=>m::Value::Err(msg),
 }
}
pub fn validate(asset:&m::Asset,prior:&[m::PriorManifest],manifest:&m::Manifest)->m::Value{
 match candidate::validate(&asset_out(asset),&prior_out(prior),&manifest_out(manifest)){
  Ok(s)=>m::Value::Edges(edges_in(&s.edges)),
  Err(msg)=>m::Value::Err(msg),
 }
}
pub fn repair(input:&m::Input)->m::Value{
 match candidate::repair(&input_out(input)){
  Ok(r)=>m::Value::Repair{edits:r.edits.iter().map(edit_in).collect(),manifest:manifest_in(&r.manifest),edges:edges_in(&r.edges)},
  Err(msg)=>m::Value::Err(msg),
 }
}
