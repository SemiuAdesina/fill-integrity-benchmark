# CXL map reconstruction crate

This crate contains a bounded synthetic coherent-memory fabric and a deliberately
incomplete map recovery implementation. The normative semantics are in
`spec/cxl-model.md`. Run `cargo run --offline --bin demo` for a small reproducer.

## Public feedback ladder

Run `cargo test --offline --locked --test diagnostics`. These inputs are public
synthetic examples, not copies of verifier fixtures.

| Semantic family | Executable diagnostic | Hidden coverage | Load-bearing |
|---|---|---|---|
| Half-open translation | `translation_uses_half_open_decoder_endpoints` | endpoint cases | yes |
| Full DMA span | `dma_checks_the_complete_half_open_span` | DMA compound | yes |
| Negative evidence + coalescing | `negative_evidence_limits_and_coalesces_the_decoder` | range recovery | yes |
| Route cost/tie | `equal_cost_routes_use_hop_sequence_order` | route cases | yes |
| Per-host views + ordering | `host_views_are_recovered_independently` | multi-host compound | yes |
| Device capacity endpoint | `translated_interval_may_end_exactly_at_capacity` | capacity cases | yes |
| Malformed input | `malformed_host_windows_fail_before_recovery` | validation cases | yes |
