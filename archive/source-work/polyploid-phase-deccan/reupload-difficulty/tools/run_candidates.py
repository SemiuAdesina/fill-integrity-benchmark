#!/usr/bin/env python3
"""Run every built candidate/variant binary on a cross with a wall-clock cap and grade
the output against a certified expected document (structure + digest + score within 1e-8)."""
import json, os, subprocess, sys, time, glob
R=os.path.expanduser("~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty")
def grade(out_path, expected):
    try: got=json.load(open(out_path))
    except Exception as e: return "NO_OUTPUT"
    if expected is None:
        return f"FINISHED total={got['total_log_likelihood']:.6f} digest={got['canonical_digest']}"
    if abs(got["total_log_likelihood"]-expected["total_log_likelihood"])>1e-8:
        return f"SUBOPT({got['total_log_likelihood']-expected['total_log_likelihood']:+.4f})"
    if got["haplotypes"]!=expected["haplotypes"] or got["paths"]!=expected["paths"]: return "WRONG_FORM"
    if got["canonical_digest"]!=expected["canonical_digest"]: return "WRONG_DIGEST"
    return "PASS"
LAB_FAMILIES={
 # author-side families built from the lab reference by switching mechanisms off
 "famB_loose_noincumbent_nobestfirst": "LAB_NO_PRICES=1,LAB_NO_PAIR=1,LAB_NO_WINDOW=1,LAB_NO_WARM=1,LAB_NO_BEST_FIRST=1,LAB_NO_FILTER=1",
 "famC_perprogeny_bound_only":         "LAB_NO_PRICES=1,LAB_NO_PAIR=1,LAB_NO_WINDOW=1,LAB_NO_WARM=1,LAB_NO_MARKER_BOUND=1,LAB_NO_FILTER=1",
 "famD_twobounds_sym_warm":            "LAB_NO_PRICES=1,LAB_NO_PAIR=1,LAB_NO_WINDOW=1",
 "famF_ref_new_pairbound":             "LAB_NO_PRICES=1",
}
def run_one(binary, cross, expected, cap, tmp, env_extra=None):
    out=f"{tmp}/{os.path.basename(binary)}.{os.getpid()}.{time.time_ns()}.json"
    t0=time.time()
    env=dict(os.environ)
    if env_extra: env.update(dict(kv.split("=") for kv in env_extra.split(",")))
    try:
        r=subprocess.run([binary,cross,out],capture_output=True,text=True,timeout=cap,env=env)
        secs=time.time()-t0
        if r.returncode!=0: return ("CRASH",secs,r.stderr[-200:])
        return (grade(out,expected),secs,"")
    except subprocess.TimeoutExpired:
        return ("TIMEOUT",cap,"")
    finally:
        try: os.remove(out)
        except: pass
if __name__=="__main__":
    cross=sys.argv[1]; exp=json.load(open(sys.argv[2])) if os.path.exists(sys.argv[2]) else None; cap=float(sys.argv[3]); labels=sys.argv[4:] or (sorted(os.listdir(f"{R}/lab/cand"))+list(LAB_FAMILIES))
    tmp=f"{R}/lab/tmp"; os.makedirs(tmp,exist_ok=True)
    for lab in labels:
        if lab in LAB_FAMILIES:
            b=f"{R}/lab/refcrate/target/release/refbench"; envx=LAB_FAMILIES[lab]
        else:
            b=f"{R}/lab/cand/{lab}/target/release/phase"; envx=None
        if not os.path.exists(b): print(f"{lab:<36} NO_BINARY"); continue
        res,secs,err=run_one(b,cross,exp,cap,tmp,envx)
        print(f"{lab:<36} {res:<18} {secs:7.2f}s {err}",flush=True)
