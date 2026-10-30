//! Placeholder only. The verifier overwrites this file with the single
//! allowlisted candidate implementation (`/app/src/lib.rs`) before building, and
//! aborts if that copy did not take effect, so this body is never graded.
#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd)]pub struct Rat{pub n:i64,pub d:i64}
#[derive(Clone,Debug)]pub struct Family{pub normal:[i64;3],pub offsets:Vec<Rat>}
#[derive(Clone,Debug)]pub struct Support{pub direction:[i64;3],pub value:Rat}
#[derive(Clone,Debug)]pub struct Input{pub families:Vec<Family>,pub supports:Vec<Support>}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct Plane{pub family:usize,pub normal:[i64;3],pub offset:Rat}
#[derive(Clone,Debug,Eq,PartialEq,Ord,PartialOrd)]pub struct Vertex{pub xyz:[Rat;3]}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct Face{pub plane:usize,pub vertices:Vec<usize>}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct Result{pub planes:Vec<Plane>,pub vertices:Vec<Vertex>,pub faces:Vec<Face>}
pub fn recover(_i:&Input)->Option<Result>{None}
