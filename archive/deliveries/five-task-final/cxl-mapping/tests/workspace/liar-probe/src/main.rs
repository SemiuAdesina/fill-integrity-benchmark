//! Regression probe for the stdout-authority property.
//!
//! The same harness as `probe`, but linked against a verifier-owned
//! deliberately incorrect implementation that also floods stdout and stderr
//! with success-looking text. The judge must still fail it, which demonstrates
//! that emitted output carries no grading authority.
#[allow(dead_code)]
mod candidate;

use cxl_model::{cases, record, Decoder, Observed, Outcome, Route, Setup, TargetOffset};

/// Verifier input -> candidate input. Pure field translation.
fn to_candidate(f: &cxl_model::Fabric) -> candidate::FabricInput {
    candidate::FabricInput {
        hosts: f
            .hosts
            .iter()
            .map(|h| candidate::Host {
                id: h.id.clone(),
                window_start: h.window_start,
                window_end: h.window_end,
                granularity: h.granularity,
            })
            .collect(),
        devices: f
            .devices
            .iter()
            .map(|d| candidate::MemoryDevice { id: d.id.clone(), capacity: d.capacity })
            .collect(),
        probes: f
            .probes
            .iter()
            .map(|p| candidate::AddressProbe {
                host: p.host.clone(),
                address: p.address,
                expected: p.expected.as_ref().map(to_candidate_target),
            })
            .collect(),
        dma: f.dma.iter().map(to_candidate_dma).collect(),
        routes: f.routes.iter().map(to_candidate_route).collect(),
    }
}

fn to_candidate_target(t: &TargetOffset) -> candidate::TargetOffset {
    candidate::TargetOffset { device: t.device.clone(), offset: t.offset }
}

fn to_candidate_route(r: &Route) -> candidate::Route {
    candidate::Route {
        host: r.host.clone(),
        device: r.device.clone(),
        hops: r.hops.clone(),
        cost: r.cost,
    }
}

fn to_candidate_dma(o: &cxl_model::DmaObservation) -> candidate::DmaObservation {
    candidate::DmaObservation {
        src_host: o.src_host.clone(),
        src_address: o.src_address,
        dst_host: o.dst_host.clone(),
        dst_address: o.dst_address,
        length: o.length,
        coherent: o.coherent,
    }
}

fn to_candidate_decoder(d: &Decoder) -> candidate::Decoder {
    candidate::Decoder {
        host: d.host.clone(),
        start: d.start,
        end: d.end,
        device: d.device.clone(),
        device_offset: d.device_offset,
    }
}

/// Candidate output -> raw record values. Pure field translation; nothing is
/// judged, normalised, repaired or rejected here.
fn from_candidate_decoder(d: &candidate::Decoder) -> Decoder {
    Decoder {
        host: d.host.clone(),
        start: d.start,
        end: d.end,
        device: d.device.clone(),
        device_offset: d.device_offset,
    }
}

fn from_candidate_route(r: &candidate::Route) -> Route {
    Route { host: r.host.clone(), device: r.device.clone(), hops: r.hops.clone(), cost: r.cost }
}

fn from_candidate_target(t: &candidate::TargetOffset) -> TargetOffset {
    TargetOffset { device: t.device.clone(), offset: t.offset }
}

/// Interrogate one candidate map with the case's verifier-owned queries.
fn interrogate(m: &candidate::RecoveredMap, c: &cxl_model::Case) -> Outcome {
    Outcome {
        decoders: m.decoders.iter().map(from_candidate_decoder).collect(),
        routes: m.routes.iter().map(from_candidate_route).collect(),
        translate: c
            .translate
            .iter()
            .map(|q| m.translate(&q.host, q.address).as_ref().map(from_candidate_target))
            .collect(),
        dma: c.dma.iter().map(|o| m.predicts_dma(&to_candidate_dma(o))).collect(),
    }
}

/// Run one case against the candidate and return the raw values observed.
fn observe(c: &cxl_model::Case) -> Observed {
    match &c.setup {
        Setup::Recover(f) => match candidate::recover(&to_candidate(f)) {
            Err(e) => Observed::Failed(e),
            Ok(m) => Observed::Recovered(interrogate(&m, c)),
        },
        Setup::Direct(ds) => {
            let m = candidate::RecoveredMap {
                decoders: ds.iter().map(to_candidate_decoder).collect(),
                routes: vec![],
            };
            Observed::Recovered(interrogate(&m, c))
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 3 {
        std::process::exit(2)
    }
    let Some(c) = cases::case(&a[1]) else {
        std::process::exit(2)
    };
    if std::fs::write(&a[2], record::encode(&observe(&c))).is_err() {
        std::process::exit(2)
    }
}
