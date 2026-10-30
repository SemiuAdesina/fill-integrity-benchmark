#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd)]pub struct Rat{pub n:i64,pub d:i64}
#[derive(Clone,Debug)]pub struct Family{pub normal:[i64;3],pub offsets:Vec<Rat>}
#[derive(Clone,Debug)]pub struct Support{pub direction:[i64;3],pub value:Rat}
#[derive(Clone,Debug)]pub struct Input{pub families:Vec<Family>,pub supports:Vec<Support>}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct Plane{pub family:usize,pub normal:[i64;3],pub offset:Rat}
#[derive(Clone,Debug,Eq,PartialEq,Ord,PartialOrd)]pub struct Vertex{pub xyz:[Rat;3]}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct Face{pub plane:usize,pub vertices:Vec<usize>}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct Result{pub planes:Vec<Plane>,pub vertices:Vec<Vertex>,pub faces:Vec<Face>}
// Starting implementation: selects the first offset of each family and returns
// no vertices or faces. See spec/ for the behaviour the library must provide.
pub fn recover(i:&Input)->Option<Result>{let planes=i.families.iter().enumerate().filter_map(|(family,f)|f.offsets.first().map(|&offset|Plane{family,normal:f.normal,offset})).collect::<Vec<_>>();(!planes.is_empty()).then_some(Result{planes,vertices:vec![],faces:vec![]})}
