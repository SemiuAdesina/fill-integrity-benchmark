#!/usr/bin/env python3
import json,sys,os
R=os.path.expanduser("~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty")
def load(ax):
    p=f"{R}/lab/corners/{ax}/results_v2.jsonl"
    if not os.path.exists(p): p=f"{R}/lab/corners/{ax}/results.jsonl"
    return [json.loads(l) for l in open(p)] if os.path.exists(p) else []
musts={
 "dr0":      lambda r: r["dr_a_marker0"]>=1 and not r["dr_allowed_b0"] and r["forbidden_dr_tempting"],
 "lowinfo":  lambda r: r["flat_cells"]>=5 and r["nearflat_cells"]>=6 and r["switches"]>0,
 "mixed":    lambda r: r["repeated_rows"]["A"]>=1 and r["rule4_needed"]["A"] and r["distinct_canonical"]>=2,
 "unlinked": lambda r: r["unlinked_intervals"]>=1 and r["repeated_rows"]["B"]>=1 and r["rule4_needed"]["B"] and r["distinct_canonical"]>=2,
}
def score(ax,r):
    s=0
    s+= 2*(r["path_level"] is True)
    s+= (r["distinct_canonical"]>=2)
    s+= (r["dr_states"]>0)
    s+= (r["switches"]>0)
    s+= (r["repeated_rows"]["A"]+r["repeated_rows"]["B"]>0)
    if ax=="dr0": s+= 2*r["dr_a_marker0"] + (r["dr_b_marker0"]==0)
    if ax=="lowinfo": s+= (r["distinct_canonical"]==1) + min(r["flat_cells"],12)/12
    if ax=="unlinked": s+= (r["rule_deciding"] in ("B","paths"))
    return s
for ax in ("dr0","lowinfo","mixed","unlinked"):
    rows=[r for r in load(ax) if r["certified"]]
    ok=[r for r in rows if musts[ax](r)]
    ok.sort(key=lambda r:-score(ax,r))
    print(f"===== {ax}: {len(rows)} certified, {len(ok)} satisfy the axis musts =====")
    for r in ok[:5]:
        print(f"  {r['name']:<14} ref={r['ref_secs']:.2f}s space={r['space']:<5} tied_assign={r['distinct_assignments']:<5} distinct_canon={r['distinct_canonical']:<3} rule={r['rule_deciding']:<5} "
              f"drA0={r['dr_a_marker0']} drB0={r['dr_b_marker0']} drs={r['dr_states']} tempt={r['forbidden_dr_tempting']} rr={r['repeated_rows']} rule4={r['rule4_needed']} "
              f"unl={r['unlinked_intervals']} flat={r['flat_cells']} near={r['nearflat_cells']} sw={r['switches']} paths={r['path_level']} digest={r['digest']}")
