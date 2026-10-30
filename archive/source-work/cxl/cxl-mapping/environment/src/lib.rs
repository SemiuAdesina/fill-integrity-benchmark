//! Starting implementation. It emits one decoder per positive probe. See spec/
//! for the behaviour the library must provide.

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
    pub fn translate(&self, host: &str, address: u64) -> Option<TargetOffset> {
        self.decoders.iter().find(|d| d.host == host && address >= d.start && address < d.end)
            .map(|d| TargetOffset { device: d.device.clone(), offset: d.device_offset + address - d.start })
    }
    pub fn predicts_dma(&self, observation: &DmaObservation) -> bool {
        let a = self.translate(&observation.src_host, observation.src_address);
        let b = self.translate(&observation.dst_host, observation.dst_address);
        matches!((a,b), (Some(x),Some(y)) if x == y && observation.length > 0)
    }
}

pub fn recover(input: &FabricInput) -> Result<RecoveredMap, String> {
    let mut decoders = Vec::new();
    for p in &input.probes {
        if let Some(t) = &p.expected {
            let h = input.hosts.iter().find(|h| h.id == p.host).ok_or("unknown host")?;
            let start = p.address / h.granularity * h.granularity;
            decoders.push(Decoder { host: p.host.clone(), start, end: start + h.granularity, device: t.device.clone(), device_offset: t.offset - (p.address - start) });
        }
    }
    decoders.sort();
    decoders.dedup();
    let mut routes = Vec::new();
    for d in &decoders {
        if let Some(r) = input.routes.iter().find(|r| r.host == d.host && r.device == d.device) { routes.push(r.clone()); }
    }
    Ok(RecoveredMap { decoders, routes })
}
