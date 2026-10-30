# Map format, version 1

What `/app/map.json` must contain, and which map is the answer.

## 1. The document

```
{
  "cloning_site": 417,
  "cuts": {"EcoRI": [396, 2739], "PstI": [1211, 2761]},
  "format_version": 1,
  "held_out": {"HindIII+PstI": [{"length": 1550, "multiplicity": 1}, {"length": 3458, "multiplicity": 1}]},
  "insert_length": 2322
}
```

* `cloning_site`: `c`, one of the positions in `enzymes.json`.
* `insert_length`: `L`, within the bounds in `digests.json`.
* `cuts`: for every panel enzyme and no other, every cut position of that
  enzyme on the construct circle of `V + L` bases, ascending, each once.
* `held_out`: for every held-out digest and no other, keyed by id, the
  predicted bands of the map's own cut positions for that digest
  (`construct-model-v1.md`, section 5), grouped: ascending by length, equal
  lengths merged into one band whose multiplicity is their number.
* Members are exactly those shown; unknown members are rejected. Numbers are
  non-negative JSON integers.

The supplied writer (`src/result_output.rs`) emits this document with members
in alphabetical order and no whitespace; a solver that uses it needs no
further care.

## 2. Validity

A map is **valid** when all of the following hold:

1. `cloning_site` is a listed cloning site and `insert_length` is within
   bounds.
2. For every enzyme, the positions outside the insert region `[c, c + L]`
   are exactly the enzyme's required positions for `(c, L)`
   (`construct-model-v1.md`, section 4), and at most `max_internal_sites`
   positions lie inside it.
3. `held_out` is what the map's own positions predict.

A document that is not valid JSON, that does not have the shape of section 1,
or that describes a map that is not valid is wrong, whatever else it
contains.

## 3. The answer

Consider every valid map that reproduces every observed digest
(`construct-model-v1.md`, section 7). The answer is the least of them under
the following order, compared component by component:

1. the total number of insert-region positions over the panel, fewer first;
2. the total deviation over the observed digests, smaller first;
3. the list of `(enzyme name, position)` pairs of every cut position,
   enzymes in ascending name order and positions ascending within an
   enzyme, compared lexicographically: pair by pair, a pair by name then by
   position, and a list that is a proper prefix of the other is smaller;
4. the cloning site, smaller first;
5. the insert length, shorter first.

Two distinct valid maps never compare equal, so the answer is unique.

## 4. Determinism

Two runs over the same problem must produce byte-identical documents.
