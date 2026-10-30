#!/usr/bin/env python3
"""Independent, spec-derived certification of a fixture's expected answer.

Re-implements spec/scoring-model.md, spec/result-format-v1.md canonical form
(all 24 relabellings, all three selection rules) and the FNV-1a digest from the
text of the specification alone, and checks the certified answer against them.
Nothing here shares code with the Rust reference."""
import json, math, itertools, sys

def switch_count(s, t):
    right = list(t); shared = 0
    for x in s:
        if x in right:
            right.remove(x); shared += 1
    return 2 - shared

def score(cross, rec):
    M, N = cross["markers"], cross["offspring"]
    hap = {P: rec["haplotypes"][P] for P in "AB"}
    dos = cross["parent_dosage"]; rec_ = cross["recombination"]; dr = cross["double_reduction"]
    for P in "AB":
        assert len(hap[P]) == 4, "four rows"
        for h in range(4):
            assert len(hap[P][h]) == M and all(v in (0, 1) for v in hap[P][h]), "binary rows of length M"
        for m in range(M):
            assert sum(hap[P][h][m] for h in range(4)) == dos[P][m], f"dosage {P}@{m}"
    assert len(rec["paths"]) == N
    total = 0.0
    for c in range(N):
        pa, pb = rec["paths"][c]["A"], rec["paths"][c]["B"]
        assert len(pa) == M and len(pb) == M
        for m in range(M):
            a, b = pa[m]; d, e = pb[m]
            assert 0 <= a <= b < 4 and 0 <= d <= e < 4, "ordered pair naming existing homologs"
            if a == b: assert dr["A"]["allowed"][m], f"illegal DR A c{c} m{m}"
            if d == e: assert dr["B"]["allowed"][m], f"illegal DR B c{c} m{m}"
            total += cross["child_gl"][c][m][hap["A"][a][m] + hap["A"][b][m] + hap["B"][d][m] + hap["B"][e][m]]
            for P, (x, y) in (("A", (a, b)), ("B", (d, e))):
                r = dr[P]["rate"][m]
                total += math.log(r) if x == y else math.log(1 - r)
            if m + 1 < M:
                for P, path in (("A", pa), ("B", pb)):
                    r = rec_[P][m]; k = switch_count(tuple(path[m]), tuple(path[m + 1]))
                    total += k * math.log(r) + (2 - k) * math.log(1 - r)
    return total

def digest(rec):
    b = bytearray()
    for P in "AB":
        for h in range(4):
            for v in rec["haplotypes"][P][h]:
                b.append(ord('0') + v)
        b.append(ord('|'))
    for p in rec["paths"]:
        for P in "AB":
            for a, c in p[P]:
                b.append(ord('0') + a); b.append(ord('0') + c)
        b.append(ord(';'))
    b.append(ord('#'))
    h = 14695981039346656037
    for x in b:
        h ^= x; h = (h * 1099511628211) & 0xFFFFFFFFFFFFFFFF
    return f"{h:016x}"

def canonical_parent(rows, states_per_progeny):
    """Spec steps 2-6 for one parent, returning the chosen rows and states."""
    best = None
    for p in itertools.permutations(range(4)):
        newrows = [None] * 4
        for h in range(4):
            newrows[p[h]] = rows[h]
        if not all(newrows[i] <= newrows[i + 1] for i in range(3)):
            continue                                                # step 3
        sig = []
        for prog in states_per_progeny:
            for a, b in prog:
                x, y = sorted((p[a], p[b])); sig += [x, y]          # step 4
        key = (tuple(map(tuple, newrows)), tuple(sig), tuple(p))    # step 5
        if best is None or key < best[0]:
            best = (key, p, newrows)
    _, p, newrows = best
    newstates = [[sorted((p[a], p[b])) for a, b in prog] for prog in states_per_progeny]
    return newrows, newstates

def is_canonical(rec):
    for P in "AB":
        rows = [list(r) for r in rec["haplotypes"][P]]
        st = [[list(s) for s in prog[P]] for prog in rec["paths"]]
        nr, ns = canonical_parent(rows, st)
        if nr != rows or ns != st:
            return False
    return True

if __name__ == "__main__":
    ok = True
    for stem in sys.argv[1:]:
        cross = json.load(open(f"{stem}.cross.json"))
        exp = json.load(open(f"{stem}.expected.json"))
        try:
            s = score(cross, exp)
            feasible = True; why = ""
        except AssertionError as e:
            s = float("nan"); feasible = False; why = str(e)
        score_ok = feasible and abs(s - exp["total_log_likelihood"]) <= 1e-8
        canon_ok = is_canonical(exp)
        dig_ok = digest(exp) == exp["canonical_digest"]
        fv_ok = exp.get("format_version") == 1
        good = score_ok and canon_ok and dig_ok and fv_ok
        ok &= good
        name = stem.split("/")[-1]
        print(f"{name:<20} feasible={feasible}{'' if feasible else ' (' + why + ')'} "
              f"independent_score={s:.10f} matches={score_ok} canonical(24-perm)={canon_ok} "
              f"digest={dig_ok} format_version={fv_ok}  {'PASS' if good else 'FAIL'}")
    sys.exit(0 if ok else 1)
