//! Regression probe for the stdout-authority property.
//!
//! The same harness as `probe`, differing only in the single `use` line that
//! selects the implementation crate: here it is a verifier-owned deliberately
//! incorrect implementation that also floods stdout and stderr with
//! success-looking text. The judge must still fail every case, which
//! demonstrates that emitted output carries no grading authority.
use plasmid_liar as api;
use plasmid_model::{cases,record,InputSpec};

fn to_input(s:&InputSpec)->api::Input{
 api::Input{
  length:s.length,
  domains:s.domains.iter().map(|d|api::SiteDomain{id:d.id.clone(),enzyme:d.enzyme.clone(),options:d.options.clone()}).collect(),
  digests:s.digests.iter().map(|d|api::Digest{enzymes:d.enzymes.clone(),fragments:d.fragments.clone(),tolerance:d.tolerance}).collect(),
  junctions:s.junctions.iter().map(|j|api::Junction{a:j.a.clone(),b:j.b.clone(),same_orientation:j.same_orientation}).collect(),
 }
}

/// Site identifiers and enzyme names must stay inside the record's character
/// set. A returned name outside it cannot be serialized unambiguously, so the
/// probe records nothing and the judge counts the case as failed.
fn serializable(id:&str,enzyme:&str)->bool{
 let ok=|s:&str|!s.is_empty()&&s.len()<=32&&s.chars().all(|c|c.is_ascii_alphanumeric()||c=='_'||c=='-');
 ok(id)&&ok(enzyme)
}

fn main(){
 let a:Vec<String>=std::env::args().collect();
 if a.len()!=3{std::process::exit(2)}
 let (case,out)=(a[1].as_str(),a[2].as_str());

 let body=if let Some((length,map,enzymes))=cases::direct(case){
  let m:Vec<api::Site>=map.iter().map(|s|api::Site{id:s.id.clone(),enzyme:s.enzyme.clone(),position:s.position,forward:s.forward}).collect();
  record::encode_fragments(&api::digest(length,&m,&enzymes))
 }else if let Some(spec)=cases::input(case){
  let input=to_input(&spec);
  match api::reconstruct(&input){
   None=>record::encode_no_map(),
   Some(sites)=>{
    if sites.iter().any(|s|!serializable(&s.id,&s.enzyme)){
     eprintln!("probe: returned site names are outside the record character set");
     std::process::exit(2)
    }
    let frags=cases::recorded_enzymes(case).map(|e|api::digest(spec.length,&sites,&e));
    let rows:Vec<record::SiteRec>=sites.iter().map(|s|record::SiteRec{position:s.position,id:s.id.clone(),enzyme:s.enzyme.clone(),forward:s.forward}).collect();
    record::encode_map(&rows,frags.as_deref())
   }
  }
 }else{
  std::process::exit(2)
 };

 if std::fs::write(out,body).is_err(){std::process::exit(2)}
}
