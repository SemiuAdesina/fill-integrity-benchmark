//! Verifier-owned frozen fixture model and raw-result codec.
//!
//! This crate carries no recovery logic and no expected answers. It holds the
//! case *inputs* (the probe has to receive them in order to run anything) and
//! the codec used to move raw functional values from the probe to the judge.
//! Because it contains no candidate code and no expectations it can safely be
//! linked into both the candidate-tainted probe and the untainted judge.
pub mod cases;
pub mod record;

/// One bounded known-block site domain, in verifier-owned neutral form.
#[derive(Clone,Debug)]
pub struct SiteDomainSpec{pub id:String,pub enzyme:String,pub options:Vec<(u16,bool)>}

/// One observed digest, in verifier-owned neutral form.
#[derive(Clone,Debug)]
pub struct DigestSpec{pub enzymes:Vec<String>,pub fragments:Vec<u16>,pub tolerance:u16}

/// One oriented junction observation, in verifier-owned neutral form.
#[derive(Clone,Debug)]
pub struct JunctionSpec{pub a:String,pub b:String,pub same_orientation:bool}

/// A complete reconstruction problem, in verifier-owned neutral form.
#[derive(Clone,Debug)]
pub struct InputSpec{pub length:u16,pub domains:Vec<SiteDomainSpec>,pub digests:Vec<DigestSpec>,pub junctions:Vec<JunctionSpec>}

/// A fully placed site used by the direct-digest cases.
#[derive(Clone,Debug)]
pub struct SiteSpec{pub id:String,pub enzyme:String,pub position:u16,pub forward:bool}
