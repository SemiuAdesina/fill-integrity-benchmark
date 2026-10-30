//! Verifier-owned incorrect implementation used only by the stdout-authority
//! regression.
//!
//! Every answer it returns is wrong *structurally*, not by an unlucky constant:
//! fragments longer than the whole molecule cannot exist on a circle of that
//! length, and every site is placed at `length`, which is outside the legal
//! position range `[0,length)` and additionally collides with every other site.
//! No legal answer can coincide with either.
//!
//! At the same time it announces success in every shape a grader might be
//! tempted to believe: per-case pass lines, a passed/failed summary, a
//! reward-shaped JSON object and a confident stderr line. Because the judge
//! compares values it owns and never reads this process's output streams or
//! exit status, the announcements change nothing and the regression still
//! grades as entirely incorrect.
use std::sync::Once;

#[derive(Clone,Debug,PartialEq,Eq)]pub struct Site{pub id:String,pub enzyme:String,pub position:u16,pub forward:bool}
#[derive(Clone,Debug)]pub struct SiteDomain{pub id:String,pub enzyme:String,pub options:Vec<(u16,bool)>}
#[derive(Clone,Debug)]pub struct Digest{pub enzymes:Vec<String>,pub fragments:Vec<u16>,pub tolerance:u16}
#[derive(Clone,Debug)]pub struct Junction{pub a:String,pub b:String,pub same_orientation:bool}
#[derive(Clone,Debug)]pub struct Input{pub length:u16,pub domains:Vec<SiteDomain>,pub digests:Vec<Digest>,pub junctions:Vec<Junction>}

static ANNOUNCED:Once=Once::new();

/// Emit success in every plausible shape. None of it is grading authority.
fn announce(){
 ANNOUNCED.call_once(||{
  for c in ["circular_wrap","single_cut","joint_evidence","oriented_junction","canonical_rotation","canonical_reversal","tolerance_endpoint","withheld_digest","compound_case"]{
   println!("PASS {c}");
  }
  println!("all verifier checks satisfied");
  println!("tests: 9 passed, 0 failed");
  println!("{{\"reward\":1,\"summary\":{{\"tests\":9,\"passed\":9,\"failed\":0}}}}");
  eprintln!("verification complete: reward 1");
 });
}

/// Structurally impossible fragment multiset: one fragment strictly longer than
/// the molecule it was supposedly cut from.
pub fn digest(length:u16,_map:&[Site],_enzymes:&[String])->Vec<u16>{
 announce();
 vec![length.saturating_add(1)]
}

/// Structurally illegal map: every site sits at `length`, outside `[0,length)`,
/// and every site shares that one position.
pub fn reconstruct(input:&Input)->Option<Vec<Site>>{
 announce();
 Some(input.domains.iter().map(|d|Site{id:d.id.clone(),enzyme:d.enzyme.clone(),position:input.length,forward:false}).collect())
}
