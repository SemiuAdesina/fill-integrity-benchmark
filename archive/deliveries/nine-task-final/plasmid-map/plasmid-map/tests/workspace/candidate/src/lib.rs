//! Placeholder only.
//!
//! The verifier overwrites this file with the single allowlisted candidate
//! implementation (`/app/src/lib.rs`) before building, and aborts if that copy
//! did not take effect, so this body is never graded. It exists so the
//! verifier-owned workspace is well formed and its lockfile is stable.
#[derive(Clone,Debug,PartialEq,Eq)]pub struct Site{pub id:String,pub enzyme:String,pub position:u16,pub forward:bool}
#[derive(Clone,Debug)]pub struct SiteDomain{pub id:String,pub enzyme:String,pub options:Vec<(u16,bool)>}
#[derive(Clone,Debug)]pub struct Digest{pub enzymes:Vec<String>,pub fragments:Vec<u16>,pub tolerance:u16}
#[derive(Clone,Debug)]pub struct Junction{pub a:String,pub b:String,pub same_orientation:bool}
#[derive(Clone,Debug)]pub struct Input{pub length:u16,pub domains:Vec<SiteDomain>,pub digests:Vec<Digest>,pub junctions:Vec<Junction>}
pub fn digest(_length:u16,_map:&[Site],_enzymes:&[String])->Vec<u16>{vec![]}
pub fn reconstruct(_input:&Input)->Option<Vec<Site>>{None}
