#!/usr/bin/env bash
#
# Verifier entry point. Reward is owned here and starts at 0.
#
# Trust model: the graded candidate process is never asked whether it passed.
# It runs unprivileged, produces raw functional values only, and a separate
# judge binary that never links candidate code decides correctness by comparing
# those values against expectations it derives itself from the frozen model.
# Candidate stdout and stderr are captured for diagnostics and are never read
# by any grading decision.
set -uo pipefail

# ---------------------------------------------------------------- environment
# Nothing the candidate could have exported may influence the authoritative
# toolchain, its lookup paths, its wrappers or its output locations.
export PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
unset RUSTC RUSTDOC RUSTFLAGS RUSTDOCFLAGS RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
unset CARGO CARGO_HOME CARGO_TARGET_DIR CARGO_BUILD_TARGET CARGO_BUILD_TARGET_DIR
unset CARGO_ENCODED_RUSTFLAGS CARGO_MANIFEST_DIR CARGO_MANIFEST_PATH
unset CARGO_BUILD_RUSTFLAGS CARGO_BUILD_RUSTC_WRAPPER CARGO_WORKSPACE_DIR
unset RUSTUP_HOME RUSTUP_TOOLCHAIN RUSTUP_DIST_SERVER RUSTUP_UPDATE_ROOT
unset LD_PRELOAD LD_LIBRARY_PATH DYLD_INSERT_LIBRARIES DYLD_LIBRARY_PATH
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY BASH_ENV ENV CDPATH
while IFS='=' read -r key ignored; do
  case "$key" in CARGO_*|RUSTUP_*) unset "$key" 2>/dev/null;; esac
done < <(env)

LOGS=/logs/verifier
REWARD="$LOGS/reward.txt"
REPORT="$LOGS/report.json"
mkdir -p "$LOGS"
printf '0\n' > "$REWARD"
: > "$LOGS/run.log"
log(){ printf '%s\n' "$*" >> "$LOGS/run.log"; }

# Verifier-owned sources must not be readable by the account the graded process
# runs under, so the expected results cannot simply be read off the filesystem.
chmod -R go-rwx /tests 2>/dev/null

RUNUSER=/usr/sbin/runuser
GRADED_USER=nobody
[ -x "$RUNUSER" ] || { log 'FATAL missing runuser; refusing to run graded code with verifier privileges'; exit 2; }
id "$GRADED_USER" >/dev/null 2>&1 || { log 'FATAL missing unprivileged account'; exit 2; }
# Prove the drop actually takes effect before any candidate code is executed.
GRADED_UID=$(env -i "$RUNUSER" -u "$GRADED_USER" -- id -u 2>/dev/null)
[ -n "$GRADED_UID" ] && [ "$GRADED_UID" != 0 ] || { log 'FATAL privilege drop ineffective'; exit 2; }
log "graded process account=$GRADED_USER uid=$GRADED_UID"

# Verifier-private build tree (root, 0700) and a separate world-traversable
# sandbox that holds only what the graded process is allowed to see.
VERIFY=$(mktemp -d /tmp/bluetooth-verify.XXXXXXXX) || exit 2
SANDBOX=$(mktemp -d /tmp/bluetooth-sandbox.XXXXXXXX) || exit 2
trap 'rm -rf "$VERIFY" "$SANDBOX"' EXIT
chmod 700 "$VERIFY"
chmod 755 "$SANDBOX"

export HOME="$VERIFY/home"
export TMPDIR="$VERIFY/tmp"
export CARGO_HOME=/usr/local/cargo
export RUSTUP_HOME=/usr/local/rustup
export CARGO_TARGET_DIR="$VERIFY/target"
export CARGO_NET_OFFLINE=true
export CARGO_TERM_COLOR=never
export RUST_BACKTRACE=0
export RUSTC=/usr/local/cargo/bin/rustc
mkdir -p "$HOME" "$TMPDIR" "$CARGO_TARGET_DIR"
CARGO=/usr/local/cargo/bin/cargo
[ -x "$CARGO" ] && [ -x "$RUSTC" ] || { log 'FATAL missing trusted toolchain'; exit 2; }

# ------------------------------------------------------------ source intake
# Allowlist: exactly one candidate file is consumed, the implementation the
# published contract asks the agent to repair. No candidate manifest, build
# script, test harness, binary or configuration is read or executed.
ALLOWED=/app/src/lib.rs
[ -f "$ALLOWED" ] && [ ! -L "$ALLOWED" ] || { log 'FATAL allowlisted implementation missing or not a regular file'; exit 2; }
[ "$(realpath -e "$ALLOWED" 2>/dev/null)" = "$ALLOWED" ] || { log 'FATAL allowlisted implementation is not a canonical path'; exit 2; }
if find /app -type l -print -quit | grep -q .; then log 'FATAL symlink present under /app'; exit 2; fi

cp -r /tests/workspace "$VERIFY/ws" || exit 2
cp "$ALLOWED" "$VERIFY/ws/probe/src/candidate.rs" || exit 2
cmp -s "$ALLOWED" "$VERIFY/ws/probe/src/candidate.rs" || { log 'FATAL candidate intake did not take effect'; exit 2; }

# --------------------------------------------------------------------- build
# Built offline from the verifier's own locked manifests. The candidate cannot
# add dependencies, alter profiles or introduce a build script.
if ! "$CARGO" build --offline --locked --workspace --manifest-path "$VERIFY/ws/Cargo.toml" > "$LOGS/build.log" 2>&1; then
  log 'FAIL candidate implementation does not build against the frozen model'
  exit 2
fi
PROBE="$CARGO_TARGET_DIR/debug/probe"
LIAR="$CARGO_TARGET_DIR/debug/liar_probe"
JUDGE="$CARGO_TARGET_DIR/debug/judge"
for b in "$PROBE" "$LIAR" "$JUDGE"; do [ -x "$b" ] || { log "FATAL missing build artifact $b"; exit 2; }; done

# The judge stays inside the 0700 verifier tree. Only the probes are published
# into the sandbox, and they carry inputs but no expected results.
install -m 0755 "$PROBE" "$SANDBOX/probe"
install -m 0755 "$LIAR" "$SANDBOX/liar_probe"
mkdir -p "$SANDBOX/results" "$SANDBOX/liar-results"
chown "$GRADED_USER" "$SANDBOX/results" "$SANDBOX/liar-results"

CASES=(joint_global_recovery pbr_resolves_rtt_clock_ambiguity rtt_resolves_pbr_ambiguity
       bounded_grid_endpoints_included channel_input_order_is_ignored
       deterministic_empty_evidence_tie circular_error_drives_selection
       predictions_are_load_bearing tied_antenna_paths_remain_distinct
       compound_wrap_rtt_paths_and_order compound_joint_maxima_and_boundary_tie
       signed_half_open_wrap_boundaries
       exact_rtt_model_order negative_distance_rejected missing_path_rejected
       empty_bias_grid_rejected)
EXPECTED_CASES=16

# Run one probe case as the unprivileged account in a cleared environment. The
# exit status is recorded for diagnostics and deliberately not acted upon.
run_probe(){ # <binary> <results-dir> <case> <stdout-log>
  local rc=0
  env -i PATH=/usr/bin:/bin HOME=/nonexistent \
    "$RUNUSER" -u "$GRADED_USER" -- "$1" "$3" "$2/$3.rec" > "$4" 2>&1 || rc=$?
  log "probe $3 exit=$rc (status not used for grading)"
}

# ------------------------------------------------- required negative control
# Verifier-owned: the judge stages correct records, corrupts exactly one
# functional value and must observe that single failure. No candidate code is
# involved and no marker is exchanged.
control_ok=0
if "$JUDGE" --selftest "$VERIFY/control" >> "$LOGS/run.log" 2>&1; then
  control_ok=1
  log 'CONTROL ok'
else
  log 'CONTROL compromised'
fi

# --------------------------------------- stdout-authority regression (benign)
# A functionally incorrect implementation that announces success in every
# plausible form must still be graded as incorrect.
regression_ok=0
for c in "${CASES[@]}"; do run_probe "$SANDBOX/liar_probe" "$SANDBOX/liar-results" "$c" "$SANDBOX/liar.$c.out"; done
if "$JUDGE" --grade "$SANDBOX/liar-results" "$VERIFY/liar-verdict.txt" >> "$LOGS/run.log" 2>&1; then
  log 'REGRESSION compromised: success-looking output was treated as correctness'
else
  regression_ok=1
  log 'REGRESSION ok: emitted output carried no grading authority'
fi

# ------------------------------------------------------- graded candidate run
for c in "${CASES[@]}"; do run_probe "$SANDBOX/probe" "$SANDBOX/results" "$c" "$LOGS/candidate.$c.out"; done

grade_rc=0
"$JUDGE" --grade "$SANDBOX/results" "$VERIFY/verdict.txt" >> "$LOGS/run.log" 2>&1 || grade_rc=$?

TOTAL=0; FAILED=0
if [ -f "$VERIFY/verdict.txt" ]; then
  read -r TOTAL FAILED < <(awk '/^TOTAL /{print $2, $4}' "$VERIFY/verdict.txt")
fi
: "${TOTAL:=0}"; : "${FAILED:=0}"
PASSED=$((TOTAL-FAILED))

printf '{"reportFormat":"CTRF","results":{"tool":{"name":"bluetooth-range-verifier-owned"},"summary":{"tests":%s,"passed":%s,"failed":%s},"tests":[]}}\n' \
  "$TOTAL" "$PASSED" "$FAILED" > "$REPORT"

# Reward is awarded only when every verifier-owned functional check passed, the
# expected test population was actually evaluated, the negative control saw its
# injected failure, and the stdout-authority regression stayed at zero.
if [ "$grade_rc" -eq 0 ] && [ "$TOTAL" -eq "$EXPECTED_CASES" ] && [ "$FAILED" -eq 0 ] \
   && [ "$control_ok" -eq 1 ] && [ "$regression_ok" -eq 1 ]; then
  printf '1\n' > "$REWARD"
fi

log "verdict total=$TOTAL failed=$FAILED control=$control_ok regression=$regression_ok reward=$(cat "$REWARD")"
[ "$(cat "$REWARD")" = 1 ]
