
# Cross input format, version 1

The primary input is a single JSON document at `/app/cross.json`. This file
describes one biparental cross of two autotetraploid parents and the low depth
read evidence observed for their progeny.

Everything here is normative. A document that violates any rule below is invalid
and must be rejected rather than repaired.

## Top level object

| Field | Type | Meaning |
|---|---|---|
| `format_version` | integer | Must be exactly `1`. Any other value is rejected. |
| `markers` | integer | Number of markers, `M`. Must satisfy `12 <= M <= 28`. |
| `offspring` | integer | Number of progeny, `N`. Must satisfy `8 <= N <= 32`. |
| `parent_dosage` | object | Alternate allele dosage of each parent at each marker. |
| `child_gl` | array | Per progeny, per marker genotype log likelihoods. |
| `recombination` | object | Per parent recombination fraction for each marker interval. |
| `double_reduction` | object | Per parent double reduction legality and rate. |

No other top level field is permitted. An unknown field is a rejection, not
something to ignore: silently accepting it would let a future format change be
misread as a version 1 document.

## Markers and alleles

Markers are biallelic. Allele `0` is the reference allele and allele `1` is the
alternate allele. Markers are ordered along a single linkage group and marker
index `m` runs from `0` to `M - 1` in that order.

## Parents and homologs

There are exactly two parents, named `"A"` and `"B"`. The two parents are
distinguishable and are never exchanged.

Each parent carries exactly four homologs, indexed `0` to `3`. A parental
haplotype assignment gives every homolog an allele at every marker, written
`hap[P][h][m]` with a value of `0` or `1`.

## `parent_dosage`

```json
"parent_dosage": { "A": [ /* M integers */ ], "B": [ /* M integers */ ] }
```

Each entry is an integer in `0..=4`. It is the number of homologs of that parent
carrying the alternate allele at that marker, so any admissible haplotype
assignment must satisfy

```
sum over h in 0..4 of hap[P][h][m]  ==  parent_dosage[P][m]
```

at every marker. This is a hard constraint on the search space, not a
preference. The number of admissible allele columns for one parent at one marker
is therefore `C(4, parent_dosage[P][m])`, which is `1, 4, 6, 4, 1` for dosages
`0, 1, 2, 3, 4` respectively.

## `child_gl`

```json
"child_gl": [ /* N entries */ [ /* M entries */ [ g0, g1, g2, g3, g4 ] ] ]
```

`child_gl[c][m][d]` is the natural logarithm of the likelihood of the reads
observed for progeny `c` at marker `m` given that the progeny's alternate allele
dosage there is exactly `d`. There are always five values, for dosages `0`
through `4` in that order.

Values are finite negative or zero doubles. They are likelihoods under a
dosage hypothesis, not posterior probabilities, and they are not normalised.

Depth is low by construction, so the five values for a marker are frequently
close together and frequently do not identify a dosage on their own.

## `recombination`

```json
"recombination": { "A": [ /* M-1 doubles */ ], "B": [ /* M-1 doubles */ ] }
```

`recombination[P][m]` is the recombination fraction for the interval between
marker `m` and marker `m + 1` in parent `P`. Each array has exactly `M - 1`
entries. Each value satisfies `0.0 < r <= 0.5`.

The two parents recombine independently and generally differ.

## `double_reduction`

```json
"double_reduction": {
  "A": { "allowed": [ /* M booleans */ ], "rate": [ /* M doubles */ ] },
  "B": { "allowed": [ /* M booleans */ ], "rate": [ /* M doubles */ ] }
}
```

`allowed[P][m]` states whether a double reduction inheritance state is
biologically admissible for parent `P` at marker `m`. `rate[P][m]` is the
probability mass assigned to the double reduction form at that marker.

The two arrays must agree with each other:

- If `allowed[P][m]` is `false` then `rate[P][m]` must be exactly `0.0`.
- If `allowed[P][m]` is `true` then `rate[P][m]` must satisfy `0.0 < rate < 1.0`.

A document where they disagree is invalid. Legality is decided by `allowed`, and
scoring is decided by `rate`; they are kept separate so that neither can be
inferred from the other by accident.

Legality is per parent and per marker. A marker may permit double reduction in
parent `A` and forbid it in parent `B`, and two adjacent markers may differ.

## Validation

A conforming reader rejects a document that fails any of the following.

1. `format_version` is not exactly `1`.
2. Any required field is absent, or any field not listed above is present.
3. `markers` outside `12..=28`, or `offspring` outside `8..=32`.
4. `parent_dosage` missing a parent, wrong length, or an entry outside `0..=4`.
5. `child_gl` with the wrong number of progeny, the wrong number of markers for
   any progeny, or a marker entry that does not have exactly five values.
6. Any `child_gl` value that is not finite.
7. `recombination` missing a parent, of length other than `M - 1`, or with an
   entry outside `0.0 < r <= 0.5`.
8. `double_reduction` missing a parent or either array, an array of length other
   than `M`, or an `allowed` and `rate` pair that disagree as described above.

Rejection is a hard failure. There is no repair, no default and no partial
acceptance.

## Size limits

The reader enforces an explicit upper bound on input size before allocating. A
document larger than 8 MiB is rejected without being parsed. The bounds on `M`
and `N` are themselves the structural limits; nothing in the format grows
without one.
