//! Verifier-owned judge — the sole grading authority.
//!
//! The judge never links candidate code and never shares a process with it. It
//! reads only the raw functional values the probe recorded, holds the expected
//! values itself, and decides correctness from its own comparisons. Candidate
//! stdout and stderr are not opened here at all, and the probe's exit status is
//! not consulted, so nothing the candidate process emits can contribute to a
//! pass. The case total it reports is its own count of the case population, not
//! a number read from any record.
mod expected;
mod reference;

use c2pa_model::{cases,record,Value};
use std::fmt::Write as _;

/// Read one recorded result strictly. A missing, oversized, irregular or
/// malformed record fails its case; it never shrinks the case population.
fn read_record(dir:&str,id:&str)->Result<Vec<Value>,String>{
 let path=std::path::Path::new(dir).join(id);
 let meta=std::fs::symlink_metadata(&path).map_err(|_|"no result recorded".to_string())?;
 if meta.file_type().is_symlink(){return Err("result path is a symlink".into())}
 if !meta.file_type().is_file(){return Err("result path is not a regular file".into())}
 if meta.len()>record::MAX_BYTES{return Err("result is implausibly large".into())}
 let raw=std::fs::read_to_string(&path).map_err(|_|"result is not readable text".to_string())?;
 record::decode(&raw)
}

fn grade_case(dir:&str,id:&str,out:&mut String)->bool{
 // The expectation must first accept the verifier's own reference observation.
 // No candidate code is involved in this step; it only prevents an expectation
 // from silently drifting away from the contract it is supposed to encode.
 let Some(plan)=cases::plan(id) else{let _=writeln!(out,"FAIL {id} unknown case");return false};
 if let Err(e)=expected::check(id,&reference::observations(&plan)){
  let _=writeln!(out,"FAIL {id} verifier expectation is inconsistent with the reference: {e}");
  return false
 }
 match read_record(dir,id).and_then(|obs|expected::check(id,&obs)){
  Ok(())=>{let _=writeln!(out,"PASS {id}");true}
  Err(e)=>{let _=writeln!(out,"FAIL {id} {e}");false}
 }
}

fn grade_all(dir:&str)->(String,usize){
 let mut out=String::new();
 let mut bad=0;
 for id in cases::CASES{if !grade_case(dir,id,&mut out){bad+=1}}
 let _=writeln!(out,"TOTAL {} FAILED {bad}",cases::CASES.len());
 (out,bad)
}

/// Required negative control, determined entirely inside the verifier.
///
/// The judge stages the full correct record set from its own reference, then
/// corrupts exactly one functional value: the second derived reference edge of
/// the compound case is pointed at an identifier built with a `#`, which never
/// occurs in any fixture id, so the corrupted value is structurally outside the
/// legal domain and cannot coincide with a correct answer. It then grades.
/// Success means the injected defect was observed: exactly one case failed and
/// it was the perturbed one. Anything else means the pipeline cannot see
/// failure and must not be trusted to award reward.
fn selftest(dir:&str)->i32{
 let target="compound_label_parent_hash_and_order_repair";
 if std::fs::create_dir_all(dir).is_err(){eprintln!("control: cannot stage results");return 2}
 for id in cases::CASES{
  let Some(plan)=cases::plan(id) else{eprintln!("control: unknown case {id}");return 2};
  let mut obs=reference::observations(&plan);
  if **id==*target{
   let Some(Value::Repair{edges,..})=obs.first_mut() else{eprintln!("control: staged compound case is not a repair");return 2};
   let Some(second)=edges.get_mut(1) else{eprintln!("control: staged compound case has no second edge");return 2};
   second.to=format!("{}#absent-assertion",second.to);
  }
  if std::fs::write(std::path::Path::new(dir).join(id),record::encode(&obs)).is_err(){eprintln!("control: cannot write staged result");return 2}
 }
 let (out,bad)=grade_all(dir);
 print!("{out}");
 let saw_target=out.lines().any(|l|l.starts_with(&format!("FAIL {target} ")));
 if bad==1&&saw_target{println!("CONTROL ok: injected defect observed");0}
 else{println!("CONTROL compromised: injected defect not observed cleanly");1}
}

fn main(){
 let a:Vec<String>=std::env::args().collect();
 if a.len()==3&&a[1]=="--selftest"{std::process::exit(selftest(&a[2]))}
 if a.len()!=4||a[1]!="--grade"{eprintln!("usage: judge --grade <results-dir> <verdict-file> | judge --selftest <dir>");std::process::exit(2)}
 let (dir,verdict)=(&a[2],&a[3]);
 let (out,bad)=grade_all(dir);
 print!("{out}");
 if std::fs::write(verdict,&out).is_err(){std::process::exit(2)}
 std::process::exit(if bad==0{0}else{1})
}
