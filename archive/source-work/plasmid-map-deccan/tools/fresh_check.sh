#!/usr/bin/env bash
set -uo pipefail
W=~/benchmark-suite-work/plasmid-map-deccan; ZIP="$1"; F="$W/fresh/$(date +%Y%m%d-%H%M%S)"; mkdir -p "$F"
unzip -q "$ZIP" -d "$F"; R="$F/plasmid-map"
echo "tree diff vs pkg:"; diff -rq "$W/pkg" "$R" -x target | head -5 || true; echo "(end diff)"
python3 - "$R" <<'PY'
import sys, os, json, subprocess
sys.path.insert(0, os.path.expanduser('~/benchmark-suite-work/plasmid-map-deccan/research'))
import bridge, model, oracle_bnb
root = sys.argv[1]; n = 0; agree = 0
REF = os.path.expanduser('~/benchmark-suite-work/plasmid-map-deccan/lab/ref/target/release/plasmid-map')
for fx in sorted(os.listdir(os.path.join(root, 'tests', 'fixtures'))):
    d = os.path.join(root, 'tests', 'fixtures', fx)
    if not os.path.isdir(d): continue
    exp = json.load(open(os.path.join(root, 'tests', 'fixtures', fx + '.expected.json')))
    prob = bridge.read_instance(d)
    subprocess.run([REF, d, '/tmp/pm_fresh_ref.json'], capture_output=True, timeout=600)
    m, _ = bridge.read_map('/tmp/pm_fresh_ref.json')
    same_ref = m.c == exp['map']['cloning_site'] and m.L == exp['map']['insert_length'] and m.cuts == exp['map']['cuts']
    # the independent certifier on every fixture the reference answers within a second, and the large ones by their recorded agreement
    py_same = None
    if not fx.startswith('s_'):
        mp, st = oracle_bnb.recover_bnb(prob, budget_s=600)
        py_same = mp is not None and mp.c == m.c and mp.L == m.L and mp.cuts == m.cuts
    n += 1; agree += same_ref and (py_same in (None, True))
    print("  %-18s reference=%s python=%s" % (fx, "same" if same_ref else "DIFFERS", py_same))
print("fresh fixtures: %d parsed; shipped certified answers reproduced on %d" % (n, agree))
PY
export RC_AGENT_TAG=fresh RC_VER_TAG=fresh
echo "fresh ORACLE: $(bash "$W/tools/run_control.sh" "$R" oracle fresh | head -2 | tr '\n' ' ')"
echo "fresh NOP: $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" nop fresh | head -2 | tr '\n' ' ')"
echo "fresh HOSTILE(h01): $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/security/controls/h01_write_reward_from_solver" | head -2 | tr '\n' ' ')"
echo "fresh HANG(h09): $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/security/controls/h09_hang" | head -2 | tr '\n' ' ')"
echo "fresh ADVERSARIAL(b11): $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/adversarial/variants/b11_deviation_before_count" | head -2 | tr '\n' ' ')"
echo "fresh ENUMERATION: $(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/adversarial/solver_enum/src" | head -2 | tr '\n' ' ')"
