use cxl_mapping::*;

fn main() {
    let input = FabricInput {
        hosts: vec![Host { id: "alpha".into(), window_start: 0x1000, window_end: 0x5000, granularity: 0x1000 }],
        devices: vec![MemoryDevice { id: "mem0".into(), capacity: 0x4000 }],
        probes: vec![AddressProbe { host: "alpha".into(), address: 0x1800, expected: Some(TargetOffset { device: "mem0".into(), offset: 0x800 }) }],
        dma: vec![],
        routes: vec![Route { host: "alpha".into(), device: "mem0".into(), hops: vec!["sw0".into()], cost: 2 }],
    };
    match recover(&input) {
        Ok(map) => println!("recovered {} decoder(s)", map.decoders.len()),
        Err(e) => eprintln!("recovery failed: {e}"),
    }
}
