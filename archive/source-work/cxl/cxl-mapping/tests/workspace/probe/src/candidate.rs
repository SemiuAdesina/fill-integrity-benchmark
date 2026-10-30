//! Placeholder only. The verifier overwrites this file with the single
//! allowlisted candidate implementation (`/app/src/lib.rs`) before building, and
//! aborts if that copy did not take effect, so this body is never graded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host { pub id: String, pub window_start: u64, pub window_end: u64, pub granularity: u64 }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryDevice { pub id: String, pub capacity: u64 }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetOffset { pub device: String, pub offset: u64 }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddressProbe { pub host: String, pub address: u64, pub expected: Option<TargetOffset> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DmaObservation { pub src_host: String, pub src_address: u64, pub dst_host: String, pub dst_address: u64, pub length: u64, pub coherent: bool }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route { pub host: String, pub device: String, pub hops: Vec<String>, pub cost: u64 }
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Decoder { pub host: String, pub start: u64, pub end: u64, pub device: String, pub device_offset: u64 }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveredMap { pub decoders: Vec<Decoder>, pub routes: Vec<Route> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FabricInput { pub hosts: Vec<Host>, pub devices: Vec<MemoryDevice>, pub probes: Vec<AddressProbe>, pub dma: Vec<DmaObservation>, pub routes: Vec<Route> }

impl RecoveredMap {
    pub fn translate(&self, _host: &str, _address: u64) -> Option<TargetOffset> { None }
    pub fn predicts_dma(&self, _observation: &DmaObservation) -> bool { false }
}

pub fn recover(_input: &FabricInput) -> Result<RecoveredMap, String> { Err("placeholder".into()) }
