#!/usr/bin/env bash
# Sequential final regression on the current pkg/: rebuild images, Oracle, NOP, population, battery, security, snapshots.
set -uo pipefail
W=~/benchmark-suite-work/relocation-infer-deccan; L=$W/lab/final; mkdir -p $L
while ! grep -q DONE $W/lab/snapshots.txt 2>/dev/null; do sleep 15; done
bash $W/tools/sync_pkg.sh > $L/sync.txt 2>&1
bash $W/tools/run_control.sh $W/pkg oracle base > $L/oracle.txt 2>&1
RC_SKIP_BUILD=1 RC_AGENT_TAG=base RC_VER_TAG=base bash $W/tools/run_control.sh $W/pkg nop base > $L/nop.txt 2>&1
bash $W/tools/run_population.sh $L/population.txt $W/adversarial/solver_b $W/adversarial/variants/p0* > /dev/null 2>&1
bash $W/tools/run_population.sh $L/battery.txt $W/adversarial/variants/b* > /dev/null 2>&1
bash $W/security/run_security.sh > /dev/null 2>&1; cp $W/lab/security.txt $L/security.txt
bash $W/tools/run_population.sh $L/snapshots.txt $W/adversarial/snapshots/S* > /dev/null 2>&1
echo ALLDONE > $L/ALLDONE
