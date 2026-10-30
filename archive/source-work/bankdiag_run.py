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

report("D1_word_truncation",
       g(8, 4, 1, [(2, 0)], [0]),
       {"stages": [], "rot": 0},
       [sp([0, 4, 8, 12, 16], [0, 1, 2, 3, 4])])

report("D2_ordered_stages",
       g(8, 1, 2, [(0, 1), (1, 2)], [0]),
       {"stages": [(1, 2), (0, 1)], "rot": 0},
       [sp([0, 1, 2, 3], [0, 1, 2, 3])])

report("D3_lane_rotation",
       g(8, 4, 1, [(2, 0)], [0, 3]),
       {"stages": [], "rot": 3},
       [sp([0, 4, 8, 12], [0, 1, 2, 3])])

report("D4_broadcast_leader",
       g(8, 4, 1, [(2, 0)], [0, 1]),
       {"stages": [], "rot": 1},
       [sp([8, 8, 8, 12], [3, 1, 2, 0])])

report("D8_stage_count_tier",
       g(16, 4, 1, [(3, 0)], [0, 6]),
       {"stages": [], "rot": 6},
       [sp([0, 4, 8, 12], [0, 1, 2, 3])])

report("D9_canonical_tie",
       g(16, 4, 1, [(3, 0)], list(range(16))),
       {"stages": [], "rot": 10},
       [sp([0, 4, 8, 12], [0, 1, 2, 3], reveal=[False] * 4, conflict=True)])
