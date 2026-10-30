//! Verifier-owned reference implementation of the published contract.
//!
//! Held only by the judge, which never links candidate code. It exists so the
//! expected values are *derived* rather than merely asserted from literals: for
//! every case the judge first checks that its own expectation accepts the
//! reference's observation, and it also uses the reference to stage the
//! known-correct record set for the negative control.
//!
//! This file lives inside the root-owned 0700 verifier tree and is deleted with
//! it; the account the probes run under can neither read it nor the judge
//! binary built from it.
use c2pa_model::{Assertion,Asset,Edge,Edit,Input,Kind,Manifest,PriorManifest,Plan,Value};
use std::collections::BTreeSet;

/// Canonical edit key, exactly the published `Edit::key` encoding.
pub fn key(e:&Edit)->String{
 match e{
  Edit::Delete{target}=>format!("0:delete:{target}"),
  Edit::ReplaceLabel{target,label}=>format!("1:label:{target}:{label}"),
  Edit::ReplaceAction{target,value}=>format!("2:action:{target}:{value}"),
  Edit::ReplaceRelationship{target,relationship}=>format!("3:relationship:{target}:{relationship}"),
  Edit::ReplaceHashedUri{target,url,hash,target_label}=>format!("4:hashed-uri:{target}:{url}:{hash}:{target_label}"),
 }
}

/// Apply an already canonical, unique edit sequence in exactly that order.
/// Deletes preserve the relative order of everything remaining; a later
/// operation targeting an already deleted assertion fails.
pub fn apply_edits(manifest:&Manifest,edits:&[Edit])->Result<Manifest,String>{
 let keys:Vec<String>=edits.iter().map(key).collect();
 if !keys.windows(2).all(|w|w[0]<w[1]){return Err("edits not in unique canonical order".into())}
 let mut out=manifest.clone();
 for e in edits{
  match e{
   Edit::Delete{target}=>{let pos=out.assertions.iter().position(|a|a.id==*target).ok_or("delete target missing")?;out.assertions.remove(pos);}
   Edit::ReplaceLabel{target,label}=>out.assertions.iter_mut().find(|a|a.id==*target).ok_or("label target missing")?.label=label.clone(),
   Edit::ReplaceAction{target,value}=>match &mut out.assertions.iter_mut().find(|a|a.id==*target).ok_or("action target missing")?.kind{Kind::Action{value:v}=>*v=value.clone(),_=>return Err("wrong action target kind".into())},
   Edit::ReplaceRelationship{target,relationship}=>match &mut out.assertions.iter_mut().find(|a|a.id==*target).ok_or("relationship target missing")?.kind{Kind::Ingredient{relationship:r,..}=>*r=relationship.clone(),_=>return Err("wrong relationship target kind".into())},
   Edit::ReplaceHashedUri{target,url,hash,target_label}=>match &mut out.assertions.iter_mut().find(|a|a.id==*target).ok_or("URI target missing")?.kind{Kind::HashedUri{url:u,hash:h,target_label:t}=>{*u=url.clone();*h=hash.clone();*t=target_label.clone()},_=>return Err("wrong URI target kind".into())},
  }
 }
 Ok(out)
}

/// The normative unsigned update validator, returning the derived reference
/// edges on success.
pub fn validate(asset:&Asset,prior:&[PriorManifest],m:&Manifest)->Result<Vec<Edge>,String>{
 if m.claim_id.is_empty()||m.asset_id!=asset.id{return Err("claim or asset mismatch".into())}
 let mut ids=BTreeSet::new();
 let mut labels=BTreeSet::new();
 for a in &m.assertions{if a.id.is_empty()||a.label.is_empty()||!ids.insert(a.id.clone())||!labels.insert(a.label.clone()){return Err("duplicate or empty id/label".into())}}
 let actions:Vec<_>=m.assertions.iter().filter_map(|a|if let Kind::Action{value}=&a.kind{Some(value)}else{None}).collect();
 if actions.len()!=1||actions[0]!="edited"{return Err("action cardinality/value".into())}
 let mut parent_edges=Vec::new();
 for a in &m.assertions{if let Kind::Ingredient{relationship,manifest_ref}=&a.kind{if relationship=="parent"{
  if !prior.iter().any(|p|p.claim_id==*manifest_ref&&p.asset_id==asset.id){return Err("parent absent from prior store".into())}
  parent_edges.push(Edge{from_id:a.id.clone(),to:manifest_ref.clone()});
 }}}
 if parent_edges.len()!=1{return Err("parent cardinality".into())}
 let metadata:Vec<_>=m.assertions.iter().filter_map(|a|if let Kind::Metadata{asset_id,mime,hash}=&a.kind{Some((asset_id,mime,hash))}else{None}).collect();
 if metadata.len()!=1||metadata[0].0!=&asset.id||metadata[0].1!=&asset.mime||metadata[0].2!=&asset.hash{return Err("metadata cardinality/value".into())}
 let mut edges=parent_edges;
 let mut uri_count=0;
 for (idx,a) in m.assertions.iter().enumerate(){if let Kind::HashedUri{url,hash,target_label}=&a.kind{
  uri_count+=1;
  if url!=&format!("asset://{}",asset.id)||hash!=&asset.hash{return Err("hashed URI value".into())}
  let matches:Vec<&Assertion>=m.assertions[..idx].iter().filter(|x|x.label==*target_label).collect();
  if matches.len()!=1{return Err("hashed URI target/order".into())}
  edges.push(Edge{from_id:a.id.clone(),to:matches[0].id.clone()});
 }}
 if uri_count==0{return Err("missing hashed URI".into())}
 edges.sort_by(|a,b|(&a.from_id,&a.to).cmp(&(&b.from_id,&b.to)));
 Ok(edges)
}

type Best=Option<(Vec<Edit>,Manifest,Vec<Edge>)>;

fn search(input:&Input,legal:&[Edit],at:usize,chosen:&mut Vec<Edit>,best:&mut Best){
 if best.as_ref().is_some_and(|b|chosen.len()>b.0.len()){return}
 if at==legal.len(){
  if let Ok(m)=apply_edits(&input.malformed,chosen){if let Ok(edges)=validate(&input.asset,&input.prior,&m){
   let keys:Vec<String>=chosen.iter().map(key).collect();
   let better=best.as_ref().is_none_or(|b|{let bk:Vec<String>=b.0.iter().map(key).collect();(chosen.len(),&keys)<(b.0.len(),&bk)});
   if better{*best=Some((chosen.clone(),m,edges))}
  }}
  return
 }
 search(input,legal,at+1,chosen,best);
 chosen.push(legal[at].clone());
 search(input,legal,at+1,chosen,best);
 chosen.pop();
}

/// Minimum-cardinality repair, ties broken by the lexicographically smallest
/// canonical key vector.
pub fn repair(input:&Input)->Result<(Vec<Edit>,Manifest,Vec<Edge>),String>{
 if input.legal.len()>20{return Err("legal edit universe exceeds bound".into())}
 let mut legal=input.legal.clone();
 legal.sort_by_key(key);
 let keys:Vec<String>=legal.iter().map(key).collect();
 if keys.windows(2).any(|w|w[0]==w[1]){return Err("duplicate legal edit".into())}
 let mut best=None;
 search(input,&legal,0,&mut Vec::new(),&mut best);
 best.ok_or_else(||"no legal repair".to_string())
}

fn value_of_repair(r:Result<(Vec<Edit>,Manifest,Vec<Edge>),String>)->Value{
 match r{Ok((edits,manifest,edges))=>Value::Repair{edits,manifest,edges},Err(m)=>Value::Err(m)}
}

/// The observations a correct implementation would have produced for a case.
pub fn observations(plan:&Plan)->Vec<Value>{
 match plan{
  Plan::Keys(edits)=>vec![Value::Keys(edits.iter().map(key).collect())],
  Plan::Apply{manifest,edits}=>vec![match apply_edits(manifest,edits){Ok(m)=>Value::Manifest(m),Err(e)=>Value::Err(e)}],
  Plan::Validate{asset,prior,manifest}=>vec![match validate(asset,prior,manifest){Ok(e)=>Value::Edges(e),Err(e)=>Value::Err(e)}],
  Plan::Repair(input)=>vec![value_of_repair(repair(input))],
  Plan::RepairThenValidate(input)=>{
   let first=value_of_repair(repair(input));
   match &first{
    Value::Repair{manifest,..}=>{
     let second=match validate(&input.asset,&input.prior,manifest){Ok(e)=>Value::Edges(e),Err(e)=>Value::Err(e)};
     vec![first,second]
    }
    _=>vec![first],
   }
  }
 }
}
