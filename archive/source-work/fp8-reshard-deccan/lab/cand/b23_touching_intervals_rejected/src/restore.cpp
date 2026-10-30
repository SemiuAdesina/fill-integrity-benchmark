// Restoration — reference implementation (architecture A: block table first).
//
// 1. validate every record (non-empty ring, cursor inside it);
// 2. group records by identity (tie group or tensor, role, block), rotate each
//    ring once from its cursor, check equal scale pairs and identical newest
//    suffixes, and keep the longest chronological history (lowest placement on
//    ties);
// 3. validate targets (duplicates, zero history length);
// 4. check index-level coverage of every active target against the merged
//    intervals of its identity group and role;
// 5. emit one state per active target block, resized (newest samples kept,
//    zeros padded on the old side), cursor 0, in the published order.
#include "fp8/model.hpp"

#include <algorithm>
#include <map>
#include <tuple>
#include <vector>

namespace fp8 {

namespace {

using Ident = std::tuple<std::string, Role, std::int64_t>;

struct Merged {
    std::int64_t scale{0};
    std::int64_t inverse_scale{0};
    std::vector<std::int64_t> chronological;  // oldest first
};

std::vector<std::int64_t> rotate(const Record& r) {
    std::vector<std::int64_t> out;
    const std::size_t L = r.amax_history.size();
    out.reserve(L);
    for (std::size_t i = 0; i < L; ++i) out.push_back(r.amax_history[(static_cast<std::size_t>(r.cursor) + i) % L]);
    return out;
}

bool suffix_equal(const std::vector<std::int64_t>& shorter, const std::vector<std::int64_t>& longer) {
    const std::size_t m = shorter.size(), n = longer.size();
    for (std::size_t i = 0; i < m; ++i)
        if (shorter[i] != longer[n - m + i]) return false;
    return true;
}

}  // namespace

Result restore(const Checkpoint& checkpoint, const Layout& layout) {
    // §2 every record
    for (const Record& r : checkpoint.records) {
        if (r.amax_history.empty()) return Result::reject("empty history");
        if (r.cursor >= static_cast<std::int64_t>(r.amax_history.size())) return Result::reject("invalid cursor");
    }
    // §4–§5 per identity
    std::map<Ident, std::vector<const Record*>> by_ident;
    for (const Record& r : checkpoint.records) by_ident[Ident{r.group(), r.role, r.block()}].push_back(&r);
    std::map<Ident, Merged> merged;
    for (const auto& [ident, recs] : by_ident) {
        std::vector<std::vector<std::int64_t>> chron;
        for (const Record* r : recs) chron.push_back(rotate(*r));
        for (const Record* r : recs)
            if (r->scale != recs[0]->scale || r->inverse_scale != recs[0]->inverse_scale) return Result::reject("scale disagreement");
        for (std::size_t i = 0; i < recs.size(); ++i)
            for (std::size_t j = 0; j < recs.size(); ++j)
                if (chron[i].size() <= chron[j].size() && !suffix_equal(chron[i], chron[j])) return Result::reject("suffix disagreement");
        std::size_t best = 0;
        for (std::size_t i = 1; i < recs.size(); ++i) {
            const Record& a = *recs[i];
            const Record& b = *recs[best];
            const auto key_a = std::make_tuple(-static_cast<std::int64_t>(chron[i].size()), a.replica, a.stage, a.shard, a.tensor);
            const auto key_b = std::make_tuple(-static_cast<std::int64_t>(chron[best].size()), b.replica, b.stage, b.shard, b.tensor);
            if (key_a < key_b) best = i;
        }
        merged[ident] = Merged{recs[0]->scale, recs[0]->inverse_scale, chron[best]};
    }
    // §6 targets
    const auto& targets = layout.targets;
    for (const Target& t : targets)
        if (t.history_length == 0) return Result::reject("zero target history length");
    for (std::size_t i = 0; i < targets.size(); ++i) {
        for (std::size_t j = i + 1; j < targets.size(); ++j) {
            const Target& a = targets[i];
            const Target& b = targets[j];
            if (a.tensor != b.tensor || a.role != b.role) continue;
            if (a.stage == b.stage && a.shard == b.shard) return Result::reject("duplicate placement");
            if (a.logical_begin <= b.logical_end && b.logical_begin <= a.logical_end) return Result::reject("overlapping targets");
        }
    }
    // §7 coverage: merged intervals per (group, role)
    std::map<std::pair<std::string, Role>, std::vector<std::pair<std::int64_t, std::int64_t>>> intervals;
    for (const Record& r : checkpoint.records) intervals[{r.group(), r.role}].push_back({r.logical_begin, r.logical_end});
    for (auto& [k, v] : intervals) std::sort(v.begin(), v.end());
    std::vector<State> states;
    for (const Target& t : targets) {
        if (!t.active) continue;
        auto it = intervals.find({t.group(), t.role});
        std::int64_t pos = t.logical_begin;
        if (it != intervals.end()) {
            for (const auto& [lo, hi] : it->second) {
                if (hi <= pos) continue;
                if (lo > pos) break;
                pos = std::max(pos, hi);
                if (pos >= t.logical_end) break;
            }
        }
        if (pos < t.logical_end) return Result::reject("incomplete coverage");
        // §8–§9
        for (std::int64_t k = t.first_block(); k <= t.last_block(); ++k) {
            auto m = merged.find(Ident{t.group(), t.role, k});
            if (m == merged.end()) return Result::reject("incomplete coverage");
            const std::vector<std::int64_t>& s = m->second.chronological;
            const std::int64_t L = t.history_length;
            const std::int64_t n = static_cast<std::int64_t>(s.size());
            State st;
            st.tensor = t.tensor;
            st.role = t.role;
            st.stage = t.stage;
            st.shard = t.shard;
            st.global_block = k;
            st.scale = m->second.scale;
            st.inverse_scale = m->second.inverse_scale;
            st.cursor = 0;
            if (L <= n) st.amax_history.assign(s.begin() + static_cast<std::ptrdiff_t>(n - L), s.end());
            else {
                st.amax_history.assign(static_cast<std::size_t>(L - n), 0);
                st.amax_history.insert(st.amax_history.end(), s.begin(), s.end());
            }
            states.push_back(std::move(st));
        }
    }
    std::sort(states.begin(), states.end(), state_before);
    return Result::ok(std::move(states));
}

}  // namespace fp8
