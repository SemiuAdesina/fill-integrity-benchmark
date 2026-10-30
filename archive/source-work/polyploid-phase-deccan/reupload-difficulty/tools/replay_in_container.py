#!/usr/bin/env python3
"""High-fidelity author-side replay: re-executes every Write/Edit/MultiEdit and Bash
tool call of one Stage 2 attempt inside a fresh task container (cargo/rustc shimmed
to no-ops so only file-system effects are reproduced), then copies /app/src out."""
import json, os, subprocess, sys, re, time
ROOT = os.path.expanduser(sys.argv[3]) if len(sys.argv) > 3 else os.path.expanduser("~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty/traj/all-trajectories1/arms/hy4_difficulty/res-4c99e01823be51bfabd69d08d577c3d7")
OUT = os.path.expanduser(sys.argv[4]) if len(sys.argv) > 4 else os.path.expanduser("~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty/analysis")
IMAGE = sys.argv[2] if len(sys.argv) > 2 else "pc-agent:reup-base"
slot = sys.argv[1]
att = [a for a in os.listdir(os.path.join(ROOT, "attempts")) if f"-slot-{slot}-" in a][0]
steps = json.load(open(os.path.join(ROOT, "attempts", att, "trajectory.json")))["steps"]
name = f"replay{slot}_" + os.path.basename(OUT)
subprocess.run(["docker", "rm", "-f", name], capture_output=True)
subprocess.run(["docker", "run", "-d", "--name", name, "--network", "none", "-w", "/app", IMAGE, "sleep", "infinity"], check=True, capture_output=True)
SHIM = r'''set -e
for b in cargo rustc rustup; do
  p=/usr/local/cargo/bin/$b; if [ -e "$p" ]; then mv "$p" "$p.real"; printf '#!/bin/sh\nexit 0\n' > "$p"; chmod +x "$p"; fi
done
which cargo || true; echo shimmed'''
subprocess.run(["docker", "exec", name, "bash", "-c", SHIM], check=True, capture_output=True)
def _read(path):
    r = subprocess.run(["docker", "exec", name, "cat", path], capture_output=True, timeout=60)
    return None if r.returncode != 0 else r.stdout.decode(errors="replace")
def _write(path, content):
    r = subprocess.run(["docker", "exec", "-i", name, "sh", "-c", 'mkdir -p "$(dirname "$1")" && cat > "$1"', "sh", path], input=content.encode(), capture_output=True, timeout=60)
    return "ok" if r.returncode == 0 else "FAIL " + r.stderr.decode(errors="replace")[:100]
os.makedirs(os.path.join(OUT, f"slot{slot}"), exist_ok=True)
log = open(os.path.join(OUT, f"slot{slot}", "replay.log"), "w")
def apply(payload):
    p = payload["path"]
    if payload["op"] == "write":
        return "W " + _write(p, payload["content"])
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
n = 0
for s in steps:
    if s.get("source") != "agent": continue
    results = {r.get("source_call_id"): r.get("content", "") for r in (s.get("observation") or {}).get("results", [])}
    for tc in s.get("tool_calls") or []:
        fn = tc.get("function_name"); a = tc.get("arguments") or {}
        obs = results.get(tc.get("tool_call_id"), "") or ""
        sid = s["step_id"]
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
            # wall-clock waits have no filesystem effect; shorten them so the replay is not gated on them
            cmd = re.sub(r"\bsleep\s+\d+(\.\d+)?", "sleep 0", cmd)
            t0 = time.time()
            try:
                r = subprocess.run(["docker", "exec", "-w", "/app", name, "bash", "-c", cmd], capture_output=True, timeout=30)
                rc = r.returncode; tail = (r.stdout + r.stderr)[-160:].decode(errors="replace").replace("\n", " ")
            except subprocess.TimeoutExpired:
                rc = "TIMEOUT"; tail = ""
                subprocess.run(["docker", "exec", name, "bash", "-c", "pkill -f python3; pkill -f phase; true"], capture_output=True)
            log.write(f"[{sid}] BASH rc={rc} {time.time()-t0:.1f}s :: {' '.join(cmd.split())[:100]} :: {tail}\n")
        n += 1
    log.flush()
dst = os.path.join(OUT, f"slot{slot}", "replay_app")
subprocess.run(["rm", "-rf", dst]); os.makedirs(dst)
for p in ["src", "Cargo.toml", "tests"]:
    subprocess.run(["docker", "cp", f"{name}:/app/{p}", dst], capture_output=True)
subprocess.run(["docker", "rm", "-f", name], capture_output=True)
print(f"slot{slot}: replayed {n} tool calls; src files:")
for dp, dn, fns in os.walk(os.path.join(dst, "src")):
    for f in fns: print("   ", os.path.relpath(os.path.join(dp, f), dst), os.path.getsize(os.path.join(dp, f)))
