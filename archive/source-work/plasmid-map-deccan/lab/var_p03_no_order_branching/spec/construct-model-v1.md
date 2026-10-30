# Construct model, version 1

How a cloning construct is described, how restriction enzymes cut it, and
what a digest gel shows. This file, `input-format-v1.md` and
`map-format-v1.md` are the complete public contract. String comparison
anywhere in the contract is bytewise; every length and position is an
integer number of bases.

## 1. Circles and positions

A DNA circle of `n` bases has bases `0 .. n - 1`; base `n - 1` is followed by
base `0`. A **cut position** `p`, with `0 <= p < n`, is the point between base
`p - 1` and base `p` (between base `n - 1` and base `0` when `p = 0`).

## 2. Recognition sites

An enzyme has a recognition sequence of `w` bases and a cut offset `o` with
`1 <= o <= w - 1`. The enzyme has a **site** at every window start `i`,
`0 <= i < n`, at which the `w` bases `i, i + 1, ..., i + w - 1`, taken modulo
`n`, equal the recognition sequence. The site's **cut position** is
`(i + o) mod n`. Sites are found on the given strand only; the recognition
sequences supplied are their own reverse complements, so this finds every
site. Two windows are two sites even when their cut positions coincide; a set
of cut positions holds each position once.

## 3. The construct

The **backbone** is the circular vector of `V` bases in `backbone.fasta`. The
construct is the backbone opened at a **cloning site** `c`, one of the
positions listed in `enzymes.json`, with an **insert** of `L` bases placed
between backbone base `c - 1` and backbone base `c`. The construct is a
circle of `N = V + L` bases: bases `0 .. c - 1` of the backbone, then the
`L` insert bases, then bases `c .. V - 1` of the backbone.

The insert's sequence is unknown. `L` is unknown within the bounds
`insert_length.min <= L <= insert_length.max` of `digests.json`.

## 4. Backbone sites on the construct

A backbone site **spans the junction** at `c` when its window contains both
base `(c - 1) mod V` and base `c mod V`. Insertion tears such a window apart,
so it says nothing about the construct. Every other backbone site survives:
its cut position on the construct is `p` when `p < c` and `p + L` when
`p >= c`, where `p` is its cut position on the backbone.

For an enzyme and an opening `(c, L)`, the **required positions** are the
construct cut positions of the enzyme's surviving backbone sites. They all
lie outside the closed **insert region** `[c, c + L]`: below `c`, or above
`c + L`.

The enzyme's **insert-region positions** are its construct cut positions
inside `[c, c + L]`. A cut at `c` or at `c + L` lies at a junction and is an
insert-region position. Every enzyme has at most `max_internal_sites`
insert-region positions.

`enzymes.json` guarantees that every panel enzyme keeps at least one
surviving backbone site at every listed cloning site, so every enzyme has at
least one required position for every opening.

## 5. Digests

A **digest** is a non-empty set of panel enzymes. Its **cuts** on a circle of
`N` bases are the union of the enzymes' cut positions, each position once. With
`k >= 1` cuts `p_1 < p_2 < ... < p_k` the **fragments** are the `k` circular
gaps `p_2 - p_1, ..., p_k - p_(k-1), N - p_k + p_1`; with no cut the digest
has one fragment of length `N`.

Fragments shorter than the **detection limit** `D` do not appear on the gel.
The digest's **predicted bands** are its fragment lengths that are at least
`D`, in ascending order, one entry per fragment; equal lengths appear as many
times as they occur.

## 6. Observed bands and matching

An observed digest lists bands ascending by length, each with a
**multiplicity**, the number of fragments that ran together in it. Its
**expanded** form repeats each length as many times as its multiplicity, in
ascending order.

The **tolerance** of an observed length `b` is `ceil(e * b / 100)` where `e`
is `error_percent`: with integer arithmetic, `(e * b + 99) div 100`.

A predicted band list **matches** an observed digest when the two lists have
the same number of entries and, pairing them in ascending order, every
predicted length lies within the tolerance of the observed length it is
paired with. The **deviation** of a match is the sum over pairs of the
squared difference of the two lengths. A pairing that is not by ascending
order is never considered.

## 7. Reproduction

A map (`map-format-v1.md`, section 1) **reproduces** an observed digest when
the predicted bands of that digest's enzymes over the map's cut positions on
the circle of `V + L` bases match the observed bands. A map reproduces the
problem when it reproduces every observed digest; its **total deviation** is
the sum of the deviations.
