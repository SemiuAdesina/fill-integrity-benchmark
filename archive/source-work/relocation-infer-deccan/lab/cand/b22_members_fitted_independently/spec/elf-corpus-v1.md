# ELF corpus format, version 1

This document defines the corpus under `/app/corpus/`: the relocatable objects
produced by the legacy toolchain, the outputs the legacy linker produced from
them, and the link maps that describe each link. Together with
`relocation-program-v1.md`, `replay-semantics-v1.md` and
`result-format-v1.md` it is the complete public contract. Nothing about the
corpus is defined anywhere else.

## 1. Layout

```
/app/corpus/manifest.json      the manifest (section 3)
/app/corpus/objects/<id>.o     ELF64 relocatable objects (ET_REL)
/app/corpus/linked/<id>.<link>.out
                               legacy-linked outputs (ET_EXEC), one per
                               successful link
```

## 2. ELF64 subset

Every file is a 64-bit little-endian ELF file (`EI_CLASS = ELFCLASS64`,
`EI_DATA = ELFDATA2LSB`) for the vendor machine `e_machine = 0x5658`. The
following structures are used and are the only ones the corpus relies on:

* ELF header: `e_type` (1 = relocatable, 2 = executable), `e_shoff`,
  `e_shentsize = 64`, `e_shnum`, `e_shstrndx`. There are no program headers.
* Section headers (64 bytes each, standard `Elf64_Shdr` layout): `sh_name`,
  `sh_type`, `sh_flags`, `sh_addr`, `sh_offset`, `sh_size`, `sh_link`,
  `sh_info`, `sh_addralign`, `sh_entsize`.
* Section types: `SHT_PROGBITS` (1) for content sections, `SHT_SYMTAB` (2),
  `SHT_STRTAB` (3), `SHT_RELA` (4). Objects contain no `SHT_REL` sections.
* Symbols (24 bytes each, standard `Elf64_Sym` layout): `st_name`, `st_info`,
  `st_other`, `st_shndx`, `st_value`, `st_size`. `st_shndx` is a section
  index, `SHN_ABS` (0xfff1) or `SHN_UNDEF` (0).
* RELA entries (24 bytes each, standard `Elf64_Rela` layout): `r_offset`,
  `r_info` with the symbol index in the upper 32 bits and the relocation type
  in the lower 32 bits, and a signed 64-bit `r_addend`.
* A relocation section `.rela<name>` relocates the section named `<name>`;
  its `sh_info` is that section's index and its `sh_link` is the symbol table
  index.

Linked outputs carry the same content sections as their object, with
`sh_addr` set to the section's link base and the relocation fields patched
by the legacy linker. They contain no relocation sections. Their symbol
tables are informational only; the manifest is authoritative for every value
used by replay (section 4).

## 3. Manifest

`manifest.json` is a JSON document:

```
{
  "format_version": 1,
  "machine": 22104,
  "relocation_types": [
    {"type": 2, "name": "R_VX_2", "container_bits": 32, "pair_group": null},
    {"type": 5, "name": "R_VX_5", "container_bits": 32, "pair_group": "G1"}
  ],
  "objects": [
    {
      "id": "obj01",
      "file": "objects/obj01.o",
      "links": [
        {
          "id": "l01",
          "bases": {".text": 67108864, ".data": 67109120, ".shared": 33554432},
          "symbols": {"ext_0_0": {"section": ".shared", "value": 4096},
                      "ext_0_1": {"section": null, "value": 67112960}},
          "result": {"status": "linked", "file": "linked/obj01.l01.out"}
        },
        {"id": "l05", "bases": {...}, "symbols": {...},
         "result": {"status": "failed",
                    "overflow": [{"section": ".text", "offset": 12}]}},
        {"id": "r01", "bases": {...}, "symbols": {...},
         "result": {"status": "replay"}}
      ]
    }
  ]
}
```

* `relocation_types` lists every relocation type that occurs in the corpus.
  `container_bits` is the width, 32 or 64, of the little-endian container
  word the type patches at `r_offset`. `pair_group` is `null` for an
  ordinary type; two types that share a non-null `pair_group` string are the
  two members of one high/low pair (section 5). A pair group has exactly two
  members.
* `objects[].id` and `links[].id` are unique identifiers; the `file` paths are
  relative to `/app/corpus/`.
* A link is one run of the legacy linker over one object. `bases` maps
  section names to their link-time base addresses; it always includes every
  content section of the object and may include further sections of the
  final program that the object does not define. `symbols` resolves every
  undefined symbol of the object (section 4).
* `result.status` is `"linked"` (the output file exists), `"failed"` (the
  legacy linker refused the link; `overflow` lists every relocation site it
  rejected, as `(section, offset)`, sorted by section name then offset) or
  `"replay"` (no legacy output is available; the recovered programs must
  predict this link).

All integers in the manifest are non-negative and below 2^63.

## 4. Sites and operands

A **site** is one RELA entry of one object: the relocation field starts at
byte `r_offset` of the relocated section and occupies `container_bits / 8`
bytes, read and written as one little-endian unsigned integer, the **container
word**. The container word as stored in the object is the **object word**;
after the legacy linker patched it in a linked output it is the **legacy
word**. Every site of an object exists in every link of that object.

For a site in link `L` the four replay operands are:

* **A** — `r_addend` of the RELA entry, a signed integer.
* **P** — the place: `bases[L][section]` plus `r_offset`, where `section` is
  the relocated section containing the site.
* **S** and **B** — determined by the symbol the entry references:
  * `st_shndx` names a section of the object: `S = st_value` (the symbol's
    offset within that section) and `B = bases[L][that section]`.
  * `st_shndx = SHN_ABS`: `S = st_value` and `B = 0`.
  * `st_shndx = SHN_UNDEF`: the manifest entry `symbols[L][name]` gives
    `S = value` and, when its `section` is a name, `B = bases[L][section]`;
    when its `section` is `null` the symbol is absolute and `B = 0`.

`S`, `P` and `B` are non-negative integers below 2^63; `A` is a signed 64-bit
integer. Arithmetic on them is defined in `replay-semantics-v1.md`.

## 5. Pair instances

Two consecutive RELA entries of the same relocation section (consecutive in
the section's entry order, which is ascending `r_offset`) whose types are the
two members of one pair group and which reference the same symbol index with
the same addend form one **pair instance**. Either member may come first.
The first entry's place is the instance's `P`; both members share `S`, `A`,
`B` and that `P`. Every entry whose type belongs to a pair group is a member
of exactly one pair instance in this corpus.

## 6. Legacy failures

When `result.status` is `"failed"`, every site listed under `overflow` was
rejected by the legacy linker (`replay-semantics-v1.md`, section 4) and every
site of the object not listed was accepted; no patched words are available for
that link.
