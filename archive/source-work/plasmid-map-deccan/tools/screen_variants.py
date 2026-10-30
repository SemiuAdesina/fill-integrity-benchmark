#!/usr/bin/env python3
"""Runs solver trees (built under lab/var_*) on every retained fixture with a cap;
reports per fixture whether the answer equals the certified map."""
import os, sys, json, subprocess, time
W = os.path.expanduser('~/benchmark-suite-work/plasmid-map-deccan')
sys.path.insert(0, os.path.join(W, 'research')); import bridge
cap = float(sys.argv[1]); names = sys.argv[2:]
fixtures = sorted(f for f in os.listdir(os.path.join(W, 'author/fixtures')) if os.path.isdir(os.path.join(W, 'author/fixtures', f)))
for name in names:
    binp = os.path.join(W, 'lab', 'var_' + name, 'target/release/plasmid-map') if name != 'enum' else os.path.join(W, 'lab/enum/target/release/plasmid-map')
    row = []
    for fx in fixtures:
        d = os.path.join(W, 'author/fixtures', fx); exp = json.load(open(os.path.join(d, 'expected.json')))
        out = '/tmp/pm_screen_%s_%s.json' % (name, fx)
        if os.path.exists(out): os.remove(out)
        t0 = time.time()
        try:
            r = subprocess.run([binp, d, out], capture_output=True, text=True, timeout=cap)
            if r.returncode != 0 or not os.path.exists(out): row.append('%s:CRASH' % fx); continue
            m, held = bridge.read_map(out)
            same = m.c == exp['map']['cloning_site'] and m.L == exp['map']['insert_length'] and m.cuts == exp['map']['cuts']
            row.append('%s:%s(%.0fs)' % (fx, 'ok' if same else 'WRONG', time.time() - t0))
        except subprocess.TimeoutExpired:
            row.append('%s:TIMEOUT' % fx)
    wrong = sum(1 for x in row if 'WRONG' in x or 'TIMEOUT' in x or 'CRASH' in x)
    print('%-32s separated_on=%d  %s' % (name, wrong, ' '.join(row)), flush=True)
