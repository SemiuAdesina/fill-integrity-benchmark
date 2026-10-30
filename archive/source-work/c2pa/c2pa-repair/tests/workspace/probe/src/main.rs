//! Verifier-owned probe.
//!
//! Links exactly one allowlisted candidate file (`/app/src/lib.rs`) as the
//! `candidate` module, performs one named case, and serializes the raw
//! functional values it observed to the path given on argv.
//!
//! The probe has no grading role. It asserts nothing, compares nothing, prints
//! no verdict, emits no marker, and its exit status is logged but never
//! consulted. Grading happens afterwards in the separate `judge` binary, which
//! never links candidate code. Anything this process writes to stdout or stderr
//! is diagnostics with zero authority.
//!
//! Inputs are translated from verifier-owned types into candidate types before
//! each call and translated back afterwards, so no value of a candidate-defined
//! type ever reaches the judge.
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
    // The V1 body called validate on the manifest repair returned. Record both
    // raw observations; the judge decides what they should have been.
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
