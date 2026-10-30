//! Verifier-owned model of the published C2PA update-manifest data contract.
//!
//! The candidate file `/app/src/lib.rs` declares the *whole* contract — the
//! types as well as `Edit::key`, `apply_edits`, `validate` and `repair`. So the
//! verifier keeps its own independent copies of those shapes here. The probe
//! translates verifier values into candidate values before a call and candidate
//! values back into verifier values afterwards, which means the judge never
//! receives a value of a type the candidate controls.
//!
//! This crate holds inputs and the record codec only. It contains no repair,
//! validation or edit-application logic and no expected results, so it can be
//! linked both into the candidate-tainted `probe` and into the untainted
//! `judge` without giving either of them the other's privileges.
pub mod cases;
pub mod record;

#[derive(Clone,Debug,PartialEq,Eq)] pub struct Asset{pub id:String,pub mime:String,pub hash:String}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct PriorManifest{pub claim_id:String,pub asset_id:String}
#[derive(Clone,Debug,PartialEq,Eq)] pub enum Kind{
 Action{value:String},
 Ingredient{relationship:String,manifest_ref:String},
 Metadata{asset_id:String,mime:String,hash:String},
 HashedUri{url:String,hash:String,target_label:String},
}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct Assertion{pub id:String,pub label:String,pub kind:Kind}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct Manifest{pub claim_id:String,pub asset_id:String,pub assertions:Vec<Assertion>}
#[derive(Clone,Debug,PartialEq,Eq)] pub enum Edit{
 Delete{target:String},
 ReplaceLabel{target:String,label:String},
 ReplaceAction{target:String,value:String},
 ReplaceRelationship{target:String,relationship:String},
 ReplaceHashedUri{target:String,url:String,hash:String,target_label:String},
}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct Edge{pub from_id:String,pub to:String}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct Input{pub asset:Asset,pub prior:Vec<PriorManifest>,pub malformed:Manifest,pub legal:Vec<Edit>}

/// What a named case asks the candidate API to do. The plan carries inputs
/// only; it says nothing about what the answer should be.
#[derive(Clone,Debug)] pub enum Plan{
 /// Collect `Edit::key` for each supplied edit, in the supplied order.
 Keys(Vec<Edit>),
 /// `apply_edits(&manifest,&edits)`.
 Apply{manifest:Manifest,edits:Vec<Edit>},
 /// `validate(&asset,&prior,&manifest)`.
 Validate{asset:Asset,prior:Vec<PriorManifest>,manifest:Manifest},
 /// `repair(&input)`.
 Repair(Input),
 /// `repair(&input)`, then `validate` on the manifest it returned. Two raw
 /// observations, exactly as the V1 case body performed two calls.
 RepairThenValidate(Input),
}

/// A raw functional value observed from the candidate API.
///
/// There is deliberately no variant that means "passed", "failed", "ok" or
/// "n tests". `Err` is not a verdict: several cases require the candidate to
/// reject their input, so the error arm is itself a functional value the judge
/// compares against its own expectation.
#[derive(Clone,Debug,PartialEq,Eq)] pub enum Value{
 Err(String),
 Keys(Vec<String>),
 Manifest(Manifest),
 Edges(Vec<Edge>),
 Repair{edits:Vec<Edit>,manifest:Manifest,edges:Vec<Edge>},
}
