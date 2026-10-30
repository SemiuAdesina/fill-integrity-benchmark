//! Verifier-owned judge -- the sole grading authority.
//!
//! The judge never links candidate code and never runs in the same process as
//! it. It reads only the raw functional values a probe recorded, holds the
//! expected values itself, and decides correctness purely from its own
//! comparisons. Candidate stdout and stderr are not read here at all, and the
//! probe's exit status is not consulted, so nothing a graded process emits can
//! contribute to a pass. The record format has no status field, so a record
//! cannot claim success either.
mod expected;
use expected::{expect,Expect,MapChecks};
use plasmid_model::cases;
use plasmid_model::record::{decode,encode_fragments,encode_map,Record,Shape,SiteRec};
use std::fmt::Write as _;

fn read_record(dir:&str,id:&str,shape:Shape)->Result<Record,String>{
 let path=std::path::Path::new(dir).join(id);
 let meta=std::fs::symlink_metadata(&path).map_err(|_|"no result recorded".to_string())?;
 if meta.file_type().is_symlink(){return Err("result path is a symlink".into())}
 if !meta.file_type().is_file(){return Err("result path is not a regular file".into())}
 if meta.len()>4096{return Err("result is implausibly large".into())}
 let raw=std::fs::read_to_string(&path).map_err(|_|"result is not readable text".to_string())?;
 decode(&raw,shape)
}

/// Apply the map properties this case requires. Returns the first violation.
fn check_map(sites:&[SiteRec],c:&MapChecks)->Option<String>{
 if c.first_position_zero{
  match sites.first(){
   None=>return Some("recovered map is empty".into()),
   Some(s)if s.position!=0=>return Some("first site is not at canonical position zero".into()),
   _=>{}
  }
 }
 if c.min_position_zero&&sites.iter().map(|s|s.position).min()!=Some(0){
  return Some("no site sits at canonical position zero".into())
 }
 if let Some((a,b))=c.differing_orientation{
  let Some(sa)=sites.iter().find(|s|s.id==a) else{return Some(format!("site {a} absent from recovered map"))};
  let Some(sb)=sites.iter().find(|s|s.id==b) else{return Some(format!("site {b} absent from recovered map"))};
  if sa.forward==sb.forward{return Some(format!("junction sites {a}/{b} share an orientation"))}
 }
 if let Some(a)=c.forward_site{
  let Some(sa)=sites.iter().find(|s|s.id==a) else{return Some(format!("site {a} absent from recovered map"))};
  if !sa.forward{return Some(format!("site {a} is not forward in the canonical representative"))}
 }
 None
}

/// Grade one case from its recorded values against expectations the judge owns.
fn grade_case(dir:&str,id:&str,out:&mut String)->bool{
 let Some(e)=expect(id) else{let _=writeln!(out,"FAIL {id} unknown case");return false};
 let got=match read_record(dir,id,e.shape){Ok(v)=>v,Err(m)=>{let _=writeln!(out,"FAIL {id} {m}");return false}};
 let bad=|out:&mut String,m:&str|{let _=writeln!(out,"FAIL {id} {m}");false};
 match got{
  Record::Fragments(f)=>{
   match e.frags{
    Some(w)if f==w=>{let _=writeln!(out,"PASS {id}");true}
    Some(_)=>bad(out,"fragments"),
    None=>bad(out,"unexpected fragment record"),
   }
  }
  Record::NoMap=>bad(out,"no map recovered"),
  Record::Map{sites,frags}=>{
   if let Some(c)=&e.map{
    if let Some(m)=check_map(&sites,c){return bad(out,&m)}
   }
   match (e.frags,frags){
    (Some(w),Some(f))if f!=w=>return bad(out,"fragments"),
    (Some(_),None)=>return bad(out,"no digest recorded"),
    _=>{}
   }
   let _=writeln!(out,"PASS {id}");true
  }
 }
}

fn grade_all(dir:&str)->(String,usize,usize){
 let mut out=String::new();
 let mut bad=0;
 for id in cases::CASES{if !grade_case(dir,id,&mut out){bad+=1}}
 let total=cases::CASES.len();
 let _=writeln!(out,"TOTAL {total} FAILED {bad}");
 (out,total,bad)
}

/// Render the fully correct record for a case, from values the judge owns.
fn correct_record(e:&Expect)->String{
 match e.shape{
  Shape::Fragments=>encode_fragments(e.frags.unwrap_or(&[])),
  Shape::Map{digest}=>{
   let sites:Vec<SiteRec>=e.canonical_sites.iter().map(|&(position,id,enzyme,forward)|SiteRec{position,id:id.to_string(),enzyme:enzyme.to_string(),forward}).collect();
   encode_map(&sites,if digest{e.frags}else{None})
  }
 }
}

/// Required negative control, determined entirely inside the verifier.
///
/// The judge fabricates the correct records itself, perturbs exactly one
/// functional value, and then grades the staged set. No candidate code runs and
/// no candidate output is consulted. Success means the injected defect was
/// observed: exactly one case failed, and it was the perturbed one. Anything
/// else means the pipeline cannot see failure and must not award reward.
fn selftest(dir:&str)->i32{
 let target="compound_case";
 if std::fs::create_dir_all(dir).is_err(){eprintln!("control: cannot stage results");return 2}
 for id in cases::CASES{
  let Some(e)=expect(id) else{eprintln!("control: unknown case {id}");return 2};
  let mut body=correct_record(&e);
  if *id==target{
   // Perturb exactly one functional value: the first recorded fragment.
   let Some(w)=e.frags else{eprintln!("control: no fragment value to perturb");return 2};
   let Some(&first)=w.first() else{eprintln!("control: empty fragment multiset");return 2};
   let mut perturbed=w.to_vec();
   perturbed[0]=first.wrapping_add(1);
   let sites:Vec<SiteRec>=e.canonical_sites.iter().map(|&(position,id,enzyme,forward)|SiteRec{position,id:id.to_string(),enzyme:enzyme.to_string(),forward}).collect();
   body=encode_map(&sites,Some(&perturbed));
  }
  if std::fs::write(std::path::Path::new(dir).join(id),body).is_err(){eprintln!("control: cannot write staged result");return 2}
 }
 let (out,total,bad)=grade_all(dir);
 print!("{out}");
 let saw=out.contains(&format!("FAIL {target} fragments"));
 if total==cases::CASES.len()&&bad==1&&saw{println!("CONTROL ok: injected defect observed");0}
 else{println!("CONTROL compromised: injected defect not observed cleanly");1}
}

fn main(){
 let a:Vec<String>=std::env::args().collect();
 if a.len()==3&&a[1]=="--selftest"{std::process::exit(selftest(&a[2]))}
 if a.len()!=4||a[1]!="--grade"{eprintln!("usage: judge --grade <results-dir> <verdict-file> | judge --selftest <dir>");std::process::exit(2)}
 let (dir,verdict)=(&a[2],&a[3]);
 let (out,_total,bad)=grade_all(dir);
 print!("{out}");
 if std::fs::write(verdict,&out).is_err(){std::process::exit(2)}
 std::process::exit(if bad==0{0}else{1})
}
