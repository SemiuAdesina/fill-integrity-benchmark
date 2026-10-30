
use std::collections::BTreeSet;
#[derive(Clone,Debug,PartialEq,Eq)] pub struct Asset{pub id:String,pub mime:String,pub hash:String}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct PriorManifest{pub claim_id:String,pub asset_id:String}
#[derive(Clone,Debug,PartialEq,Eq)] pub enum AssertionKind{Action{value:String},Ingredient{relationship:String,manifest_ref:String},Metadata{asset_id:String,mime:String,hash:String},HashedUri{url:String,hash:String,target_label:String}}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct Assertion{pub id:String,pub label:String,pub kind:AssertionKind}
#[derive(Clone,Debug,PartialEq,Eq)] pub struct UpdateManifest{pub claim_id:String,pub asset_id:String,pub assertions:Vec<Assertion>}
#[derive(Clone,Debug,PartialEq,Eq)] pub enum Edit{Delete{target:String},ReplaceLabel{target:String,label:String},ReplaceAction{target:String,value:String},ReplaceRelationship{target:String,relationship:String},ReplaceHashedUri{target:String,url:String,hash:String,target_label:String}}
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
    let keys:Vec<String>=edits.iter().map(Edit::key).collect();
    if !keys.windows(2).all(|w|w[0]<w[1]){return Err("edits not in unique canonical order".into())}
    let mut out=manifest.clone();
    for e in edits{match e{
        Edit::Delete{target}=>{let pos=out.assertions.iter().position(|a|a.id==*target).ok_or("delete target missing")?;out.assertions.remove(pos);},
        Edit::ReplaceLabel{target,label}=>out.assertions.iter_mut().find(|a|a.id==*target).ok_or("label target missing")?.label=label.clone(),
        Edit::ReplaceAction{target,value}=>match &mut out.assertions.iter_mut().find(|a|a.id==*target).ok_or("action target missing")?.kind{AssertionKind::Action{value:v}=>*v=value.clone(),_=>return Err("wrong action target kind".into())},
        Edit::ReplaceRelationship{target,relationship}=>match &mut out.assertions.iter_mut().find(|a|a.id==*target).ok_or("relationship target missing")?.kind{AssertionKind::Ingredient{relationship:r,..}=>*r=relationship.clone(),_=>return Err("wrong relationship target kind".into())},
        Edit::ReplaceHashedUri{target,url,hash,target_label}=>match &mut out.assertions.iter_mut().find(|a|a.id==*target).ok_or("URI target missing")?.kind{AssertionKind::HashedUri{url:u,hash:h,target_label:t}=>{*u=url.clone();*h=hash.clone();*t=target_label.clone()},_=>return Err("wrong URI target kind".into())},
    }}
    Ok(out)
}

pub fn validate(asset:&Asset,prior:&[PriorManifest],m:&UpdateManifest)->Result<ValidationSummary,String>{
    if m.claim_id.is_empty()||m.asset_id!=asset.id{return Err("claim or asset mismatch".into())}
    let mut ids=BTreeSet::new();let mut labels=BTreeSet::new();
    for a in &m.assertions{if a.id.is_empty()||a.label.is_empty()||!ids.insert(a.id.clone())||!labels.insert(a.label.clone()){return Err("duplicate or empty id/label".into())}}
    let actions:Vec<_>=m.assertions.iter().filter_map(|a|if let AssertionKind::Action{value}=&a.kind{Some(value)}else{None}).collect();
    if actions.len()!=1||actions[0]!="edited"{return Err("action cardinality/value".into())}
    let mut parent_edges=Vec::new();
    for a in &m.assertions{if let AssertionKind::Ingredient{relationship,manifest_ref}=&a.kind{if relationship=="parent"{
        if !prior.iter().any(|p|p.claim_id==*manifest_ref&&p.asset_id==asset.id){return Err("parent absent from prior store".into())}
        parent_edges.push(ReferenceEdge{from_id:a.id.clone(),to:manifest_ref.clone()});
    }}}
    if parent_edges.len()!=1{return Err("parent cardinality".into())}
    let metadata:Vec<_>=m.assertions.iter().filter_map(|a|if let AssertionKind::Metadata{asset_id,mime,hash}=&a.kind{Some((asset_id,mime,hash))}else{None}).collect();
    if metadata.len()!=1||metadata[0].0!=&asset.id||metadata[0].1!=&asset.mime||metadata[0].2!=&asset.hash{return Err("metadata cardinality/value".into())}
    let mut edges=parent_edges;let mut uri_count=0;
    for (idx,a) in m.assertions.iter().enumerate(){if let AssertionKind::HashedUri{url,hash,target_label}=&a.kind{
        uri_count+=1;
        if url!=&format!("asset://{}",asset.id)||hash!=&asset.hash{return Err("hashed URI value".into())}
        let matches:Vec<_>=m.assertions[..idx].iter().filter(|x|x.label==*target_label).collect();
        if matches.len()!=1{return Err("hashed URI target/order".into())}
        edges.push(ReferenceEdge{from_id:a.id.clone(),to:matches[0].id.clone()});
    }}
    if uri_count==0{return Err("missing hashed URI".into())}
    edges.sort_by(|a,b|(&a.from_id,&a.to).cmp(&(&b.from_id,&b.to)));
    Ok(ValidationSummary{edges})
}

fn search(input:&RepairInput,legal:&[Edit],at:usize,chosen:&mut Vec<Edit>,best:&mut Option<(Vec<Edit>,UpdateManifest,Vec<ReferenceEdge>)>){
    if best.as_ref().is_some_and(|b|chosen.len()>b.0.len()){return}
    if at==legal.len(){
        if let Ok(m)=apply_edits(&input.malformed,chosen){if let Ok(s)=validate(&input.asset,&input.prior_store,&m){
            let keys:Vec<String>=chosen.iter().map(Edit::key).collect();
            let better=best.as_ref().is_none_or(|b|{let bk:Vec<String>=b.0.iter().map(Edit::key).collect();(chosen.len(),&keys)<(b.0.len(),&bk)});
            if better{*best=Some((chosen.clone(),m,s.edges))}
        }}return
    }
    search(input,legal,at+1,chosen,best);
    chosen.push(legal[at].clone());search(input,legal,at+1,chosen,best);chosen.pop();
}

pub fn repair(input:&RepairInput)->Result<Repair,String>{
    if input.legal_edits.len()>20{return Err("legal edit universe exceeds bound".into())}
    let mut legal=input.legal_edits.clone();legal.sort_by_key(Edit::key);
    let keys:Vec<String>=legal.iter().map(Edit::key).collect();
    if keys.windows(2).any(|w|w[0]==w[1]){return Err("duplicate legal edit".into())}
    let mut best=None;search(input,&legal,0,&mut Vec::new(),&mut best);
    best.map(|(edits,manifest,edges)|Repair{edits,manifest,edges}).ok_or("no legal repair".into())
}
