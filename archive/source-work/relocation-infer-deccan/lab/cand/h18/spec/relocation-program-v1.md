# Relocation programs, version 1

A **relocation program** is the complete machine-readable description of how
the legacy linker computes and encodes one relocation type. This document
defines the finite space of programs, the normal form every program must be
written in, and the total order that selects one program when several are
observationally equivalent. `replay-semantics-v1.md` defines what a program
does; `result-format-v1.md` defines how it is serialised.

## 1. Fields

| field | values |
|---|---|
| `expression` | `"S"`, `"S+A"`, `"S-P"`, `"S+A-P"`, `"S+B+A-P"` |
| `scale` | 1, 2, 4, 8 |
| `signedness` | `"unsigned"`, `"signed"` |
| `overflow` | `"reject"`, `"wrap"` |
| `slices` | an ordered list of 1 to 3 slices, each `{"lsb": n, "width": m}` |
| `pair` | `null`, or `{"group": g, "role": "hi" or "lo", "split": k}` |

The **width** `w` of a program is the sum of its slice widths. There is no
separate width field.

## 2. Slices

A slice designates `width` consecutive bits of the container word, starting at
bit `lsb`, where bit 0 is the least significant bit of the container word read
as a little-endian integer and bit `container_bits - 1` is the most
significant. Slices are listed in **logical order**: the first slice holds the
least significant `width` bits of the encoded value, the second slice holds the
next bits, and so on. Physical position and logical order are independent: a
later slice may sit at a lower bit position than an earlier one.

Constraints:

* every slice has `width >= 1` and `lsb + width <= container_bits`;
* slices do not overlap;
* there are at most 3 slices;
* **normal form**: no slice is immediately followed, in logical order, by a
  slice that begins at `lsb + width` of the previous one. Two such slices
  would be one contiguous slice and must be written as one.

Every container bit that belongs to no slice is a preserved bit: replay never
changes it.

## 3. Pairs

Types whose `pair_group` in the manifest is `null` have `pair: null`. The two
members of a pair group each carry a `pair` object with that `group`,
complementary roles (one `"hi"`, one `"lo"`) and the same `split` `k`,
`1 <= k <= 63`. Both members also carry the same `expression` and the same
`scale`; their `signedness`, `overflow` and `slices` are independent. A
program set in which the two members disagree on `expression`, `scale`,
`group` or `split`, or in which they do not have complementary roles, is
invalid.

## 4. Canonical key

Programs are totally ordered by the following tuple, compared
lexicographically, component by component, smaller first:

1. `expression`, ranked `S` = 0, `S+A` = 1, `S-P` = 2, `S+A-P` = 3,
   `S+B+A-P` = 4;
2. `scale`, numerically;
3. `signedness`, ranked `unsigned` = 0, `signed` = 1;
4. `overflow`, ranked `reject` = 0, `wrap` = 1;
5. the number of slices;
6. the slices in logical order, each compared as `(lsb, width)`;
7. `pair`: `null` = 0 before any pair object = 1;
8. `pair.role`, ranked `hi` = 0, `lo` = 1;
9. `pair.split`, numerically.

For a pair group the two members are ordered together: the joint key is the
key of the member with the smaller relocation type number followed by the key
of the other member.

## 5. Fitting and the canonical program

A program **fits** a relocation type when replaying it (`replay-semantics-v1.md`)
over every link of every object in the corpus reproduces every observation:
every legacy word of every `"linked"` link, and every acceptance and rejection
recorded by every `"failed"` link. A pair of programs fits a pair group when
replaying it reproduces every observation of both members.

Several programs may fit the same observations. The **required answer** for a
type is the fitting program with the smallest canonical key (for a pair group,
the fitting pair with the smallest joint key). Some equivalences are
unconditional consequences of the semantics — for example, with
`overflow = "wrap"` the encoded bits do not depend on `signedness` — and the
key resolves them the same way as any other tie.

One program describes one relocation type everywhere it occurs: across every
site, every object and every link of the corpus.
