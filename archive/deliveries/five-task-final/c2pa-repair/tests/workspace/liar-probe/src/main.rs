//! Regression probe for the stdout-authority property.
//!
//! The same harness as `probe`, but linked against a verifier-owned
//! deliberately incorrect implementation that also floods stdout and stderr
//! with success-looking text. The judge must still fail every case, which
//! demonstrates that emitted output carries no grading authority.
mod candidate;
mod translate;

use c2pa_model::{cases,record,Plan,Value};

fn observe(plan:&Plan)->Vec<Value>{
 match plan{
  Plan::Keys(edits)=>vec![Value::Keys(edits.iter().map(|e|candidate::Edit::key(&translate::edit_out(e))).collect())],
  Plan::Apply{manifest,edits}=>vec![translate::apply(manifest,edits)],
  Plan::Validate{asset,prior,manifest}=>vec![translate::validate(asset,prior,manifest)],
  Plan::Repair(input)=>vec![translate::repair(input)],
  Plan::RepairThenValidate(input)=>{
   let first=translate::repair(input);
   match &first{
    Value::Repair{manifest,..}=>{let second=translate::validate(&input.asset,&input.prior,manifest);vec![first,second]}
    _=>vec![first],
   }
  }
 }
}

fn main(){
 let a:Vec<String>=std::env::args().collect();
 if a.len()!=3{std::process::exit(2)}
 let Some(plan)=cases::plan(&a[1]) else{std::process::exit(2)};
 let observations=observe(&plan);
 if std::fs::write(&a[2],record::encode(&observations)).is_err(){std::process::exit(2)}
}
