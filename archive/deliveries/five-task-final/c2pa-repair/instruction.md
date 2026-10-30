# Repair a malformed C2PA update manifest

The Rust crate is at `/app`. Repair `src/lib.rs` and leave the corrected source
tree there. The public model and edit types are already defined; the normative
unsigned update validator and edit interpreter are in
`/app/spec/update-validation.md`.

The current `repair` greedily tries individual edits and then all supplied edits.
Instead, find the minimum-cardinality subset of the finite legal edit universe
whose complete application passes the full validator. Sort chosen edits by the
public key before application, preserve assertion ordering except for deletions,
and use the lexicographically smallest key vector when multiple minimum repairs
exist. Return the selected edits, fully repaired manifest, and validator-derived
reference graph.

Validity includes exact action and parent cardinality, prior-store resolution,
exact asset metadata, unique IDs and labels, hashed-URI values, and post-edit
backward label references. Validate only after a complete sequence. Do not add
general JSON repair, signing, key management, hashing, or network dependencies.
