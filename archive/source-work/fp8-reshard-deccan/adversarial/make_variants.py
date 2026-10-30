#!/usr/bin/env python3
"""Adversarial population (slots 03-08), partial-fix battery and development
snapshots, each a candidate `restore.cpp` derived from the reference by one
targeted, plausible change. Output: adversarial/variants/<name>/restore.cpp
and adversarial/snapshots/<name>/restore.cpp."""
import os, shutil, sys
HERE = os.path.dirname(os.path.abspath(__file__))
REF = os.path.join(HERE, "..", "pkg", "solution", "fixed", "src", "restore.cpp")
STARTER = os.path.join(HERE, "..", "pkg", "environment", "src", "restore.cpp")

ROTATE = "for (std::size_t i = 0; i < L; ++i) out.push_back(r.amax_history[(static_cast<std::size_t>(r.cursor) + i) % L]);"
SUFFIX_BODY = "        if (shorter[i] != longer[n - m + i]) return false;"
EMPTY = '        if (r.amax_history.empty()) return Result::reject("empty history");'
CURSOR = '        if (r.cursor >= static_cast<std::int64_t>(r.amax_history.size())) return Result::reject("invalid cursor");'
BY_IDENT = "for (const Record& r : checkpoint.records) by_ident[Ident{r.group(), r.role, r.block()}].push_back(&r);"
SCALE = '            if (r->scale != recs[0]->scale || r->inverse_scale != recs[0]->inverse_scale) return Result::reject("scale disagreement");'
SUFFIX_LOOP = '                if (chron[i].size() <= chron[j].size() && !suffix_equal(chron[i], chron[j])) return Result::reject("suffix disagreement");'
KEY_A = "const auto key_a = std::make_tuple(-static_cast<std::int64_t>(chron[i].size()), a.replica, a.stage, a.shard, a.tensor);"
KEY_B = "const auto key_b = std::make_tuple(-static_cast<std::int64_t>(chron[best].size()), b.replica, b.stage, b.shard, b.tensor);"
ZERO_LEN = '        if (t.history_length == 0) return Result::reject("zero target history length");'
SAME_TR = "            if (a.tensor != b.tensor || a.role != b.role) continue;"
PLACEMENT = '            if (a.stage == b.stage && a.shard == b.shard) return Result::reject("duplicate placement");'
OVERLAP = '            if (a.logical_begin < b.logical_end && b.logical_begin < a.logical_end) return Result::reject("overlapping targets");'
INTERVALS = "for (const Record& r : checkpoint.records) intervals[{r.group(), r.role}].push_back({r.logical_begin, r.logical_end});"
COV = '        if (pos < t.logical_end) return Result::reject("incomplete coverage");'
INACTIVE = "        if (!t.active) continue;"
LOOKUP_INT = "auto it = intervals.find({t.group(), t.role});"
LOOKUP_MERGED = "auto m = merged.find(Ident{t.group(), t.role, k});"
BLOCKS = "for (std::int64_t k = t.first_block(); k <= t.last_block(); ++k) {"
RESIZE = """            if (L <= n) st.amax_history.assign(s.begin() + static_cast<std::ptrdiff_t>(n - L), s.end());
            else {
                st.amax_history.assign(static_cast<std::size_t>(L - n), 0);
                st.amax_history.insert(st.amax_history.end(), s.begin(), s.end());
            }"""
CURSOR_OUT = "            st.cursor = 0;"
SCALE_COPY = "            st.scale = m->second.scale;\n            st.inverse_scale = m->second.inverse_scale;"
SORT = "    std::sort(states.begin(), states.end(), state_before);"

V = {}
def v(name, family, desc, patches): V[name] = (family, desc, patches)

# ---------------------------------------------------------------- population
v("p03_rotation_from_newest", "03 cursor convention", "treats the cursor as the index of the newest sample and rotates from the slot after it",
  [(ROTATE, "for (std::size_t i = 0; i < L; ++i) out.push_back(r.amax_history[(static_cast<std::size_t>(r.cursor) + 1 + i) % L]);")])
v("p04_physical_identity", "04 physical identity", "keys blocks by stage and shard as well as by logical identity",
  [(BY_IDENT, "for (const Record& r : checkpoint.records) by_ident[Ident{r.group() + \"@\" + std::to_string(r.stage) + \":\" + std::to_string(r.shard), r.role, r.block()}].push_back(&r);"),
   (INTERVALS, "for (const Record& r : checkpoint.records) intervals[{r.group() + \"@\" + std::to_string(r.stage) + \":\" + std::to_string(r.shard), r.role}].push_back({r.logical_begin, r.logical_end});"),
   (LOOKUP_INT, "auto it = intervals.find({t.group() + \"@\" + std::to_string(t.stage) + \":\" + std::to_string(t.shard), t.role});"),
   (LOOKUP_MERGED, "auto m = merged.find(Ident{t.group() + \"@\" + std::to_string(t.stage) + \":\" + std::to_string(t.shard), t.role, k});")])
v("p05_full_ring_equality", "05 consistency", "requires records of a block to have identical histories, not identical newest suffixes",
  [("    const std::size_t m = shorter.size(), n = longer.size();", "    const std::size_t m = shorter.size(), n = longer.size();\n    if (m != n) return false;")])
v("p06_identity_ignores_tie_group", "06 tie groups", "identifies blocks by tensor name even inside a tie group",
  [(BY_IDENT, BY_IDENT.replace("r.group()", "r.tensor")), (INTERVALS, INTERVALS.replace("r.group()", "r.tensor")),
   (LOOKUP_INT, LOOKUP_INT.replace("t.group()", "t.tensor")), (LOOKUP_MERGED, LOOKUP_MERGED.replace("t.group()", "t.tensor"))])
v("p07_resize_keeps_oldest", "07 resizing", "a shorter target keeps the oldest samples and a longer one pads zeros on the new side",
  [(RESIZE, """            if (L <= n) st.amax_history.assign(s.begin(), s.begin() + static_cast<std::ptrdiff_t>(L));
            else {
                st.amax_history.assign(s.begin(), s.end());
                st.amax_history.resize(static_cast<std::size_t>(L), 0);
            }""")])
v("p08_block_level_coverage", "08 coverage", "treats a block as covered as soon as any record describes it",
  [(COV, "        (void)pos;")])

# ------------------------------------------------------------------ battery
v("b01_rotation_off_by_one_backward", "cursor", "rotates from the slot before the cursor",
  [(ROTATE, "for (std::size_t i = 0; i < L; ++i) out.push_back(r.amax_history[(static_cast<std::size_t>(r.cursor) + L - 1 + i) % L]);")])
v("b02_ring_taken_as_stored", "cursor", "uses the ring in storage order without rotating",
  [(ROTATE, "for (std::size_t i = 0; i < L; ++i) out.push_back(r.amax_history[i]);")])
v("b03_roles_merged", "identity", "forward and backward records share one state per block",
  [(BY_IDENT, BY_IDENT.replace("r.role", "Role::Forward")), (INTERVALS, INTERVALS.replace("r.role", "Role::Forward")),
   (LOOKUP_INT, LOOKUP_INT.replace("t.role", "Role::Forward")), (LOOKUP_MERGED, LOOKUP_MERGED.replace("t.role", "Role::Forward"))])
v("b04_shortest_history_chosen", "selection", "keeps the shortest compatible history",
  [(KEY_A, KEY_A.replace("-static_cast", "static_cast")), (KEY_B, KEY_B.replace("-static_cast", "static_cast"))])
v("b05_tiebreak_tensor_before_replica", "selection", "breaks ties by tensor name before replica (no observable effect by construction)",
  [(KEY_A, "const auto key_a = std::make_tuple(-static_cast<std::int64_t>(chron[i].size()), a.tensor, a.replica, a.stage, a.shard);"),
   (KEY_B, "const auto key_b = std::make_tuple(-static_cast<std::int64_t>(chron[best].size()), b.tensor, b.replica, b.stage, b.shard);")])
v("b06_scale_check_skipped", "consistency", "never compares the scale pairs of a block's records",
  [(SCALE, "            (void)r;")])
v("b07_cursor_equal_to_length_allowed", "validation", "accepts a cursor equal to the ring length",
  [(CURSOR, CURSOR.replace(">=", ">"))])
v("b08_suffix_compares_oldest", "consistency", "compares the oldest samples instead of the newest",
  [(SUFFIX_BODY, "        if (shorter[i] != longer[i]) return false;")])
v("b09_inactive_targets_emitted", "targets", "writes states for inactive targets too",
  [(INACTIVE, "        (void)t.active;")])
v("b10_inactive_targets_need_coverage", "targets", "requires coverage for inactive targets as well",
  [(INACTIVE, "        const bool emit = t.active;"), (BLOCKS, "if (!emit) continue;\n        " + BLOCKS)])
v("b11_overlap_not_detected", "targets", "detects duplicate placements but not overlapping intervals",
  [(OVERLAP, "            (void)b;")])
v("b12_duplicate_ignores_role", "targets", "treats a forward and a backward target on one placement as duplicates",
  [(SAME_TR, "            if (a.tensor != b.tensor) continue;")])
v("b13_zero_length_only_checked_when_active", "targets", "ignores a zero history length on an inactive target",
  [(ZERO_LEN, '        if (t.active && t.history_length == 0) return Result::reject("zero target history length");')])
v("b14_last_block_end_inclusive", "blocks", "computes the last block from logical_end instead of logical_end - 1",
  [(BLOCKS, "for (std::int64_t k = t.first_block(); k <= t.logical_end / t.block_size; ++k) {")])
v("b15_order_backward_first", "ordering", "orders backward states before forward ones",
  [(SORT, "    std::sort(states.begin(), states.end(), [](const State& a, const State& b) { if (a.tensor != b.tensor) return a.tensor < b.tensor; if (a.role != b.role) return a.role == Role::Backward; return std::make_tuple(a.stage, a.shard, a.global_block) < std::make_tuple(b.stage, b.shard, b.global_block); });")])
v("b16_stage_compared_as_string", "ordering", "orders stages and shards as decimal strings",
  [(SORT, "    std::sort(states.begin(), states.end(), [](const State& a, const State& b) { if (a.tensor != b.tensor) return a.tensor < b.tensor; if (a.role != b.role) return a.role == Role::Forward; return std::make_tuple(std::to_string(a.stage), std::to_string(a.shard), a.global_block) < std::make_tuple(std::to_string(b.stage), std::to_string(b.shard), b.global_block); });")])
v("b17_cursor_after_padding", "resizing", "sets the cursor to the first real sample after zero padding",
  [(CURSOR_OUT, "            st.cursor = (L > n) ? (L - n) : 0;")])
v("b18_scale_recomputed_from_history", "scale", "recomputes the scale pair from the resized history with the model's rule",
  [(SCALE_COPY, "            { std::int64_t peak = 1; for (auto x : (L <= n ? std::vector<std::int64_t>(s.begin() + static_cast<std::ptrdiff_t>(n - L), s.end()) : s)) peak = std::max(peak, x); std::int64_t inv = 1; while (inv < peak) inv *= 2; st.inverse_scale = inv; st.scale = (std::int64_t{1} << 24) / inv; }")])
v("b19_pad_with_oldest_sample", "resizing", "pads a longer target by repeating the oldest sample",
  [(RESIZE, RESIZE.replace("assign(static_cast<std::size_t>(L - n), 0)", "assign(static_cast<std::size_t>(L - n), s.front())"))])
v("b20_unsorted_output", "ordering", "emits states in layout order",
  [(SORT, "    (void)state_before;")])
v("b21_partial_states_on_gap", "coverage", "skips uncovered targets instead of rejecting",
  [(COV, "        if (pos < t.logical_end) continue;")])
v("b22_record_validation_skipped", "validation", "never validates rings or cursors",
  [(EMPTY, "        (void)r;"), (CURSOR, "")])
v("b23_touching_intervals_rejected", "targets", "treats intervals that touch as overlapping",
  [(OVERLAP, '            if (a.logical_begin <= b.logical_end && b.logical_begin <= a.logical_end) return Result::reject("overlapping targets");')])
v("b24_suffix_only_between_equal_lengths", "consistency", "compares histories only when they have the same length",
  [(SUFFIX_LOOP, '                if (chron[i].size() == chron[j].size() && !suffix_equal(chron[i], chron[j])) return Result::reject("suffix disagreement");')])
v("b25_coverage_ignores_tie_partner_records", "coverage", "coverage counts only records of the target's own tensor",
  [(INTERVALS, "for (const Record& r : checkpoint.records) intervals[{r.tensor, r.role}].push_back({r.logical_begin, r.logical_end});"),
   (LOOKUP_INT, "auto it = intervals.find({t.tensor, t.role});")])

# ---------------------------------------------------------------- snapshots
NO_ROT = V["b02_ring_taken_as_stored"][2]
NO_CONS = [(SCALE, "            (void)r;"), (SUFFIX_LOOP, "                (void)j;"), (KEY_A, "const auto key_a = std::make_tuple(0, a.replica, a.stage, a.shard, a.tensor); (void)i;"), (KEY_B, "const auto key_b = std::make_tuple(0, b.replica, b.stage, b.shard, b.tensor);")]
NO_COV = V["p08_block_level_coverage"][2]
NO_TARGETS = [(ZERO_LEN, ""), (PLACEMENT, ""), (OVERLAP, "            (void)b;")]
NO_RECORDS = V["b22_record_validation_skipped"][2]
NAIVE_RESIZE = V["p07_resize_keeps_oldest"][2]
NO_SORT = V["b20_unsorted_output"][2]
SNAPSHOTS = {
    "S1_parse_and_emit": NO_ROT + NO_CONS + NO_COV + NO_TARGETS + NO_RECORDS + NAIVE_RESIZE + NO_SORT,
    "S2_rotation": NO_CONS + NO_COV + NO_TARGETS + NO_RECORDS + NAIVE_RESIZE + NO_SORT,
    "S3_consistency_and_selection": NO_COV + NO_TARGETS + NO_RECORDS + NAIVE_RESIZE + NO_SORT,
    "S4_record_and_target_validation": NO_COV + NAIVE_RESIZE + NO_SORT,
    "S5_index_coverage": NAIVE_RESIZE + NO_SORT,
    "S6_resizing": NO_SORT,
    "S7_final": [],
}

def apply(text, patches, name):
    for old, new in patches:
        if text.count(old) != 1:
            raise SystemExit("%s: pattern not unique (%d): %s" % (name, text.count(old), old[:60]))
        text = text.replace(old, new)
    return text

def main():
    ref = open(REF).read()
    out = os.path.join(HERE, "variants"); shutil.rmtree(out, ignore_errors=True)
    for name, (family, desc, patches) in V.items():
        d = os.path.join(out, name); os.makedirs(d)
        open(os.path.join(d, "restore.cpp"), "w").write(apply(ref, patches, name))
        open(os.path.join(d, "VARIANT.txt"), "w").write("%s\n%s\n%s\n" % (name, family, desc))
    snaps = os.path.join(HERE, "snapshots"); shutil.rmtree(snaps, ignore_errors=True)
    for name, patches in SNAPSHOTS.items():
        d = os.path.join(snaps, name); os.makedirs(d)
        open(os.path.join(d, "restore.cpp"), "w").write(apply(ref, patches, name))
    d = os.path.join(snaps, "S0_starter"); os.makedirs(d); shutil.copy(STARTER, os.path.join(d, "restore.cpp"))
    print("variants:", len(V), "snapshots:", len(SNAPSHOTS) + 1)

if __name__ == "__main__":
    main()
