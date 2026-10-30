#!/usr/bin/env python3
import json,subprocess,sys,time,os
R=os.path.expanduser("~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty")
cap=float(sys.argv[1]); env=dict(os.environ); env.update(dict(kv.split("=") for kv in sys.argv[2].split(",") if kv)) if len(sys.argv)>2 and sys.argv[2] else None
for f in sys.argv[3:]:
    t0=time.time()
    try:
        r=subprocess.run([f"{R}/lab/refcrate/target/release/refbench",f],capture_output=True,text=True,timeout=cap,env=env)
        if r.returncode!=0: print(f"{os.path.basename(f):<32} ERROR {r.stderr[-200:]}"); continue
        d=json.loads(r.stdout.strip().splitlines()[-1].replace(':inf',':1e308').replace(':-inf',':-1e308').replace(':NaN',':null'))
        print(f"{os.path.basename(f):<32} secs={d['secs']:7.3f} warm={d['warm_secs']:.2f}s/gap={d['warm_gap']:.4f} pricegap={d['price_gap']:.2f} windowgap={d['window_gap']:.2f} filt={d['pairs_before']}->{d['pairs_after']} exp={d['expanded']} rootgap={d['root_gap']:.2f} ties={d['tie_replacements']} distinct={d['distinct_optima']} digest={d['digest']}")
    except subprocess.TimeoutExpired:
        print(f"{os.path.basename(f):<32} TIMEOUT>{cap}s")
