# Reconstruct a circular plasmid map

Repair `src/lib.rs` without changing its public API. `SiteDomain` entries are
original known synthetic sequence blocks: each named site must choose exactly
one supplied `(position, forward)` option. Positions are in `[0,length)` and no
two sites may occupy one position.

For each digest, collect all cuts belonging to any named enzyme, deduplicate
coincident cuts, and emit clockwise circular fragment lengths. One cut emits the
whole length. Compare fragment **multisets** after sorting. A paired absolute
error exactly equal to tolerance is accepted; a pair beyond tolerance counts as
unmatched. Junction evidence requires two site IDs to be circular neighbors and
their orientation equality to match `same_orientation`.

Minimize `(unmatched,total_accepted_error,junction_violations,canonical_bytes)`
globally. Rotations and whole-map reversal are equivalent. Reversal maps
position `p` to `(-p mod length)` and flips every orientation. Return the least
byte serialization across every rotation of both directions. Hidden checks run
withheld digests on the returned map.

Run `cargo run --offline --example diagnostic` for synthetic public feedback on
circular digestion, joint evidence, orientation, and canonical normalization.
