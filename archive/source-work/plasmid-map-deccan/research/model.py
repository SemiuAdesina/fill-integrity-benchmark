#!/usr/bin/env python3
"""Author-side executable model of the plasmid-map contract (never ships).

Coordinates: a circle of N bases has cut positions 0..N-1; a cut at position p
lies between base p-1 and base p. Recombinant: vector bases [0,c) then L insert
bases then vector bases [c,V); N = V + L. A vector site (window start i, cut
p) is 'junction-spanning' when its recognition window contains both base
(c-1) mod V and base c; such sites are not required. Every other vector site
maps to p if p < c else p + L. Internal positions lie in the closed interval
[c, c+L]. Digest: union of positions (coincident cuts merge); fragments are
the circular gaps (an uncut circle: one fragment N); fragments below D are not
observed. Matching: predicted fragments >= D and observed bands expanded by
multiplicity, both ascending, equal counts, |pred-obs| <= ceil(e*obs/100).
Deviation: sum (pred-obs)^2. Objective: min (total internal sites, total
deviation, sorted (enzyme, position) list, c, L)."""
import json, math, itertools, bisect
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple

# Real Type IIP enzymes: recognition sequence and top-strand cut offset (bases before the cut).
ENZYMES = {
    "EcoRI": ("GAATTC", 1), "BamHI": ("GGATCC", 1), "HindIII": ("AAGCTT", 1), "PstI": ("CTGCAG", 5),
    "SalI": ("GTCGAC", 1), "XbaI": ("TCTAGA", 1), "KpnI": ("GGTACC", 5), "SacI": ("GAGCTC", 5),
    "SphI": ("GCATGC", 5), "SmaI": ("CCCGGG", 3), "NcoI": ("CCATGG", 1), "NdeI": ("CATATG", 2),
    "XhoI": ("CTCGAG", 1), "BglII": ("AGATCT", 1), "EcoRV": ("GATATC", 3), "ScaI": ("AGTACT", 3),
    "PvuII": ("CAGCTG", 3), "DraI": ("TTTAAA", 3), "HpaI": ("GTTAAC", 3), "NheI": ("GCTAGC", 1),
    "SpeI": ("ACTAGT", 1), "ClaI": ("ATCGAT", 2), "NruI": ("TCGCGA", 3), "AatII": ("GACGTC", 5),
    "ApaI": ("GGGCCC", 5), "BspHI": ("TCATGA", 1), "BsrGI": ("TGTACA", 1), "MluI": ("ACGCGT", 1),
    "NsiI": ("ATGCAT", 5), "StuI": ("AGGCCT", 3), "PvuI": ("CGATCG", 4), "BclI": ("TGATCA", 1),
    "MfeI": ("CAATTG", 1), "AgeI": ("ACCGGT", 1), "AflII": ("CTTAAG", 1), "AvrII": ("CCTAGG", 1),
    "BspEI": ("TCCGGA", 1), "BstBI": ("TTCGAA", 2), "EagI": ("CGGCCG", 1), "FspI": ("TGCGCA", 3),
    "HincII": None, "NotI": ("GCGGCCGC", 2), "PacI": ("TTAATTAA", 5), "SwaI": ("ATTTAAAT", 4),
    "PmeI": ("GTTTAAAC", 4), "AscI": ("GGCGCGCC", 2), "SbfI": ("CCTGCAGG", 6), "FseI": ("GGCCGGCC", 6),
    "BstZ17I": ("GTATAC", 3), "SnaBI": ("TACGTA", 3), "XmnI": None, "SspI": ("AATATT", 3),
    "AflIII": None, "BsaAI": None, "MscI": ("TGGCCA", 3), "PmlI": ("CACGTG", 3), "Bsu36I": ("CCTNAGG", 2),
}
ENZYMES = {k: v for k, v in ENZYMES.items() if v is not None and set(v[0]) <= set("ACGT")}


def read_fasta(path):
    header = None; seq = []
    for line in open(path):
        line = line.strip()
        if not line: continue
        if line.startswith(">"):
            if header is None: header = line[1:]
            else: break
        else: seq.append(line.upper())
    return header, "".join(seq)


@dataclass
class Enzyme:
    name: str; site: str; offset: int


@dataclass
class Band:
    length: int; multiplicity: int


@dataclass
class Digest:
    id: str; enzymes: List[str]; bands: List[Band]  # bands ascending by length


@dataclass
class Problem:
    backbone: str                      # circular vector sequence
    enzymes: List[Enzyme]              # panel, ascending by name
    cloning_sites: List[Tuple[str, int]]  # (enzyme name, cut position in the vector)
    error_percent: int
    detection_limit: int
    insert_min: int
    insert_max: int
    max_internal: int
    observed: List[Digest]
    held_out: List[Tuple[str, List[str]]]  # (id, enzymes)

    @property
    def V(self): return len(self.backbone)


# ------------------------------------------------------------------ sites
def scan_sites(seq: str, site: str, offset: int):
    """(window start, cut position) for every occurrence on the circular sequence."""
    n = len(seq); w = len(site); ext = seq + seq[:w - 1]
    out = []
    start = ext.find(site)
    while start != -1 and start < n:
        out.append((start, (start + offset) % n))
        start = ext.find(site, start + 1)
    return out


def spans_junction(start, w, c, V):
    bases = {(start + k) % V for k in range(w)}
    return ((c - 1) % V) in bases and (c % V) in bases


def required_positions(p: Problem, enzyme: Enzyme, c: int, L: int):
    V = p.V; w = len(enzyme.site)
    req = []
    for start, cut in scan_sites(p.backbone, enzyme.site, enzyme.offset):
        if spans_junction(start, w, c, V): continue
        req.append(cut if cut < c else cut + L)
    return sorted(set(req))


# ----------------------------------------------------------------- digest
def fragments(positions, N):
    ps = sorted(set(positions))
    if not ps: return [N]
    out = [ps[i + 1] - ps[i] for i in range(len(ps) - 1)]
    out.append(N - ps[-1] + ps[0])
    return out


def predicted_bands(cuts_by_enzyme: Dict[str, List[int]], enzymes: List[str], N: int, D: int):
    """Ascending fragment lengths >= D of the union digest."""
    union = set()
    for e in enzymes: union.update(cuts_by_enzyme.get(e, []))
    return sorted(f for f in fragments(union, N) if f >= D)


def group(lengths):
    out = []
    for f in sorted(lengths):
        if out and out[-1].length == f: out[-1].multiplicity += 1
        else: out.append(Band(f, 1))
    return out


def expand(bands: List[Band]):
    out = []
    for b in bands: out.extend([b.length] * b.multiplicity)
    return sorted(out)


def tolerance(obs: int, e: int):
    return (e * obs + 99) // 100


def match(pred_sorted: List[int], bands: List[Band], e: int):
    """(matches, deviation)."""
    obs = expand(bands)
    if len(obs) != len(pred_sorted): return False, None
    dev = 0
    for pr, ob in zip(pred_sorted, obs):
        if abs(pr - ob) > tolerance(ob, e): return False, None
        dev += (pr - ob) * (pr - ob)
    return True, dev


# ------------------------------------------------------------------- maps
@dataclass
class Map:
    c: int; L: int; cuts: Dict[str, List[int]]   # enzyme -> ascending positions on the circle of N = V + L


def valid_map(p: Problem, m: Map) -> Optional[str]:
    """None when physically valid, else the reason."""
    if m.c not in [c for _, c in p.cloning_sites]: return "cloning site is not one of the vector's"
    if not (p.insert_min <= m.L <= p.insert_max): return "insert length outside the bounds"
    N = p.V + m.L
    names = [e.name for e in p.enzymes]
    if sorted(m.cuts) != names: return "cuts must name exactly the panel enzymes"
    for e in p.enzymes:
        ps = m.cuts[e.name]
        if ps != sorted(set(ps)) or any(q < 0 or q >= N for q in ps): return "%s positions not ascending, unique and within the circle" % e.name
        req = required_positions(p, e, m.c, m.L)
        outside = [q for q in ps if q < m.c or q > m.c + m.L]
        if outside != req: return "%s vector-region positions do not match the sequence" % e.name
        internal = [q for q in ps if m.c <= q <= m.c + m.L]
        if len(internal) > p.max_internal: return "%s has more than %d internal sites" % (e.name, p.max_internal)
    return None


def anchored(p: Problem) -> bool:
    """Input rule: every enzyme keeps at least one non-spanning backbone site at every cloning site."""
    for _, c in p.cloning_sites:
        for e in p.enzymes:
            if not required_positions(p, e, c, 0): return False
    return True


def internal_count(p: Problem, m: Map):
    return sum(1 for e in p.enzymes for q in m.cuts[e.name] if m.c <= q <= m.c + m.L)


def evaluate(p: Problem, m: Map):
    """(matches_all, total deviation) over the observed digests."""
    N = p.V + m.L; total = 0
    for d in p.observed:
        ok, dev = match(predicted_bands(m.cuts, d.enzymes, N, p.detection_limit), d.bands, p.error_percent)
        if not ok: return False, None
        total += dev
    return True, total


def key(p: Problem, m: Map, dev: int):
    return (internal_count(p, m), dev, [(e, q) for e in sorted(m.cuts) for q in m.cuts[e]], m.c, m.L)


def held_out_predictions(p: Problem, m: Map):
    N = p.V + m.L
    return {hid: group(predicted_bands(m.cuts, enz, N, p.detection_limit)) for hid, enz in p.held_out}


# --------------------------------------------------------------- documents
def problem_to_files(p: Problem, header: str):
    fasta = ">" + header + "\n" + "\n".join(p.backbone[i:i + 70] for i in range(0, len(p.backbone), 70)) + "\n"
    enzymes = {"format_version": 1,
               "enzymes": [{"name": e.name, "site": e.site, "cut_offset": e.offset} for e in p.enzymes],
               "cloning_sites": [{"enzyme": n, "position": c} for n, c in p.cloning_sites]}
    digests = {"format_version": 1, "error_percent": p.error_percent, "detection_limit": p.detection_limit,
               "insert_length": {"min": p.insert_min, "max": p.insert_max}, "max_internal_sites": p.max_internal,
               "observed": [{"id": d.id, "enzymes": d.enzymes, "bands": [{"length": b.length, "multiplicity": b.multiplicity} for b in d.bands]} for d in p.observed],
               "held_out": [{"id": hid, "enzymes": enz} for hid, enz in p.held_out]}
    return fasta, enzymes, digests


def load_problem(fasta_path, enzymes_doc, digests_doc) -> Problem:
    _, seq = read_fasta(fasta_path)
    enz = sorted([Enzyme(e["name"], e["site"], e["cut_offset"]) for e in enzymes_doc["enzymes"]], key=lambda e: e.name)
    obs = [Digest(d["id"], list(d["enzymes"]), [Band(b["length"], b["multiplicity"]) for b in d["bands"]]) for d in digests_doc["observed"]]
    return Problem(seq, enz, [(s["enzyme"], s["position"]) for s in enzymes_doc["cloning_sites"]], digests_doc["error_percent"],
                   digests_doc["detection_limit"], digests_doc["insert_length"]["min"], digests_doc["insert_length"]["max"],
                   digests_doc["max_internal_sites"], obs, [(h["id"], list(h["enzymes"])) for h in digests_doc["held_out"]])


def map_to_doc(p: Problem, m: Map):
    return {"format_version": 1, "cloning_site": m.c, "insert_length": m.L,
            "cuts": {e: list(m.cuts[e]) for e in sorted(m.cuts)},
            "held_out": {hid: [{"length": b.length, "multiplicity": b.multiplicity} for b in bands] for hid, bands in sorted(held_out_predictions(p, m).items())}}


def digest_id(enzymes):
    return "+".join(sorted(enzymes))
