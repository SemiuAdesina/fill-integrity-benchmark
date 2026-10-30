# Replay semantics, version 1

This document defines exactly what patched word, if any, a relocation program
produces for a site in a link. It is the arithmetic the legacy linker
implements; the verifier replays submitted programs with the same rules.

All arithmetic is over mathematical integers. No step is performed in a
fixed machine width unless this document says so.

## 1. Operands

`S`, `A`, `P` and `B` are defined per site and link in `elf-corpus-v1.md`,
section 4. For a pair instance the shared `P` is the place of the instance's
first RELA entry (`elf-corpus-v1.md`, section 5).

## 2. Value

1. Evaluate the program's `expression` over the integers:
   `S`, `S + A`, `S - P`, `S + A - P` or `S + B + A - P`.
2. Divide the result by `scale`, truncating toward zero: the quotient `Q` is
   the integer with `|Q| = floor(|V| / scale)` and the sign of `V` (zero when
   `|V| < scale`).

For a program with `pair: null`, the **component** `X` is `Q`.

For a pair instance, `Q` is computed once from the shared operands and the
shared `expression` and `scale`, and split at `k = split`:

* `lo = Q mod 2^k`, the Euclidean remainder, always in `[0, 2^k - 1]`;
* `hi = (Q - lo) / 2^k`, an exact integer (the Euclidean quotient; it is
  negative when `Q < 0`).

The member whose role is `"lo"` has component `X = lo`; the member whose
role is `"hi"` has component `X = hi`. Each member then continues
independently with its own `signedness`, `overflow` and `slices`.

## 3. Width semantics

Let `w` be the program's width (sum of slice widths). The representable range
is

* `signedness = "signed"`: `[-2^(w-1), 2^(w-1) - 1]`,
* `signedness = "unsigned"`: `[0, 2^w - 1]`.

Both endpoints are included.

## 4. Overflow

If `X` is inside the range, the encoded bits are `X mod 2^w` (the Euclidean
remainder, i.e. the ordinary two's-complement pattern of `X` in `w` bits).

If `X` is outside the range:

* `overflow = "reject"`: the relocation is **rejected**. The legacy linker
  refuses the whole link and reports the site (`elf-corpus-v1.md`, section 6);
  a prediction for the site is JSON `null` (`result-format-v1.md`).
* `overflow = "wrap"`: the encoded bits are `X mod 2^w`, the Euclidean
  remainder in `[0, 2^w - 1]`.

Rejection is decided per member of a pair: a rejected member does not affect
the other member's outcome.

## 5. Encoding

The encoded bits `E` (`0 <= E < 2^w`) are written into the container word.
Slices are consumed in logical order: the first slice receives bits
`0 .. width_1 - 1` of `E`, the second receives the next `width_2` bits, and so
on, until every bit of `E` has been placed. Within a slice, bit `i` of the
chunk is written to container bit `lsb + i`.

Every container bit that belongs to no slice keeps the value it has in the
object word. The result, read as a little-endian integer of `container_bits`
bits, is the **patched word**.

## 6. Observation

A `"linked"` link records the patched word of every site of the object. A
`"failed"` link records that every listed site was rejected and every
unlisted site was accepted, with no patched words. A `"replay"` link records
nothing; the recovered programs must predict it.
