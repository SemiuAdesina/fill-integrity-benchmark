#!/usr/bin/env python3
"""Generate a pool of simulated crosses over structural knobs and measure the
instrumented reference (and its weakened lab variants) with a wall-clock cap."""
import json, os, subprocess, sys, time, itertools, random
sys.path.insert(0, os.path.dirname(__file__))
from gen_cross import simulate
R=os.path.expanduser("~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty")
BENCH=f"{R}/lab/refcrate/target/release/refbench"
def run(bench, cross_path, cap, env=None, out=None):
    e=dict(os.environ); e.update(env or {})
    t0=time.time()
    try:
        r=subprocess.run([bench,cross_path]+([out] if out else []),capture_output=True,text=True,timeout=cap,env=e)
        if r.returncode!=0: return {"status":"error","secs":time.time()-t0,"err":r.stderr[-300:]}
        d=json.loads(r.stdout.strip().splitlines()[-1].replace(':inf',':1e308').replace(':-inf',':-1e308').replace(':NaN',':null')); d["status"]="ok"; return d
    except subprocess.TimeoutExpired:
        return {"status":"timeout","secs":cap}
def main():
    out_dir=sys.argv[1]; cap=float(sys.argv[2]); os.makedirs(out_dir,exist_ok=True)
    configs=json.load(open(sys.argv[3]))
    log=open(f"{out_dir}/results.jsonl","a")
    for cfg in configs:
        name=cfg["name"]; path=f"{out_dir}/{name}.cross.json"
        kw={k:v for k,v in cfg.items() if k not in ("name","seed","M","N")}
        cross,_=simulate(cfg["seed"],cfg["M"],cfg["N"],**kw)
        json.dump(cross,open(path,"w"),separators=(",",":"))
        rec={"name":name,"cfg":cfg}
        rec["ref"]=run(BENCH,path,cap,{"LAB_NO_PRICES":"1"},out=path.replace(".cross.json",".expected.json"))
        log.write(json.dumps(rec)+"\n"); log.flush()
        r=rec["ref"]
        print(f"{name:<40} M={cfg['M']:<3}N={cfg['N']:<3} ref={r['status']}:{r.get('secs',0):7.2f}s exp={r.get('expanded','-')} gap={r.get('root_gap',float('nan')):.1f} warmgap={r.get('warm_gap',float('nan')):.2f} ties={r.get('tie_replacements','-')} rr={r.get('repeated_rows','-')} dr={r.get('dr_states','-')} sw={r.get('switches','-')}", flush=True)
if __name__=="__main__": main()
