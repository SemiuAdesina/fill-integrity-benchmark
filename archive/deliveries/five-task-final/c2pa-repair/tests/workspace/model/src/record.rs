//! Raw functional-result codec.
//!
//! A record holds the values the candidate API returned and nothing else.
//! There is no status field, no pass/fail flag, no case name, no case count, no
//! marker and no nonce, so a record carries no authority of its own — the judge
//! alone decides whether the values it contains are the right ones. There is no
//! slot in which a candidate could assert success, so it cannot assert success
//! even if it tries.
//!
//! Every string is carried as lowercase hex of its UTF-8 bytes, which removes
//! any separator ambiguity and makes malformed input trivially detectable.
//! Parsing is strict: fixed block tags, fixed line counts per tag, fixed field
//! order, exact field keys, exact component arity, a bounded block count and a
//! required trailing newline. Anything else is a decode error and therefore a
//! graded failure, never a skip.
use crate::{Assertion,Edge,Edit,Kind,Manifest,Value};

/// Upper bound on the number of observations one case may record. No case
/// performs more than two candidate calls.
pub const MAX_BLOCKS:usize=2;
/// Upper bound on a record in bytes; the judge refuses anything larger.
pub const MAX_BYTES:u64=16384;

// ------------------------------------------------------------------ encoding
fn hx(s:&str)->String{let mut o=String::with_capacity(s.len()*2);for b in s.as_bytes(){o.push_str(&format!("{b:02x}"))}o}
fn join(v:Vec<String>)->String{v.join(";")}

fn enc_kind(k:&Kind)->String{
 match k{
  Kind::Action{value}=>format!("A:{}",hx(value)),
  Kind::Ingredient{relationship,manifest_ref}=>format!("I:{}:{}",hx(relationship),hx(manifest_ref)),
  Kind::Metadata{asset_id,mime,hash}=>format!("M:{}:{}:{}",hx(asset_id),hx(mime),hx(hash)),
  Kind::HashedUri{url,hash,target_label}=>format!("U:{}:{}:{}",hx(url),hx(hash),hx(target_label)),
 }
}
fn enc_assertions(v:&[Assertion])->String{join(v.iter().map(|a|format!("{}:{}:{}",hx(&a.id),hx(&a.label),enc_kind(&a.kind))).collect())}
fn enc_edges(v:&[Edge])->String{join(v.iter().map(|e|format!("{}:{}",hx(&e.from_id),hx(&e.to))).collect())}
fn enc_edit(e:&Edit)->String{
 match e{
  Edit::Delete{target}=>format!("D:{}",hx(target)),
  Edit::ReplaceLabel{target,label}=>format!("L:{}:{}",hx(target),hx(label)),
  Edit::ReplaceAction{target,value}=>format!("A:{}:{}",hx(target),hx(value)),
  Edit::ReplaceRelationship{target,relationship}=>format!("R:{}:{}",hx(target),hx(relationship)),
  Edit::ReplaceHashedUri{target,url,hash,target_label}=>format!("H:{}:{}:{}:{}",hx(target),hx(url),hx(hash),hx(target_label)),
 }
}
fn enc_edits(v:&[Edit])->String{join(v.iter().map(enc_edit).collect())}

fn enc_one(v:&Value,o:&mut String){
 match v{
  Value::Err(m)=>{o.push_str("ERR\n");o.push_str(&format!("msg={}\n",hx(m)))}
  Value::Keys(k)=>{o.push_str("KEYS\n");o.push_str(&format!("keys={}\n",join(k.iter().map(|s|hx(s)).collect())))}
  Value::Manifest(m)=>{o.push_str("MANIFEST\n");o.push_str(&format!("claim={}\nasset={}\nassertions={}\n",hx(&m.claim_id),hx(&m.asset_id),enc_assertions(&m.assertions)))}
  Value::Edges(e)=>{o.push_str("EDGES\n");o.push_str(&format!("edges={}\n",enc_edges(e)))}
  Value::Repair{edits,manifest,edges}=>{o.push_str("REPAIR\n");o.push_str(&format!("edits={}\nclaim={}\nasset={}\nassertions={}\nedges={}\n",enc_edits(edits),hx(&manifest.claim_id),hx(&manifest.asset_id),enc_assertions(&manifest.assertions),enc_edges(edges)))}
 }
}

/// Serialize the raw observations of one case.
pub fn encode(vs:&[Value])->String{let mut o=String::new();for v in vs{enc_one(v,&mut o)}o}

// ------------------------------------------------------------------ decoding
fn unhx(s:&str)->Result<String,String>{
 if s.len()%2!=0{return Err("hex field has odd length".into())}
 let b=s.as_bytes();
 let mut out=Vec::with_capacity(s.len()/2);
 let mut i=0;
 while i<b.len(){
  let d=|c:u8|->Result<u8,String>{match c{b'0'..=b'9'=>Ok(c-b'0'),b'a'..=b'f'=>Ok(c-b'a'+10),_=>Err("hex field has a non-hex digit".to_string())}};
  out.push(d(b[i])?*16+d(b[i+1])?);
  i+=2;
 }
 String::from_utf8(out).map_err(|_|"hex field is not UTF-8".to_string())
}
fn parts(s:&str)->Vec<&str>{if s.is_empty(){vec![]}else{s.split(';').collect()}}
fn field<'a>(lines:&[&'a str],i:usize,key:&str)->Result<&'a str,String>{
 let l=lines.get(i).ok_or_else(||format!("missing field {key}"))?;
 l.strip_prefix(key).and_then(|r|r.strip_prefix('=')).ok_or_else(||format!("expected field {key}, found {l:?}"))
}
fn dec_kind(f:&[&str])->Result<Kind,String>{
 match f{
  ["A",v]=>Ok(Kind::Action{value:unhx(v)?}),
  ["I",r,m]=>Ok(Kind::Ingredient{relationship:unhx(r)?,manifest_ref:unhx(m)?}),
  ["M",a,m,h]=>Ok(Kind::Metadata{asset_id:unhx(a)?,mime:unhx(m)?,hash:unhx(h)?}),
  ["U",u,h,t]=>Ok(Kind::HashedUri{url:unhx(u)?,hash:unhx(h)?,target_label:unhx(t)?}),
  _=>Err("assertion kind is not one of A/I/M/U with its exact arity".into()),
 }
}
fn dec_assertions(s:&str)->Result<Vec<Assertion>,String>{
 parts(s).into_iter().map(|p|{
  let f:Vec<&str>=p.split(':').collect();
  if f.len()<3{return Err("assertion needs id, label and kind".into())}
  Ok(Assertion{id:unhx(f[0])?,label:unhx(f[1])?,kind:dec_kind(&f[2..])?})
 }).collect()
}
fn dec_edges(s:&str)->Result<Vec<Edge>,String>{
 parts(s).into_iter().map(|p|{
  let f:Vec<&str>=p.split(':').collect();
  if f.len()!=2{return Err("edge needs exactly from and to".into())}
  Ok(Edge{from_id:unhx(f[0])?,to:unhx(f[1])?})
 }).collect()
}
fn dec_edits(s:&str)->Result<Vec<Edit>,String>{
 parts(s).into_iter().map(|p|{
  let f:Vec<&str>=p.split(':').collect();
  match f.as_slice(){
   ["D",t]=>Ok(Edit::Delete{target:unhx(t)?}),
   ["L",t,l]=>Ok(Edit::ReplaceLabel{target:unhx(t)?,label:unhx(l)?}),
   ["A",t,v]=>Ok(Edit::ReplaceAction{target:unhx(t)?,value:unhx(v)?}),
   ["R",t,r]=>Ok(Edit::ReplaceRelationship{target:unhx(t)?,relationship:unhx(r)?}),
   ["H",t,u,h,l]=>Ok(Edit::ReplaceHashedUri{target:unhx(t)?,url:unhx(u)?,hash:unhx(h)?,target_label:unhx(l)?}),
   _=>Err("edit is not one of D/L/A/R/H with its exact arity".into()),
  }
 }).collect()
}

/// Strictly decode a whole record into the observations it contains.
///
/// The record must consist only of complete, well-formed blocks, must end with
/// a newline, must contain no carriage returns, and must not exceed
/// [`MAX_BLOCKS`] blocks. The caller checks the number of observations it
/// expected for the case; a record that is short, long, reordered or padded is
/// a decode error.
pub fn decode(s:&str)->Result<Vec<Value>,String>{
 if s.is_empty(){return Err("record is empty".into())}
 if !s.ends_with('\n'){return Err("record does not end with a newline".into())}
 if s.contains('\r'){return Err("record contains a carriage return".into())}
 let lines:Vec<&str>=s[..s.len()-1].split('\n').collect();
 let mut out=Vec::new();
 let mut i=0;
 while i<lines.len(){
  if out.len()==MAX_BLOCKS{return Err("record holds more observations than any case makes".into())}
  let rest=&lines[i+1..];
  let (v,used)=match lines[i]{
   "ERR"=>(Value::Err(unhx(field(rest,0,"msg")?)?),1),
   "KEYS"=>{
    let k:Result<Vec<String>,String>=parts(field(rest,0,"keys")?).into_iter().map(unhx).collect();
    (Value::Keys(k?),1)
   }
   "MANIFEST"=>(Value::Manifest(Manifest{
     claim_id:unhx(field(rest,0,"claim")?)?,
     asset_id:unhx(field(rest,1,"asset")?)?,
     assertions:dec_assertions(field(rest,2,"assertions")?)?,
    }),3),
   "EDGES"=>(Value::Edges(dec_edges(field(rest,0,"edges")?)?),1),
   "REPAIR"=>(Value::Repair{
     edits:dec_edits(field(rest,0,"edits")?)?,
     manifest:Manifest{
      claim_id:unhx(field(rest,1,"claim")?)?,
      asset_id:unhx(field(rest,2,"asset")?)?,
      assertions:dec_assertions(field(rest,3,"assertions")?)?,
     },
     edges:dec_edges(field(rest,4,"edges")?)?,
    },5),
   other=>return Err(format!("unknown record block {other:?}")),
  };
  out.push(v);
  i+=1+used;
 }
 if out.is_empty(){return Err("record holds no observation".into())}
 Ok(out)
}
