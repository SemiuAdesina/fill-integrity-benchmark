#!/usr/bin/env python3
"""Author-side trajectory extraction for attributable-fill-detection: command history,
assistant prose, cargo runs with observation tails, bash write-like commands, Agent
sub-calls, python-replayed files (Write/Edit only) and a per-attempt summary."""
import json, os, re, sys, collections
ROOT = os.path.expanduser("~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2/export/arms/hy4_difficulty/res-6e11ed06f0a9553b95f53ac9bfcdc9b7")
OUT = os.path.expanduser("~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2/analysis")
summary = {}
for att in sorted(os.listdir(os.path.join(ROOT, "attempts"))):
    slot = att.split("-slot-")[1][:2]
    d = os.path.join(ROOT, "attempts", att)
    steps = json.load(open(os.path.join(d, "trajectory.json")))["steps"]
    od = os.path.join(OUT, f"slot{slot}"); os.makedirs(od, exist_ok=True)
    files = {}; hist = collections.Counter(); cmdlog, prose, cargo, bash_writes, agents, reads = [], [], [], [], [], []
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
                cmdlog.append(f"[{sid}] WRITE {a.get('file_path')} ({len(a.get('content',''))} bytes) obs={obs[:60]!r}")
            elif fn in ("Edit", "MultiEdit"):
                p = a.get("file_path"); edits = [(a.get("old_string",""), a.get("new_string",""), bool(a.get("replace_all")))] if fn == "Edit" else [(e.get("old_string",""), e.get("new_string",""), bool(e.get("replace_all"))) for e in a.get("edits", [])]
                ok = "ok"
                for old, new, all_ in edits:
                    if p in files:
                        if all_: files[p] = files[p].replace(old, new)
                        elif old in files[p]: files[p] = files[p].replace(old, new, 1)
                        else: ok = "MISS"
                    else: ok = "NOFILE"
                cmdlog.append(f"[{sid}] {fn.upper()} {p} ({sum(len(o) for o,_,_ in edits)}->{sum(len(n) for _,n,_ in edits)}) {ok} obs={obs[:60]!r}")
            elif fn == "Bash":
                c = a.get("command", ""); one = " ".join(c.split())[:220]
                cmdlog.append(f"[{sid}] BASH {one}")
                if re.search(r"(cat\s*>|tee\s|sed\s+-i|python3?\s|>\s*/app/|cp\s.*/app/|mv\s.*/app/|git checkout|patch\s|perl\s+-|awk\s.*>\s)", c):
                    bash_writes.append((sid, c[:6000]))
                if re.search(r"cargo|reproduce|rustc", c):
                    cargo.append(f"[{sid}] CMD: {one}\n--- obs tail ---\n{obs[-1500:]}\n")
            elif fn == "Read":
                reads.append((sid, a.get("file_path"), obs))
                cmdlog.append(f"[{sid}] READ {a.get('file_path')} off={a.get('offset')} lim={a.get('limit')}")
            elif fn == "Agent":
                agents.append(f"[{sid}] AGENT prompt:\n{a.get('prompt','')[:3000]}\n--- result ---\n{obs[:3000]}\n")
                cmdlog.append(f"[{sid}] AGENT {a.get('description','')!r}")
            else:
                cmdlog.append(f"[{sid}] {fn} {json.dumps(a)[:150]}")
    open(os.path.join(od, "commands.txt"), "w").write("\n".join(cmdlog))
    open(os.path.join(od, "prose.txt"), "w").write("\n\n".join(prose))
    open(os.path.join(od, "cargo_runs.txt"), "w").write("\n".join(cargo))
    open(os.path.join(od, "bash_writes.txt"), "w").write("\n\n".join(f"[{i}]\n{c}" for i, c in bash_writes))
    open(os.path.join(od, "agents.txt"), "w").write("\n\n".join(agents))
    # last full Read of each /app/src file (ground truth for reconstruction checks)
    rd = os.path.join(od, "reads"); os.makedirs(rd, exist_ok=True)
    for sid, p, obs in reads:
        if p and p.startswith("/app/src/"):
            open(os.path.join(rd, f"{p.strip('/').replace('/', '__')}.step{sid}.txt"), "w").write(obs)
    fd = os.path.join(od, "python_replayed_files"); os.makedirs(fd, exist_ok=True)
    for p, c in files.items():
        if p: open(os.path.join(fd, p.strip("/").replace("/", "__")), "w").write(c)
    tc = json.load(open(os.path.join(d, "terminal-commit.json")))
    summary[slot] = dict(attempt=att, steps=len(steps), tools=dict(hist), files=sorted(k for k in files if k),
                         bash_writes=len(bash_writes), cargo_runs=len(cargo), agents=len(agents), term=tc.get("termination_reason"),
                         reward=tc.get("reward"), turns=tc.get("assistant_message_count"))
    print(f"slot{slot}: steps={len(steps)} turns={tc.get('assistant_message_count')} reward={tc.get('reward')} term={tc.get('termination_reason')} tools={dict(hist)} bash_writes={len(bash_writes)} cargo_runs={len(cargo)} agents={len(agents)}")
    print(f"   files: {sorted(k for k in files if k)}")
json.dump(summary, open(os.path.join(OUT, "summary.json"), "w"), indent=1)
