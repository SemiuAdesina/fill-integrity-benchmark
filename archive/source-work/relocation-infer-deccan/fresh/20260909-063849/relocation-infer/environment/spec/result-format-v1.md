# Result format, version 1

The deliverable is one JSON document, `/app/relocations.json`, produced by
`reloc-recover` from the corpus alone. It carries the recovered program of
every relocation type and a predicted patched word for every site of every
link in the manifest.

## 1. Document

```
{
  "format_version": 1,
  "programs": [
    {"type": 2, "expression": "S+A-P", "scale": 4, "signedness": "signed",
     "overflow": "reject", "slices": [{"lsb": 12, "width": 20}], "pair": null},
    {"type": 5, "expression": "S+B+A-P", "scale": 1, "signedness": "signed",
     "overflow": "reject", "slices": [{"lsb": 12, "width": 20}],
     "pair": {"group": "G1", "role": "hi", "split": 12}}
  ],
  "predictions": [
    {"object": "obj01", "link": "l01", "section": ".text", "offset": 0,
     "word": 2151677952},
    {"object": "obj01", "link": "l05", "section": ".text", "offset": 12,
     "word": null}
  ]
}
```

## 2. Programs

* Exactly one entry per relocation type listed in the manifest, no more and
  no fewer, ordered by ascending `type`.
* Every field of `relocation-program-v1.md`, section 1, is present; the
  program satisfies every constraint of that document, including normal form
  and the pair rules.
* The program for each type must be the required answer of
  `relocation-program-v1.md`, section 5: the fitting program with the smallest
  canonical key.

## 3. Predictions

* Exactly one entry per site per link, for every link of every object in the
  manifest whatever its `result.status`, and no other entries.
* Entries are ordered by `object`, then `link`, then `section` (bytewise
  string order), then `offset` ascending.
* `word` is the patched word the submitted program set produces for the site
  in that link under `replay-semantics-v1.md`, as a non-negative integer below
  `2^container_bits`, or JSON `null` when the site is rejected.

Predictions are derived values: the verifier recomputes every one from the
submitted programs and the manifest, and a prediction that disagrees with
that recomputation, or with the legacy linker's recorded outcome, is wrong
even when the programs are right.

## 4. Numbers and encoding

All numbers are JSON integers written in decimal without exponent, sign
prefix (other than `-` for negative addends, which do not appear in this
document), leading zeros or fractional part. Strings are ASCII. The document
is UTF-8 with no byte-order mark. Object member names are exactly as shown;
no other members are permitted.

## 5. Rejection

A document is rejected without further evaluation when it is not valid JSON,
when any required member is missing or has the wrong type, when an unknown
member is present, when `format_version` is not 1, when a program violates
`relocation-program-v1.md`, or when the program or prediction lists violate
sections 2 and 3 above.
