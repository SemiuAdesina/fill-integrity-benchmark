//! Verifier-owned case inputs, transcribed unchanged from the V1 harness.
//!
//! Only inputs live here. No expected fragment multiset, orientation, position
//! or canonical form appears in this file, so linking it into the
//! candidate-tainted probe discloses nothing the candidate could exploit.
use crate::{DigestSpec,InputSpec,JunctionSpec,SiteDomainSpec,SiteSpec};

/// The complete case population. The judge grades exactly these ids, and the
/// verifier entry point requires this many verdicts before any reward.
pub const CASES:&[&str]=&[
 "circular_wrap",
 "single_cut",
 "joint_evidence",
 "oriented_junction",
 "canonical_rotation",
 "canonical_reversal",
 "tolerance_endpoint",
 "withheld_digest",
 "compound_case",
];

fn s(v:&str)->String{v.to_string()}
fn es(v:&[&str])->Vec<String>{v.iter().map(|x|x.to_string()).collect()}
fn dom(id:&str,e:&str,o:&[(u16,bool)])->SiteDomainSpec{SiteDomainSpec{id:s(id),enzyme:s(e),options:o.to_vec()}}
fn dg(e:&[&str],f:&[u16],t:u16)->DigestSpec{DigestSpec{enzymes:es(e),fragments:f.to_vec(),tolerance:t}}
fn site(id:&str,e:&str,p:u16,f:bool)->SiteSpec{SiteSpec{id:s(id),enzyme:s(e),position:p,forward:f}}

/// The shared reconstruction fixture: repeated enzyme A blocks, a B block with
/// a coincident-position orientation choice, single and double digest evidence
/// and one oriented junction constraint.
fn base()->InputSpec{
 InputSpec{
  length:20,
  domains:vec![
   dom("a1","A",&[(1,false),(2,false)]),
   dom("a2","A",&[(11,true),(12,true)]),
   dom("b1","B",&[(6,false),(6,true),(7,false)]),
  ],
  digests:vec![dg(&["A"],&[10,10],0),dg(&["A","B"],&[5,5,10],0)],
  junctions:vec![JunctionSpec{a:s("a1"),b:s("b1"),same_orientation:false}],
 }
}

/// Inputs for a reconstruction case. Unknown ids return `None` so the probe
/// cannot be steered onto an undefined problem.
pub fn input(id:&str)->Option<InputSpec>{
 match id{
  "joint_evidence"|"oriented_junction"|"canonical_rotation"|"canonical_reversal"|"withheld_digest"=>Some(base()),
  // Boundary observation: the single-enzyme evidence sits exactly at tolerance.
  "tolerance_endpoint"=>{let mut i=base();i.digests[0]=dg(&["A"],&[9,11],1);Some(i)}
  // Compound: wrap, repeats, boundary noise, junction parity and normalization.
  "compound_case"=>{let mut i=base();i.digests[1]=dg(&["A","B"],&[4,6,10],1);Some(i)}
  _=>None,
 }
}

/// Inputs for a case that exercises the digest model on an already placed map.
/// Returns the plasmid length, the map, and the enzymes being digested.
pub fn direct(id:&str)->Option<(u16,Vec<SiteSpec>,Vec<String>)>{
 match id{
  // Two cuts on a circle: the final-to-first interval wraps.
  "circular_wrap"=>Some((20,vec![site("x","E",2,true),site("y","E",17,false)],es(&["E"]))),
  // One cut on a circle yields one fragment spanning the whole molecule.
  "single_cut"=>Some((20,vec![site("x","E",7,true)],es(&["E"]))),
  _=>None,
 }
}

/// Enzyme set whose digest a reconstruction case records after recovery.
/// `None` means the case records no digest at all, exactly as in V1.
pub fn recorded_enzymes(id:&str)->Option<Vec<String>>{
 match id{
  "joint_evidence"=>Some(es(&["A","B"])),
  "tolerance_endpoint"=>Some(es(&["A"])),
  "withheld_digest"=>Some(es(&["B"])),
  "compound_case"=>Some(es(&["A","B"])),
  _=>None,
 }
}
