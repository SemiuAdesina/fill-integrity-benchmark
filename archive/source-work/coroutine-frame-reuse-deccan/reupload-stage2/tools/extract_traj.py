#!/usr/bin/env python3
"""Author-side trajectory extraction for coroutine-frame-reuse: command history,
assistant prose, build/run observations, agent-authored test files, python-replayed
final deliverables, per-attempt summary."""
import json, os, re, sys, collections
ROOT = os.path.expanduser("~/benchmark-suite-work/coroutine-frame-reuse-deccan/reupload-stage2/export/arms/hy4_difficulty/res-14043ce34a2950e7840ebc5af992fd04")
OUT = os.path.expanduser("~/benchmark-suite-work/coroutine-frame-reuse-deccan/reupload-stage2/analysis")
DELIV = ("/app/include/roundel/runtime.hpp", "/app/src/runtime.cpp")
summary = {}
for att in sorted(os.listdir(os.path.join(ROOT, "attempts"))):
    slot = att.split("-slot-")[1][:2]
    d = os.path.join(ROOT, "attempts", att)
    steps = json.load(open(os.path.join(d, "trajectory.json")))["steps"]
    od = os.path.join(OUT, f"slot{slot}"); os.makedirs(od, exist_ok=True)
    files = {}; hist = collections.Counter(); cmdlog, prose, builds, bash_writes = [], [], [], []
    for s in steps:
        if s.get("source") != "agent": continue
        msg = s.get("message") or ""
        if msg.strip(): prose.append(f"[step {s['step_id']}] {msg.strip()}")
        results = {r.get("source_call_id"): r.get("content", "") for r in (s.get("observation") or {}).get("results", [])}
        for tc in s.get("tool_calls") or []:
            fn = tc.get("function_name"); a = tc.get("arguments") or {}; hist[fn] += 1
            obs = results.get(tc.get("tool_call_id"), "") or ""; sid = s["step_id"]
            if fn == "Write":
                files[a.get("file_path")] = a.get("content", "")
                cmdlog.append(f"[{sid}] WRITE {a.get('file_path')} ({len(a.get('content',''))} bytes)")
            elif fn == "Edit":
                p = a.get("file_path"); old = a.get("old_string", ""); new = a.get("new_string", ""); ok = "ok"
                if p in files:
                    if a.get("replace_all"): files[p] = files[p].replace(old, new)
                    elif old in files[p]: files[p] = files[p].replace(old, new, 1)
                    else: ok = "MISS"
                else: ok = "NOFILE"
                if "error" in obs.lower()[:200] or "not found" in obs.lower()[:300]: ok += "/toolerr"
                cmdlog.append(f"[{sid}] EDIT {p} ({len(old)}->{len(new)}) {ok}")
            elif fn == "Bash":
                c = a.get("command", ""); one = " ".join(c.split())[:220]
                cmdlog.append(f"[{sid}] BASH {one}")
                if re.search(r"(cat\s*>|tee\s|sed\s+-i|python3?\s|>\s*/app/|cp\s.*/app/|mv\s.*/app/|git checkout|patch\s|perl\s+-)", c):
                    bash_writes.append((sid, c[:4000]))
                if re.search(r"cmake|clang|make\b|roundel_demo|/tmp/t/|\./build|asan|a\.out", c):
                    builds.append(f"[{sid}] CMD: {one}\n--- obs tail ---\n{obs[-1200:]}\n")
            else:
                cmdlog.append(f"[{sid}] {fn} {json.dumps(a)[:150]}")
    open(os.path.join(od, "commands.txt"), "w").write("\n".join(cmdlog))
    open(os.path.join(od, "prose.txt"), "w").write("\n\n".join(prose))
    open(os.path.join(od, "build_runs.txt"), "w").write("\n".join(builds))
    open(os.path.join(od, "bash_writes.txt"), "w").write("\n\n".join(f"[{i}]\n{c}" for i, c in bash_writes))
    fd = os.path.join(od, "python_replayed_files"); os.makedirs(fd, exist_ok=True)
    for p, c in files.items():
        if p: open(os.path.join(fd, p.strip("/").replace("/", "__")), "w").write(c)
    tc = json.load(open(os.path.join(d, "terminal-commit.json")))
    summary[slot] = dict(attempt=att, steps=len(steps), tools=dict(hist), files=sorted(k for k in files if k),
                         bash_writes=len(bash_writes), build_runs=len(builds), term=tc.get("termination_reason"),
                         full_credit=tc.get("full_credit"), turns=tc.get("assistant_message_count"))
    print(f"slot{slot}: steps={len(steps)} turns={tc.get('assistant_message_count')} full_credit={tc.get('full_credit')} term={tc.get('termination_reason')} tools={dict(hist)} bash_writes={len(bash_writes)} build_runs={len(builds)}")
    print(f"   files: {sorted(k for k in files if k)}")
json.dump(summary, open(os.path.join(OUT, "summary.json"), "w"), indent=1)
