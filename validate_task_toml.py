#!/usr/bin/env python3
"""Harbor 0.13.2 task.toml schema validator."""

import sys
from pathlib import Path

try:
    from harbor.models.task.config import TaskConfig
except ImportError:
    print("Harbor not installed. Install with: pip install harbor==0.13.2", file=sys.stderr)
    sys.exit(1)

if len(sys.argv) != 2:
    print(f"usage: {sys.argv[0]} <task.toml>", file=sys.stderr)
    sys.exit(1)

task_toml_path = Path(sys.argv[1])
if not task_toml_path.exists():
    print(f"error: {task_toml_path} not found", file=sys.stderr)
    sys.exit(1)

try:
    config = TaskConfig.model_validate_toml(task_toml_path.read_text())
    print(f"✓ {task_toml_path.name} is valid under Harbor 0.13.2 schema")
    print(f"  verifier.environment_mode = {config.verifier.environment_mode}")
    print(f"  verifier.timeout_sec = {config.verifier.timeout_sec}")
    print(f"  agent.timeout_sec = {config.agent.timeout_sec}")
    print(f"  metadata.category = {config.metadata.get('category')}")
except Exception as exc:
    print(f"✗ {task_toml_path.name} validation failed:", file=sys.stderr)
    print(f"  {exc}", file=sys.stderr)
    sys.exit(1)
