#!/usr/bin/env python3
"""High-fidelity author-side replay of one attempt inside a fresh task container.
Re-executes every Write/Edit and Bash tool call (compilers shimmed to no-ops so
only file-system effects are reproduced), snapshots the two deliverable files after
every tool call, and copies the final bytes out.
usage: replay_in_container.py <slot> [image] [export_root] [out_root]"""
import json, os, subprocess, sys, re, time, hashlib
slot = sys.argv[1]
IMAGE = sys.argv[2] if len(sys.argv) > 2 else "rc-agent:reup-base"
ROOT = os.path.expanduser(sys.argv[3]) if len(sys.argv) > 3 else os.path.expanduser("~/benchmark-suite-work/coroutine-frame-reuse-deccan/reupload-stage2/export/arms/hy4_difficulty/res-14043ce34a2950e7840ebc5af992fd04")
OUT = os.path.expanduser(sys.argv[4]) if len(sys.argv) > 4 else os.path.expanduser("~/benchmark-suite-work/coroutine-frame-reuse-deccan/reupload-stage2/replay")
DELIV = {"/app/include/roundel/runtime.hpp": "runtime.hpp", "/app/src/runtime.cpp": "runtime.cpp"}
att = [a for a in os.listdir(os.path.join(ROOT, "attempts")) if f"-slot-{slot}-" in a][0]
steps = json.load(open(os.path.join(ROOT, "attempts", att, "trajectory.json")))["steps"]
name = f"cfr_replay{slot}"
subprocess.run(["docker", "rm", "-f", name], capture_output=True)
subprocess.run(["docker", "run", "-d", "--name", name, "--network", "none", "-w", "/app", IMAGE, "sleep", "infinity"], check=True, capture_output=True)
SHIM = r'''set -e
mkdir -p /opt/shim
for b in cmake make ninja clang clang++ g++ gcc c++ cc ctest; do printf '#!/bin/sh\nexit 0\n' > /opt/shim/$b; chmod +x /opt/shim/$b; done
echo shimmed'''
subprocess.run(["docker", "exec", name, "bash", "-c", SHIM], check=True, capture_output=True)
ENV = ["env", "PATH=/opt/shim:/usr/local/bin:/usr/bin:/bin"]
def _read(path):
    r = subprocess.run(["docker", "exec", name, "cat", path], capture_output=True, timeout=60)
    return None if r.returncode != 0 else r.stdout.decode(errors="replace")
def _write(path, content):
    r = subprocess.run(["docker", "exec", "-i", name, "sh", "-c", 'mkdir -p "$(dirname "$1")" && cat > "$1"', "sh", path], input=content.encode(), capture_output=True, timeout=60)
    return "ok" if r.returncode == 0 else "FAIL " + r.stderr.decode(errors="replace")[:100]
od = os.path.join(OUT, f"slot{slot}"); snapdir = os.path.join(od, "snapshots"); os.makedirs(snapdir, exist_ok=True)
log = open(os.path.join(od, "replay.log"), "w")
def apply(payload):
    p = payload["path"]
    if payload["op"] == "write": return "W " + _write(p, payload["content"])
    cur = _read(p)
    if cur is None: return "E nofile"
    notes = []
    for old, new, all_ in payload["edits"]:
        if old not in cur: notes.append("miss:" + repr(old[:40])); continue
        cur = cur.replace(old, new) if all_ else cur.replace(old, new, 1)
    return "E " + _write(p, cur) + (" " + ";".join(notes) if notes else "")
def bad_obs(obs):
    head = (obs or "")[:400].lower()
    return any(k in head for k in ("<tool_use_error>", "error:", "has not been read", "not found in file", "found multiple", "string to replace not found", "no changes to make"))
last_digest = None; snaps = []
def snapshot(sid, n):
    global last_digest
    contents = {}
    for p, short in DELIV.items():
        contents[short] = _read(p) or ""
    dig = hashlib.sha256((contents["runtime.hpp"] + "\0" + contents["runtime.cpp"]).encode()).hexdigest()[:12]
    if dig != last_digest:
        sd = os.path.join(snapdir, f"{n:03d}_step{sid}_{dig}"); os.makedirs(sd, exist_ok=True)
        for short, c in contents.items(): open(os.path.join(sd, short), "w").write(c)
        snaps.append((n, sid, dig)); last_digest = dig
        log.write(f"    SNAPSHOT {n:03d} step{sid} {dig}\n")
n = 0
snapshot("start", n)
for s in steps:
    if s.get("source") != "agent": continue
    results = {r.get("source_call_id"): r.get("content", "") for r in (s.get("observation") or {}).get("results", [])}
    for tc in s.get("tool_calls") or []:
        fn = tc.get("function_name"); a = tc.get("arguments") or {}
        obs = results.get(tc.get("tool_call_id"), "") or ""; sid = s["step_id"]; n += 1
        if fn == "Write":
            if bad_obs(obs): log.write(f"[{sid}] WRITE SKIP (tool error) {a.get('file_path')} :: {obs[:120]!r}\n"); continue
            log.write(f"[{sid}] WRITE {a.get('file_path')} -> {apply(dict(op='write', path=a['file_path'], content=a.get('content','')))}\n")
        elif fn == "Edit":
            if bad_obs(obs): log.write(f"[{sid}] EDIT SKIP (tool error) {a.get('file_path')} :: {obs[:120]!r}\n"); continue
            log.write(f"[{sid}] EDIT {a.get('file_path')} -> {apply(dict(op='edit', path=a['file_path'], edits=[[a.get('old_string',''), a.get('new_string',''), bool(a.get('replace_all'))]]))}\n")
        elif fn == "MultiEdit":
            if bad_obs(obs): log.write(f"[{sid}] MULTIEDIT SKIP\n"); continue
            log.write(f"[{sid}] MULTIEDIT {a.get('file_path')} -> {apply(dict(op='edit', path=a['file_path'], edits=[[e.get('old_string',''), e.get('new_string',''), bool(e.get('replace_all'))] for e in a.get('edits',[])]))}\n")
        elif fn == "Bash":
            cmd = a.get("command", "")
            cmd = re.sub(r"\bsleep\s+\d+(\.\d+)?", "sleep 0", cmd)
            t0 = time.time()
            try:
                r = subprocess.run(["docker", "exec", "-w", "/app", name] + ENV + ["bash", "-c", cmd], capture_output=True, timeout=60)
                rc = r.returncode; tail = (r.stdout + r.stderr)[-160:].decode(errors="replace").replace("\n", " ")
            except subprocess.TimeoutExpired:
                rc = "TIMEOUT"; tail = ""
                subprocess.run(["docker", "exec", name, "bash", "-c", "pkill -f python3; pkill -f roundel; true"], capture_output=True)
            log.write(f"[{sid}] BASH rc={rc} {time.time()-t0:.1f}s :: {' '.join(cmd.split())[:100]} :: {tail}\n")
        else:
            continue
        snapshot(sid, n)
    log.flush()
fin = os.path.join(od, "final"); os.makedirs(fin, exist_ok=True)
for p, short in DELIV.items():
    c = _read(p); open(os.path.join(fin, short), "w").write(c or "")
subprocess.run(["docker", "rm", "-f", name], capture_output=True)
json.dump(snaps, open(os.path.join(od, "snapshots.json"), "w"))
print(f"slot{slot}: replayed {n} tool calls; {len(snaps)} distinct deliverable snapshots; final sizes:", {s: os.path.getsize(os.path.join(fin, s)) for s in DELIV.values()})
