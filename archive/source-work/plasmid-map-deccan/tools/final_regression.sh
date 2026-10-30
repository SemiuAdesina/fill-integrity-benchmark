#!/usr/bin/env bash
# Full regression through the real verifier: sync, images, oracle, NOP, then the
# population, battery, snapshots and security controls in parallel streams.
set -uo pipefail
W=~/benchmark-suite-work/plasmid-map-deccan; L=$W/lab/final; mkdir -p $L; rm -f $L/ALLDONE
bash $W/tools/sync_pkg.sh > $L/sync.txt 2>&1
bash $W/tools/run_control.sh $W/pkg oracle base > $L/oracle.txt 2>&1
RC_SKIP_BUILD=1 RC_AGENT_TAG=base RC_VER_TAG=base bash $W/tools/run_control.sh $W/pkg nop base > $L/nop.txt 2>&1
( bash $W/tools/run_population.sh $L/population.txt $W/adversarial/solver_enum/src $W/adversarial/variants/p01_bisection_only $W/adversarial/variants/p02_no_tangent_bound $W/adversarial/variants/p05_interval_propagation_only > /dev/null 2>&1; bash $W/tools/run_population.sh $L/snapshots.txt $W/adversarial/snapshots/S* > /dev/null 2>&1 ) &
( bash $W/tools/run_population.sh $L/battery.txt $W/adversarial/variants/b* $W/adversarial/variants/p03_no_order_branching $W/adversarial/variants/p04_no_visibility_windows > /dev/null 2>&1 ) &
( bash $W/security/run_security.sh > /dev/null 2>&1 ) &
wait
echo ALLDONE > $L/ALLDONE
