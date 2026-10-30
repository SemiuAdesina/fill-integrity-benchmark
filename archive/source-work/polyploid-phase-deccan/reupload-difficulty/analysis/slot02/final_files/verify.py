#!/usr/bin/env python3
"""Independent check of a result document against the specification."""
import json, sys, itertools, math

def load(p):
    return json.load(open(p))

def score(cross, res):
    M = cross['markers']; N = cross['offspring']
    hap = res['haplotypes']; paths = res['paths']
    total = 0.0
    rate = {P: cross['double_reduction'][P]['rate'] for P in 'AB'}
    allowed = {P: cross['double_reduction'][P]['allowed'] for P in 'AB'}
    rec = {P: cross['recombination'][P] for P in 'AB'}
    for m in range(M):
        for P in 'AB':
            found = sum(hap[P][h][m] for h in range(4))
            assert found == cross['parent_dosage'][P][m], (P, m, found)
    for c in range(N):
        for m in range(M):
            for P in 'AB':
                a, b = paths[c][P][m]
                assert 0 <= a <= b < 4
                if a == b:
                    assert allowed[P][m], (c, P, m)
            d = 0
            for P in 'AB':
                a, b = paths[c][P][m]
                d += hap[P][a][m] + hap[P][b][m]
            total += cross['child_gl'][c][m][d]
            for P in 'AB':
                a, b = paths[c][P][m]
                r = rate[P][m]
                total += math.log(r) if a == b else math.log(1.0 - r)
            if m + 1 < M:
                for P in 'AB':
                    s = paths[c][P][m]; t = paths[c][P][m+1]
                    inter = 0
                    right = list(t)
                    for x in s:
                        if x in right:
                            right.remove(x); inter += 1
                    k = 2 - inter
                    r = rec[P][m]
                    total += k * math.log(r) + (2 - k) * math.log(1.0 - r)
    return total

def digest(res):
    M = len(res['haplotypes']['A'][0])
    out = bytearray()
    for P in 'AB':
        for h in range(4):
            for m in range(M):
                out.append(ord('0') + res['haplotypes'][P][h][m])
        out.append(ord('|'))
    for path in res['paths']:
        for P in 'AB':
            for a, b in path[P]:
                out.append(ord('0') + a); out.append(ord('0') + b)
        out.append(ord(';'))
    out.append(ord('#'))
    h = 14695981039346656037
    for x in out:
        h ^= x
        h = (h * 1099511628211) % (1 << 64)
    return '%016x' % h

def relabel_key(res, P):
    rows = [None]*4; sig = []
    return rows, sig

def canonical_defect(res):
    M = len(res['haplotypes']['A'][0])
    for P in 'AB':
        rows = res['haplotypes'][P]
        for p in itertools.permutations(range(4)):
            newrows = [None]*4
            for h in range(4):
                newrows[p[h]] = rows[h]
            sig = []
            for path in res['paths']:
                for a, b in path[P]:
                    x, y = sorted((p[a], p[b]))
                    sig.append(x); sig.append(y)
            key = (newrows, sig, tuple(p))
            ident = (rows, [v for path in res['paths'] for a, b in path[P] for v in (a, b)], (0,1,2,3))
            if key < ident:
                return "parent %s: permutation %s sorts earlier" % (P, p)
    return None

def main():
    cross = load(sys.argv[1] if len(sys.argv) > 1 else '/app/cross.json')
    res = load(sys.argv[2] if len(sys.argv) > 2 else '/app/result.json')
    total = score(cross, res)
    print('scorer total      =', repr(total))
    print('reported total    =', repr(res['total_log_likelihood']))
    print('delta             =', abs(total - res['total_log_likelihood']))
    d = canonical_defect(res)
    print('canonical defect  =', d)
    print('digest computed   =', digest(res))
    print('digest reported   =', res['canonical_digest'])

main()
