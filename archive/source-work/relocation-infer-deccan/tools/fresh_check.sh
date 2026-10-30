#!/usr/bin/env bash
# Fresh-extraction certification: extract the ZIP into an empty directory, compare with pkg/,
# cold-build both images from the extraction, run Oracle, NOP, a hostile control, a hang control,
# and one adversarial solver, and validate the fresh corpus structurally with the independent parser.
set -uo pipefail
W=~/benchmark-suite-work/relocation-infer-deccan; ZIP="$1"; F="$W/fresh/$(date +%Y%m%d-%H%M%S)"; mkdir -p "$F"
unzip -q "$ZIP" -d "$F"
R="$F/relocation-infer"
echo "tree diff vs pkg (excluding target dirs):"; diff -rq "$W/pkg" "$R" -x target | head -5 || true; echo "(end diff)"
python3 - "$R" <<'PY'
import sys, os
sys.path.insert(0, os.path.expanduser('~/benchmark-suite-work/relocation-infer-deccan/research'))
import model
root = sys.argv[1]; n = 0
for fx in sorted(os.listdir(os.path.join(root, 'tests', 'fixtures'))):
    d = os.path.join(root, 'tests', 'fixtures', fx)
    if not os.path.isdir(d): continue
    man, objs, outs, sites = model.load_corpus(os.path.join(d, 'corpus'))
    for e in list(objs.values()) + [o for o in outs.values() if o is not None]:
        assert e.e_machine == model.EM_VX and e.sections[0].type == 0
    n += 1
man, objs, outs, sites = model.load_corpus(os.path.join(root, 'environment', 'corpus'))
print("fresh corpus structural validation: %d fixture corpora + public corpus parse with the independent ELF reader; %d public sites" % (n, len(sites)))
PY
export RC_AGENT_TAG=fresh RC_VER_TAG=fresh
r1=$(bash "$W/tools/run_control.sh" "$R" oracle fresh | head -2 | tr '\n' ' '); echo "fresh ORACLE: $r1"
r2=$(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" nop fresh | head -2 | tr '\n' ' '); echo "fresh NOP: $r2"
r3=$(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/security/controls/h01_write_reward_from_probe" | head -2 | tr '\n' ' '); echo "fresh HOSTILE(h01): $r3"
r4=$(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/security/controls/h09_hang" | head -2 | tr '\n' ' '); echo "fresh HANG(h09): $r4"
r5=$(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/adversarial/variants/p07_prefers_signed" | head -2 | tr '\n' ' '); echo "fresh ADVERSARIAL(p07): $r5"
r6=$(RC_SKIP_BUILD=1 bash "$W/tools/run_control.sh" "$R" patched fresh "$W/adversarial/solver_b" | head -2 | tr '\n' ' '); echo "fresh SOLVER_B: $r6"
# oracle A/B spot-check on the fresh public corpus
python3 - "$R" <<'PY'
import sys, os
W=os.path.expanduser('~/benchmark-suite-work/relocation-infer-deccan')
sys.path.insert(0, W+'/research'); sys.path.insert(0, W+'/oracle-a'); sys.path.insert(0, W+'/oracle-b')
import oracle_a, oracle_b, json
root=sys.argv[1]
for fx in ['public','case_c']:
    d=os.path.join(root,'tests','fixtures',fx,'corpus'); ra=oracle_a.solve(d); rb=oracle_b.solve(d)
    exp=json.load(open(os.path.join(root,'tests','fixtures',fx+'.expected.json')))
    canon={}
    for k,v in ra.items():
        if isinstance(k,tuple): canon[v['types'][0]]=v['canonical'][0]; canon[v['types'][1]]=v['canonical'][1]
        else: canon[k]=v['canonical']
    ok=all(canon[p['type']].to_json(p['type'])==p for p in exp['programs']) and all(set(ra[k]['fits'])==set(rb[k]['fits']) for k in ra)
    print("fresh oracle A/B spot-check %s: %s" % (fx, "AGREE with shipped certified programs" if ok else "DISAGREE"))
PY
