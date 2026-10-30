#!/usr/bin/env python3
"""Run the lab reference with a cap; on success write <stem>.expected.json and a stats line."""
import json,subprocess,sys,time,os
R=os.path.expanduser("~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty")
cap=float(sys.argv[1])
for f in sys.argv[2:]:
    exp=f.replace(".cross.json",".expected.json"); t0=time.time()
    try:
        r=subprocess.run([f"{R}/lab/refcrate/target/release/refbench",f,exp],capture_output=True,text=True,timeout=cap)
        line=r.stdout.strip().splitlines()[-1] if r.returncode==0 else "ERROR "+r.stderr[-200:]
        print(f"{os.path.basename(f)} {time.time()-t0:.1f}s {line}",flush=True)
    except subprocess.TimeoutExpired:
        print(f"{os.path.basename(f)} TIMEOUT>{cap}s",flush=True)
