#!/usr/bin/env python3
"""Recompute the corner properties for every certified candidate of one axis
(re-enumerates; the certification itself is not repeated)."""
import json,os,sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import corner_gen as cg
R=os.path.expanduser("~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty")
ax=sys.argv[1]; d=f"{R}/lab/corners/{ax}"
out=open(f"{d}/results_v2.jsonl","w")
for line in open(f"{d}/results.jsonl"):
    r=json.loads(line)
    if not r["certified"]: continue
    cross=json.load(open(f"{d}/{r['name']}.cross.json")); exp=json.load(open(f"{d}/{r['name']}.expected.json"))
    best,opts,space=cg.enumerate_optima(cross)
    relaxed=None
    if ax=="dr0": relaxed,_,_=cg.enumerate_optima(cross,legality="relaxed")
    r.update(cg.props(cross,exp,opts,relaxed))
    out.write(json.dumps(r)+"\n"); out.flush()
print(ax,"done")
