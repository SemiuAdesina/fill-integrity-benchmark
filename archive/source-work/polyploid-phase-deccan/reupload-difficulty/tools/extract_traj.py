#!/usr/bin/env python3
"""Author-side trajectory extraction. Replays Write/Edit/MultiEdit tool calls to
reconstruct the final candidate-authored files, dumps command history, assistant
prose, and diagnostics outcomes per attempt."""
import json, os, re, sys, collections
ROOT = os.path.expanduser("~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty/traj/all-trajectories1/arms/hy4_difficulty/res-4c99e01823be51bfabd69d08d577c3d7")
OUT = os.path.expanduser("~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty/analysis")
attempts = sorted(os.listdir(os.path.join(ROOT, "attempts")))
summary = {}
for att in attempts:
    slot = att.split("-slot-")[1][:2]
    d = os.path.join(ROOT, "attempts", att)
    t = json.load(open(os.path.join(d, "trajectory.json")))
    steps = t["steps"]
    od = os.path.join(OUT, f"slot{slot}")
    os.makedirs(od, exist_ok=True)
    files = {}          # path -> content (replayed)
    bash_writes = []    # bash commands that touch src
    hist = collections.Counter()
    cmdlog, prose, diag, cargo_runs = [], [], [], []
    for s in steps:
        if s.get("source") != "agent":
            continue
        msg = s.get("message") or ""
        if msg.strip():
            prose.append(f"[step {s['step_id']}] {msg.strip()}")
        results = {r.get("source_call_id"): r.get("content", "") for r in (s.get("observation") or {}).get("results", [])}
        for tc in s.get("tool_calls") or []:
            fn = tc.get("function_name"); a = tc.get("arguments") or {}
            hist[fn] += 1
            obs = results.get(tc.get("tool_call_id"), "") or ""
            if fn == "Write":
                files[a.get("file_path")] = a.get("content", "")
                cmdlog.append(f"[{s['step_id']}] WRITE {a.get('file_path')} ({len(a.get('content',''))} bytes)")
            elif fn == "Edit":
                p = a.get("file_path"); old = a.get("old_string", ""); new = a.get("new_string", "")
                ok = "ok"
                if p in files:
                    if a.get("replace_all"):
                        files[p] = files[p].replace(old, new)
                    elif old in files[p]:
                        files[p] = files[p].replace(old, new, 1)
                    else:
                        ok = "MISS"
                else:
                    ok = "NOFILE"
                if "error" in obs.lower()[:200] or "not found" in obs.lower()[:300]:
                    ok += "/toolerr"
                cmdlog.append(f"[{s['step_id']}] EDIT {p} ({len(old)}->{len(new)}) {ok}")
            elif fn == "MultiEdit":
                p = a.get("file_path")
                for e in a.get("edits", []):
                    if p in files and e.get("old_string","") in files[p]:
                        files[p] = files[p].replace(e["old_string"], e.get("new_string",""), 1)
                cmdlog.append(f"[{s['step_id']}] MULTIEDIT {p} ({len(a.get('edits',[]))} edits)")
            elif fn == "Bash":
                c = a.get("command", "")
                one = " ".join(c.split())[:220]
                cmdlog.append(f"[{s['step_id']}] BASH {one}")
                if re.search(r"(cat\s*>|tee\s|sed\s+-i|python3?\s.*(open\(|write)|>\s*/app/src|>\s*src/|cp\s.*src/|mv\s.*src/|git checkout|patch\s)", c):
                    bash_writes.append((s["step_id"], c[:3000]))
                if "cargo" in c:
                    tail = obs[-1500:]
                    cargo_runs.append(f"[{s['step_id']}] CMD: {one}\n--- obs tail ---\n{tail}\n")
                if "solver_checks" in c or "diagnostics" in c:
                    m = re.findall(r"test result: (\w+)\. (\d+) passed; (\d+) failed", obs)
                    diag.append((s["step_id"], one[:120], m))
            else:
                cmdlog.append(f"[{s['step_id']}] {fn} {json.dumps(a)[:150]}")
    # dump
    with open(os.path.join(od, "commands.txt"), "w") as f: f.write("\n".join(cmdlog))
    with open(os.path.join(od, "prose.txt"), "w") as f: f.write("\n\n".join(prose))
    with open(os.path.join(od, "cargo_runs.txt"), "w") as f: f.write("\n".join(cargo_runs))
    with open(os.path.join(od, "bash_writes.txt"), "w") as f:
        f.write("\n\n".join(f"[{i}]\n{c}" for i, c in bash_writes))
    fd = os.path.join(od, "final_files"); os.makedirs(fd, exist_ok=True)
    for p, c in files.items():
        if p and p.startswith("/app/"):
            q = os.path.join(fd, p[len("/app/"):].replace("/", "__"))
            with open(q, "w") as f: f.write(c)
    last_diag = diag[-1] if diag else None
    summary[slot] = dict(attempt=att, steps=len(steps), tools=dict(hist), files=sorted(files), bash_writes=len(bash_writes),
                         diag_runs=len(diag), last_diag=last_diag,
                         term=json.load(open(os.path.join(d,"terminal-commit.json"))).get("termination_reason"),
                         reward=json.load(open(os.path.join(d,"terminal-commit.json"))).get("reward"))
    print(f"slot{slot}: steps={len(steps)} tools={dict(hist)} bash_writes={len(bash_writes)} diag_runs={len(diag)} last_diag={last_diag}")
    print(f"   files: {sorted(files)}")
json.dump(summary, open(os.path.join(OUT, "summary.json"), "w"), indent=1)
