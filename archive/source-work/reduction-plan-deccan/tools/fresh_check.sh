#!/usr/bin/env bash
# Fresh-extraction check: unzip the final ZIP, diff against pkg, recompute every
# certified answer with author-side oracles (exhaustive A and B on the small
# fixtures; pruned C and D on the scale fixtures), then run the real verifier
# on the extraction in oracle, NOP, hostile, hang, adversarial and solver_d modes.
set -uo pipefail
W=~/benchmark-suite-work/reduction-plan-deccan; ZIP="$1"; F="$W/fresh/$(date +%Y%m%d-%H%M%S)"; mkdir -p "$F"
unzip -q "$ZIP" -d "$F"; R="$F/reduction-plan"
echo "tree diff vs pkg:"; diff -rq "$W/pkg" "$R" -x build | head -5 || true; echo "(end diff)"
python3 - "$R" <<'PY'
import sys, os, json, time
sys.path.insert(0, os.path.expanduser('~/benchmark-suite-work/reduction-plan-deccan/research')); sys.path.insert(0, os.path.expanduser('~/benchmark-suite-work/reduction-plan-deccan/oracle-b'))
import model, oracle_b, bnb, oracle_d
root = sys.argv[1]; n=0; agree=0; slow=[]
for fx in sorted(os.listdir(os.path.join(root, 'tests', 'fixtures'))):
    d = os.path.join(root, 'tests', 'fixtures', fx)
    if not os.path.isdir(d): continue
    doc=json.load(open(d+'/problem.json')); exp=json.load(open(os.path.join(root,'tests','fixtures',fx+'.expected.json')))
    p=model.load_problem(doc); t0=time.time()
    if fx.startswith('s_'):
        import subprocess, tempfile
        sa, pa, _ = bnb.recover_c(p, budget_s=1800); which='C=solver_d'
        with tempfile.TemporaryDirectory() as td:
            out = os.path.join(td, 'r.json')
            subprocess.run([os.path.expanduser('~/benchmark-suite-work/reduction-plan-deccan/lab/cand/solver_d/reduction-recover'), '--problem', d + '/problem.json', '--out', out], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=600)
            r = json.load(open(out))
        sb = r['status']; pb = None if sb != 'ok' else {"paths": r['paths'], "combine": r['combine'], "sink_value": r['sink_value']}
    else:
        sa, pa, _ = model.recover(p); sb, pb = oracle_b.recover_b(doc); which='A=B'
    a = None if pa is None else {"paths": pa["paths"], "combine": pa["combine"], "sink_value": pa["sink_value"]}
    b = None if pb is None else {"paths": pb["paths"], "combine": pb["combine"], "sink_value": pb["sink_value"]}
    n+=1; ok = (sa==sb==exp['status'] and a==b==exp['plan']); agree += ok
    print("  %-26s %s %s %.1fs" % (fx, which, 'agree' if ok else 'DISAGREE', time.time()-t0))
print("fresh fixtures: %d parsed; both oracles = shipped certified answer on %d" % (n, agree))
PY
export RC_AGENT_TAG=fresh RC_VER_TAG=fresh
echo "fresh ORACLE: $(bash "$W/tools/run_control.sh" "$R" oracle fresh | head -2 | tr '\n' ' ')"
echo "fresh NOP: $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" nop fresh | head -2 | tr '\n' ' ')"
echo "fresh HOSTILE(h01): $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/security/controls/h01_write_reward_from_probe" | head -2 | tr '\n' ' ')"
echo "fresh HANG(h09): $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/security/controls/h09_hang" | head -2 | tr '\n' ' ')"
echo "fresh ADVERSARIAL(p03): $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/adversarial/variants/p03_latency_in_occupancy" | head -2 | tr '\n' ' ')"
echo "fresh SOLVER_D: $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/adversarial/solver_d" | head -2 | tr '\n' ' ')"
echo "fresh EXHAUSTIVE(solver_b): $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/adversarial/solver_b" | head -2 | tr '\n' ' ')"
