
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
        let mut found = None;
        for d in &self.decoders {
            if d.host == host && d.start <= address && address < d.end {
                if found.is_some() { return None; }
                found = Some(TargetOffset { device: d.device.clone(), offset: d.device_offset + address - d.start });
            }
        }
        found
    }
    pub fn predicts_dma(&self, o: &DmaObservation) -> bool {
        if o.length == 0 { return false; }
        let end_a = match o.src_address.checked_add(o.length - 1) { Some(v) => v, None => return false };
        let end_b = match o.dst_address.checked_add(o.length - 1) { Some(v) => v, None => return false };
        let a0 = self.translate(&o.src_host, o.src_address);
        let a1 = self.translate(&o.src_host, end_a);
        let b0 = self.translate(&o.dst_host, o.dst_address);
        let b1 = self.translate(&o.dst_host, end_b);
        matches!((a0,a1,b0,b1), (Some(x0),Some(x1),Some(y0),Some(y1)) if x0.device==x1.device && x0.device==y0.device && y0.device==y1.device && x0.offset==y0.offset && x1.offset==x0.offset+o.length-1 && y1.offset==y0.offset+o.length-1)
    }
}

fn validate_input(input: &FabricInput) -> Result<(), String> {
    if input.hosts.is_empty() || input.devices.is_empty() { return Err("empty fabric".into()); }
    for h in &input.hosts {
        if h.granularity == 0 || h.window_start >= h.window_end || h.window_start % h.granularity != 0 || h.window_end % h.granularity != 0 || (h.window_end-h.window_start)/h.granularity > 12 { return Err("invalid host window".into()); }
    }
    for p in &input.probes {
        let h = input.hosts.iter().find(|h| h.id == p.host).ok_or("unknown probe host")?;
        if p.address < h.window_start || p.address >= h.window_end { return Err("probe outside host window".into()); }
        if let Some(t) = &p.expected {
            let d = input.devices.iter().find(|d| d.id == t.device).ok_or("unknown probe device")?;
            if t.offset >= d.capacity { return Err("probe beyond capacity".into()); }
        }
    }
    Ok(())
}

fn candidates(input: &FabricInput) -> Vec<Decoder> {
    let mut out = Vec::new();
    for p in input.probes.iter().filter(|p| p.expected.is_some()) {
        let t = p.expected.as_ref().unwrap();
        let h = input.hosts.iter().find(|h| h.id == p.host).unwrap();
        let dev = input.devices.iter().find(|d| d.id == t.device).unwrap();
        let mut start = h.window_start;
        while start <= p.address {
            let displacement = p.address - start;
            if t.offset >= displacement {
                let base = t.offset - displacement;
                if base % h.granularity == 0 {
                    let mut end = start + h.granularity;
                    while end <= h.window_end {
                        if end > p.address && base.checked_add(end-start).is_some_and(|x| x <= dev.capacity) {
                            let d = Decoder { host: h.id.clone(), start, end, device: dev.id.clone(), device_offset: base };
                            let consistent = input.probes.iter().filter(|q| q.host == h.id).all(|q| {
                                let covered = d.start <= q.address && q.address < d.end;
                                if !covered { true } else {
                                    match &q.expected {
                                        None => false,
                                        Some(want) => want.device == d.device && want.offset == d.device_offset + q.address-d.start,
                                    }
                                }
                            });
                            if consistent { out.push(d); }
                        }
                        end += h.granularity;
                    }
                }
            }
            start += h.granularity;
        }
    }
    out.sort(); out.dedup(); out
}

fn valid_complete(input: &FabricInput, ds: &[Decoder]) -> bool {
    for pair in ds.windows(2) {
        if pair[0].host == pair[1].host && pair[1].start < pair[0].end { return false; }
        if pair[0].host == pair[1].host && pair[0].end == pair[1].start && pair[0].device == pair[1].device && pair[0].device_offset + (pair[0].end-pair[0].start) == pair[1].device_offset { return false; }
    }
    let map = RecoveredMap { decoders: ds.to_vec(), routes: vec![] };
    if !input.probes.iter().all(|p| map.translate(&p.host,p.address) == p.expected) { return false; }
    input.dma.iter().all(|o| map.predicts_dma(o) == o.coherent)
}

fn search(input: &FabricInput, cs: &[Decoder], at: usize, chosen: &mut Vec<Decoder>, best: &mut Option<Vec<Decoder>>) {
    if best.as_ref().is_some_and(|b| chosen.len() > b.len()) { return; }
    if at == cs.len() {
        let mut v=chosen.clone(); v.sort();
        if valid_complete(input,&v) && best.as_ref().is_none_or(|b| (v.len(),&v) < (b.len(),b)) { *best=Some(v); }
        return;
    }
    search(input,cs,at+1,chosen,best);
    chosen.push(cs[at].clone());
    search(input,cs,at+1,chosen,best);
    chosen.pop();
}

pub fn recover(input: &FabricInput) -> Result<RecoveredMap, String> {
    validate_input(input)?;
    let cs=candidates(input);
    if cs.len() > 28 { return Err("fixture exceeds bounded search".into()); }
    let mut best=None;
    search(input,&cs,0,&mut Vec::new(),&mut best);
    let decoders=best.ok_or("no coherent decoder map")?;
    let mut keys: Vec<(String,String)>=decoders.iter().map(|d|(d.host.clone(),d.device.clone())).collect();
    keys.sort(); keys.dedup();
    let mut routes=Vec::new();
    for (host,device) in keys {
        let mut legal: Vec<Route>=input.routes.iter().filter(|r| r.host==host && r.device==device).cloned().collect();
        legal.sort_by(|a,b| (a.cost,&a.hops).cmp(&(b.cost,&b.hops)));
        routes.push(legal.into_iter().next().ok_or("missing route")?);
    }
    routes.sort_by(|a,b| (&a.host,&a.device,&a.hops).cmp(&(&b.host,&b.device,&b.hops)));
    Ok(RecoveredMap { decoders, routes })
}
