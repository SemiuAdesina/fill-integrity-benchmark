#!/usr/bin/env python3
"""check_result.py <restored.json> <expected.json>: exact outcome/state comparison plus replay-trace check."""
import json, sys, os
sys.path.insert(0, os.path.expanduser('~/benchmark-suite-work/fp8-reshard-deccan/research'))
import model
text = open(sys.argv[1]).read(); exp = json.load(open(sys.argv[2]))
try:
    got = model.parse_result(text)
except Exception as e:
    print("MALFORMED:", e); sys.exit(1)
if exp["result"] is None:
    print("OK reject" if got is None else "WRONG: expected reject, got %d states" % len(got)); sys.exit(0 if got is None else 1)
if got is None:
    print("WRONG: expected %d states, got reject" % len(exp["result"])); sys.exit(1)
want = exp["result"]; gj = [s.to_json() for s in got]
if gj != want:
    bad = [i for i in range(max(len(gj), len(want))) if i >= len(gj) or i >= len(want) or gj[i] != want[i]]
    print("WRONG: %d/%d states differ (first at %d)" % (len(bad), len(want), bad[0]));
    if bad[0] < len(gj) and bad[0] < len(want): print("  got ", gj[bad[0]]); print("  want", want[bad[0]])
    sys.exit(1)
for r in exp["replay"]:
    s = got[r["state_index"]]
    if model.trace(s, [tuple(p) for p in r["pairs"]]) != r["trace"]:
        print("WRONG: replay trace differs for state", r["state_index"]); sys.exit(1)
print("OK %d states, %d replays" % (len(gj), len(exp["replay"])))
