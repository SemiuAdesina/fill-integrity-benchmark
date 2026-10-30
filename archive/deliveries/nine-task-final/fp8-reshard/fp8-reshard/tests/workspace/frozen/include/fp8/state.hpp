#pragma once
#include <cstdint>
#include <string>
#include <vector>
namespace fp8 { enum class Role{Forward,Backward};struct SourceState{std::string tensor;std::string tie;Role role{};unsigned replica{};unsigned shard{};unsigned stage{};std::int64_t scale_q{};std::int64_t inverse_scale_q{};std::vector<std::int64_t> history;unsigned cursor{};};struct Target{std::string tensor;std::string tie;Role role{};unsigned shard{};unsigned stage{};unsigned history_length{};bool active{true};};struct Restored{Target target;std::int64_t scale_q{};std::int64_t inverse_scale_q{};std::vector<std::int64_t> history;unsigned cursor{};};struct Step{Restored state;std::int64_t quantized{};};std::vector<std::int64_t> chronological(const SourceState&);Step cpu_step(const Restored&,std::int64_t,std::int64_t);std::string bytes(const std::vector<Restored>&);std::vector<Restored> restore(const std::vector<SourceState>&,const std::vector<Target>&);}
