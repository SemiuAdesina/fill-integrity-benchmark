//! Verifier-owned frozen model for the CXL fabric mapping task.
//!
//! This crate is linked by every verifier binary and never contains candidate
//! code. It holds two things and nothing else:
//!
//! * `cases` — the *inputs* of each named case, because the probe has to
//!   receive them in order to run the candidate at all;
//! * `record` — the codec for the raw functional values a probe observed.
//!
//! The types here are the verifier's own copies of the published data contract.
//! They are deliberately independent of whatever the candidate file declares:
//! the probe translates between the two, so a candidate can never hand the
//! judge a value of a type it controls.
pub mod cases;
pub mod record;

/// One host memory window, as published in `src/lib.rs`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Host {
    pub id: String,
    pub window_start: u64,
    pub window_end: u64,
    pub granularity: u64,
}

/// One CXL memory device with a byte capacity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryDevice {
    pub id: String,
    pub capacity: u64,
}

/// A device-relative target, as observed or expected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetOffset {
    pub device: String,
    pub offset: u64,
}

/// One address observation. `expected: None` is authoritative negative
/// evidence: that host address must not decode at all.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddressProbe {
    pub host: String,
    pub address: u64,
    pub expected: Option<TargetOffset>,
}

/// One cross-host DMA observation over a half-open byte range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DmaObservation {
    pub src_host: String,
    pub src_address: u64,
    pub dst_host: String,
    pub dst_address: u64,
    pub length: u64,
    pub coherent: bool,
}

/// One legal fabric route from a host to a device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Route {
    pub host: String,
    pub device: String,
    pub hops: Vec<String>,
    pub cost: u64,
}

/// One recovered half-open decoder range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decoder {
    pub host: String,
    pub start: u64,
    pub end: u64,
    pub device: String,
    pub device_offset: u64,
}

/// A complete recovery input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fabric {
    pub hosts: Vec<Host>,
    pub devices: Vec<MemoryDevice>,
    pub probes: Vec<AddressProbe>,
    pub dma: Vec<DmaObservation>,
    pub routes: Vec<Route>,
}

/// A `translate` interrogation the probe performs on the candidate's map.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query {
    pub host: String,
    pub address: u64,
}

/// How a case obtains the map it interrogates.
///
/// `Recover` calls the candidate's `recover`. `Direct` builds the candidate's
/// `RecoveredMap` from verifier-owned decoders and interrogates it without any
/// recovery step, exactly as the corresponding V1 cases did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Setup {
    Recover(Fabric),
    Direct(Vec<Decoder>),
}

/// The complete verifier-owned input of one named case: how to obtain the map
/// and which interrogations to perform. No expected value appears here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Case {
    pub setup: Setup,
    pub translate: Vec<Query>,
    pub dma: Vec<DmaObservation>,
}

/// The raw functional values a probe observed from one case.
///
/// Note what is absent: there is no status, verdict, marker, case name, count
/// or summary field anywhere in this structure, and therefore no slot in the
/// serialized form in which a candidate could assert that it passed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Outcome {
    pub decoders: Vec<Decoder>,
    pub routes: Vec<Route>,
    pub translate: Vec<Option<TargetOffset>>,
    pub dma: Vec<bool>,
}

/// What the candidate did, as observed. `Failed` carries the error string the
/// candidate returned, because one V1 case asserted its exact text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Observed {
    Failed(String),
    Recovered(Outcome),
}
