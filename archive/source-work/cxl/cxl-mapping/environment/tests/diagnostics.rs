
//! Synthetic public checks for `spec/cxl-model.md`.

use cxl_mapping::{recover, AddressProbe, Decoder, DmaObservation, FabricInput, Host,
    MemoryDevice, RecoveredMap, Route, TargetOffset};

fn route(host: &str, device: &str, hops: &[&str], cost: u64) -> Route {
    Route { host: host.into(), device: device.into(),
        hops: hops.iter().map(|h| h.to_string()).collect(), cost }
}
fn target(device: &str, offset: u64) -> Option<TargetOffset> {
    Some(TargetOffset { device: device.into(), offset })
}

#[test]
fn translation_uses_half_open_decoder_endpoints() {
    let map = RecoveredMap { decoders: vec![Decoder {
        host: "h".into(), start: 16, end: 32, device: "m".into(), device_offset: 0,
    }], routes: vec![] };
    assert_eq!(map.translate("h", 16), target("m", 0));
    assert_eq!(map.translate("h", 31), target("m", 15));
    assert_eq!(map.translate("h", 32), None,
        "FAILED: decoder end is excluded from [start,end)");
}

#[test]
fn dma_checks_the_complete_half_open_span() {
    let map = RecoveredMap { decoders: vec![
        Decoder { host: "a".into(), start: 0, end: 16, device: "m".into(), device_offset: 0 },
        Decoder { host: "b".into(), start: 100, end: 116, device: "m".into(), device_offset: 0 },
    ], routes: vec![] };
    let mut o = DmaObservation { src_host: "a".into(), src_address: 0,
        dst_host: "b".into(), dst_address: 100, length: 16, coherent: true };
    assert!(map.predicts_dma(&o));
    o.length = 17;
    assert!(!map.predicts_dma(&o), "FAILED: a DMA range may not cross a decoder endpoint");
    o.length = 0;
    assert!(!map.predicts_dma(&o));
}

#[test]
fn negative_evidence_limits_and_coalesces_the_decoder() {
    let input = FabricInput {
        hosts: vec![Host { id: "h".into(), window_start: 0, window_end: 32, granularity: 8 }],
        devices: vec![MemoryDevice { id: "m".into(), capacity: 32 }],
        probes: vec![
            AddressProbe { host: "h".into(), address: 0, expected: target("m", 0) },
            AddressProbe { host: "h".into(), address: 15, expected: target("m", 15) },
            AddressProbe { host: "h".into(), address: 16, expected: None },
        ], dma: vec![], routes: vec![route("h", "m", &["root", "m"], 1)],
    };
    let map = recover(&input).expect("synthetic fabric has one valid map");
    assert_eq!(map.decoders, vec![Decoder {
        host: "h".into(), start: 0, end: 16, device: "m".into(), device_offset: 0,
    }], "FAILED: use negative evidence and coalesce adjacent equivalent ranges");
}

#[test]
fn equal_cost_routes_use_hop_sequence_order() {
    let input = FabricInput {
        hosts: vec![Host { id: "h".into(), window_start: 0, window_end: 8, granularity: 8 }],
        devices: vec![MemoryDevice { id: "m".into(), capacity: 8 }],
        probes: vec![AddressProbe { host: "h".into(), address: 0, expected: target("m", 0) }],
        dma: vec![], routes: vec![
            route("h", "m", &["z", "m"], 4), route("h", "m", &["a", "m"], 4),
        ],
    };
    let map = recover(&input).expect("synthetic fabric has legal routes");
    assert_eq!(map.routes[0].hops, vec!["a".to_string(), "m".to_string()],
        "FAILED: equal route cost uses hop sequence, not input order");
}

#[test]
fn host_views_are_recovered_independently() {
    let input = FabricInput {
        hosts: vec![
            Host { id: "b".into(), window_start: 100, window_end: 108, granularity: 4 },
            Host { id: "a".into(), window_start: 0, window_end: 8, granularity: 4 },
        ], devices: vec![MemoryDevice { id: "m".into(), capacity: 8 }],
        probes: vec![
            AddressProbe { host: "b".into(), address: 103, expected: target("m", 3) },
            AddressProbe { host: "a".into(), address: 3, expected: target("m", 3) },
        ], dma: vec![], routes: vec![
            route("b", "m", &["b", "m"], 1), route("a", "m", &["a", "m"], 1),
        ],
    };
    let map = recover(&input).expect("both host views are valid");
    assert_eq!(map.translate("a", 3), map.translate("b", 103));
    assert_eq!(map.decoders[0].host, "a", "FAILED: decoder output is canonical by host");
}

#[test]
fn translated_interval_may_end_exactly_at_capacity() {
    let input = FabricInput {
        hosts: vec![Host { id: "h".into(), window_start: 0, window_end: 8, granularity: 8 }],
        devices: vec![MemoryDevice { id: "m".into(), capacity: 16 }],
        probes: vec![AddressProbe { host: "h".into(), address: 7, expected: target("m", 15) }],
        dma: vec![], routes: vec![route("h", "m", &["h", "m"], 1)],
    };
    let map = recover(&input).expect("capacity endpoint is a legal range end");
    assert_eq!(map.decoders[0].device_offset, 8);
    assert_eq!(map.translate("h", 7), target("m", 15));
}

#[test]
fn malformed_host_windows_fail_before_recovery() {
    let input = FabricInput {
        hosts: vec![Host { id: "h".into(), window_start: 0, window_end: 8, granularity: 0 }],
        devices: vec![MemoryDevice { id: "m".into(), capacity: 8 }],
        probes: vec![], dma: vec![], routes: vec![],
    };
    assert!(recover(&input).is_err(), "FAILED: zero decoder granularity is invalid input");
}
