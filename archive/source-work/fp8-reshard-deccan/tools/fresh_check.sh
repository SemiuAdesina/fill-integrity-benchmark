#!/usr/bin/env bash
set -uo pipefail
W=~/benchmark-suite-work/fp8-reshard-deccan; ZIP="$1"; F="$W/fresh/$(date +%Y%m%d-%H%M%S)"; mkdir -p "$F"
unzip -q "$ZIP" -d "$F"; R="$F/fp8-reshard"
echo "tree diff vs pkg:"; diff -rq "$W/pkg" "$R" -x build | head -5 || true; echo "(end diff)"
python3 - "$R" <<'PY'
import sys, os, json
sys.path.insert(0, os.path.expanduser('~/benchmark-suite-work/fp8-reshard-deccan/research')); sys.path.insert(0, os.path.expanduser('~/benchmark-suite-work/fp8-reshard-deccan/oracle-b'))
import model, oracle_b
root = sys.argv[1]; n=0; agree=0
for fx in sorted(os.listdir(os.path.join(root, 'tests', 'fixtures'))):
    d = os.path.join(root, 'tests', 'fixtures', fx)
    if not os.path.isdir(d): continue
    ck=json.load(open(d+'/checkpoint.json')); tg=json.load(open(d+'/target.json')); exp=json.load(open(os.path.join(root,'tests','fixtures',fx+'.expected.json')))
    recs=model.load_records(ck); tgs=model.load_targets(tg); assert model.well_formed(recs,tgs)
    try: a=[s.to_json() for s in model.restore(recs,tgs)]
    except model.Reject: a=None
    try: b=oracle_b.restore_b(ck,tg)
    except oracle_b.RejectB: b=None
    n+=1; agree += (a==b==exp['result'])
print("fresh fixtures: %d parsed and well-formed; oracle A = oracle B = shipped certified answer on %d" % (n, agree))
PY
export RC_AGENT_TAG=fresh RC_VER_TAG=fresh
echo "fresh ORACLE: $(bash "$W/tools/run_control.sh" "$R" oracle fresh | head -2 | tr '\n' ' ')"
echo "fresh NOP: $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" nop fresh | head -2 | tr '\n' ' ')"
echo "fresh HOSTILE(h01): $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/security/controls/h01_write_reward_from_probe" | head -2 | tr '\n' ' ')"
echo "fresh HANG(h09): $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/security/controls/h09_hang" | head -2 | tr '\n' ' ')"
echo "fresh ADVERSARIAL(p03): $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/adversarial/variants/p03_rotation_from_newest" | head -2 | tr '\n' ' ')"
echo "fresh SOLVER_B: $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/adversarial/solver_b" | head -2 | tr '\n' ' ')"
