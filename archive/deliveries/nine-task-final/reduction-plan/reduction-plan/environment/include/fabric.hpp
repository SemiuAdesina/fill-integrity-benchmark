#pragma once
#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <utility>
#include <vector>
namespace benchmark-suite_fabric {
struct Link { std::string id, from, to; std::int64_t bytes_per_tick{}, latency{}; auto operator<=>(const Link&) const = default; };
struct Source { std::string id, node; std::int64_t bytes{}, value{}; auto operator<=>(const Source&) const = default; };
struct Problem {
  std::string sink;
  std::vector<Link> links;
  std::vector<Source> sources;
  std::vector<std::string> reduction_nodes;
  std::map<std::string,std::int64_t> accumulator_limits;
  std::int64_t saturation_min{}, saturation_max{};
  std::map<std::string,std::int64_t> observed_link_bytes;
  std::optional<std::int64_t> observed_completion;
};
struct Plan {
  std::vector<std::pair<std::string,std::string>> parents;
  std::vector<std::string> combine_nodes;
  std::vector<std::pair<std::string,std::int64_t>> link_bytes;
  std::vector<std::pair<std::string,std::int64_t>> accumulators;
  std::vector<std::int64_t> sink_values;
  std::int64_t completion{};
  bool operator==(const Plan&) const = default;
};
enum class Error { InvalidProblem, NoPlan, Overflow };
std::int64_t saturating_add(std::int64_t a, std::int64_t b, std::int64_t lo, std::int64_t hi);
std::optional<Plan> replay(const Problem&, const std::vector<std::vector<std::string>>& source_paths, const std::vector<std::string>& combine_nodes);
std::pair<std::optional<Plan>,Error> recover(const Problem&);
}
