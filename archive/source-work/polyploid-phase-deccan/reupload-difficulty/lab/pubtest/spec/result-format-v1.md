
# Result output format, version 1

The program writes exactly one JSON document to `/app/result.json`.

## Shape

```json
{
  "format_version": 1,
  "haplotypes": {
    "A": [ [ /* M values of 0 or 1 */ ], /* four rows */ ],
    "B": [ [ /* M values of 0 or 1 */ ], /* four rows */ ]
  },
  "paths": [
    {
      "A": [ [a, b], /* M entries */ ],
      "B": [ [a, b], /* M entries */ ]
    }
    /* N entries, progeny 0 first */
  ],
  "total_log_likelihood": -123.456789,
  "canonical_digest": "0123456789abcdef"
}
```

Every field is required. Rows of `haplotypes` are in homolog index order `0..3`.
Entries of `paths` are in progeny index order. Each path entry is an array of `M`
two element arrays, in marker order.

Each `[a, b]` entry must already satisfy `0 <= a <= b < 4`. A pair written in the
other order is not a different notation for the same state, it is malformed
output, and it is rejected without being scored.

`total_log_likelihood` is the value the frozen scorer assigns to the
reconstruction being reported. It must agree with the global optimum to within
`1e-8`, and it must agree with what the scorer actually computes for the
haplotypes and paths in the same document. Reporting an optimal number beside a
suboptimal reconstruction fails both checks.

## Canonical form

Homolog indices are arbitrary labels. Permuting the four homolog rows of a parent
and applying the same permutation to every inheritance state for that parent
yields a reconstruction with an identical score. All such reconstructions are the
same answer, so exactly one of them is accepted.

The canonical form of a reconstruction is produced as follows, independently for
parent `A` and parent `B`. The parents are never exchanged with each other.

1. Read each of the four homolog rows as a sequence of `M` values, marker `0`
   first.
2. Consider every permutation `p` of `{0, 1, 2, 3}`, where applying `p` moves the
   row currently at index `h` to index `p(h)`.
3. Keep only those permutations whose resulting four rows are in ascending
   lexicographic order, comparing rows element by element with `0 < 1`.
4. If more than one permutation survives step 3 — which happens exactly when two
   or more rows are identical — keep only those that minimise the *path
   signature*: the flat integer sequence obtained by walking progeny in index
   order, markers in order, emitting the remapped `a` then the remapped `b` for
   this parent, and comparing those sequences lexicographically.
5. If more than one permutation still survives, take the one whose image tuple
   `(p(0), p(1), p(2), p(3))` is lexicographically smallest.
6. Apply the chosen permutation: reorder the rows, and rewrite every inheritance
   state `(a, b)` for this parent as the sorted pair of `p(a)` and `p(b)`.

Steps 4 and 5 exist because step 3 alone does not determine a unique
permutation. When two homologs carry identical alleles at every marker, swapping
them leaves the haplotype matrix untouched but changes the paths, so the paths
have to break the tie or the output is not reproducible.

Canonicalisation is idempotent: canonicalising an already canonical
reconstruction returns it unchanged.

## Choosing between distinct optima

Canonical form removes relabelling. It does not remove the possibility that two
genuinely different reconstructions attain the same optimal score. When that
happens the required answer is the smallest in the following order, first
difference deciding:

1. The canonical haplotype matrix of parent `A`, flattened row major into `4 * M`
   values.
2. Then the canonical haplotype matrix of parent `B`, flattened the same way.
3. Then the full path signature: progeny in index order, and for each progeny
   parent `A`'s states then parent `B`'s states, each state contributing `a` then
   `b`.

Two totals count as the same score when they differ by at most `1e-8`, as
defined in `scoring-model.md`.

## `canonical_digest`

The digest is a 64 bit FNV-1a hash of a fixed serialisation of the canonical
reconstruction, written as exactly sixteen lowercase hexadecimal characters,
most significant nibble first, zero padded.

The hash is defined by:

```
h = 14695981039346656037
for each byte x:
    h = h XOR x
    h = (h * 1099511628211) mod 2^64
```

The byte sequence it consumes is built from the canonical reconstruction, in
this order:

1. For parent `A`, then parent `B`:
   - for each homolog row `0..3`, for each marker `0..M-1`, the ASCII byte
     `'0'` or `'1'` for that allele;
   - then one ASCII `'|'` after the parent's four rows.
2. For each progeny in index order:
   - for parent `A`, then parent `B`: for each marker in order, the ASCII digit
     for `a` then the ASCII digit for `b`;
   - then one ASCII `';'` after that progeny's two parents.
3. One final ASCII `'#'`.

The score is deliberately not part of the digest. The score is checked
separately against a tolerance; the digest covers the discrete structure only, so
that it is exactly reproducible.

A helper implementing this hash ships with the project. What it hashes is your
canonical reconstruction, so the helper being supplied does not make the digest
free: an answer that is optimal but not canonical, or canonical but not optimal,
produces a digest that does not match.

## Rejection

The result is rejected, and no partial credit is given, when any of the
following holds.

- The document is absent, is not valid JSON, or has a `format_version` other
  than `1`.
- A required field is missing or has the wrong shape or length.
- A haplotype value is not `0` or `1`, or a parent's rows do not sum to
  `parent_dosage` at some marker.
- A state pair is out of range or is written with `a > b`.
- A state is a double reduction form at a marker where that parent does not
  allow it.
- The reconstruction is not in canonical form.
- `total_log_likelihood` disagrees with the scorer's value for the reported
  haplotypes and paths by more than `1e-8`.
- The score is below the global optimum by more than `1e-8`.
- `canonical_digest` is not sixteen lowercase hexadecimal characters, or does
  not match the digest of the reported canonical reconstruction.
