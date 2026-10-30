#!/usr/bin/env python3
"""check_result.py <result.json> <expected.json>: exact status/plan/sink_value comparison."""
import json, sys, os
sys.path.insert(0, os.path.expanduser('~/benchmark-suite-work/reduction-plan-deccan/research'))
import model
text = open(sys.argv[1]).read(); exp = json.load(open(sys.argv[2]))
try: status, plan = model.parse_result(text)
except Exception as e: print("MALFORMED:", e); sys.exit(1)
if status != exp["status"]: print("WRONG: status %s, expected %s" % (status, exp["status"])); sys.exit(1)
if status != "ok": print("OK", status); sys.exit(0)
want = exp["plan"]
if plan["paths"] != want["paths"] or plan["combine"] != want["combine"]: print("WRONG: plan differs: got", plan["paths"], plan["combine"], "want", want["paths"], want["combine"]); sys.exit(1)
if plan["sink_value"] != want["sink_value"]: print("WRONG: sink_value %d, expected %d" % (plan["sink_value"], want["sink_value"])); sys.exit(1)
print("OK plan matches"); sys.exit(0)
