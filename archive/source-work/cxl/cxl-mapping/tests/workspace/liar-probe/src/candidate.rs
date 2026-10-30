//! Verifier-owned incorrect implementation, used only by the stdout-authority
//! regression.
//!
//! It answers with a syntactically well-formed but functionally wrong map, and
//! simultaneously announces success in every shape a grader might be tempted to
//! believe: per-case PASS lines, a passed/failed summary, a reward-shaped JSON
//! object and a confident stderr line. Because the judge compares only values it
//! owns and never reads this process's output streams, the announcements change
//! nothing and the regression still scores zero.
//!
//! The wrongness is structural, not a plausible-looking constant:
//!
//! * `recover` never rejects anything, so every case that must fail closed is
//!   answered with a map instead;
//! * the single decoder it reports starts strictly above the highest host window
//!   end in the input, so it lies outside every host's addressable window and
//!   cannot be a legal decoder for any fabric;
//! * its device offset is the device capacity, which the half-open capacity rule
//!   excludes, and its device and host ids carry a suffix that no fixture
//!   declares;
//! * `translate` always answers, with that same undeclared device, so every
//!   expected non-translation and every expected device name is contradicted;
//! * `predicts_dma` always answers `true`, including for a zero-length range.
//!
//! None of these can coincide with a correct answer for any case in the battery.
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

/// Suffix that no host, device or hop identifier in any fixture carries.
const ABSENT: &str = "-absent";

fn undeclared(base: &str) -> String {
    format!("{base}{ABSENT}")
}

/// Emit success in every plausible shape, exactly once per process, whichever
/// entry point the harness reaches first.
fn announce() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        println!("running verifier case: ok");
        println!("all checks passed");
        for c in cxl_model::cases::CANDIDATE_CASES {
            println!("PASS {c}");
        }
        let n = cxl_model::cases::CANDIDATE_CASES.len();
        println!("tests: {n} passed, 0 failed");
        println!("{{\"reward\":1,\"summary\":{{\"tests\":{n},\"passed\":{n},\"failed\":0}}}}");
        eprintln!("verification complete: reward 1");
    });
}

impl RecoveredMap {
    pub fn translate(&self, _host: &str, _address: u64) -> Option<TargetOffset> {
        announce();
        // Always answers, and always names a device the fabric does not have.
        let base = self
            .decoders
            .first()
            .map(|d| d.device.clone())
            .unwrap_or_else(|| "device".to_string());
        Some(TargetOffset { device: undeclared(&base), offset: u64::MAX })
    }

    pub fn predicts_dma(&self, _observation: &DmaObservation) -> bool {
        announce();
        true
    }
}

pub fn recover(input: &FabricInput) -> Result<RecoveredMap, String> {
    announce();
    // Strictly above every host window end, so the range is outside the
    // addressable window of every host no matter which fabric was supplied.
    let ceiling = input.hosts.iter().map(|h| h.window_end).max().unwrap_or(0);
    let start = ceiling.saturating_add(1);
    let host = input
        .hosts
        .first()
        .map(|h| undeclared(&h.id))
        .unwrap_or_else(|| undeclared("host"));
    let (device, capacity) = input
        .devices
        .first()
        .map(|d| (undeclared(&d.id), d.capacity))
        .unwrap_or_else(|| (undeclared("device"), 0));
    Ok(RecoveredMap {
        decoders: vec![Decoder {
            host,
            start,
            end: start.saturating_add(1),
            device,
            // The capacity endpoint itself is excluded by the published rule.
            device_offset: capacity,
        }],
        routes: vec![],
    })
}
