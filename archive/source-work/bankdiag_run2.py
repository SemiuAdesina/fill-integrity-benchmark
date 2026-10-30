import sys
sys.path.insert(0, "/Users/macbok/benchmark-suite-work")
from bankdiag import *

def g(bc, ww, ms, st, rots):
    return {"bank_count": bc, "word_width": ww, "max_stages": ms,
            "allowed_stages": st, "allowed_rot": rots}

def sp(addr, lanes, reveal=None, conflict=False):
    return {"addr": addr, "lanes": lanes,
            "reveal": reveal if reveal is not None else [True] * len(addr),
            "conflict": conflict}

report("D5_conflict_distinct_addresses",
       g(4, 4, 0, [], [0, 1, 2, 3]),
       {"stages": [], "rot": 0},
       [sp([8, 8, 12], [0, 1, 2], reveal=[False] * 3, conflict=True)])

report("D6_global_evidence",
       g(8, 4, 2, [(2, 0), (0, 1)], [0, 1]),
       {"stages": [(0, 1)], "rot": 1},
       [sp([0, 4], [0, 0], reveal=[False, False], conflict=True),
        sp([8, 12], [1, 2])],
       single_trace=True)

report("D7_minimum_operation_count",
       g(8, 4, 1, [(2, 0)], [3, 0]),
       {"stages": [], "rot": 3},
       [sp([0, 4], [0, 0], reveal=[False, False], conflict=True)])

report("D8_stage_count_tier",
       g(8, 4, 1, [(0, 2)], [0, 1, 2, 3, 4, 5, 6, 7]),
       {"stages": [], "rot": 1},
       [sp([0, 4], [0, 1], reveal=[False, False], conflict=True)])
