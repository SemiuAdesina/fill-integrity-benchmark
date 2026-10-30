#!/usr/bin/env bash
# Timing measurements for the A6 report: starter cold build, public diagnostics,
# reference reproducer, verifier wall time.
set -uo pipefail

BASE="/Users/macbok/Downloads/benchmark-suite-solvability-repair/fill-v8-work/build/attributable-fill-detection"
WORK="/tmp/fillv8-measure"
export PATH="${HOME}/.cargo/bin:${PATH}"
export VERIFIER_CARGO="${HOME}/.cargo/bin/cargo"

rm -rf "${WORK}"
mkdir -p "${WORK}/app"
cp -R "${BASE}/environment/Cargo.toml" "${BASE}/environment/Cargo.lock" \
      "${BASE}/environment/src" "${BASE}/environment/tests" "${WORK}/app/"

cd "${WORK}/app" || exit 1

echo "== starter: cargo build --offline --all-targets (cold, as the Dockerfile runs it)"
/usr/bin/time -p cargo build --offline --all-targets 2>&1 | tail -5

echo "== starter: cargo test --offline --test diagnostics"
/usr/bin/time -p cargo test --offline --test diagnostics 2>&1 | tail -8

echo "== starter: cargo run --offline --bin reproduce"
/usr/bin/time -p cargo run --offline --bin reproduce 2>&1 | tail -8

for rel in src/types/snapshot.rs src/engine/fill_classifier.rs \
           src/engine/neutralization.rs src/engine/cycle.rs src/engine/mod.rs; do
    cp "${BASE}/solution/fixed/${rel}" "${WORK}/app/${rel}"
done

echo "== reference: cargo build --offline --all-targets (incremental)"
/usr/bin/time -p cargo build --offline --all-targets 2>&1 | tail -5

echo "== reference: cargo test --offline --test diagnostics"
/usr/bin/time -p cargo test --offline --test diagnostics 2>&1 | tail -8

echo "== reference: cargo run --offline --bin reproduce"
/usr/bin/time -p cargo run --offline --bin reproduce 2>&1 | tail -6

echo "== verifier over the reference tree"
rm -rf "${WORK}/logs"
mkdir -p "${WORK}/logs"
/usr/bin/time -p env APP_DIR="${WORK}/app" REWARD_DIR="${WORK}/logs" \
    bash "${BASE}/tests/test.sh" 2>&1 | tail -8
echo "reward: $(cat "${WORK}/logs/reward.txt")"
