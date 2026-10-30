//! Starting implementation: it accepts the first one-edit repair it finds, and
//! otherwise applies every legal edit. See spec/ for the required behaviour.

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

impl Edit{pub fn key(&self)->String{match self{
    Edit::Delete{target}=>format!("0:delete:{target}"),
    Edit::ReplaceLabel{target,label}=>format!("1:label:{target}:{label}"),
    Edit::ReplaceAction{target,value}=>format!("2:action:{target}:{value}"),
    Edit::ReplaceRelationship{target,relationship}=>format!("3:relationship:{target}:{relationship}"),
    Edit::ReplaceHashedUri{target,url,hash,target_label}=>format!("4:hashed-uri:{target}:{url}:{hash}:{target_label}"),
}}}

pub fn apply_edits(manifest:&UpdateManifest,edits:&[Edit])->Result<UpdateManifest,String>{
    let mut out=manifest.clone();
    for e in edits{match e{
        Edit::Delete{target}=>{let n=out.assertions.len();out.assertions.retain(|a|a.id!=*target);if out.assertions.len()+1!=n{return Err("delete target missing".into())}},
        Edit::ReplaceLabel{target,label}=>out.assertions.iter_mut().find(|a|a.id==*target).ok_or("label target missing")?.label=label.clone(),
        Edit::ReplaceAction{target,value}=>match &mut out.assertions.iter_mut().find(|a|a.id==*target).ok_or("action target missing")?.kind{AssertionKind::Action{value:v}=>*v=value.clone(),_=>return Err("wrong action target kind".into())},
        Edit::ReplaceRelationship{target,relationship}=>match &mut out.assertions.iter_mut().find(|a|a.id==*target).ok_or("relationship target missing")?.kind{AssertionKind::Ingredient{relationship:r,..}=>*r=relationship.clone(),_=>return Err("wrong relationship target kind".into())},
        Edit::ReplaceHashedUri{target,url,hash,target_label}=>match &mut out.assertions.iter_mut().find(|a|a.id==*target).ok_or("URI target missing")?.kind{AssertionKind::HashedUri{url:u,hash:h,target_label:t}=>{*u=url.clone();*h=hash.clone();*t=target_label.clone()},_=>return Err("wrong URI target kind".into())},
    }}Ok(out)
}

pub fn validate(asset:&Asset,prior:&[PriorManifest],m:&UpdateManifest)->Result<ValidationSummary,String>{
    if m.asset_id!=asset.id{return Err("asset mismatch".into())}
    let actions=m.assertions.iter().filter(|a|matches!(&a.kind,AssertionKind::Action{value} if value=="edited")).count();
    let parents=m.assertions.iter().filter(|a|matches!(&a.kind,AssertionKind::Ingredient{relationship,..} if relationship=="parent")).count();
    if actions!=1||parents!=1{return Err("action or parent cardinality".into())}
    if !m.assertions.iter().any(|a|matches!(&a.kind,AssertionKind::Metadata{asset_id,mime,hash} if asset_id==&asset.id&&mime==&asset.mime&&hash==&asset.hash)){return Err("metadata mismatch".into())}
    if !m.assertions.iter().any(|a|matches!(&a.kind,AssertionKind::HashedUri{..})){return Err("missing hashed URI".into())}
    let _=prior;
    Ok(ValidationSummary{edges:vec![]})
}

pub fn repair(input:&RepairInput)->Result<Repair,String>{
    let mut legal=input.legal_edits.clone();legal.sort_by_key(Edit::key);
    for e in &legal{if let Ok(m)=apply_edits(&input.malformed,std::slice::from_ref(e)){if let Ok(s)=validate(&input.asset,&input.prior_store,&m){return Ok(Repair{edits:vec![e.clone()],manifest:m,edges:s.edges})}}}
    let m=apply_edits(&input.malformed,&legal)?;let s=validate(&input.asset,&input.prior_store,&m)?;
    Ok(Repair{edits:legal,manifest:m,edges:s.edges})
}
