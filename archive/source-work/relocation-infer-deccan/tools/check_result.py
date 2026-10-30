#!/usr/bin/env python3
"""check_result.py <relocations.json> <expected.json>: compare programs and predictions."""
import json, sys
got=json.load(open(sys.argv[1])); exp=json.load(open(sys.argv[2]))
gp={p['type']:p for p in got.get('programs',[])}; ep={p['type']:p for p in exp['programs']}
bad=[]
for t,p in ep.items():
    if t not in gp: bad.append(f"type {t}: missing"); continue
    if gp[t]!=p: bad.append(f"type {t}: got {json.dumps(gp[t])} expected {json.dumps(p)}")
for t in gp:
    if t not in ep: bad.append(f"type {t}: unexpected program")
gpred={(q['object'],q['link'],q['section'],q['offset']):q['word'] for q in got.get('predictions',[])}
epred={(q['object'],q['link'],q['section'],q['offset']):q['word'] for q in exp['predictions']}
mism=[k for k in epred if gpred.get(k,'MISSING')!=epred[k]]
extra=[k for k in gpred if k not in epred]
print("programs:", "OK" if not bad else f"{len(bad)} wrong"); [print("  ",b) for b in bad[:10]]
print(f"predictions: {len(epred)-len(mism)}/{len(epred)} match; extra={len(extra)}")
for k in mism[:8]: print("   mismatch",k,"got",gpred.get(k,'MISSING'),"expected",epred[k])
sys.exit(0 if not bad and not mism and not extra else 1)
