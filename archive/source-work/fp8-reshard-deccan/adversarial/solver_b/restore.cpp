// Restoration — independent architecture B (index-first, newest-first rings).
//
// Differs from the reference in organisation: targets are validated before
// records; coverage is a per-(group, role) boolean index map; rings are read
// newest-first (so the suffix rule is a prefix comparison and resizing is a
// truncation or a right-padding of the newest-first sequence); candidates per
// block are sorted rather than scanned; states are produced per target in
// layout order and sorted once at the end.
#include "fp8/model.hpp"

#include <algorithm>
#include <map>
#include <set>
#include <tuple>
#include <vector>

namespace fp8 {

namespace {

std::vector<std::int64_t> newest_first(const Record& r) {
    const std::int64_t L = static_cast<std::int64_t>(r.amax_history.size());
    std::vector<std::int64_t> out;
    for (std::int64_t i = 0; i < L; ++i) out.push_back(r.amax_history[static_cast<std::size_t>(((r.cursor - 1 - i) % L + L) % L)]);
    return out;
}

}  // namespace

Result restore(const Checkpoint& ck, const Layout& ly) {
    // targets first: zero lengths, duplicate placements, overlaps
    for (const Target& t : ly.targets)
        if (t.history_length == 0) return Result::reject("zero target history length");
    std::set<std::tuple<std::string, Role, std::int64_t, std::int64_t>> placements;
    for (const Target& t : ly.targets)
        if (!placements.insert({t.tensor, t.role, t.stage, t.shard}).second) return Result::reject("duplicate placement");
    std::map<std::pair<std::string, Role>, std::vector<std::pair<std::int64_t, std::int64_t>>> spans;
    for (const Target& t : ly.targets) spans[{t.tensor, t.role}].push_back({t.logical_begin, t.logical_end});
    for (auto& [k, v] : spans) {
        std::sort(v.begin(), v.end());
        for (std::size_t i = 1; i < v.size(); ++i)
            if (v[i].first < v[i - 1].second) return Result::reject("overlapping targets");
    }
    // records: validity, per-block candidates newest-first
    struct Cand { std::vector<std::int64_t> nf; std::int64_t scale, inv, replica, stage, shard; std::string tensor; };
    std::map<std::tuple<std::string, Role, std::int64_t>, std::vector<Cand>> cands;
    std::map<std::pair<std::string, Role>, std::vector<char>> cover;
    for (const Record& r : ck.records) {
        if (r.amax_history.empty() || r.cursor >= static_cast<std::int64_t>(r.amax_history.size())) return Result::reject("invalid record");
        cands[{r.group(), r.role, r.block()}].push_back(Cand{newest_first(r), r.scale, r.inverse_scale, r.replica, r.stage, r.shard, r.tensor});
        std::vector<char>& m = cover[{r.group(), r.role}];
        if (static_cast<std::int64_t>(m.size()) < r.logical_end) m.resize(static_cast<std::size_t>(r.logical_end), 0);
        for (std::int64_t i = r.logical_begin; i < r.logical_end; ++i) m[static_cast<std::size_t>(i)] = 1;
    }
    std::map<std::tuple<std::string, Role, std::int64_t>, Cand> chosen;
    for (auto& [ident, cs] : cands) {
        for (const Cand& c : cs)
            if (c.scale != cs[0].scale || c.inv != cs[0].inv) return Result::reject("scale disagreement");
        for (const Cand& a : cs)
            for (const Cand& b : cs) {
                const std::size_t n = std::min(a.nf.size(), b.nf.size());
                if (!std::equal(a.nf.begin(), a.nf.begin() + static_cast<std::ptrdiff_t>(n), b.nf.begin())) return Result::reject("suffix disagreement");
            }
        std::sort(cs.begin(), cs.end(), [](const Cand& a, const Cand& b) {
            return std::make_tuple(-static_cast<std::int64_t>(a.nf.size()), a.replica, a.stage, a.shard, a.tensor) <
                   std::make_tuple(-static_cast<std::int64_t>(b.nf.size()), b.replica, b.stage, b.shard, b.tensor);
        });
        chosen.emplace(ident, cs.front());
    }
    std::vector<State> states;
    for (const Target& t : ly.targets) {
        if (!t.active) continue;
        auto cm = cover.find({t.group(), t.role});
        for (std::int64_t i = t.logical_begin; i < t.logical_end; ++i)
            if (cm == cover.end() || static_cast<std::int64_t>(cm->second.size()) <= i || !cm->second[static_cast<std::size_t>(i)]) return Result::reject("incomplete coverage");
        for (std::int64_t k = t.first_block(); k <= t.last_block(); ++k) {
            const Cand& c = chosen.at({t.group(), t.role, k});
            std::vector<std::int64_t> kept(c.nf.begin(), c.nf.begin() + static_cast<std::ptrdiff_t>(std::min<std::size_t>(c.nf.size(), static_cast<std::size_t>(t.history_length))));
            while (static_cast<std::int64_t>(kept.size()) < t.history_length) kept.push_back(0);
            std::reverse(kept.begin(), kept.end());
            states.push_back(State{t.tensor, t.role, t.stage, t.shard, k, c.scale, c.inv, kept, 0});
        }
    }
    std::stable_sort(states.begin(), states.end(), state_before);
    return Result::ok(std::move(states));
}

}  // namespace fp8
