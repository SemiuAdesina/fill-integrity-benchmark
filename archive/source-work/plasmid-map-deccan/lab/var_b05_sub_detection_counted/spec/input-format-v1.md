# Input format, version 1

Three files describe one problem. `backbone.fasta` is the vector record,
`enzymes.json` the enzyme panel and the cloning sites, `digests.json` the
gel data and the bounds. The supplied reader (`src/input.rs`) rejects any
file that breaks a rule below; a rejected problem is never graded.

## 1. `backbone.fasta`

One FASTA record: a header line beginning with `>`, naming the source record,
then the sequence over any number of lines. The sequence is circular. Only
`A`, `C`, `G` and `T` are accepted, in either case; the sequence is folded to
upper case. A second record, an empty sequence, or any other character is an
error.

## 2. `enzymes.json`

```
{
  "format_version": 1,
  "enzymes": [
    {"name": "EcoRI", "site": "GAATTC", "cut_offset": 1},
    {"name": "PstI",  "site": "CTGCAG", "cut_offset": 5}
  ],
  "cloning_sites": [
    {"enzyme": "BamHI",   "position": 417},
    {"enzyme": "HindIII", "position": 447}
  ]
}
```

| member | meaning |
|---|---|
| `enzymes` | the panel: `name` (non-empty, unique, without `+`), `site` (4 to 12 upper-case bases), `cut_offset` (`1 <= cut_offset <= len(site) - 1`) |
| `cloning_sites` | the backbone positions at which the insert may have been placed: `position` is a cut position on the backbone (`0 <= position < V`), positions distinct; `enzyme` names the enzyme the site is known by and is informational |

Rules: at least one enzyme; at least one cloning site; every panel enzyme
keeps at least one surviving backbone site at every cloning site
(`construct-model-v1.md`, section 4).

Enzymes are used in ascending order of `name`; cloning sites in ascending
order of `position`.

## 3. `digests.json`

```
{
  "format_version": 1,
  "error_percent": 2,
  "detection_limit": 200,
  "insert_length": {"min": 500, "max": 6000},
  "max_internal_sites": 3,
  "observed": [
    {"id": "EcoRI", "enzymes": ["EcoRI"],
     "bands": [{"length": 1104, "multiplicity": 1}, {"length": 3941, "multiplicity": 1}]},
    {"id": "EcoRI+PstI", "enzymes": ["EcoRI", "PstI"],
     "bands": [{"length": 388, "multiplicity": 2}, {"length": 4270, "multiplicity": 1}]}
  ],
  "held_out": [
    {"id": "HindIII+PstI", "enzymes": ["HindIII", "PstI"]}
  ]
}
```

| member | meaning |
|---|---|
| `error_percent` | `e`, an integer from 1 to 20 |
| `detection_limit` | `D`, at least 1 |
| `insert_length` | `1 <= min <= max` |
| `max_internal_sites` | at least 0 |
| `observed` | the digests whose bands were read from gels |
| `held_out` | the digests whose bands the map must predict |

A digest's `enzymes` are panel names, ascending, without duplicates; its `id`
is the names joined by `+` in that order. Ids are unique across `observed`
and `held_out` together. Bands are ascending by `length` with distinct
lengths; `length` and `multiplicity` are at least 1. A digest may have no
bands.

Every panel enzyme has an observed single digest, the digest over that
enzyme alone.

## 4. What graded problems look like

Every graded problem has at least one valid map that reproduces it. Backbones
are real cloning vectors of 2.5 to 5.5 kilobases; panels have 3 to 5 enzymes;
inserts are real restriction fragments of up to 6 kilobases; every single
digest and three to six double digests are observed, two digests are held
out; `error_percent` is 1 to 3 and `detection_limit` 100 to 300. The public
problem in `/app` is one such problem.
