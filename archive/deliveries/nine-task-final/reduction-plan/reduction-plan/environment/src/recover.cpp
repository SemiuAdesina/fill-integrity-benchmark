#include "fabric.hpp"
#include <algorithm>
#include <limits>
#include <set>
#include <tuple>

namespace benchmark-suite_fabric {
namespace {

struct Payload {
  std::int64_t ready{}, bytes{}, value{};
  std::vector<std::string> origins;
};

bool payload_less(const Payload &a, const Payload &b) {
  return std::tie(a.ready, a.origins) < std::tie(b.ready, b.origins);
}

bool checked_add(std::int64_t a, std::int64_t b, std::int64_t &out) {
  const auto value = static_cast<__int128>(a) + static_cast<__int128>(b);
  if (value < std::numeric_limits<std::int64_t>::min() ||
      value > std::numeric_limits<std::int64_t>::max())
    return false;
  out = static_cast<std::int64_t>(value);
  return true;
}

bool valid_problem(const Problem &p) {
  if (p.sink.empty() || p.sources.empty() || p.saturation_min > p.saturation_max) return false;
  std::set<std::string> link_ids;
  std::set<std::string> source_ids;
  for (const Link &link : p.links) {
    if (link.id.empty() || link.from.empty() || link.to.empty() || link.bytes_per_tick <= 0 ||
        link.latency < 0 || !link_ids.insert(link.id).second)
      return false;
  }
  for (const Source &source : p.sources) {
    if (source.id.empty() || source.node.empty() || source.bytes <= 0 ||
        !source_ids.insert(source.id).second)
      return false;
  }
  return true;
}

struct ReplayContext {
  const Problem &problem;
  std::map<std::string, Link> links;
  std::map<std::string, std::string> parent;
  std::map<std::string, std::vector<std::string>> children;
  std::map<std::string, std::vector<Source>> local_sources;
  std::set<std::string> combine;
  std::set<std::string> applied;
  std::map<std::string, std::int64_t> link_bytes;
  std::map<std::string, std::int64_t> accumulators;
  bool ok{true};

  explicit ReplayContext(const Problem &p) : problem(p) {}

  std::vector<Payload> compute(const std::string &node) {
    std::vector<Payload> payloads;
    if (const auto it = local_sources.find(node); it != local_sources.end()) {
      for (const Source &source : it->second)
        payloads.push_back(Payload{0, source.bytes, source.value, {source.id}});
    }

    std::vector<std::string> child_nodes = children[node];
    std::sort(child_nodes.begin(), child_nodes.end());
    for (const std::string &child : child_nodes) {
      std::vector<Payload> child_payloads = compute(child);
      if (!ok) return {};
      std::sort(child_payloads.begin(), child_payloads.end(), payload_less);
      const auto link_it = links.find(parent[child]);
      if (link_it == links.end()) {
        ok = false;
        return {};
      }
      const Link &link = link_it->second;
      std::int64_t free_tick = 0;
      for (Payload payload : child_payloads) {
        const std::int64_t sent_bytes = payload.bytes;
        const std::int64_t transfer =
            (payload.bytes + link.bytes_per_tick - 1) / link.bytes_per_tick;
        const std::int64_t start = std::max(payload.ready, free_tick);
        std::int64_t with_latency{}, finish{};
        if (!checked_add(start, link.latency, with_latency) ||
            !checked_add(with_latency, transfer, finish)) {
          ok = false;
          return {};
        }
        free_tick = finish;
        payload.ready = finish;
        payloads.push_back(std::move(payload));
        std::int64_t total{};
        if (!checked_add(link_bytes[link.id], sent_bytes, total)) {
          ok = false;
          return {};
        }
        link_bytes[link.id] = total;
      }
    }

    std::sort(payloads.begin(), payloads.end(), payload_less);
    if (!combine.contains(node)) return payloads;
    if (payloads.size() < 2) {
      ok = false;
      return {};
    }
    const std::int64_t bytes = payloads.front().bytes;
    if (std::any_of(payloads.begin(), payloads.end(),
                    [&](const Payload &payload) { return payload.bytes != bytes; })) {
      ok = false;
      return {};
    }
    const auto capacity = problem.accumulator_limits.find(node);
    if (capacity == problem.accumulator_limits.end() || bytes <= 0 ||
        payloads.size() >
            static_cast<std::size_t>(std::numeric_limits<std::int64_t>::max() / bytes)) {
      ok = false;
      return {};
    }
    const std::int64_t need = bytes * static_cast<std::int64_t>(payloads.size());
    if (need > capacity->second) {
      ok = false;
      return {};
    }

    Payload merged;
    merged.bytes = bytes;
    merged.value = payloads.front().value;
    for (std::size_t i = 0; i < payloads.size(); ++i) {
      merged.ready = std::max(merged.ready, payloads[i].ready);
      merged.origins.insert(merged.origins.end(), payloads[i].origins.begin(),
                            payloads[i].origins.end());
      if (i != 0)
        merged.value = saturating_add(merged.value, payloads[i].value,
                                      problem.saturation_min, problem.saturation_max);
    }
    std::sort(merged.origins.begin(), merged.origins.end());
    accumulators[node] = need;
    applied.insert(node);
    return {merged};
  }
};

}  // namespace

std::int64_t saturating_add(std::int64_t a, std::int64_t b, std::int64_t lo,
                            std::int64_t hi) {
  const auto value = static_cast<__int128>(a) + static_cast<__int128>(b);
  if (value < lo) return lo;
  if (value > hi) return hi;
  return static_cast<std::int64_t>(value);
}

std::optional<Plan> replay(const Problem &p,
                           const std::vector<std::vector<std::string>> &source_paths,
                           const std::vector<std::string> &combine_nodes) {
  if (!valid_problem(p) || source_paths.size() != p.sources.size()) return std::nullopt;
  ReplayContext context(p);
  for (const Link &link : p.links) context.links.emplace(link.id, link);

  std::vector<Source> sources = p.sources;
  std::sort(sources.begin(), sources.end(),
            [](const Source &a, const Source &b) { return a.id < b.id; });
  for (std::size_t i = 0; i < sources.size(); ++i) {
    std::string node = sources[i].node;
    context.local_sources[node].push_back(sources[i]);
    std::set<std::string> seen;
    for (const std::string &link_id : source_paths[i]) {
      const auto link = context.links.find(link_id);
      if (link == context.links.end() || link->second.from != node || !seen.insert(node).second)
        return std::nullopt;
      if (const auto old = context.parent.find(node);
          old != context.parent.end() && old->second != link_id)
        return std::nullopt;
      context.parent[node] = link_id;
      node = link->second.to;
    }
    if (node != p.sink) return std::nullopt;
  }

  for (const auto &[node, link_id] : context.parent)
    context.children[context.links[link_id].to].push_back(node);
  context.combine.insert(combine_nodes.begin(), combine_nodes.end());
  if (context.combine.size() != combine_nodes.size()) return std::nullopt;
  const std::set<std::string> allowed(p.reduction_nodes.begin(), p.reduction_nodes.end());
  for (const std::string &node : context.combine)
    if (!allowed.contains(node)) return std::nullopt;

  std::vector<Payload> payloads = context.compute(p.sink);
  if (!context.ok || context.applied != context.combine || payloads.empty()) return std::nullopt;
  std::sort(payloads.begin(), payloads.end(),
            [](const Payload &a, const Payload &b) { return a.origins < b.origins; });

  Plan plan;
  for (const Payload &payload : payloads) {
    plan.completion = std::max(plan.completion, payload.ready);
    plan.sink_values.push_back(payload.value);
  }
  for (const auto &entry : context.parent) plan.parents.push_back(entry);
  plan.combine_nodes.assign(context.combine.begin(), context.combine.end());
  for (const auto &entry : context.link_bytes) plan.link_bytes.push_back(entry);
  for (const auto &entry : context.accumulators) plan.accumulators.push_back(entry);
  for (const auto &[id, expected] : p.observed_link_bytes) {
    const auto it = context.link_bytes.find(id);
    const std::int64_t actual = it == context.link_bytes.end() ? 0 : it->second;
    if (actual != expected) return std::nullopt;
  }
  if (p.observed_completion && plan.completion != *p.observed_completion) return std::nullopt;
  return plan;
}

std::pair<std::optional<Plan>, Error> recover(const Problem &p) {
  // The deterministic replay mechanism is complete. Global route/tree/combine
  // inference and canonical optimization are the intended repair surface.
  if (!valid_problem(p)) return {std::nullopt, Error::InvalidProblem};
  return {std::nullopt, Error::NoPlan};
}

}  // namespace benchmark-suite_fabric
