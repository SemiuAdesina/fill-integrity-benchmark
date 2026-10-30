# Unsigned update-manifest validation and edit semantics

The asset metadata, prior manifest store, malformed update manifest, and finite
legal edit universe are supplied. This task never signs, hashes, parses general
JSON, or uses a network. Hash strings are synthetic values supplied by the
fixture and compared byte-for-byte.

Assertions have stable IDs and an ordered position. IDs and labels must be
nonempty and unique. An update is valid when its asset ID matches the asset;
exactly one action assertion has value `edited`; exactly one ingredient has
relationship `parent` and names a claim in the prior store for the same asset;
exactly one metadata assertion exactly matches the asset ID, MIME and hash; and
at least one hashed-URI assertion has URL `asset://<asset-id>`, the asset hash,
and a target label that resolves to exactly one earlier assertion in the
post-edit ordered manifest. Every hashed URI is checked.

Legal operations are delete assertion, replace label, replace action value,
replace ingredient relationship, and replace all hashed-URI fields. Operations
target stable assertion IDs. The public `Edit::key` encoding defines canonical
order. A chosen subset is sorted by that key, then applied in that exact order.
A later operation targeting an already deleted assertion fails. Deletes preserve
the relative order of everything remaining. Validation happens once, after the
entire sequence; individually valid intermediate states have no authority.

Choose the smallest subset whose application passes full validation. Break an
equal-cardinality tie by the lexicographically smallest vector of canonical edit
keys. Return that sequence, the repaired manifest, and the reference edges
derived by running the validator. The legal universe is unique by key and has at
most twenty edits.
