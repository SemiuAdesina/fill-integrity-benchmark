//! Placeholder only. The verifier overwrites this file with the single
//! allowlisted candidate implementation (`/app/src/lib.rs`) before building, and
//! aborts if that copy did not take effect, so this body is never graded.
use std::collections::BTreeMap;
#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord)]pub struct PbrObservation{pub frequency_khz:i64,pub path_id:String,pub measured_phase_md:i64}
#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord)]pub struct RttObservation{pub path_id:String,pub path_delay_ps:i64,pub measured_rtt_ps:i64}
#[derive(Clone,Debug,PartialEq,Eq)]pub struct Problem{pub distance_candidates_mm:Vec<i64>,pub clock_bias_candidates_ps:Vec<i64>,pub phase_bias_candidates_md:BTreeMap<String,Vec<i64>>,pub pbr:Vec<PbrObservation>,pub rtt:Vec<RttObservation>}
#[derive(Clone,Debug,PartialEq,Eq)]pub struct Calibration{pub distance_mm:i64,pub clock_bias_ps:i64,pub phase_biases_md:Vec<(String,i64)>,pub pbr_predictions_md:Vec<i64>,pub rtt_predictions_ps:Vec<i64>}
#[derive(Clone,Debug,PartialEq,Eq)]pub enum Error{EmptyGrid,MissingPath(String),NegativeDistance,ArithmeticOverflow}
pub fn wrap_phase_md(_v:i128)->Result<i64,Error>{Err(Error::ArithmeticOverflow)}
pub fn predict_phase_md(_f:i64,_d:i64,_b:i64)->Result<i64,Error>{Err(Error::ArithmeticOverflow)}
pub fn predict_rtt_ps(_d:i64,_c:i64,_p:i64)->Result<i64,Error>{Err(Error::ArithmeticOverflow)}
pub fn recover(_p:&Problem)->Result<Calibration,Error>{Err(Error::EmptyGrid)}
