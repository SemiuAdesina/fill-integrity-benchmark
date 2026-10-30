//! Raw functional-result codec.
//!
//! A record can express only the values the candidate API actually returned:
//! a fragment multiset, or a reconstructed map plus the fragment multiset that
//! was derived from it. There is deliberately no status field, no pass or fail
//! flag, no case count, no marker and no nonce, so a record has no slot in
//! which a candidate could assert that it succeeded. Decoding is strict --
//! fixed field order, exact field set, bounded lengths, restricted character
//! set -- and any deviation is a decode error, which the judge counts as a
//! failure.

/// Maximum sites a decoded map may contain.
const MAX_SITES:usize=64;
/// Maximum fragments a decoded multiset may contain.
const MAX_FRAGS:usize=64;
/// Maximum characters in a site id or enzyme name.
const MAX_NAME:usize=32;

/// One placed site, exactly as the candidate returned it.
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct SiteRec{pub position:u16,pub id:String,pub enzyme:String,pub forward:bool}

/// The shape the judge requires of a given case's record. The judge derives it
/// from the verifier-owned case table, never from the record itself, so the
/// candidate cannot choose which shape it is graded against.
#[derive(Clone,Copy,Debug)]
pub enum Shape{Fragments,Map{digest:bool}}

/// A decoded record.
#[derive(Clone,Debug)]
pub enum Record{Fragments(Vec<u16>),NoMap,Map{sites:Vec<SiteRec>,frags:Option<Vec<u16>>}}

fn csv(v:&[u16])->String{v.iter().map(|x|x.to_string()).collect::<Vec<_>>().join(",")}

/// Serialize the raw fragment multiset a direct digest returned.
pub fn encode_fragments(f:&[u16])->String{format!("FRAGMENTS\nfrags={}\n",csv(f))}

/// Serialize a `reconstruct` result of `None`.
pub fn encode_no_map()->String{"NONE\n".to_string()}

/// Serialize a reconstructed map in the exact order the candidate returned it,
/// optionally followed by the fragment multiset derived from that map.
pub fn encode_map(sites:&[SiteRec],frags:Option<&[u16]>)->String{
 let joined=sites.iter().map(|s|format!("{}:{}:{}:{}",s.position,s.id,s.enzyme,if s.forward{'F'}else{'R'})).collect::<Vec<_>>().join("|");
 match frags{
  None=>format!("MAP\nsites={joined}\n"),
  Some(f)=>format!("MAP\nsites={joined}\nfrags={}\n",csv(f)),
 }
}

fn frag_list(s:&str)->Result<Vec<u16>,String>{
 if s.is_empty(){return Ok(vec![])}
 let parts:Vec<&str>=s.split(',').collect();
 if parts.len()>MAX_FRAGS{return Err("too many fragments".into())}
 parts.iter().map(|x|x.parse::<u16>().map_err(|_|format!("bad fragment {x:?}"))).collect()
}

fn name(s:&str)->Result<String,String>{
 if s.is_empty()||s.len()>MAX_NAME{return Err("name length out of range".into())}
 if !s.chars().all(|c|c.is_ascii_alphanumeric()||c=='_'||c=='-'){return Err(format!("illegal characters in name {s:?}"))}
 Ok(s.to_string())
}

fn site_list(s:&str)->Result<Vec<SiteRec>,String>{
 if s.is_empty(){return Ok(vec![])}
 let parts:Vec<&str>=s.split('|').collect();
 if parts.len()>MAX_SITES{return Err("too many sites".into())}
 parts.iter().map(|tok|{
  let f:Vec<&str>=tok.split(':').collect();
  if f.len()!=4{return Err(format!("malformed site {tok:?}"))}
  let position=f[0].parse::<u16>().map_err(|_|format!("bad position {:?}",f[0]))?;
  let forward=match f[3]{"F"=>true,"R"=>false,_=>return Err(format!("bad orientation {:?}",f[3]))};
  Ok(SiteRec{position,id:name(f[1])?,enzyme:name(f[2])?,forward})
 }).collect()
}

fn field<'a>(lines:&[&'a str],i:usize,key:&str)->Result<&'a str,String>{
 let l=lines.get(i).ok_or_else(||format!("missing field {key}"))?;
 l.strip_prefix(key).and_then(|r|r.strip_prefix('=')).ok_or_else(||format!("expected field {key}, found {l:?}"))
}

/// Strictly decode a record against the shape the judge expects. Any extra,
/// missing, reordered or unexpected line is malformed.
pub fn decode(s:&str,shape:Shape)->Result<Record,String>{
 let lines:Vec<&str>=s.lines().collect();
 match (shape,lines.first()){
  (Shape::Fragments,Some(&"FRAGMENTS"))=>{
   if lines.len()!=2{return Err(format!("expected 2 lines, found {}",lines.len()))}
   Ok(Record::Fragments(frag_list(field(&lines,1,"frags")?)?))
  }
  (Shape::Map{..},Some(&"NONE"))=>{
   if lines.len()!=1{return Err("trailing content after NONE".into())}
   Ok(Record::NoMap)
  }
  (Shape::Map{digest},Some(&"MAP"))=>{
   let want=if digest{3}else{2};
   if lines.len()!=want{return Err(format!("expected {want} lines, found {}",lines.len()))}
   let sites=site_list(field(&lines,1,"sites")?)?;
   let frags=if digest{Some(frag_list(field(&lines,2,"frags")?)?)}else{None};
   Ok(Record::Map{sites,frags})
  }
  (Shape::Fragments,_)=>Err("record must begin with FRAGMENTS".into()),
  (Shape::Map{..},_)=>Err("record must begin with MAP or NONE".into()),
 }
}
