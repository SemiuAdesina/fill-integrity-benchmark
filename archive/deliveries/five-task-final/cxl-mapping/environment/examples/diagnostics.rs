//! Public rule-level diagnostics.
//!
//! Each check is a small synthetic instance of one rule from `spec/cxl-model.md`.
//! The fixtures and expected answers here are public; the graded population is
//! separate and combines the same rules adversarially. A failing check names the
//! rule that was violated.
//!
//!     cargo run --offline --example diagnostics
use cxl_mapping::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static FAILURES: AtomicUsize = AtomicUsize::new(0);

fn fail(rule: &str, detail: String) {
    FAILURES.fetch_add(1, Ordering::Relaxed);
    println!("FAILED: {rule}\n        {detail}");
}

fn ok(rule: &str) { println!("ok      {rule}"); }

fn host(id: &str, s: u64, e: u64, g: u64) -> Host {
    Host { id: id.into(), window_start: s, window_end: e, granularity: g }
}
fn device(id: &str, capacity: u64) -> MemoryDevice {
    MemoryDevice { id: id.into(), capacity }
}
fn positive(h: &str, address: u64, d: &str, offset: u64) -> AddressProbe {
    AddressProbe { host: h.into(), address, expected: Some(TargetOffset { device: d.into(), offset }) }
}
fn negative(h: &str, address: u64) -> AddressProbe {
    AddressProbe { host: h.into(), address, expected: None }
}
fn route(h: &str, d: &str, hops: &[&str], cost: u64) -> Route {
    Route { host: h.into(), device: d.into(), hops: hops.iter().map(|s| (*s).into()).collect(), cost }
}
fn decoder(h: &str, start: u64, end: u64, d: &str, device_offset: u64) -> Decoder {
    Decoder { host: h.into(), start, end, device: d.into(), device_offset }
}
fn transfer(sh: &str, sa: u64, dh: &str, da: u64, length: u64, coherent: bool) -> DmaObservation {
    DmaObservation { src_host: sh.into(), src_address: sa, dst_host: dh.into(), dst_address: da, length, coherent }
}
fn fabric(hosts: Vec<Host>, devices: Vec<MemoryDevice>, probes: Vec<AddressProbe>,
          dma: Vec<DmaObservation>, routes: Vec<Route>) -> FabricInput {
    FabricInput { hosts, devices, probes, dma, routes }
}

fn expect_decoders(rule: &str, input: &FabricInput, want: &[Decoder]) {
    match recover(input) {
        Ok(map) if map.decoders == want => ok(rule),
        Ok(map) => fail(rule, format!("expected {want:?}, recovered {:?}", map.decoders)),
        Err(e) => fail(rule, format!("expected {want:?}, the call rejected the input: {e}")),
    }
}

fn expect_routes(rule: &str, input: &FabricInput, want: &[Route]) {
    match recover(input) {
        Ok(map) if map.routes == want => ok(rule),
        Ok(map) => fail(rule, format!("expected {want:?}, recovered {:?}", map.routes)),
        Err(e) => fail(rule, format!("expected {want:?}, the call rejected the input: {e}")),
    }
}

fn expect_rejected(rule: &str, input: &FabricInput) {
    match recover(input) {
        Err(_) => ok(rule),
        Ok(map) => fail(rule, format!("expected a rejection, recovered {:?}", map.decoders)),
    }
}

fn expect_bool(rule: &str, got: bool, want: bool) {
    if got == want { ok(rule) } else { fail(rule, format!("expected {want}, observed {got}")) }
}

fn main() {
    let one_host = || vec![host("h0", 0, 64, 16)];
    let one_route = || vec![route("h0", "d0", &["s0"], 1)];

    expect_decoders("a decoder covers [start,end), so a negative probe at its end address stays unmapped",
        &fabric(one_host(), vec![device("d0", 64)],
                vec![positive("h0", 0, "d0", 0), negative("h0", 16)], vec![], one_route()),
        &[decoder("h0", 0, 16, "d0", 0)]);

    expect_decoders("the map uses the fewest decoders, so one aligned range with a single translation delta replaces several",
        &fabric(one_host(), vec![device("d0", 64)],
                vec![positive("h0", 0, "d0", 0), positive("h0", 32, "d0", 32),
                     negative("h0", 48)], vec![], one_route()),
        &[decoder("h0", 0, 48, "d0", 0)]);

    expect_decoders("a negative probe is authoritative, so no decoder may span it",
        &fabric(one_host(), vec![device("d0", 64)],
                vec![positive("h0", 0, "d0", 0), negative("h0", 32), positive("h0", 48, "d0", 48)],
                vec![], one_route()),
        &[decoder("h0", 0, 16, "d0", 0), decoder("h0", 48, 64, "d0", 48)]);

    expect_decoders("decoders on one host may touch but must not overlap",
        &fabric(one_host(), vec![device("d0", 64), device("d1", 64)],
                vec![positive("h0", 0, "d0", 0), positive("h0", 16, "d1", 0), negative("h0", 32)],
                vec![], vec![route("h0", "d0", &["s0"], 1), route("h0", "d1", &["s0"], 1)]),
        &[decoder("h0", 0, 16, "d0", 0), decoder("h0", 16, 32, "d1", 0)]);

    expect_decoders("translation is host-scoped, so two hosts may expose the same device bytes at different addresses",
        &fabric(vec![host("h0", 0, 32, 16), host("h1", 64, 96, 16)], vec![device("d0", 32)],
                vec![positive("h0", 0, "d0", 0), negative("h0", 16),
                     positive("h1", 64, "d0", 0), negative("h1", 80)],
                vec![], vec![route("h0", "d0", &["s0"], 1), route("h1", "d0", &["s0"], 1)]),
        &[decoder("h0", 0, 16, "d0", 0), decoder("h1", 64, 80, "d0", 0)]);

    expect_rejected("the translated device interval must fit inside [0,capacity), so this evidence admits no valid map",
        &fabric(one_host(), vec![device("d0", 56)],
                vec![positive("h0", 0, "d0", 0), positive("h0", 48, "d0", 48)], vec![], one_route()));

    expect_rejected("every (host,device) a decoder uses needs a legal route",
        &fabric(one_host(), vec![device("d0", 64)],
                vec![positive("h0", 0, "d0", 0), negative("h0", 16)], vec![], vec![]));

    expect_routes("exactly one legal route per used (host,device) is chosen, at minimum cost",
        &fabric(one_host(), vec![device("d0", 64)],
                vec![positive("h0", 0, "d0", 0), negative("h0", 16)], vec![],
                vec![route("h0", "d0", &["s9"], 5), route("h0", "d0", &["s1", "s2"], 3)]),
        &[route("h0", "d0", &["s1", "s2"], 3)]);

    expect_routes("a route cost tie is broken by the lexicographically smaller hop sequence",
        &fabric(one_host(), vec![device("d0", 64)],
                vec![positive("h0", 0, "d0", 0), negative("h0", 16)], vec![],
                vec![route("h0", "d0", &["s0", "s3"], 4), route("h0", "d0", &["s0", "s1"], 4)]),
        &[route("h0", "d0", &["s0", "s1"], 4)]);

    let split = RecoveredMap {
        decoders: vec![decoder("h0", 0, 16, "d0", 0), decoder("h1", 64, 80, "d0", 0)],
        routes: vec![],
    };
    expect_bool("an address outside every decoder is unmapped, and the end address is outside",
        split.translate("h0", 16).is_none(), true);
    expect_bool("a DMA range is the full half-open span, so it may not cross a decoder endpoint",
        split.predicts_dma(&transfer("h0", 0, "h1", 64, 32, true)), false);
    expect_bool("a DMA observation of zero length is never coherent",
        split.predicts_dma(&transfer("h0", 0, "h1", 64, 0, true)), false);
    expect_bool("two host views of the same device interval are coherent across the whole span",
        split.predicts_dma(&transfer("h0", 0, "h1", 64, 16, true)), true);

    let failures = FAILURES.load(Ordering::Relaxed);
    if failures == 0 {
        println!("all public rule diagnostics passed");
    } else {
        println!("public rule diagnostics failing: {failures}");
        std::process::exit(1);
    }
}
