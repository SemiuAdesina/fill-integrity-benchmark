# Recover per-host CXL fabric address maps

The Rust crate to repair is at `/app`. Leave the corrected source tree there.
The public data model is in `src/lib.rs`; the normative decoder and routing
semantics are in `/app/spec/cxl-model.md`.

`recover` currently creates one decoder from each positive probe and takes the
first route it sees. That local strategy is not a reconstruction. Repair it so
it recovers a globally valid, canonical per-host map from all address and DMA
evidence.

Your result must preserve host-specific views, aligned half-open ranges,
non-overlap, device-capacity endpoints, shared device offsets, and the exact DMA
range model. Select the minimum decoder count, coalescing compatible adjacent
ranges. Choose legal routes by total cost and the documented hop-sequence tie.
Return decoders and routes in canonical order. Negative observations are
authoritative.

The supplied fixtures are bounded as stated in the spec. Do not use the network,
change the public structs, or replace the coherent-memory problem with a lookup
table. `cargo run --offline --bin demo` shows a small starting failure.

Run `cargo test --offline --locked --test diagnostics` for synthetic public
checks of the individual translation, DMA, evidence, routing, capacity and
validation rules. Each failure names the public rule still violated; the
diagnostics contain no graded fixtures or hidden expected answers.
