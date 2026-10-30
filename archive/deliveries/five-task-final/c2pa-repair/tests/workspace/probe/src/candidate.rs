//! Placeholder only.
//!
//! The verifier overwrites this file with the single allowlisted candidate
//! implementation (`/app/src/lib.rs`) before building, and aborts the run if
//! that copy did not take effect, so this body is never graded. It exists so
//! the workspace is well formed on its own and declares the same published
//! contract the candidate must declare.

#[derive(Clone,Debug,PartialEq,Eq)] pub struct Asset{pub id:String,pub mime:String,pub hash:String}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct PriorManifest{pub claim_id:String,pub asset_id:String}
#[derive(Clone,Debug,PartialEq,Eq)] pub enum AssertionKind{
 Action{value:String}, Ingredient{relationship:String,manifest_ref:String},
 Metadata{asset_id:String,mime:String,hash:String}, HashedUri{url:String,hash:String,target_label:String},
}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct Assertion{pub id:String,pub label:String,pub kind:AssertionKind}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct UpdateManifest{pub claim_id:String,pub asset_id:String,pub assertions:Vec<Assertion>}
#[derive(Clone,Debug,PartialEq,Eq)] pub enum Edit{
 Delete{target:String}, ReplaceLabel{target:String,label:String}, ReplaceAction{target:String,value:String},
 ReplaceRelationship{target:String,relationship:String}, ReplaceHashedUri{target:String,url:String,hash:String,target_label:String},
}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct ReferenceEdge{pub from_id:String,pub to:String}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct ValidationSummary{pub edges:Vec<ReferenceEdge>}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct Repair{pub edits:Vec<Edit>,pub manifest:UpdateManifest,pub edges:Vec<ReferenceEdge>}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct RepairInput{pub asset:Asset,pub prior_store:Vec<PriorManifest>,pub malformed:UpdateManifest,pub legal_edits:Vec<Edit>}

impl Edit{pub fn key(&self)->String{String::new()}}
pub fn apply_edits(_m:&UpdateManifest,_e:&[Edit])->Result<UpdateManifest,String>{Err("placeholder".into())}
pub fn validate(_a:&Asset,_p:&[PriorManifest],_m:&UpdateManifest)->Result<ValidationSummary,String>{Err("placeholder".into())}
pub fn repair(_i:&RepairInput)->Result<Repair,String>{Err("placeholder".into())}
