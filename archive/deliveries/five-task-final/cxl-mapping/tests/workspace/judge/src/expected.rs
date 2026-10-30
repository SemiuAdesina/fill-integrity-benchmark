//! Verifier-owned expected values — held only by the judge.
//!
//! Every expectation below is the same property the V1 graded harness asserted
//! inside the graded process, moved out of it. Nothing here is derived from a
//! probe record: `grade` receives the raw values a probe observed and compares
//! them against literals this crate owns. The judge binary is the only artifact
//! that carries this file, it lives in a root-owned 0700 tree, and the account
//! the probe runs under cannot read it.
//!
//! The granularity of each check is deliberately identical to V1: a case that
//! asserted a whole vector still asserts the whole vector, and a case that
//! asserted two endpoints still asserts exactly those two endpoints. Neither
//! more nor less is required of the candidate than before.
use cxl_model::{Decoder, Observed, Outcome, Route, TargetOffset};

fn dec(host: &str, start: u64, end: u64, dev: &str, off: u64) -> Decoder {
    Decoder { host: host.into(), start, end, device: dev.into(), device_offset: off }
}
fn rt(host: &str, dev: &str, cost: u64, hops: &[&str]) -> Route {
    Route {
        host: host.into(),
        device: dev.into(),
        cost,
        hops: hops.iter().map(|x| x.to_string()).collect(),
    }
}
fn tgt(dev: &str, off: u64) -> Option<TargetOffset> {
    Some(TargetOffset { device: dev.into(), offset: off })
}

/// Require that the candidate recovered a map at all.
fn recovered(o: &Observed) -> Result<&Outcome, String> {
    match o {
        Observed::Failed(e) => Err(format!("recovery failed: {e:?}")),
        Observed::Recovered(r) => Ok(r),
    }
}

fn want<T: PartialEq + std::fmt::Debug>(what: &str, got: &T, expect: &T) -> Result<(), String> {
    if got == expect {
        Ok(())
    } else {
        Err(format!("{what}: expected {expect:?}, observed {got:?}"))
    }
}

fn nth_decoder(r: &Outcome, i: usize) -> Result<&Decoder, String> {
    r.decoders.get(i).ok_or_else(|| format!("decoder {i} missing"))
}

fn nth_translate(r: &Outcome, i: usize) -> Result<&Option<TargetOffset>, String> {
    r.translate.get(i).ok_or_else(|| format!("translate result {i} missing"))
}

fn nth_dma(r: &Outcome, i: usize) -> Result<bool, String> {
    r.dma.get(i).copied().ok_or_else(|| format!("dma result {i} missing"))
}

fn device_of(t: &Option<TargetOffset>, what: &str) -> Result<String, String> {
    match t {
        None => Err(format!("{what}: expected a translation, observed none")),
        Some(t) => Ok(t.device.clone()),
    }
}

/// Apply this case's expectations to the raw values a probe observed.
pub fn grade(id: &str, o: &Observed) -> Result<(), String> {
    match id {
        // Exactly one maximal aligned decoder, and half-open translation at the
        // top endpoint.
        // Rejects: one granularity-sized decoder per positive probe, which yields a
        // larger map, and inclusive end handling, which translates the end address.
        "reconstructs_one_maximal_half_open_decoder" => {
            let r = recovered(o)?;
            want("decoders", &r.decoders, &vec![dec("h0", 0x2000, 0x5000, "m0", 0)])?;
            want("translate(h0,0x4fff)", nth_translate(r, 0)?, &tgt("m0", 0x2fff))?;
            want("translate(h0,0x5000)", nth_translate(r, 1)?, &None)
        }
        // The negative observation splits the span into two decoders.
        // Rejects: ignoring negative probes, which returns a single decoder covering
        // the address the host reported as unmapped.
        "negative_observation_splits_otherwise_plausible_span" => {
            let r = recovered(o)?;
            want("decoder count", &r.decoders.len(), &2)?;
            let a = nth_decoder(r, 0)?;
            let b = nth_decoder(r, 1)?;
            want("decoders[0] range", &(a.start, a.end), &(0, 0x1000))?;
            want("decoders[1] range", &(b.start, b.end), &(0x2000, 0x5000))
        }
        // Two touching decoders for different devices: the shared endpoint is a
        // self-consistency check the judge performs on the observed values.
        // Rejects: inclusive endpoints, which make the two decoders overlap, and
        // coalescing adjacent decoders that resolve to different devices.
        "touching_ranges_are_legal_but_not_overlapping" => {
            let r = recovered(o)?;
            want("decoder count", &r.decoders.len(), &2)?;
            let a = nth_decoder(r, 0)?;
            let b = nth_decoder(r, 1)?;
            if a.end != b.start {
                return Err(format!(
                    "decoders must touch: decoders[0].end={} decoders[1].start={}",
                    a.end, b.start
                ));
            }
            want(
                "translate(h,0x2000).device",
                &device_of(nth_translate(r, 0)?, "translate(h,0x2000)")?,
                &"b".to_string(),
            )
        }
        // Route selection by cost then hop sequence.
        // Rejects: the first legal route, and the first route among the equal-cost
        // group in declaration order.
        "chooses_route_by_cost_then_hop_sequence" => {
            let r = recovered(o)?;
            want("routes", &r.routes, &vec![rt("h", "m", 2, &["b", "a"])])
        }
        // Two host views of the same device bytes, joined by a coherent DMA.
        // Rejects: a single global host view, which cannot place the same device
        // bytes at two unrelated host bases, and any assumption that host address
        // and device offset differ by one shared constant.
        "per_host_views_share_device_bytes" => {
            let r = recovered(o)?;
            want("predicts_dma", &nth_dma(r, 0)?, &true)?;
            want("decoders[0].start", &nth_decoder(r, 0)?.start, &0x1000)?;
            want("decoders[1].start", &nth_decoder(r, 1)?.start, &0x8000)
        }
        // Identical host addresses naming different devices.
        // Rejects: a global address-to-target table keyed by address alone, which
        // gives both hosts the same device.
        "identical_host_addresses_can_name_different_targets" => {
            let r = recovered(o)?;
            want(
                "translate(a,0x100).device",
                &device_of(nth_translate(r, 0)?, "translate(a,0x100)")?,
                &"ma".to_string(),
            )?;
            want(
                "translate(b,0x100).device",
                &device_of(nth_translate(r, 1)?, "translate(b,0x100)")?,
                &"mb".to_string(),
            )
        }
        // A DMA range whose tail leaves the decoder is not coherent.
        // Rejects: validating only the start address of the span, or otherwise
        // ignoring the transfer length.
        "dma_range_may_not_cross_decoder_endpoint" => {
            let r = recovered(o)?;
            want("predicts_dma", &nth_dma(r, 0)?, &false)
        }
        // The DMA interpreter alone, over a verifier-supplied map.
        // Rejects: ignoring the length field, and an off-by-one that stops one byte
        // short of the end of the span.
        "dma_interpreter_checks_entire_half_open_length" => {
            let r = recovered(o)?;
            want("predicts_dma", &nth_dma(r, 0)?, &false)
        }
        // Device capacity bounds the decoder.
        // Rejects: ignoring device capacity and extending the decoder to fill the
        // declared host window.
        "capacity_endpoint_is_excluded" => {
            let r = recovered(o)?;
            want("decoders", &r.decoders, &vec![dec("h", 0x1000, 0x3000, "m", 0)])
        }
        // Canonical serialization breaks the global tie.
        // Rejects: no canonical ordering -- returning whichever minimum-count map the
        // search happened to reach first.
        "canonical_decoder_serialization_breaks_global_tie" => {
            let r = recovered(o)?;
            let a = nth_decoder(r, 0)?;
            want("decoders[0].start", &a.start, &0)?;
            want("decoders[0].end", &a.end, &0x2000)
        }
        // Compound case: decoder count, coherent DMA, an unmapped address, and
        // the equal-cost route tie.
        // Rejects: a global host view, coalescing across the adjacent target
        // boundary, ignoring the capacity bound, and first-route selection; each
        // changes one of the four values this arm compares.
        "compound_two_views_adjacent_boundary_capacity_and_route_tie" => {
            let r = recovered(o)?;
            want("decoder count", &r.decoders.len(), &3)?;
            want("predicts_dma", &nth_dma(r, 0)?, &true)?;
            want("translate(h0,0x6000)", nth_translate(r, 0)?, &None)?;
            let route = r
                .routes
                .iter()
                .find(|x| x.host == "h0" && x.device == "shared")
                .ok_or_else(|| "no route recovered for h0 -> shared".to_string())?;
            want(
                "route(h0,shared).hops",
                &route.hops,
                &vec!["a".to_string(), "q".to_string()],
            )
        }
        // A coherent DMA whose span reaches past the last probed granule forces
        // every candidate decoder to extend into device bytes no probe pins
        // down. Each host view on its own is satisfiable, the DMA is long
        // enough to be coherent, and the alignment rule permits only whole
        // granules -- so the shortest legal cover overruns the device capacity
        // endpoint and no valid map exists. Recovery must fail rather than
        // return the map that ignoring the capacity limit would admit.
        // Rejects: ignoring the DMA span, ignoring the capacity bound, or emitting
        // one decoder per positive probe -- each satisfies every probe and still
        // returns a map where the contract requires failure.
        "compound_dma_span_forces_coverage_past_capacity_endpoint" => match o {
            Observed::Failed(e) => want("error", e, &"no coherent decoder map".to_string()),
            Observed::Recovered(_) => Err("expected recovery to fail: covering the coherent \
                 DMA span with whole granules cannot stay inside the device capacity"
                .into()),
        },
        // Contradictory DMA evidence is rejected; V1 asserted only `is_err`.
        // Rejects: treating DMA observations as advisory and dropping the ones that
        // conflict with the probe evidence.
        "rejects_inconsistent_dma_evidence" => match o {
            Observed::Failed(_) => Ok(()),
            Observed::Recovered(_) => {
                Err("expected recovery to fail on inconsistent DMA evidence".into())
            }
        },
        // A used mapping without a legal route is rejected by exact message.
        // Rejects: emitting a decoder map without checking route legality, and
        // synthesising a default route for a mapping with none supplied.
        "requires_route_for_every_used_mapping" => match o {
            Observed::Failed(e) => want("error", e, &"missing route".to_string()),
            Observed::Recovered(_) => {
                Err("expected recovery to fail with \"missing route\"".into())
            }
        },
        // Canonical output order is host then start.
        // Rejects: emitting decoders and routes in fabric declaration order.
        "decoder_order_is_host_then_start" => {
            let r = recovered(o)?;
            want("decoders[0].host", &nth_decoder(r, 0)?.host, &"a".to_string())?;
            let first = r.routes.first().ok_or_else(|| "route 0 missing".to_string())?;
            want("routes[0].host", &first.host, &"a".to_string())
        }
        // Alignment comes from the host window, not the probe address.
        // Rejects: inferring granularity or the window base from the probed
        // addresses, which produces a finer, unaligned range.
        "alignment_is_not_inferred_from_probe_address" => {
            let r = recovered(o)?;
            let a = nth_decoder(r, 0)?;
            want("decoders[0] range", &(a.start, a.end), &(0x2000, 0x6000))
        }
        // Frozen support case: translation is half-open at both endpoints.
        // Rejects: inclusive-end translation, and an off-by-one at the start that
        // maps the address just below the decoder.
        "translate_is_half_open" => {
            let r = recovered(o)?;
            want("translate(h,9)", nth_translate(r, 0)?, &None)?;
            want("translate(h,10)", nth_translate(r, 1)?, &tgt("m", 30))?;
            want("translate(h,19)", nth_translate(r, 2)?, &tgt("m", 39))?;
            want("translate(h,20)", nth_translate(r, 3)?, &None)
        }
        // Frozen support case: a zero-length DMA is never coherent.
        // Rejects: treating a vacuous span as trivially coherent because no byte in
        // it fails the check.
        "zero_length_dma_is_never_coherent" => {
            let r = recovered(o)?;
            want("predicts_dma", &nth_dma(r, 0)?, &false)
        }
        // Frozen support case: a malformed host window fails closed.
        // Rejects: aligning by dividing or taking a remainder against the declared
        // granularity before validating that it is nonzero.
        "malformed_windows_fail_closed" => match o {
            Observed::Failed(_) => Ok(()),
            Observed::Recovered(_) => {
                Err("expected recovery to fail on a malformed host window".into())
            }
        },
        // Public requirement: a decoder's device offset is a multiple of the
        // host's granularity, exactly as its start and end are.
        // Semantic behavior: the fabric supplied here is legal in every other
        // respect -- aligned window inside the granule bound, probe inside the
        // window, target offset inside capacity, a route supplied -- so the
        // offset lattice is the only ground on which it can be refused, and the
        // contract requires nothing rather than a map.
        // Rejects: aligning the host-side window only and letting the device
        // offset land wherever the observation puts it, which returns a map that
        // reproduces the observation through a decoder no fabric can program.
        "device_offset_must_be_granularity_aligned" => match o {
            Observed::Failed(e) => want("error", e, &"no coherent decoder map".to_string()),
            Observed::Recovered(_) => Err("expected recovery to fail: no decoder whose \
                 device offset is on the granularity lattice reproduces this observation"
                .into()),
        },
        _ => Err("unknown case".into()),
    }
}

/// The verifier's own known-correct record for a case, used only to stage the
/// negative control. No candidate code contributes to these values.
pub fn canonical(id: &str) -> Observed {
    fn ok(
        decoders: Vec<Decoder>,
        routes: Vec<Route>,
        translate: Vec<Option<TargetOffset>>,
        dma: Vec<bool>,
    ) -> Observed {
        Observed::Recovered(Outcome { decoders, routes, translate, dma })
    }
    match id {
        "reconstructs_one_maximal_half_open_decoder" => ok(
            vec![dec("h0", 0x2000, 0x5000, "m0", 0)],
            vec![rt("h0", "m0", 1, &["s"])],
            vec![tgt("m0", 0x2fff), None],
            vec![],
        ),
        "negative_observation_splits_otherwise_plausible_span" => ok(
            vec![dec("h", 0, 0x1000, "m", 0), dec("h", 0x2000, 0x5000, "m", 0x2000)],
            vec![rt("h", "m", 1, &["s"])],
            vec![],
            vec![],
        ),
        "touching_ranges_are_legal_but_not_overlapping" => ok(
            vec![dec("h", 0, 0x2000, "a", 0), dec("h", 0x2000, 0x4000, "b", 0)],
            vec![rt("h", "a", 1, &["x"]), rt("h", "b", 1, &["y"])],
            vec![tgt("b", 0)],
            vec![],
        ),
        "chooses_route_by_cost_then_hop_sequence" => ok(
            vec![dec("h", 0, 0x1000, "m", 0)],
            vec![rt("h", "m", 2, &["b", "a"])],
            vec![],
            vec![],
        ),
        "per_host_views_share_device_bytes" => ok(
            vec![
                dec("east", 0x1000, 0x5000, "shared", 0),
                dec("west", 0x8000, 0xc000, "shared", 0),
            ],
            vec![rt("east", "shared", 2, &["e"]), rt("west", "shared", 2, &["w"])],
            vec![],
            vec![true],
        ),
        "identical_host_addresses_can_name_different_targets" => ok(
            vec![dec("a", 0, 0x1000, "ma", 0), dec("b", 0, 0x1000, "mb", 0)],
            vec![rt("a", "ma", 1, &["a"]), rt("b", "mb", 1, &["b"])],
            vec![tgt("ma", 0x100), tgt("mb", 0x100)],
            vec![],
        ),
        "dma_range_may_not_cross_decoder_endpoint" => ok(
            vec![dec("a", 0, 0x2000, "m", 0), dec("b", 0x5000, 0x8000, "m", 0)],
            vec![rt("a", "m", 1, &["a"]), rt("b", "m", 1, &["b"])],
            vec![],
            vec![false],
        ),
        "dma_interpreter_checks_entire_half_open_length" => ok(
            vec![dec("a", 0, 0x2000, "m", 0), dec("b", 0x5000, 0x8000, "m", 0)],
            vec![],
            vec![],
            vec![false],
        ),
        "capacity_endpoint_is_excluded" => ok(
            vec![dec("h", 0x1000, 0x3000, "m", 0)],
            vec![rt("h", "m", 1, &["s"])],
            vec![],
            vec![],
        ),
        "canonical_decoder_serialization_breaks_global_tie" => ok(
            vec![dec("h", 0, 0x2000, "m", 0)],
            vec![rt("h", "m", 1, &["s"])],
            vec![],
            vec![],
        ),
        "compound_two_views_adjacent_boundary_capacity_and_route_tie" => ok(
            vec![
                dec("h0", 0x1000, 0x3000, "left", 0),
                dec("h0", 0x3000, 0x6000, "shared", 0),
                dec("h1", 0xa000, 0xd000, "shared", 0),
            ],
            vec![
                rt("h0", "left", 1, &["l"]),
                rt("h0", "shared", 3, &["a", "q"]),
                rt("h1", "shared", 2, &["b"]),
            ],
            vec![None],
            vec![true],
        ),
        "compound_dma_span_forces_coverage_past_capacity_endpoint" => {
            Observed::Failed("no coherent decoder map".to_string())
        }
        "rejects_inconsistent_dma_evidence" => {
            Observed::Failed("no coherent decoder map".to_string())
        }
        "requires_route_for_every_used_mapping" => Observed::Failed("missing route".to_string()),
        "decoder_order_is_host_then_start" => ok(
            vec![dec("a", 0x8000, 0xa000, "m", 0), dec("z", 0, 0x2000, "m", 0)],
            vec![rt("a", "m", 1, &["a"]), rt("z", "m", 1, &["z"])],
            vec![],
            vec![],
        ),
        "alignment_is_not_inferred_from_probe_address" => ok(
            vec![dec("h", 0x2000, 0x6000, "m", 0)],
            vec![rt("h", "m", 1, &["s"])],
            vec![],
            vec![],
        ),
        "translate_is_half_open" => ok(
            vec![dec("h", 10, 20, "m", 30)],
            vec![],
            vec![None, tgt("m", 30), tgt("m", 39), None],
            vec![],
        ),
        "zero_length_dma_is_never_coherent" => ok(vec![], vec![], vec![], vec![false]),
        "malformed_windows_fail_closed" => Observed::Failed("invalid host window".to_string()),
        "device_offset_must_be_granularity_aligned" => {
            Observed::Failed("no coherent decoder map".to_string())
        }
        _ => Observed::Failed("unknown case".to_string()),
    }
}
