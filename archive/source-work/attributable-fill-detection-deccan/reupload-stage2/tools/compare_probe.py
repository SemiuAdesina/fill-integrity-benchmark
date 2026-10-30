#!/usr/bin/env python3
"""Compare probe records across labels. usage: compare_probe.py <ref_label> <labels...> -- <cases...>"""
import sys, os
R=os.path.expanduser("~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2/lab/probe")
args=sys.argv[1:]; i=args.index("--"); ref=args[0]; labels=args[1:i]; cases=args[i+1:]
def rec(l,c):
    p=f"{R}/{l}/results/{c}"; return open(p).read() if os.path.exists(p) else None
def summ(t):
    if t is None: return "nofile"
    pos=[l.split("|")[1].split("=")[1] for l in t.splitlines() if l.startswith("position|")]
    tk=[(l.split("strategy=")[1].split("|")[0], l.split("size=")[1].split("|")[0]) for l in t.splitlines() if l.startswith("venue-order|") and "ImmediateOrCancel" in l]
    canc=[l for l in t.splitlines() if l.startswith("cancelled-count")]
    att=[l.split("result=")[1] for l in t.splitlines() if l.startswith("attempt|")]
    return f"pos={pos} takers={tk} {canc[0] if canc else ''} attempts={att}"
print(f"{'case':<88}"+"".join(f"{l:>8}" for l in labels))
for c in cases:
    r=rec(ref,c); row=[]
    for l in labels:
        t=rec(l,c); row.append("  same" if t==r else ("  MISS" if t is None else "  DIFF"))
    print(f"{c:<88}"+"".join(f"{x:>8}" for x in row))
if "-v" in os.environ.get("PROBE_VERBOSE","-v"):
    for c in cases:
        print(f"\n== {c}\n  {ref:>8}: {summ(rec(ref,c))}")
        for l in labels:
            if rec(l,c)!=rec(ref,c): print(f"  {l:>8}: {summ(rec(l,c))}")
