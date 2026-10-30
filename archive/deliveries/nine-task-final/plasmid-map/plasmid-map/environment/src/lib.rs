#[derive(Clone,Debug,PartialEq,Eq)]pub struct Site{pub id:String,pub enzyme:String,pub position:u16,pub forward:bool}
#[derive(Clone,Debug)]pub struct SiteDomain{pub id:String,pub enzyme:String,pub options:Vec<(u16,bool)>}
#[derive(Clone,Debug)]pub struct Digest{pub enzymes:Vec<String>,pub fragments:Vec<u16>,pub tolerance:u16}
#[derive(Clone,Debug)]pub struct Junction{pub a:String,pub b:String,pub same_orientation:bool}
#[derive(Clone,Debug)]pub struct Input{pub length:u16,pub domains:Vec<SiteDomain>,pub digests:Vec<Digest>,pub junctions:Vec<Junction>}
pub fn digest(length:u16,map:&[Site],enzymes:&[String])->Vec<u16>{let mut p:Vec<u16>=map.iter().filter(|s|enzymes.contains(&s.enzyme)).map(|s|s.position).collect();p.sort();p.dedup();if p.is_empty(){return vec![]}if p.len()==1{return vec![length]}let mut out=Vec::new();for i in 0..p.len(){out.push(if i+1<p.len(){p[i+1]-p[i]}else{length-p[i]+p[0]})}out.sort();out}
// Broken baseline: chooses the first known-block option independently and does
// not optimize joint circular evidence or normalize reversal.
pub fn reconstruct(input:&Input)->Option<Vec<Site>>{if input.length==0{return None}let mut out=Vec::new();for d in &input.domains{let &(position,forward)=d.options.first()?;out.push(Site{id:d.id.clone(),enzyme:d.enzyme.clone(),position,forward});}out.sort_by_key(|s|(s.position,s.id.clone()));Some(out)}
