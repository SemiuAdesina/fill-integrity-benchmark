#!/usr/bin/env python3
import subprocess, sys
secs = float(sys.argv[1]); cmd = sys.argv[2:]
try:
    r = subprocess.run(cmd, timeout=secs, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    sys.exit(r.returncode)
except subprocess.TimeoutExpired:
    sys.exit(124)
