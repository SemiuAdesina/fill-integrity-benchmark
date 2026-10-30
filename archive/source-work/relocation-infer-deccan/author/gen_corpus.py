#!/usr/bin/env python3
"""Author-side corpus generator with generation-and-rejection certification.

Fixture -> latent programs -> real ELF64 RELA objects + link maps -> legacy
outputs via the model -> oracle A / oracle B -> identifiability checks ->
retain or reject.  Writes:
  author/fixtures/<id>/corpus/            candidate-visible corpus
  author/fixtures/<id>/expected.json      verifier-owned certified answers
  author/fixtures/<id>/certification.json author-only certification record
Latent programs never ship.
"""
import sys, os, json, random, shutil, copy
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "research"))
sys.path.insert(0, os.path.join(HERE, "..", "oracle-a"))
sys.path.insert(0, os.path.join(HERE, "..", "oracle-b"))
from model import (Program, SectionDesc, SymbolDesc, RelaDesc, build_elf, ET_REL, ET_EXEC,
                   SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE, evaluate, trunc_div, replay_word, replay_pair,
                   split_pair, encode_bits, load_corpus, extract_bits, valid_program)
import oracle_a, oracle_b

SEC_FLAGS = {".text": SHF_ALLOC | SHF_EXECINSTR, ".data": SHF_ALLOC | SHF_WRITE, ".rodata": SHF_ALLOC,
             ".init": SHF_ALLOC | SHF_EXECINSTR}


class TypeSpec:
    def __init__(self, tid, container, latent: Program, pair_group=None, value_plan=None):
        self.tid, self.container, self.latent, self.pair_group = tid, container, latent, pair_group
        self.value_plan = value_plan or {}


class SiteSpec:
    """One relocation record in an object."""
    def __init__(self, section, offset, tid, symbol, addend, pair_second=None):
        self.section, self.offset, self.tid, self.symbol, self.addend = section, offset, tid, symbol, addend
        self.pair_second = pair_second  # (tid2, offset2) for a pair instance; second record follows first


class ObjectSpec:
    def __init__(self, oid, sections, symbols, sites):
        # sections: {name: size}; symbols: {name: (kind, section|None, value)} kind in defined/abs/undef
        self.oid, self.sections, self.symbols, self.sites = oid, sections, symbols, sites


class LinkSpec:
    def __init__(self, lid, kind, bases, symbols):
        # kind: visible | replay | hidden ; bases: {section: base}; symbols: {name: (section|None, value)}
        self.lid, self.kind, self.bases, self.symbols = lid, kind, bases, symbols


# ------------------------------------------------------------- materialisation
def object_words(rng, size, container_at):
    """Random instruction/data bytes for a section; container_at: {offset: bits}."""
    data = bytearray(rng.getrandbits(8) for _ in range(size))
    for off, bits in container_at.items():
        n = bits // 8
        w = rng.getrandbits(bits)
        data[off:off + n] = w.to_bytes(n, "little")
    return bytes(data)


def resolve(site_sym, obj: ObjectSpec, link: LinkSpec):
    kind, sec, val = obj.symbols[site_sym]
    if kind == "abs":
        return val, 0
    if kind == "defined":
        return val, link.bases[sec]
    sec2, val2 = link.symbols[site_sym]
    return val2, (0 if sec2 is None else link.bases[sec2])


def compute_truth(types, obj: ObjectSpec, link: LinkSpec, words):
    """words: {(section, offset): obj_word}. Returns {(section, offset): word|None}."""
    out = {}
    for s in obj.sites:
        S, B = resolve(s.symbol, obj, link)
        t = types[s.tid]
        if s.pair_second is None:
            P = link.bases[s.section] + s.offset
            out[(s.section, s.offset)] = replay_word(t.latent, S, s.addend, P, B, words[(s.section, s.offset)])
        else:
            tid2, off2 = s.pair_second
            t2 = types[tid2]
            P = link.bases[s.section] + s.offset  # first record's place
            w1, w2 = replay_pair(t.latent, t2.latent, S, s.addend, P, B,
                                 words[(s.section, s.offset)], words[(s.section, off2)])
            out[(s.section, s.offset)] = w1
            out[(s.section, off2)] = w2
    return out


def patch(data: bytes, offset: int, bits: int, word: int) -> bytes:
    n = bits // 8
    return data[:offset] + word.to_bytes(n, "little") + data[offset + n:]


def materialise(fx, out_dir, rng):
    """fx: dict with types (list TypeSpec), objects (list ObjectSpec), links {oid: [LinkSpec]}."""
    corpus = os.path.join(out_dir, "corpus")
    shutil.rmtree(out_dir, ignore_errors=True)
    os.makedirs(os.path.join(corpus, "objects"))
    os.makedirs(os.path.join(corpus, "linked"))
    types = {t.tid: t for t in fx["types"]}
    manifest = {"format_version": 1, "machine": 0x5658,
                "relocation_types": [{"type": t.tid, "name": "R_VX_%d" % t.tid, "container_bits": t.container,
                                      "pair_group": t.pair_group} for t in sorted(fx["types"], key=lambda t: t.tid)],
                "objects": []}
    expected = {"fixture": fx["id"], "programs": [t.latent.to_json(t.tid) for t in sorted(fx["types"], key=lambda t: t.tid)],
                "predictions": [], "hidden_links": []}
    for obj in fx["objects"]:
        cont_at = {}
        for s in obj.sites:
            cont_at.setdefault(s.section, {})[s.offset] = types[s.tid].container
            if s.pair_second:
                cont_at[s.section][s.pair_second[1]] = types[s.pair_second[0]].container
        sec_data = {name: object_words(rng, size, cont_at.get(name, {})) for name, size in obj.sections.items()}
        # stale immediates: some field bits nonzero, most zero
        words = {}
        for name, offs in cont_at.items():
            for off, bits in offs.items():
                if rng.random() >= getattr(obj, "stale_rate", 0.3):
                    # zero the field bits of the latent layout so the object looks assembler-clean
                    site_t = [s for s in obj.sites if s.section == name and (s.offset == off or (s.pair_second and s.pair_second[1] == off))][0]
                    tid = site_t.tid if site_t.offset == off else site_t.pair_second[0]
                    mask = 0
                    for lsb, w in types[tid].latent.slices:
                        mask |= ((1 << w) - 1) << lsb
                    cur = int.from_bytes(sec_data[name][off:off + bits // 8], "little") & ~mask & ((1 << bits) - 1)
                    sec_data[name] = patch(sec_data[name], off, bits, cur)
                words[(name, off)] = int.from_bytes(sec_data[name][off:off + bits // 8], "little")
        sections = [SectionDesc(n, SEC_FLAGS[n], sec_data[n]) for n in obj.sections]
        symbols = []
        for name, (kind, sec, val) in obj.symbols.items():
            symbols.append(SymbolDesc(name, {"abs": "ABS", "undef": "UNDEF"}.get(kind, sec), val))
        relas = []
        for s in obj.sites:
            relas.append(RelaDesc(s.section, s.offset, s.symbol, s.tid, s.addend))
            if s.pair_second:
                relas.append(RelaDesc(s.section, s.pair_second[1], s.symbol, s.pair_second[0], s.addend))
        relas.sort(key=lambda r: (r.section, r.offset))
        blob = build_elf(ET_REL, sections, symbols, relas)
        fname = "objects/%s.o" % obj.oid
        open(os.path.join(corpus, fname), "wb").write(blob)
        mobj = {"id": obj.oid, "file": fname, "links": []}
        for link in fx["links"][obj.oid]:
            truth = compute_truth(types, obj, link, words)
            sym_res = {n: {"section": v[0], "value": v[1]} for n, v in sorted(link.symbols.items())}
            entry = {"id": link.lid, "bases": dict(sorted(link.bases.items())), "symbols": sym_res}
            site_list = [{"section": sec, "offset": off, "word": w} for (sec, off), w in sorted(truth.items())]
            if link.kind == "hidden":
                expected["hidden_links"].append({"object": obj.oid, "id": link.lid, "bases": entry["bases"],
                                                 "symbols": sym_res, "sites": site_list})
                continue
            for sl in site_list:
                expected["predictions"].append({"object": obj.oid, "link": link.lid, "section": sl["section"],
                                                "offset": sl["offset"], "word": sl["word"]})
            if link.kind == "replay":
                entry["result"] = {"status": "replay"}
            else:
                rejected = [(sec, off) for (sec, off), w in sorted(truth.items()) if w is None]
                if rejected:
                    entry["result"] = {"status": "failed", "overflow": [{"section": s, "offset": o} for s, o in rejected]}
                else:
                    patched = dict(sec_data)
                    for (sec, off), w in truth.items():
                        patched[sec] = patch(patched[sec], off, cont_at[sec][off], w)
                    resolved = {}
                    for name, (kind, sec, val) in obj.symbols.items():
                        if kind == "defined":
                            resolved[name] = link.bases[sec] + val
                        elif kind == "undef":
                            s2, v2 = link.symbols[name]
                            resolved[name] = v2 + (0 if s2 is None else link.bases[s2])
                    lsecs = [SectionDesc(n, SEC_FLAGS[n], patched[n]) for n in obj.sections]
                    lblob = build_elf(ET_EXEC, lsecs, symbols, [], addrs={n: link.bases[n] for n in obj.sections}, resolved=resolved)
                    lname = "linked/%s.%s.out" % (obj.oid, link.lid)
                    open(os.path.join(corpus, lname), "wb").write(lblob)
                    entry["result"] = {"status": "linked", "file": lname}
            mobj["links"].append(entry)
        manifest["objects"].append(mobj)
    expected["predictions"].sort(key=lambda p: (p["object"], p["link"], p["section"], p["offset"]))
    json.dump(manifest, open(os.path.join(corpus, "manifest.json"), "w"), indent=1)
    json.dump(expected, open(os.path.join(out_dir, "expected.json"), "w"), indent=1)
    return manifest, expected


# --------------------------------------------------------------- certification
def replay_program_on(prog_by_type, pair_progs, man, sites_by_key, link_sites):
    """Replay a candidate program set on a list of site descriptors (S,A,P,B, words)."""
    out = []
    for s in link_sites:
        out.append(s)
    return out


def certify(fx, out_dir):
    corpus = os.path.join(out_dir, "corpus")
    expected = json.load(open(os.path.join(out_dir, "expected.json")))
    ra = oracle_a.solve(corpus)
    rb = oracle_b.solve(corpus)
    types = {t.tid: t for t in fx["types"]}
    man, objs, outs, sites = load_corpus(corpus)
    rows, ok_all = [], True
    # per-type observation statistics
    def type_sites(tid):
        return [s for s in sites if s.type == tid]
    truth_prog = {t.tid: t.latent for t in fx["types"]}

    def predictions_of(progs):
        """progs: {tid: Program}. Replay on every manifest link (visible+replay) and hidden links."""
        preds = {}
        for s in sites:
            if s.pair_id is None:
                preds[(s.obj, s.link, s.section, s.offset)] = replay_word(progs[s.type], s.S, s.A, s.P, s.B, s.obj_word)
        by_pid = {}
        for s in sites:
            if s.pair_id is not None:
                by_pid.setdefault(s.pair_id, []).append(s)
        for pid, ss in by_pid.items():
            first = [s for s in ss if s.first_in_pair][0]
            second = [s for s in ss if not s.first_in_pair][0]
            w1, w2 = replay_pair(progs[first.type], progs[second.type], first.S, first.A, first.pair_P, first.B,
                                 first.obj_word, second.obj_word)
            preds[(first.obj, first.link, first.section, first.offset)] = w1
            preds[(second.obj, second.link, second.section, second.offset)] = w2
        return preds

    def hidden_predictions(progs):
        preds = {}
        for obj in fx["objects"]:
            words = None
            for link in fx["links"][obj.oid]:
                if link.kind != "hidden":
                    continue
                if words is None:
                    elf = objs[obj.oid]
                    words = {}
                    for s in obj.sites:
                        sec = elf.section_by_name(s.section)
                        words[(s.section, s.offset)] = int.from_bytes(sec.data[s.offset:s.offset + types[s.tid].container // 8], "little")
                        if s.pair_second:
                            t2 = types[s.pair_second[0]]
                            words[(s.section, s.pair_second[1])] = int.from_bytes(sec.data[s.pair_second[1]:s.pair_second[1] + t2.container // 8], "little")
                fake_types = {tid: TypeSpec(tid, types[tid].container, progs[tid], types[tid].pair_group) for tid in types}
                truth = compute_truth(fake_types, obj, link, words)
                for k, v in truth.items():
                    preds[(obj.oid, link.lid) + k] = v
        return preds

    canonical = {}
    for key, va in ra.items():
        vb = rb[key]
        fa = set(va["fits"]) if not isinstance(key, tuple) else set(va["fits"])
        fb = set(vb["fits"])
        agree = (fa == fb)
        ca, cb = va["canonical"], vb["canonical"]
        if isinstance(key, tuple):
            t1, t2 = va["types"]
            latent = (truth_prog[t1], truth_prog[t2])
            label = "pair %s (types %d,%d)" % (key[1], t1, t2)
            nobs = len([s for s in sites if s.type == t1 and s.out_word is not None])
        else:
            latent = truth_prog[key]
            label = "type %d" % key
            nobs = len([s for s in sites if s.type == key and s.out_word is not None])
        row = {"target": label, "visible_linked_obs": nobs,
               "visible_obs_total": len([s for s in sites if (s.type == key if not isinstance(key, tuple) else s.type in va["types"]) and s.rejected is not None]),
               "latent": [p.to_json(0) for p in (latent if isinstance(key, tuple) else (latent,))],
               "oracle_a_fits": len(fa), "oracle_b_fits": len(fb), "fit_sets_equal": agree,
               "canonical_a": None if ca is None else [p.to_json(0) for p in (ca if isinstance(key, tuple) else (ca,))],
               "canonical_equal": ca == cb, "latent_in_fits": latent in fa}
        if ca is None or not agree or ca != cb or latent not in fa:
            row["retain"] = False
            row["reason"] = "oracle disagreement or latent not recovered"
            ok_all = False
            rows.append(row)
            continue
        if isinstance(key, tuple):
            canonical[t1], canonical[t2] = ca
        else:
            canonical[key] = ca
        # changed-mask equals canonical slice union (visible identifiability of the field)
        for tid, prog in ((va["types"][0], ca[0]), (va["types"][1], ca[1])) if isinstance(key, tuple) else ((key, ca),):
            mask = 0
            for s in type_sites(tid):
                if s.out_word is not None:
                    mask |= s.out_word ^ s.obj_word
            union = 0
            for lsb, w in prog.slices:
                union |= ((1 << w) - 1) << lsb
            row.setdefault("field_mask_equals_changed", True)
            if mask != union:
                row["field_mask_equals_changed"] = False
                ok_all = False
        rows.append(row)
    if not ok_all:
        return rows, False, None
    # held-out agreement: every fit (all combinations across types) must agree with the canonical predictions
    canon_pred = predictions_of(canonical)
    canon_hidden = hidden_predictions(canonical)
    truth_pred = predictions_of(truth_prog)
    truth_hidden = hidden_predictions(truth_prog)
    if canon_pred != truth_pred or canon_hidden != truth_hidden:
        rows.append({"target": "held-out", "retain": False, "reason": "canonical program disagrees with latent truth on held-out data"})
        return rows, False, None
    # per-target alternative fits: substitute each alternative fit alone and compare (fits are independent per type)
    for key, va in ra.items():
        alts = va["fits"]
        disagreements = 0
        for alt in alts:
            progs = dict(canonical)
            if isinstance(key, tuple):
                progs[va["types"][0]], progs[va["types"][1]] = alt
            else:
                progs[key] = alt
            if predictions_of(progs) != truth_pred or hidden_predictions(progs) != truth_hidden:
                disagreements += 1
        for row in rows:
            if row["target"].startswith("type %d" % key if not isinstance(key, tuple) else "pair %s" % key[1]):
                row["held_out_disagreeing_fits"] = disagreements
                row["retain"] = disagreements == 0
                row["reason"] = "ok" if disagreements == 0 else "underdetermined: an alternative fit predicts different held-out words"
                if disagreements:
                    ok_all = False
    # expected.json carries the CANONICAL programs (the graded identity), not the latent ones
    expected["programs"] = [canonical[t.tid].to_json(t.tid) for t in sorted(fx["types"], key=lambda t: t.tid)]
    json.dump(expected, open(os.path.join(out_dir, "expected.json"), "w"), indent=1)
    return rows, ok_all, canonical


# ------------------------------------------------------------ fixture builders
def value_targets(rng, prog: Program, n, plan):
    """Post-scale logical targets for one type: bit coverage, boundaries, negatives,
    out-of-range for reject/wrap evidence."""
    w = prog.width
    if prog.sign == "signed":
        lo, hi = -(1 << (w - 1)), (1 << (w - 1)) - 1
    else:
        lo, hi = 0, (1 << w) - 1
    t = []
    t += [lo, hi, 0, 1, -1 if lo < 0 else hi - 1, lo + 1, hi // 2, lo // 2 if lo < 0 else hi // 3]
    if plan.get("overflow_evidence", True):
        t += [hi + 1, lo - 1] if prog.sign == "signed" else [hi + 1, hi + 2]
    while len(t) < n:
        t.append(rng.randint(lo, hi))
    rng.shuffle(t)
    return t[:n]


def build_public(rng):
    """The candidate-visible corpus: six vendor relocation types over four objects."""
    types = [
        TypeSpec(1, 64, Program("S+A", 1, "unsigned", "reject", ((0, 32),)), None),
        TypeSpec(2, 32, Program("S+A-P", 4, "signed", "reject", ((12, 20),))),
        TypeSpec(3, 32, Program("S+B+A-P", 1, "signed", "wrap", ((0, 12), (20, 12)))),
        TypeSpec(4, 32, Program("S-P", 2, "signed", "reject", ((8, 4), (25, 6), (7, 1)))),
        TypeSpec(5, 32, Program("S+B+A-P", 1, "signed", "reject", ((12, 20),), ("G1", "hi", 12)), "G1"),
        TypeSpec(6, 32, Program("S+B+A-P", 1, "unsigned", "reject", ((20, 12),), ("G1", "lo", 12)), "G1"),
    ]
    return generic_fixture("public", rng, types, n_objects=4, sites_per_type=2, ext_sections=[".shared", ".vendor_rt"])


def page(rng, lo, hi):
    return rng.randrange(lo, hi, 0x1000)


def layout_bases(rng, obj_sections, ext_sections, start_range=(1 << 26, 1 << 28), ext_range=(1 << 24, 1 << 29)):
    """Realistic link map: the object's sections are laid out consecutively from a
    random page-aligned start; external sections land anywhere."""
    bases = {}
    cur = page(rng, *start_range)
    for name, size in obj_sections.items():
        bases[name] = cur
        cur += size + rng.choice([0, 0x10, 0x40, 0x100, 0x1000])
        cur = (cur + 0xF) & ~0xF
    for name in ext_sections:
        bases[name] = page(rng, *ext_range)
    return bases


def in_range_targets(rng, prog: Program, n):
    w = prog.width
    if prog.sign == "signed":
        lo, hi = -(1 << (w - 1)), (1 << (w - 1)) - 1
    else:
        lo, hi = 0, (1 << w) - 1
    t = [lo, hi, 0, 1, hi - 1, lo + 1, hi // 2, (lo // 2) if lo < 0 else (hi // 3), -1 if lo < 0 else 2]
    while len(t) < n:
        t.append(rng.randint(lo, hi))
    rng.shuffle(t)
    return t


def out_of_range_target(rng, prog: Program):
    w = prog.width
    if prog.sign == "signed":
        lo, hi = -(1 << (w - 1)), (1 << (w - 1)) - 1
        return rng.choice([hi + 1, lo - 1, hi + rng.randint(2, 1 << 8), lo - rng.randint(2, 1 << 8)])
    hi = (1 << w) - 1
    return rng.choice([hi + 1, hi + 2, hi + rng.randint(3, 1 << 8)])


def pre_scale(rng, q, scale):
    """A pre-scale value whose truncating quotient is q (exercises truncation)."""
    r = rng.randint(0, scale - 1)
    return q * scale + (r if q >= 0 else -r)


def generic_fixture(fid, rng, types, n_objects, sites_per_type, ext_sections, visible_links=5,
                    replay_links=1, hidden_links=2, wrap_overflow_rate=0.15, policy=None, stale_rate=0.3,
                    start_range=(1 << 26, 1 << 28), ext_range=(1 << 24, 1 << 29), overflow_rate=0.5):
    """Build objects, sites, link maps and symbol resolutions for one fixture.

    policy: {tid: {...}} with optional keys
      targets(rng, prog, n) -> list of post-scale targets (default in_range_targets)
      symbols: "external" | "mixed" (default expression-aware)
      resolve(rng, link, obj_sections) -> section name | None for external symbols
      addend(rng) -> addend
      overflow: rate of out-of-range targets in the overflow link (default overflow_rate)
      overflow_anywhere: rate of out-of-range targets in any visible link (default 0)
    """
    policy = policy or {}
    tmap = {t.tid: t for t in types}
    unpaired = [t for t in types if t.pair_group is None]
    groups = {}
    for t in types:
        if t.pair_group:
            groups.setdefault(t.pair_group, []).append(t)
    objects, links = [], {}
    for oi in range(n_objects):
        oid = "obj%02d" % (oi + 1)
        plan = []
        for t in unpaired:
            plan += [("single", t)] * sites_per_type
        for g, ts in groups.items():
            plan += [("pair", tuple(sorted(ts, key=lambda t: t.tid)))] * sites_per_type
        rng.shuffle(plan)
        text_off, data_off = 0, 0
        sites = []
        symbols = {"local_%d" % oi: ("defined", ".data", 8 * rng.randint(0, 4)),
                   "ro_%d" % oi: ("defined", ".rodata", 4 * rng.randint(0, 4)),
                   "fn_%d" % oi: ("defined", ".text", 0),
                   "abs_%d" % oi: ("abs", None, rng.randint(1 << 8, 1 << 16))}
        ext_count = 0
        for kind, t in plan:
            tspec = t if kind == "single" else t[0]
            pol = policy.get(tspec.tid, {})
            if kind == "single" and t.container == 64:
                sec, o = ".data", data_off
                data_off += 8 + 8 * rng.randint(0, 2)
            else:
                sec, o = ".text", text_off
                text_off += (4 if kind == "single" else 8) + 4 * rng.randint(0, 2)
            expr = tspec.latent.expr
            r = rng.random()
            sym_mode = pol.get("symbols")
            if sym_mode == "external" or (sym_mode is None and "P" in expr and expr != "S+B+A-P"):
                sym = "ext_%d_%d" % (oi, ext_count); ext_count += 1
                symbols[sym] = ("undef", None, 0)
            elif expr == "S+B+A-P":
                if r < 0.65:
                    sym = "ext_%d_%d" % (oi, ext_count); ext_count += 1
                    symbols[sym] = ("undef", None, 0)
                else:
                    sym = rng.choice(["local_%d" % oi, "ro_%d" % oi, "fn_%d" % oi])
            else:
                if r < 0.6:
                    sym = "ext_%d_%d" % (oi, ext_count); ext_count += 1
                    symbols[sym] = ("undef", None, 0)
                elif r < 0.8:
                    sym = "abs_%d" % oi
                else:
                    sym = rng.choice(["local_%d" % oi, "ro_%d" % oi])
            if "addend" in pol:
                addend = pol["addend"](rng)
            elif expr in ("S", "S+A") and not sym.startswith("ext_"):
                addend = rng.choice([0, 0, 4, 8, 16, 12, 24, 1, 3])   # a working legacy build never references below offset 0
            else:
                addend = rng.choice([0, 0, 0, 4, 8, -4, -8, 16, -16, 12, 1, -1, 3, 24])
            if kind == "single":
                sites.append(SiteSpec(sec, o, t.tid, sym, addend))
            else:
                t1, t2 = t
                first, second = (t1, t2) if rng.random() < 0.5 else (t2, t1)
                sites.append(SiteSpec(sec, o, first.tid, sym, addend, pair_second=(second.tid, o + 4)))
        sections = {".text": text_off + 4 * rng.randint(2, 6), ".data": max(data_off, 40) + 8 * rng.randint(1, 4),
                    ".rodata": 32 + 4 * rng.randint(0, 6)}
        obj = ObjectSpec(oid, sections, symbols, sites)
        obj.stale_rate = stale_rate
        objects.append(obj)
        lk = []
        base0 = layout_bases(rng, sections, ext_sections, start_range, ext_range)
        LinkSpec.obj_index = property(lambda self: getattr(self, "_oi", 0))
        lk.append(LinkSpec("l01", "visible", base0, {}))
        # a relink that moves only the object's own sections by a small padding change
        b2 = dict(base0); shift = rng.choice([0x10, 0x20, 0x40, 0x80, -0x10, -0x40])
        for n in sections: b2[n] = base0[n] + shift
        lk.append(LinkSpec("l02", "visible", b2, {}))
        b3 = dict(base0)
        for n in ext_sections: b3[n] = page(rng, *ext_range)
        lk.append(LinkSpec("l03", "visible", b3, {}))
        lk.append(LinkSpec("l04", "visible", dict(base0), {}))
        for li in range(5, visible_links + 1):
            lk.append(LinkSpec("l%02d" % li, "visible", layout_bases(rng, sections, ext_sections, start_range, ext_range), {}))
        for li in range(replay_links):
            lk.append(LinkSpec("r%02d" % (li + 1), "replay", layout_bases(rng, sections, ext_sections, start_range, ext_range), {}))
        for li in range(hidden_links):
            lk.append(LinkSpec("h%02d" % (li + 1), "hidden", layout_bases(rng, sections, ext_sections, start_range, ext_range), {}))
        for link in lk:
            link._oi = oi
        links[oid] = lk
        overflow_link = "l%02d" % visible_links
        for s in sites:
            kind_sym = symbols[s.symbol][0]
            if kind_sym != "undef":
                continue
            t = tmap[s.tid]
            pol = policy.get(t.tid, {})
            def choose_section(link, pol=pol):
                if "resolve" in pol:
                    return pol["resolve"](rng, link, sections)
                if "B" in t.latent.expr or rng.random() < 0.5:
                    return rng.choice([None] + ext_sections + [".data", ".rodata"])
                return None
            if s.pair_second:
                k = t.latent.pair[2]
                hi_t = t if t.latent.pair[1] == "hi" else tmap[s.pair_second[0]]
                whi = hi_t.latent.width
                lo_hi, hi_hi = (-(1 << (whi - 1)), (1 << (whi - 1)) - 1) if hi_t.latent.sign == "signed" else (0, (1 << whi) - 1)
                pool = [(1 << k) - 1, 1 << k, -1 if lo_hi < 0 else 1, -(1 << k) if lo_hi < 0 else (1 << k) + 1,
                        (-(1 << k) - 1) if lo_hi < 0 else 0, (1 << k) * 5 - 1, 0, (1 << k) * 3, hi_hi * (1 << k) + (1 << k) - 1, lo_hi * (1 << k),
                        (1 << k) * 2 - 1, -(1 << k) * 2, -(1 << k) * 3 - 1 if lo_hi < 0 else (1 << k) * 7]
                if "targets" in pol:
                    pool = pol["targets"](rng, hi_t.latent, 40)
                while len(pool) < 40:
                    pool.append(rng.randint(lo_hi, hi_hi) * (1 << k) + rng.randint(0, (1 << k) - 1))
                rng.shuffle(pool)
                scale = t.latent.scale
                inherited = None
                for link in lk:
                    if link.lid in ("l02", "l03") and inherited is not None:
                        link.symbols[s.symbol] = inherited
                        continue
                    if link.lid == overflow_link and hi_t.latent.ovf == "reject" and rng.random() < pol.get("overflow", overflow_rate):
                        q = (hi_hi + 1) * (1 << k) + rng.randint(0, (1 << k) - 1)
                    else:
                        q = pool.pop()
                    V = pre_scale(rng, q, scale)
                    P = link.bases[s.section] + s.offset
                    sec_choice = choose_section(link)
                    B = 0 if sec_choice is None else link.bases[sec_choice]
                    S = V - evaluate(t.latent.expr, 0, s.addend, P, B)
                    if S < 0:
                        sec_choice = None
                        S = V - evaluate(t.latent.expr, 0, s.addend, P, 0)
                    if S < 0:
                        S = 0
                    link.symbols[s.symbol] = (sec_choice, S)
                    if link.lid == "l01":
                        inherited = (sec_choice, S)
                continue
            pool = pol["targets"](rng, t.latent, 40) if "targets" in pol else in_range_targets(rng, t.latent, 40)
            scale = t.latent.scale
            inherited = None
            for link in lk:
                if link.lid in ("l02", "l03") and inherited is not None:
                    link.symbols[s.symbol] = inherited
                    continue
                if t.latent.ovf == "reject" and link.kind == "visible" and (
                        (link.lid == overflow_link and rng.random() < pol.get("overflow", overflow_rate))
                        or rng.random() < pol.get("overflow_anywhere", 0.0)):
                    q = out_of_range_target(rng, t.latent)
                elif t.latent.ovf == "wrap" and rng.random() < pol.get("wrap_overflow", wrap_overflow_rate):
                    q = out_of_range_target(rng, t.latent)
                else:
                    q = pool.pop()
                V = pre_scale(rng, q, scale)
                P = link.bases[s.section] + s.offset
                sec_choice = choose_section(link)
                B = 0 if sec_choice is None else link.bases[sec_choice]
                S = V - evaluate(t.latent.expr, 0, s.addend, P, B)
                if S < 0:
                    sec_choice = None
                    S = V - evaluate(t.latent.expr, 0, s.addend, P, 0)
                if S < 0:
                    S = 0
                link.symbols[s.symbol] = (sec_choice, S)
                if link.lid == "l01":
                    inherited = (sec_choice, S)
        for link in lk:
            for name, (kind_sym, _, _) in symbols.items():
                if kind_sym == "undef" and name not in link.symbols:
                    link.symbols[name] = (None, rng.randint(0, 1 << 16))
    # a legacy corpus only holds objects that linked at least once; repair sites that
    # reject in every visible link (an uncontrolled symbol with a hopeless addend)
    for obj in objects:
        words = {}
        for s in obj.sites:
            words[(s.section, s.offset)] = 0
            if s.pair_second:
                words[(s.section, s.pair_second[1])] = 0
        for _ in range(3):
            vis = [l for l in links[obj.oid] if l.kind == "visible"]
            always = set()
            per_link = [compute_truth(tmap, obj, l, words) for l in vis]
            for key in words:
                if all(t[key] is None for t in per_link):
                    always.add(key)
            if not always:
                break
            for s in obj.sites:
                if (s.section, s.offset) in always or (s.pair_second and (s.section, s.pair_second[1]) in always):
                    s.addend = 0
    return {"id": fid, "types": types, "objects": objects, "links": links}


# ---------------------------------------------------------------- hidden cases
def neg_targets(frac_neg):
    def f(rng, prog, n):
        w = prog.width
        lo, hi = -(1 << (w - 1)), (1 << (w - 1)) - 1
        t = [lo, lo + 1, -1, -2, -3, hi, 0, 1, lo // 2, -(hi // 3)]
        while len(t) < n:
            t.append(rng.randint(lo, -1) if rng.random() < frac_neg else rng.randint(0, hi))
        rng.shuffle(t)
        return t
    return f


def positive_half_targets(rng, prog, n):
    w = prog.width
    hi = (1 << (w - 1)) - 1
    t = [0, 1, hi, hi - 1, hi // 2, 2, hi // 3]
    while len(t) < n:
        t.append(rng.randint(0, hi))
    rng.shuffle(t)
    return t


def boundary_targets(rng, prog, n):
    w = prog.width
    if prog.sign == "signed":
        lo, hi = -(1 << (w - 1)), (1 << (w - 1)) - 1
    else:
        lo, hi = 0, (1 << w) - 1
    t = [hi, lo, hi - 1, lo + 1, hi, lo, 0, 1, -1 if lo < 0 else hi - 2, hi // 2]
    while len(t) < n:
        t.append(rng.choice([hi, lo, rng.randint(lo, hi), rng.randint(lo, hi)]))
    rng.shuffle(t)
    return t


def build_case_a(rng):
    """SECTION-BASE VS PLACE: three displacement types; one of them is
    consistent with S+A-P on every observation of two objects (absolute
    symbols) and only the third object separates S+B+A-P."""
    types = [
        TypeSpec(1, 32, Program("S+A-P", 1, "signed", "reject", ((0, 16),))),
        TypeSpec(2, 32, Program("S+B+A-P", 1, "signed", "reject", ((16, 16),))),
        TypeSpec(3, 32, Program("S-P", 4, "signed", "reject", ((10, 22),))),
    ]
    def resolve_abs_then_section(rng, link, obj_sections):
        return None if link.obj_index < 2 else rng.choice([".shared", ".data", ".rodata"])
    pol = {2: {"symbols": "external", "resolve": resolve_abs_then_section},
           1: {"resolve": lambda rng, link, secs: rng.choice([".shared", ".vendor_rt", ".rodata", None])}}
    fx = generic_fixture("case_a", rng, types, n_objects=3, sites_per_type=3, ext_sections=[".shared", ".vendor_rt"], policy=pol)
    return fx


def build_case_b(rng):
    """NEGATIVE DISPLACEMENT + SPLIT FIELDS with large addresses and stale field bits."""
    types = [
        TypeSpec(1, 32, Program("S+A-P", 4, "signed", "reject", ((21, 5), (7, 6), (28, 3)))),
        TypeSpec(2, 32, Program("S+B+A-P", 2, "signed", "wrap", ((0, 8), (16, 8)))),
        TypeSpec(3, 32, Program("S-P", 8, "signed", "reject", ((12, 12),))),
    ]
    pol = {1: {"targets": neg_targets(0.7)}, 2: {"targets": neg_targets(0.6), "wrap_overflow": 0.25}, 3: {"targets": neg_targets(0.65)}}
    return generic_fixture("case_b", rng, types, n_objects=3, sites_per_type=3, ext_sections=[".shared"], policy=pol,
                           stale_rate=0.6, start_range=(1 << 33, 1 << 36), ext_range=(1 << 32, 1 << 37))


def build_case_c(rng):
    """HI/LO CARRY BOUNDARY: two pair groups, split 16 and 12, the lower type
    id being the low half in one group and the high half in the other."""
    types = [
        TypeSpec(1, 32, Program("S+B+A-P", 1, "unsigned", "reject", ((0, 16),), ("G2", "lo", 16)), "G2"),
        TypeSpec(2, 32, Program("S+B+A-P", 1, "signed", "reject", ((16, 16),), ("G2", "hi", 16)), "G2"),
        TypeSpec(3, 32, Program("S+A-P", 4, "signed", "reject", ((12, 20),), ("G3", "hi", 10)), "G3"),
        TypeSpec(4, 32, Program("S+A-P", 4, "unsigned", "reject", ((20, 10),), ("G3", "lo", 10)), "G3"),
    ]
    return generic_fixture("case_c", rng, types, n_objects=3, sites_per_type=3, ext_sections=[".shared", ".vendor_rt"])


def build_case_d(rng):
    """CANONICAL EQUIVALENCE: every visible and held-out observation of each
    type is explained by several programs; the published key picks one."""
    types = [
        TypeSpec(1, 32, Program("S+A", 1, "unsigned", "reject", ((0, 24),))),     # A always 0: S fits too
        TypeSpec(2, 32, Program("S+B+A-P", 1, "signed", "reject", ((8, 16),))),   # never negative: unsigned fits too
        TypeSpec(3, 32, Program("S+A-P", 1, "signed", "reject", ((0, 20),))),     # never out of range: wrap fits too
        TypeSpec(4, 32, Program("S+A-P", 1, "signed", "wrap", ((4, 12),))),       # wrap: signedness unobservable
    ]
    pol = {1: {"addend": lambda rng: 0, "symbols": "external"},
           2: {"targets": positive_half_targets, "symbols": "external", "overflow": 0.0},
           3: {"overflow": 0.0}}
    return generic_fixture("case_d", rng, types, n_objects=3, sites_per_type=3, ext_sections=[".shared"], policy=pol)


def build_case_e(rng):
    """REJECT VS WRAP BOUNDARY: values sit on both inclusive endpoints and one
    step beyond them, for reject and wrap types, signed and unsigned."""
    types = [
        TypeSpec(1, 32, Program("S+A-P", 1, "signed", "reject", ((0, 12),))),
        TypeSpec(2, 32, Program("S+A", 1, "unsigned", "wrap", ((0, 16),))),
        TypeSpec(3, 32, Program("S+B+A-P", 1, "signed", "wrap", ((4, 12),))),
        TypeSpec(4, 32, Program("S", 1, "unsigned", "reject", ((24, 8),))),
    ]
    pol = {1: {"targets": boundary_targets, "overflow_anywhere": 0.12, "overflow": 1.0},
           2: {"targets": boundary_targets, "wrap_overflow": 0.3, "symbols": "external"},
           3: {"targets": boundary_targets, "wrap_overflow": 0.3},
           4: {"targets": boundary_targets, "overflow_anywhere": 0.12, "overflow": 1.0, "symbols": "external"}}
    return generic_fixture("case_e", rng, types, n_objects=3, sites_per_type=2, ext_sections=[".shared"], policy=pol, visible_links=6)


def build_case_f(rng):
    """MULTIPLE RELOCATION TYPES IN ONE OBJECT: a scalar, a three-slice, a
    64-bit-container and a paired type interleaved in two dense objects."""
    types = [
        TypeSpec(3, 32, Program("S+B+A-P", 1, "signed", "reject", ((12, 20),), ("G4", "hi", 12)), "G4"),
        TypeSpec(4, 32, Program("S+B+A-P", 1, "unsigned", "reject", ((20, 12),), ("G4", "lo", 12)), "G4"),
        TypeSpec(7, 32, Program("S-P", 2, "signed", "reject", ((8, 4), (25, 6), (7, 1)))),
        TypeSpec(9, 64, Program("S+A", 1, "unsigned", "reject", ((0, 32),))),
        TypeSpec(11, 32, Program("S+A-P", 4, "signed", "wrap", ((16, 16),))),
    ]
    return generic_fixture("case_f", rng, types, n_objects=2, sites_per_type=4, ext_sections=[".shared", ".vendor_rt"], stale_rate=0.5)


BUILDERS = {"public": (build_public, 1000), "case_a": (build_case_a, 2000), "case_b": (build_case_b, 3000),
            "case_c": (build_case_c, 4000), "case_d": (build_case_d, 5000), "case_e": (build_case_e, 6000), "case_f": (build_case_f, 7000)}


def run_fixture(builder, seed_start, out_root, max_tries=40):
    for attempt in range(max_tries):
        rng = random.Random(seed_start + attempt)
        fx = builder(rng)
        out_dir = os.path.join(out_root, fx["id"])
        materialise(fx, out_dir, rng)
        rows, ok, canonical = certify(fx, out_dir)
        cert = {"fixture": fx["id"], "seed": seed_start + attempt, "attempt": attempt, "retained": ok, "rows": rows}
        json.dump(cert, open(os.path.join(out_dir, "certification.json"), "w"), indent=1)
        if ok:
            print("RETAINED %s seed=%d attempt=%d" % (fx["id"], seed_start + attempt, attempt))
            for r in rows:
                print("   %-28s obs=%s fitsA=%s fitsB=%s equal=%s latent_in=%s mask_ok=%s heldout_bad=%s" % (
                    r.get("target"), r.get("visible_linked_obs"), r.get("oracle_a_fits"), r.get("oracle_b_fits"),
                    r.get("fit_sets_equal"), r.get("latent_in_fits"), r.get("field_mask_equals_changed"),
                    r.get("held_out_disagreeing_fits")))
            return fx, canonical
        print("REJECTED %s attempt=%d: %s" % (fx["id"], attempt, [(r.get("target"), r.get("reason"), r.get("oracle_a_fits"), r.get("oracle_b_fits"), r.get("field_mask_equals_changed")) for r in rows if not r.get("retain", True)]))
    raise SystemExit("no retained fixture for " + builder.__name__)


if __name__ == "__main__":
    out_root = os.path.join(HERE, "fixtures")
    which = sys.argv[1:] or ["public"]
    if which == ["all"]:
        which = list(BUILDERS)
    for w in which:
        b, seed = BUILDERS[w]
        run_fixture(b, seed, out_root)
