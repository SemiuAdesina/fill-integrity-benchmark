#!/usr/bin/env python3
"""Author-side executable model of the PUBLIC relocation-recovery contract.

Everything here is derived from the public specification set that ships in the
task (spec/elf-corpus-v1.md, spec/relocation-program-v1.md,
spec/replay-semantics-v1.md, spec/result-format-v1.md).  It is used by the
corpus generator and by the two certification oracles.  It never ships.
"""
import json
import struct
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple

# --------------------------------------------------------------------- grammar
EXPRS = ["S", "S+A", "S-P", "S+A-P", "S+B+A-P"]
SCALES = [1, 2, 4, 8]
SIGNS = ["unsigned", "signed"]
OVFS = ["reject", "wrap"]
ROLES = ["hi", "lo"]
MAX_SLICES = 3
EM_VX = 0x5658  # 'VX' — fictional vendor machine number used by the corpus

Slice = Tuple[int, int]  # (lsb, width)


@dataclass(frozen=True)
class Program:
    expr: str
    scale: int
    sign: str
    ovf: str
    slices: Tuple[Slice, ...]
    pair: Optional[Tuple[str, str, int]] = None  # (group, role, split k)

    @property
    def width(self) -> int:
        return sum(w for _, w in self.slices)

    def key(self):
        pr = (0, 0, 0) if self.pair is None else (1, ROLES.index(self.pair[1]), self.pair[2])
        return (EXPRS.index(self.expr), self.scale, SIGNS.index(self.sign), OVFS.index(self.ovf),
                len(self.slices), tuple(self.slices)) + pr

    def to_json(self, type_id: int) -> dict:
        d = {"type": type_id, "expression": self.expr, "scale": self.scale, "signedness": self.sign,
             "overflow": self.ovf, "slices": [{"lsb": l, "width": w} for l, w in self.slices]}
        d["pair"] = None if self.pair is None else {"group": self.pair[0], "role": self.pair[1], "split": self.pair[2]}
        return d

    @staticmethod
    def from_json(d: dict) -> "Program":
        pair = None
        if d.get("pair") is not None:
            pair = (d["pair"]["group"], d["pair"]["role"], int(d["pair"]["split"]))
        return Program(d["expression"], int(d["scale"]), d["signedness"], d["overflow"],
                       tuple((int(s["lsb"]), int(s["width"])) for s in d["slices"]), pair)


def valid_layout(slices: Tuple[Slice, ...], container: int) -> bool:
    """Grammar rule: 1..3 slices, each width>=1, inside the container, pairwise
    disjoint, and normalised: no slice is physically followed (ascending) by the
    next logical slice (such a pair would be one contiguous slice)."""
    if not 1 <= len(slices) <= MAX_SLICES:
        return False
    used = 0
    for lsb, w in slices:
        if w < 1 or lsb < 0 or lsb + w > container:
            return False
        m = ((1 << w) - 1) << lsb
        if used & m:
            return False
        used |= m
    for (l0, w0), (l1, _) in zip(slices, slices[1:]):
        if l0 + w0 == l1:
            return False
    return True


def valid_program(p: Program, container: int, pair_group: Optional[str]) -> bool:
    if p.expr not in EXPRS or p.scale not in SCALES or p.sign not in SIGNS or p.ovf not in OVFS:
        return False
    if not valid_layout(p.slices, container):
        return False
    if pair_group is None:
        return p.pair is None
    if p.pair is None or p.pair[0] != pair_group or p.pair[1] not in ROLES:
        return False
    return 1 <= p.pair[2] <= 63


# ------------------------------------------------------------------- semantics
def evaluate(expr: str, S: int, A: int, P: int, B: int) -> int:
    if expr == "S":
        return S
    if expr == "S+A":
        return S + A
    if expr == "S-P":
        return S - P
    if expr == "S+A-P":
        return S + A - P
    if expr == "S+B+A-P":
        return S + B + A - P
    raise ValueError(expr)


def trunc_div(v: int, d: int) -> int:
    q = abs(v) // d
    return q if v >= 0 else -q


def split_pair(q: int, k: int) -> Tuple[int, int]:
    """Euclidean split: lo in [0, 2^k), hi = Euclidean quotient."""
    lo = q % (1 << k)
    hi = (q - lo) // (1 << k)
    return hi, lo


def encode_bits(x: int, width: int, sign: str, ovf: str) -> Optional[int]:
    """Width semantics then Reject/Wrap.  Returns the w-bit pattern or None."""
    if sign == "signed":
        lo, hi = -(1 << (width - 1)), (1 << (width - 1)) - 1
    else:
        lo, hi = 0, (1 << width) - 1
    if lo <= x <= hi:
        return x % (1 << width)
    if ovf == "reject":
        return None
    return x % (1 << width)


def place_bits(bits: int, slices: Tuple[Slice, ...], word: int) -> int:
    out = word
    for lsb, w in slices:
        chunk = bits & ((1 << w) - 1)
        bits >>= w
        out = (out & ~(((1 << w) - 1) << lsb)) | (chunk << lsb)
    return out


def extract_bits(word: int, slices: Tuple[Slice, ...]) -> int:
    val = 0
    shift = 0
    for lsb, w in slices:
        val |= ((word >> lsb) & ((1 << w) - 1)) << shift
        shift += w
    return val


def scalar_value(p: Program, S: int, A: int, P: int, B: int) -> int:
    return trunc_div(evaluate(p.expr, S, A, P, B), p.scale)


def replay_word(p: Program, S: int, A: int, P: int, B: int, obj_word: int) -> Optional[int]:
    """Unpaired relocation: full evaluation to the patched container word."""
    q = scalar_value(p, S, A, P, B)
    bits = encode_bits(q, p.width, p.sign, p.ovf)
    return None if bits is None else place_bits(bits, p.slices, obj_word)


def replay_pair(p_first: Program, p_second: Program, S: int, A: int, P: int, B: int,
                w_first: int, w_second: int) -> Tuple[Optional[int], Optional[int]]:
    """Paired instance; P is the place of the first record.  Both members share
    expression/scale/split; each encodes its own component with its own
    signedness/overflow/slices.  Rejection is per member."""
    assert p_first.pair and p_second.pair
    q = scalar_value(p_first, S, A, P, B)
    hi, lo = split_pair(q, p_first.pair[2])
    outs = []
    for p, w in ((p_first, w_first), (p_second, w_second)):
        comp = hi if p.pair[1] == "hi" else lo
        bits = encode_bits(comp, p.width, p.sign, p.ovf)
        outs.append(None if bits is None else place_bits(bits, p.slices, w))
    return outs[0], outs[1]


# ------------------------------------------------------------------- ELF64 LE
SHT_NULL, SHT_PROGBITS, SHT_SYMTAB, SHT_STRTAB, SHT_RELA = 0, 1, 2, 3, 4
SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR = 1, 2, 4
SHN_UNDEF, SHN_ABS = 0, 0xFFF1
ET_REL, ET_EXEC = 1, 2


@dataclass
class SectionDesc:
    name: str
    flags: int
    data: bytes


@dataclass
class SymbolDesc:
    name: str
    shndx: str  # section name, "ABS" or "UNDEF"
    value: int
    binding: int = 1  # GLOBAL


@dataclass
class RelaDesc:
    section: str
    offset: int
    symbol: str
    type: int
    addend: int


def _strtab(names: List[str]) -> Tuple[bytes, Dict[str, int]]:
    blob = b"\0"
    off = {"": 0}
    for n in names:
        if n and n not in off:
            off[n] = len(blob)
            blob += n.encode() + b"\0"
    return blob, off


def build_elf(e_type: int, sections: List[SectionDesc], symbols: List[SymbolDesc],
              relas: List[RelaDesc], addrs: Optional[Dict[str, int]] = None,
              resolved: Optional[Dict[str, int]] = None) -> bytes:
    """Serialise a minimal but structurally valid ELF64 little-endian file.
    Section index layout: [0]=null, then PROGBITS sections in the given order,
    then one .rela.<name> per relocated section (ET_REL only), then .symtab,
    .strtab, .shstrtab."""
    addrs = addrs or {}
    sec_index = {s.name: i + 1 for i, s in enumerate(sections)}
    rela_by_sec: Dict[str, List[RelaDesc]] = {}
    if e_type == ET_REL:
        for r in relas:
            rela_by_sec.setdefault(r.section, []).append(r)
    rela_names = [".rela" + s.name for s in sections if s.name in rela_by_sec]
    n_prog = len(sections)
    n_rela = len(rela_names)
    symtab_idx = 1 + n_prog + n_rela
    strtab_idx = symtab_idx + 1
    shstr_idx = strtab_idx + 1
    sym_names = [s.name for s in symbols]
    strtab, str_off = _strtab(sym_names)
    # symbol table: null, then locals (section symbols are not emitted), then globals
    sym_entries = [struct.pack("<IBBHQQ", 0, 0, 0, 0, 0, 0)]
    sym_index = {}
    ordered = sorted(symbols, key=lambda s: (s.binding, sym_names.index(s.name)))
    first_global = None
    for s in ordered:
        if s.shndx == "ABS":
            shndx = SHN_ABS
        elif s.shndx == "UNDEF":
            shndx = SHN_UNDEF
        else:
            shndx = sec_index[s.shndx]
        value = s.value
        if resolved and s.name in resolved:
            value = resolved[s.name]
        info = (s.binding << 4) | 0
        if s.binding == 1 and first_global is None:
            first_global = len(sym_entries)
        sym_index[s.name] = len(sym_entries)
        sym_entries.append(struct.pack("<IBBHQQ", str_off[s.name], info, 0, shndx, value, 0))
    if first_global is None:
        first_global = len(sym_entries)
    symtab = b"".join(sym_entries)
    shstr_names = [s.name for s in sections] + rela_names + [".symtab", ".strtab", ".shstrtab"]
    shstrtab, shstr_off = _strtab(shstr_names)
    # lay out file
    ehsize, shentsize = 64, 64
    body = b""
    offsets = {}
    cursor = ehsize
    chunks = []
    def add(name, data, align=8):
        nonlocal cursor
        pad = (-cursor) % align
        chunks.append(b"\0" * pad + data)
        cursor += pad
        offsets[name] = cursor
        cursor += len(data)
    for s in sections:
        add(s.name, s.data, 16)
    for rn in rela_names:
        secname = rn[len(".rela"):]
        entries = b"".join(struct.pack("<QQq", r.offset, (sym_index[r.symbol] << 32) | r.type, r.addend)
                           for r in rela_by_sec[secname])
        add(rn, entries)
    add(".symtab", symtab)
    add(".strtab", strtab, 1)
    add(".shstrtab", shstrtab, 1)
    pad = (-cursor) % 8
    chunks.append(b"\0" * pad)
    cursor += pad
    shoff = cursor
    body = b"".join(chunks)
    # section headers
    headers = [struct.pack("<IIQQQQIIQQ", 0, SHT_NULL, 0, 0, 0, 0, 0, 0, 0, 0)]
    for s in sections:
        headers.append(struct.pack("<IIQQQQIIQQ", shstr_off[s.name], SHT_PROGBITS, s.flags,
                                   addrs.get(s.name, 0), offsets[s.name], len(s.data), 0, 0, 16, 0))
    for rn in rela_names:
        secname = rn[len(".rela"):]
        headers.append(struct.pack("<IIQQQQIIQQ", shstr_off[rn], SHT_RELA, 0, 0, offsets[rn],
                                   24 * len(rela_by_sec[secname]), symtab_idx, sec_index[secname], 8, 24))
    headers.append(struct.pack("<IIQQQQIIQQ", shstr_off[".symtab"], SHT_SYMTAB, 0, 0, offsets[".symtab"],
                               len(symtab), strtab_idx, first_global, 8, 24))
    headers.append(struct.pack("<IIQQQQIIQQ", shstr_off[".strtab"], SHT_STRTAB, 0, 0, offsets[".strtab"],
                               len(strtab), 0, 0, 1, 0))
    headers.append(struct.pack("<IIQQQQIIQQ", shstr_off[".shstrtab"], SHT_STRTAB, 0, 0, offsets[".shstrtab"],
                               len(shstrtab), 0, 0, 1, 0))
    shnum = len(headers)
    ident = b"\x7fELF" + bytes([2, 1, 1, 0]) + b"\0" * 8
    ehdr = ident + struct.pack("<HHIQQQIHHHHHH", e_type, EM_VX, 1, 0, 0, shoff, 0, ehsize, 0, 0,
                               shentsize, shnum, shstr_idx)
    return ehdr + body + b"".join(headers)


@dataclass
class ElfSection:
    name: str
    type: int
    flags: int
    addr: int
    offset: int
    size: int
    link: int
    info: int
    entsize: int
    data: bytes


@dataclass
class ElfSymbol:
    name: str
    value: int
    info: int
    shndx: int


@dataclass
class ElfRela:
    section: int  # index of relocated section
    offset: int
    sym: int
    type: int
    addend: int


@dataclass
class ElfFile:
    e_type: int
    e_machine: int
    sections: List[ElfSection]
    symbols: List[ElfSymbol]
    relas: List[ElfRela]

    def section_by_name(self, name: str) -> ElfSection:
        for s in self.sections:
            if s.name == name:
                return s
        raise KeyError(name)


def parse_elf(blob: bytes) -> ElfFile:
    """Independent ELF64-LE reader (author side)."""
    assert blob[:4] == b"\x7fELF" and blob[4] == 2 and blob[5] == 1, "not ELF64 little-endian"
    (e_type, e_machine, e_version, e_entry, e_phoff, e_shoff, e_flags, e_ehsize, e_phentsize,
     e_phnum, e_shentsize, e_shnum, e_shstrndx) = struct.unpack_from("<HHIQQQIHHHHHH", blob, 16)
    assert e_shentsize == 64
    raw = []
    for i in range(e_shnum):
        raw.append(struct.unpack_from("<IIQQQQIIQQ", blob, e_shoff + 64 * i))
    shstr = raw[e_shstrndx]
    shdata = blob[shstr[4]:shstr[4] + shstr[5]]
    def cstr(tab: bytes, off: int) -> str:
        end = tab.index(b"\0", off)
        return tab[off:end].decode()
    sections = []
    for (nm, ty, fl, ad, of, sz, ln, inf, al, es) in raw:
        data = b"" if ty == SHT_NULL else blob[of:of + sz]
        sections.append(ElfSection(cstr(shdata, nm), ty, fl, ad, of, sz, ln, inf, es, data))
    symbols: List[ElfSymbol] = []
    relas: List[ElfRela] = []
    for s in sections:
        if s.type == SHT_SYMTAB:
            strtab = sections[s.link].data
            for j in range(s.size // 24):
                st_name, st_info, st_other, st_shndx, st_value, st_size = struct.unpack_from("<IBBHQQ", s.data, 24 * j)
                symbols.append(ElfSymbol(cstr(strtab, st_name), st_value, st_info, st_shndx))
    for s in sections:
        if s.type == SHT_RELA:
            for j in range(s.size // 24):
                r_offset, r_info, r_addend = struct.unpack_from("<QQq", s.data, 24 * j)
                relas.append(ElfRela(s.info, r_offset, r_info >> 32, r_info & 0xFFFFFFFF, r_addend))
    return ElfFile(e_type, e_machine, sections, symbols, relas)


# ----------------------------------------------------------------- corpus view
@dataclass
class Site:
    obj: str
    link: str
    section: str
    offset: int
    type: int
    container: int
    S: int
    A: int
    P: int
    B: int
    obj_word: int
    out_word: Optional[int]      # legacy word, None when unobserved (failed/replay)
    rejected: Optional[bool]     # True/False for linked/failed links, None for replay
    pair_id: Optional[Tuple[str, str, str, int]] = None  # (obj, link, section, first offset)
    first_in_pair: bool = False
    pair_P: Optional[int] = None


def read_word(data: bytes, offset: int, container: int) -> int:
    n = container // 8
    return int.from_bytes(data[offset:offset + n], "little")


def load_corpus(root: str):
    """Return (manifest, {obj_id: ElfFile}, {(obj,link): ElfFile|None}, sites)."""
    import os
    man = json.load(open(os.path.join(root, "manifest.json")))
    types = {t["type"]: t for t in man["relocation_types"]}
    objs, outs, sites = {}, {}, []
    for o in man["objects"]:
        elf = parse_elf(open(os.path.join(root, o["file"]), "rb").read())
        objs[o["id"]] = elf
        for lk in o["links"]:
            status = lk["result"]["status"]
            out = None
            if status == "linked":
                out = parse_elf(open(os.path.join(root, lk["result"]["file"]), "rb").read())
            outs[(o["id"], lk["id"])] = out
            failed = set()
            if status == "failed":
                failed = {(f["section"], f["offset"]) for f in lk["result"]["overflow"]}
            bases = lk["bases"]
            for_pairs = {}
            relas_sorted = sorted(elf.relas, key=lambda r: (r.section, r.offset))
            # pair identification: consecutive entries, same pair group, same symbol & addend
            prev = None
            for r in relas_sorted:
                sec = elf.sections[r.section]
                t = types[r.type]
                sym = elf.symbols[r.sym]
                if sym.shndx == SHN_ABS:
                    S, B = sym.value, 0
                elif sym.shndx == SHN_UNDEF:
                    res = lk["symbols"][sym.name]
                    S = res["value"]
                    B = 0 if res["section"] is None else bases[res["section"]]
                else:
                    S = sym.value
                    B = bases[elf.sections[sym.shndx].name]
                P = bases[sec.name] + r.offset
                cont = t["container_bits"]
                ow = read_word(sec.data, r.offset, cont)
                if out is not None:
                    outw = read_word(out.section_by_name(sec.name).data, r.offset, cont)
                    rej = False
                else:
                    outw = None
                    rej = ((sec.name, r.offset) in failed) if status == "failed" else None
                site = Site(o["id"], lk["id"], sec.name, r.offset, r.type, cont, S, r.addend, P, B, ow, outw, rej)
                if t["pair_group"] is not None:
                    if (prev is not None and prev.type != r.type and types[prev.type]["pair_group"] == t["pair_group"]
                            and elf.relas[0] is not None and prev.sym == r.sym and prev.addend == r.addend
                            and prev.section == r.section and prev_site.pair_id is None):
                        pid = (o["id"], lk["id"], sec.name, prev.offset)
                        prev_site.pair_id = pid
                        prev_site.first_in_pair = True
                        prev_site.pair_P = prev_site.P
                        site.pair_id = pid
                        site.pair_P = prev_site.P
                    prev, prev_site = r, site
                else:
                    prev, prev_site = None, None
                sites.append(site)
    return man, objs, outs, sites


def self_test():
    # arithmetic
    assert trunc_div(-7, 2) == -3 and trunc_div(7, 2) == 3
    assert split_pair(-1, 12) == (-1, 4095)
    assert encode_bits(-1, 8, "signed", "reject") == 0xFF
    assert encode_bits(128, 8, "signed", "reject") is None
    assert encode_bits(128, 8, "signed", "wrap") == 0x80
    assert encode_bits(-1, 8, "unsigned", "wrap") == 0xFF
    p = Program("S+A-P", 4, "signed", "reject", ((7, 5), (25, 7)))
    assert p.width == 12
    w = replay_word(p, 0x1000, 8, 0x1004, 0, 0xFFFFFFFF)
    assert w is not None and extract_bits(w, p.slices) == ((4 // 4) % 4096)
    # ELF round trip
    secs = [SectionDesc(".text", SHF_ALLOC | SHF_EXECINSTR, bytes(range(64))), SectionDesc(".data", SHF_ALLOC | SHF_WRITE, bytes(32))]
    syms = [SymbolDesc("foo", ".data", 8), SymbolDesc("bar", "ABS", 77), SymbolDesc("ext", "UNDEF", 0)]
    rel = [RelaDesc(".text", 4, "foo", 3, -8), RelaDesc(".text", 12, "ext", 5, 16)]
    blob = build_elf(ET_REL, secs, syms, rel)
    e = parse_elf(blob)
    assert e.e_machine == EM_VX and e.e_type == ET_REL
    assert [s.name for s in e.sections] == ["", ".text", ".data", ".rela.text", ".symtab", ".strtab", ".shstrtab"]
    assert len(e.relas) == 2 and e.relas[0].addend == -8 and e.symbols[e.relas[0].sym].name == "foo"
    assert e.symbols[e.relas[1].sym].shndx == SHN_UNDEF
    blob2 = build_elf(ET_EXEC, secs, syms, [], addrs={".text": 0x10000, ".data": 0x20000}, resolved={"foo": 0x20008})
    e2 = parse_elf(blob2)
    assert e2.section_by_name(".text").addr == 0x10000 and e2.relas == []
    print("model self-test ok; ELF object bytes:", len(blob), "linked bytes:", len(blob2))


if __name__ == "__main__":
    self_test()
