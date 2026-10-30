//! Verifier-owned case inputs.
//!
//! Only the *inputs* live here, because the candidate-tainted probe has to
//! receive them in order to run at all. The expected results are held
//! exclusively by the `judge` crate, which the candidate never links and cannot
//! read at run time.
//!
//! Every fabric, probe list, route list, DMA observation, decoder literal and
//! interrogation address below is carried over unchanged from the V1 harness:
//! cases 1–15 from `tests/graded.rs` and cases 16–18 from the verifier-owned
//! `tests/frozen.rs`, in the order `tests/cases.txt` listed them.
use crate::{
    AddressProbe, Case, Decoder, DmaObservation, Fabric, Host, MemoryDevice, Query, Route, Setup,
    TargetOffset,
};

/// Case ids whose evaluation requires executing the candidate implementation.
/// The first nineteen entries are the complete V1 positive population, in the V1
/// order; the last closes a normative rule the spec states and the V1 fixtures
/// never made load-bearing.
///
/// Each entry records the public behaviour it makes load-bearing and the
/// plausible wrong implementation it exists to reject. Expected values stay in
/// the judge; nothing below states what any answer is.
pub const CANDIDATE_CASES: &[&str] = &[
    // Behaviour: a decoder is recovered as one maximal granularity-aligned
    // half-open window, so the address one past its end is unmapped.
    // Rejects: one granularity-sized decoder per positive probe, and inclusive
    // end handling that maps the address at the window end.
    "reconstructs_one_maximal_half_open_decoder",
    // Behaviour: a negative observation forbids coverage, so a span the positive
    // observations alone would join has to be split.
    // Rejects: ignoring negative probes, which fuses the span into one decoder
    // that covers an address the host reported as unmapped.
    "negative_observation_splits_otherwise_plausible_span",
    // Behaviour: two decoders may touch at an address without overlapping, and
    // the touching address belongs to exactly one of them.
    // Rejects: inclusive endpoints, which overlap at the boundary, and
    // coalescing adjacent decoders that resolve to different devices.
    "touching_ranges_are_legal_but_not_overlapping",
    // Behaviour: a route is chosen by lowest cost first, then by hop sequence.
    // Rejects: taking the first legal route, or the first of the equal-cost
    // routes in declaration order.
    "chooses_route_by_cost_then_hop_sequence",
    // Behaviour: two hosts map the same device bytes at unrelated window bases,
    // and cross-host DMA is judged through both recovered maps.
    // Rejects: a single global host view, or assuming a host address and a
    // device offset differ by one shared constant.
    "per_host_views_share_device_bytes",
    // Behaviour: the same numeric host address on two hosts resolves to
    // different devices.
    // Rejects: a global address-to-target table keyed by address alone.
    "identical_host_addresses_can_name_different_targets",
    // Behaviour: a DMA span whose start translates but whose tail leaves the
    // decoder window is not coherent.
    // Rejects: validating only the start address, or otherwise ignoring the full
    // DMA span.
    "dma_range_may_not_cross_decoder_endpoint",
    // Behaviour: the DMA interpreter checks the entire half-open length against
    // the decoders it is given, with no recovery involved.
    // Rejects: ignoring the length field, and inclusive or off-by-one treatment
    // of the final byte.
    "dma_interpreter_checks_entire_half_open_length",
    // Behaviour: a decoder cannot reach past device capacity, and the capacity
    // address itself is outside the device.
    // Rejects: ignoring capacity and extending the decoder to fill the declared
    // host window.
    "capacity_endpoint_is_excluded",
    // Behaviour: when several maps share the minimum decoder count, the
    // published serialized ordering picks the answer.
    // Rejects: no canonical ordering -- returning whichever minimum-count map
    // the search reached first.
    "canonical_decoder_serialization_breaks_global_tie",
    // Behaviour: two host views, an adjacent target boundary, a capacity
    // endpoint and an equal-cost route tie all have to hold together.
    // Rejects: any solver correct on one rule but not their conjunction -- a
    // global host view, coalescing across the target boundary, ignoring
    // capacity, or first-route selection each answer differently here.
    "compound_two_views_adjacent_boundary_capacity_and_route_tie",
    // Behaviour: a coherent DMA span longer than the granule the probes pin
    // down forces additional coverage, which capacity and granularity then
    // constrain; the three rules have to be read together.
    // Rejects: ignoring the DMA span, ignoring capacity, or emitting one
    // decoder per positive probe -- each satisfies the probes and still gets
    // this fabric wrong.
    "compound_dma_span_forces_coverage_past_capacity_endpoint",
    // Behaviour: DMA evidence that contradicts the address observations makes
    // the whole recovery fail closed rather than being dropped.
    // Rejects: treating DMA observations as advisory input to be ignored when
    // they conflict with the probe evidence.
    "rejects_inconsistent_dma_evidence",
    // Behaviour: every mapping the recovered map uses needs a supplied legal
    // route, so an empty route list fails closed.
    // Rejects: emitting decoders without checking route legality, or inventing
    // a default route.
    "requires_route_for_every_used_mapping",
    // Behaviour: the emitted decoder list is ordered by host then start
    // address, whatever order the hosts were declared in.
    // Rejects: emitting decoders in fabric declaration order.
    "decoder_order_is_host_then_start",
    // Behaviour: decoder alignment comes from the host's declared granularity,
    // not from the addresses that happened to be probed.
    // Rejects: inferring the granularity or the window base from observed probe
    // addresses, which yields a finer, unaligned map.
    "alignment_is_not_inferred_from_probe_address",
    // Behaviour: translation includes a decoder's start and excludes its end.
    // Rejects: inclusive-end translation, and an off-by-one at the start.
    "translate_is_half_open",
    // Behaviour: a zero-length DMA is never coherent.
    // Rejects: treating a vacuous span as trivially satisfied because no byte
    // fails the check.
    "zero_length_dma_is_never_coherent",
    // Behaviour: a host window declared with granularity zero is rejected.
    // Rejects: aligning by dividing or taking a remainder against the declared
    // granularity before validating it.
    "malformed_windows_fail_closed",
    // Behaviour: a decoder's device offset is itself a multiple of the host's
    // granularity, so evidence that can only be satisfied by an offset off that
    // lattice describes no legal decoder and the recovery fails closed.
    // Rejects: aligning only the host-side window and letting the device offset
    // fall wherever the probe puts it, which returns a map that reproduces every
    // observation through a decoder the fabric cannot program.
    "device_offset_must_be_granularity_aligned",
];

fn h(id: &str, start: u64, end: u64, g: u64) -> Host {
    Host { id: id.into(), window_start: start, window_end: end, granularity: g }
}
fn d(id: &str, cap: u64) -> MemoryDevice {
    MemoryDevice { id: id.into(), capacity: cap }
}
fn p(host: &str, a: u64, dev: &str, off: u64) -> AddressProbe {
    AddressProbe {
        host: host.into(),
        address: a,
        expected: Some(TargetOffset { device: dev.into(), offset: off }),
    }
}
fn n(host: &str, a: u64) -> AddressProbe {
    AddressProbe { host: host.into(), address: a, expected: None }
}
fn r(host: &str, dev: &str, cost: u64, hops: &[&str]) -> Route {
    Route {
        host: host.into(),
        device: dev.into(),
        cost,
        hops: hops.iter().map(|x| x.to_string()).collect(),
    }
}
fn dec(host: &str, start: u64, end: u64, dev: &str, off: u64) -> Decoder {
    Decoder { host: host.into(), start, end, device: dev.into(), device_offset: off }
}
fn dma(sh: &str, sa: u64, dh: &str, da: u64, len: u64, coherent: bool) -> DmaObservation {
    DmaObservation {
        src_host: sh.into(),
        src_address: sa,
        dst_host: dh.into(),
        dst_address: da,
        length: len,
        coherent,
    }
}
fn q(host: &str, a: u64) -> Query {
    Query { host: host.into(), address: a }
}
fn base(
    hosts: Vec<Host>,
    devices: Vec<MemoryDevice>,
    probes: Vec<AddressProbe>,
    routes: Vec<Route>,
) -> Fabric {
    Fabric { hosts, devices, probes, dma: vec![], routes }
}

/// Assemble a case from a fabric plus the interrogations it performs.
fn recover_case(f: Fabric, translate: Vec<Query>, dma_queries: Vec<DmaObservation>) -> Case {
    Case { setup: Setup::Recover(f), translate, dma: dma_queries }
}

/// Inputs for a candidate-executed case. Returns `None` for unknown ids so the
/// probe cannot be steered onto an undefined case.
pub fn case(id: &str) -> Option<Case> {
    match id {
        // One host window over one device, with negative observations inside
        // and just past it. Interrogates the last in-window address and the
        // one after it.
        // Behaviour: one maximal aligned half-open window, end address excluded.
        // Rejects: a granularity-sized decoder per positive probe, and inclusive end
        // handling that maps the address at the window end.
        "reconstructs_one_maximal_half_open_decoder" => Some(recover_case(
            base(
                vec![h("h0", 0x1000, 0x6000, 0x1000)],
                vec![d("m0", 0x5000)],
                vec![
                    n("h0", 0x1800),
                    p("h0", 0x2800, "m0", 0x800),
                    p("h0", 0x4fff, "m0", 0x2fff),
                    n("h0", 0x5000),
                ],
                vec![r("h0", "m0", 1, &["s"])],
            ),
            vec![q("h0", 0x4fff), q("h0", 0x5000)],
            vec![],
        )),
        // One host and device, with a negative observation sitting between
        // positive ones. No interrogation.
        // Behaviour: negative evidence forbids coverage and splits the span.
        // Rejects: ignoring negative probes, which joins it into one decoder.
        "negative_observation_splits_otherwise_plausible_span" => Some(recover_case(
            base(
                vec![h("h", 0, 0x5000, 0x1000)],
                vec![d("m", 0x5000)],
                vec![
                    p("h", 0x100, "m", 0x100),
                    n("h", 0x1800),
                    p("h", 0x2100, "m", 0x2100),
                    p("h", 0x4fff, "m", 0x4fff),
                ],
                vec![r("h", "m", 1, &["s"])],
            ),
            vec![],
            vec![],
        )),
        // One host window covering two devices in sequence. Interrogates the
        // address at which the observations change device.
        // Behaviour: decoders may touch but not overlap, and the touching address
        // belongs to exactly one of them.
        // Rejects: inclusive endpoints, and coalescing across different devices.
        "touching_ranges_are_legal_but_not_overlapping" => Some(recover_case(
            base(
                vec![h("h", 0, 0x4000, 0x1000)],
                vec![d("a", 0x2000), d("b", 0x2000)],
                vec![
                    p("h", 0x0800, "a", 0x0800),
                    p("h", 0x1800, "a", 0x1800),
                    p("h", 0x2800, "b", 0x0800),
                    p("h", 0x3800, "b", 0x1800),
                ],
                vec![r("h", "a", 1, &["x"]), r("h", "b", 1, &["y"])],
            ),
            vec![q("h", 0x2000)],
            vec![],
        )),
        // One host and device with four supplied routes, three of which share
        // the lowest cost. No interrogation.
        // Behaviour: lowest cost first, then hop sequence order.
        // Rejects: the first legal route, or the first equal-cost route declared.
        "chooses_route_by_cost_then_hop_sequence" => Some(recover_case(
            base(
                vec![h("h", 0, 0x2000, 0x1000)],
                vec![d("m", 0x2000)],
                vec![p("h", 0x100, "m", 0x100)],
                vec![
                    r("h", "m", 4, &["a"]),
                    r("h", "m", 2, &["z"]),
                    r("h", "m", 2, &["b", "q"]),
                    r("h", "m", 2, &["b", "a"]),
                ],
            ),
            vec![],
            vec![],
        )),
        // Two disjoint host windows observing the same device, plus one DMA
        // observation between them, which is also the interrogation.
        // Behaviour: unrelated window bases onto shared device bytes, with DMA
        // judged through both recovered maps.
        // Rejects: a single global host view, or one shared address-offset constant.
        "per_host_views_share_device_bytes" => {
            let mut f = base(
                vec![h("east", 0x1000, 0x5000, 0x1000), h("west", 0x8000, 0xc000, 0x1000)],
                vec![d("shared", 0x4000)],
                vec![
                    p("east", 0x1800, "shared", 0x800),
                    p("east", 0x4fff, "shared", 0x3fff),
                    p("west", 0x8800, "shared", 0x800),
                    p("west", 0xbfff, "shared", 0x3fff),
                ],
                vec![r("east", "shared", 2, &["e"]), r("west", "shared", 2, &["w"])],
            );
            f.dma.push(dma("east", 0x1f00, "west", 0x8f00, 0x100, true));
            let o = f.dma[0].clone();
            Some(recover_case(f, vec![], vec![o]))
        }
        // Two hosts whose windows begin at the same address, each observing a
        // different device. Interrogates that address on both hosts.
        // Behaviour: the same numeric address resolves per host.
        // Rejects: a global address-to-target table keyed by address alone.
        "identical_host_addresses_can_name_different_targets" => Some(recover_case(
            base(
                vec![h("a", 0, 0x2000, 0x1000), h("b", 0, 0x2000, 0x1000)],
                vec![d("ma", 0x2000), d("mb", 0x2000)],
                vec![p("a", 0x100, "ma", 0x100), p("b", 0x100, "mb", 0x100)],
                vec![r("a", "ma", 1, &["a"]), r("b", "mb", 1, &["b"])],
            ),
            vec![q("a", 0x100), q("b", 0x100)],
            vec![],
        )),
        // Two host windows over one device, with a DMA observation whose
        // length runs past the observed span. That observation is the
        // interrogation.
        // Behaviour: a span whose start translates but whose tail leaves the window
        // is not coherent.
        // Rejects: validating only the start address, ignoring the full DMA span.
        "dma_range_may_not_cross_decoder_endpoint" => {
            let mut f = base(
                vec![h("a", 0, 0x3000, 0x1000), h("b", 0x5000, 0x8000, 0x1000)],
                vec![d("m", 0x3000)],
                vec![
                    p("a", 0x100, "m", 0x100),
                    p("a", 0x1fff, "m", 0x1fff),
                    n("a", 0x2100),
                    p("b", 0x5100, "m", 0x100),
                    p("b", 0x7100, "m", 0x2100),
                ],
                vec![r("a", "m", 1, &["a"]), r("b", "m", 1, &["b"])],
            );
            f.dma.push(dma("a", 0x1f80, "b", 0x6f80, 0x100, false));
            let o = f.dma[0].clone();
            Some(recover_case(f, vec![], vec![o]))
        }
        // Decoders supplied by the verifier instead of recovered, so only the
        // DMA interpreter is exercised. One DMA observation is interrogated.
        // Behaviour: every byte of the half-open length is checked against the
        // supplied decoders, with no recovery involved.
        // Rejects: ignoring the length, and off-by-one on the final byte.
        "dma_interpreter_checks_entire_half_open_length" => Some(Case {
            setup: Setup::Direct(vec![
                dec("a", 0, 0x2000, "m", 0),
                dec("b", 0x5000, 0x8000, "m", 0),
            ]),
            translate: vec![],
            dma: vec![dma("a", 0x1f80, "b", 0x6f80, 0x100, false)],
        }),
        // A host window wider than the device it observes, with a negative
        // observation past the device. No interrogation.
        // Behaviour: a decoder stops short of device capacity, which is excluded.
        // Rejects: ignoring capacity and filling the declared host window.
        "capacity_endpoint_is_excluded" => Some(recover_case(
            base(
                vec![h("h", 0, 0x4000, 0x1000)],
                vec![d("m", 0x2000)],
                vec![p("h", 0x2fff, "m", 0x1fff), n("h", 0x3000)],
                vec![r("h", "m", 1, &["s"])],
            ),
            vec![],
            vec![],
        )),
        // One host and device with a single positive and a single negative
        // observation, leaving several equal-size solutions. No interrogation.
        // Behaviour: among equal-count maps the serialized ordering decides.
        // Rejects: no canonical ordering -- returning the first minimum found.
        "canonical_decoder_serialization_breaks_global_tie" => Some(recover_case(
            base(
                vec![h("h", 0, 0x4000, 0x1000)],
                vec![d("m", 0x4000)],
                vec![p("h", 0x1100, "m", 0x1100), n("h", 0x2800)],
                vec![r("h", "m", 1, &["s"])],
            ),
            vec![],
            vec![],
        )),
        // Compound fixture: two host windows, two devices, seven observations,
        // one DMA observation and four routes including an equal-cost pair.
        // Interrogates one address plus that DMA observation.
        // Behaviour: two host views, an adjacent target boundary, a capacity
        // endpoint and an equal-cost route tie all interact in one answer.
        // Rejects: a global host view, coalescing across the target boundary,
        // ignoring capacity, or first-route selection.
        "compound_two_views_adjacent_boundary_capacity_and_route_tie" => {
            let mut f = base(
                vec![h("h0", 0x1000, 0x7000, 0x1000), h("h1", 0x9000, 0xf000, 0x1000)],
                vec![d("left", 0x2000), d("shared", 0x3000)],
                vec![
                    p("h0", 0x1800, "left", 0x800),
                    p("h0", 0x2fff, "left", 0x1fff),
                    p("h0", 0x3000, "shared", 0),
                    p("h0", 0x5fff, "shared", 0x2fff),
                    p("h1", 0xa000, "shared", 0),
                    p("h1", 0xcfff, "shared", 0x2fff),
                    n("h1", 0xd000),
                ],
                vec![
                    r("h0", "left", 1, &["l"]),
                    r("h0", "shared", 3, &["z"]),
                    r("h0", "shared", 3, &["a", "q"]),
                    r("h1", "shared", 2, &["b"]),
                ],
            );
            f.dma.push(dma("h0", 0x5f00, "h1", 0xcf00, 0x100, true));
            let o = f.dma[0].clone();
            Some(recover_case(f, vec![q("h0", 0x6000)], vec![o]))
        }
        // Two host windows onto one device, each carrying a single positive
        // observation, plus one coherent cross-host DMA whose length runs well
        // past the granule those observations pin down. Deciding it requires
        // reading the capacity limit, the granularity rule and the DMA span
        // together rather than one at a time. No interrogation.
        // Rejects: ignoring the DMA span, ignoring capacity, or one decoder per
        // positive probe -- each satisfies the probes and still gets this wrong.
        "compound_dma_span_forces_coverage_past_capacity_endpoint" => {
            let mut f = base(
                vec![h("h0", 0, 0x5000, 0x1000), h("h1", 0x8000, 0xc000, 0x1000)],
                vec![d("pool", 0x3c00)],
                vec![p("h0", 0x800, "pool", 0x800), p("h1", 0x8800, "pool", 0x800)],
                vec![r("h0", "pool", 1, &["s"]), r("h1", "pool", 1, &["s"])],
            );
            f.dma.push(dma("h0", 0x800, "h1", 0x8800, 0x3000, true));
            Some(recover_case(f, vec![], vec![]))
        }
        // A DMA observation that contradicts the address observations. No
        // interrogation.
        // Behaviour: contradictory DMA evidence fails the whole recovery closed.
        // Rejects: treating DMA observations as advisory and dropping conflicts.
        "rejects_inconsistent_dma_evidence" => {
            let mut f = base(
                vec![h("a", 0, 0x2000, 0x1000), h("b", 0x4000, 0x6000, 0x1000)],
                vec![d("m", 0x2000)],
                vec![p("a", 0x100, "m", 0x100), p("b", 0x4100, "m", 0x100)],
                vec![r("a", "m", 1, &["a"]), r("b", "m", 1, &["b"])],
            );
            f.dma.push(dma("a", 0x100, "b", 0x4100, 1, false));
            Some(recover_case(f, vec![], vec![]))
        }
        // An observation implying a mapping, with an empty route list. No
        // interrogation.
        // Behaviour: every used mapping needs a supplied legal route.
        // Rejects: emitting decoders unchecked, or inventing a default route.
        "requires_route_for_every_used_mapping" => Some(recover_case(
            base(
                vec![h("h", 0, 0x2000, 0x1000)],
                vec![d("m", 0x2000)],
                vec![p("h", 0x100, "m", 0x100)],
                vec![],
            ),
            vec![],
            vec![],
        )),
        // Two hosts and their routes declared out of lexicographic order. No
        // interrogation.
        // Behaviour: output is ordered host then start, not as declared.
        // Rejects: emitting decoders in fabric declaration order.
        "decoder_order_is_host_then_start" => Some(recover_case(
            base(
                vec![h("z", 0, 0x2000, 0x1000), h("a", 0x8000, 0xa000, 0x1000)],
                vec![d("m", 0x2000)],
                vec![p("z", 0x100, "m", 0x100), p("a", 0x8100, "m", 0x100)],
                vec![r("z", "m", 1, &["z"]), r("a", "m", 1, &["a"])],
            ),
            vec![],
            vec![],
        )),
        // A host window whose granularity is coarser than the spacing of the
        // observed addresses. No interrogation.
        // Behaviour: alignment comes from the declared granularity.
        // Rejects: inferring granularity or base from the probed addresses, which
        // yields a finer, unaligned map.
        "alignment_is_not_inferred_from_probe_address" => Some(recover_case(
            base(
                vec![h("h", 0x2000, 0x6000, 0x2000)],
                vec![d("m", 0x4000)],
                vec![p("h", 0x2800, "m", 0x800), p("h", 0x5fff, "m", 0x3fff)],
                vec![r("h", "m", 1, &["s"])],
            ),
            vec![],
            vec![],
        )),
        // Frozen support case: one verifier-supplied decoder, interrogated at
        // both of its endpoints and at one address on each side.
        // Behaviour: start included, end excluded.
        // Rejects: inclusive-end translation, and an off-by-one at the start.
        "translate_is_half_open" => Some(Case {
            setup: Setup::Direct(vec![dec("h", 10, 20, "m", 30)]),
            translate: vec![q("h", 9), q("h", 10), q("h", 19), q("h", 20)],
            dma: vec![],
        }),
        // Frozen support case: verifier-supplied empty decoder list,
        // interrogated with a zero-length DMA observation.
        // Behaviour: a zero-length DMA is never coherent.
        // Rejects: treating a vacuous span as trivially satisfied.
        "zero_length_dma_is_never_coherent" => Some(Case {
            setup: Setup::Direct(vec![]),
            translate: vec![],
            dma: vec![dma("a", 0, "b", 0, 0, false)],
        }),
        // Frozen support case: a host window declared with granularity 0. No
        // interrogation.
        // Behaviour: granularity zero is rejected.
        // Rejects: dividing or taking a remainder by the granularity before
        // validating it.
        "malformed_windows_fail_closed" => Some(recover_case(
            Fabric {
                hosts: vec![h("h", 0, 5, 0)],
                devices: vec![d("m", 10)],
                probes: vec![],
                dma: vec![],
                routes: vec![],
            },
            vec![],
            vec![],
        )),
        // A normative rule the spec states and the V1 fixtures never exercised.
        // One host, one device, one route and a single positive observation whose
        // target offset sits half a granule off the host address it is reported
        // at. Every other published rule is satisfied: the window is aligned and
        // within the granule bound, the address is inside it, the offset is inside
        // capacity, and a legal route is supplied. No interrogation.
        // Behaviour: the device offset is on the granularity lattice too, so this
        // evidence describes no programmable decoder and recovery fails closed.
        // Rejects: aligning the host window only and taking the device offset
        // straight from the observation, which returns a map that reproduces the
        // observation through a decoder no fabric could program.
        "device_offset_must_be_granularity_aligned" => Some(recover_case(
            base(
                vec![h("h", 0x2000, 0x6000, 0x1000)],
                vec![d("m", 0x4000)],
                vec![p("h", 0x2100, "m", 0x900)],
                vec![r("h", "m", 1, &["s"])],
            ),
            vec![],
            vec![],
        )),
        _ => None,
    }
}
