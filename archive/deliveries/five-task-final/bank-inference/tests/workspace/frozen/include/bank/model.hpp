#pragma once
#include <cstdint>
#include <string>
#include <vector>
namespace bank {
struct XorStage { unsigned source_bit{}; unsigned destination_bit{}; friend bool operator==(const XorStage&, const XorStage&) = default; };
struct Grammar { unsigned bank_count{}; unsigned word_width{}; unsigned max_stages{}; std::vector<XorStage> allowed_stages; std::vector<unsigned> allowed_lane_rotations; };
struct Mapping { std::vector<XorStage> stages; unsigned lane_rotation{}; };
struct Trace { std::vector<std::uint64_t> byte_addresses; std::vector<unsigned> lanes; std::vector<int> observed_banks; int observed_conflict{-1}; };
struct Replay { std::vector<unsigned> banks; unsigned conflict_degree{}; };
Replay replay(const Grammar&, const Mapping&, const Trace&);
std::string canonical(const Mapping&);
Mapping recover(const Grammar&, const std::vector<Trace>&);
}
