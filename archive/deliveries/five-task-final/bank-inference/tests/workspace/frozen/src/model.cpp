#include "bank/model.hpp"
#include <algorithm>
#include <map>
#include <set>
#include <sstream>
#include <stdexcept>
namespace bank {
namespace {
void validate(const Grammar& g,const Mapping& m){if(g.bank_count<2||(g.bank_count&(g.bank_count-1))||!g.word_width||m.stages.size()>g.max_stages)throw std::invalid_argument("invalid");if(std::find(g.allowed_lane_rotations.begin(),g.allowed_lane_rotations.end(),m.lane_rotation)==g.allowed_lane_rotations.end())throw std::invalid_argument("rotation");std::set<std::pair<unsigned,unsigned>> seen;for(auto s:m.stages)if(std::find(g.allowed_stages.begin(),g.allowed_stages.end(),s)==g.allowed_stages.end()||!seen.emplace(s.source_bit,s.destination_bit).second)throw std::invalid_argument("stage");}
}
Replay replay(const Grammar& g,const Mapping& m,const Trace& t){validate(g,m);const auto n=t.byte_addresses.size();if(!n||t.lanes.size()!=n||(!t.observed_banks.empty()&&t.observed_banks.size()!=n))throw std::invalid_argument("trace");Replay r;r.banks.resize(n);unsigned mask=g.bank_count-1;for(size_t i=0;i<n;++i){uint64_t word=t.byte_addresses[i]/g.word_width;for(auto s:m.stages){uint64_t bit=(word>>s.source_bit)&1U;word^=bit<<s.destination_bit;}unsigned lane=(t.lanes[i]+m.lane_rotation)%g.bank_count;r.banks[i]=static_cast<unsigned>(word^lane)&mask;}std::map<uint64_t,std::vector<size_t>> groups;for(size_t i=0;i<n;++i)groups[t.byte_addresses[i]].push_back(i);for(auto& [a,v]:groups){(void)a;auto leader=*std::min_element(v.begin(),v.end(),[&](size_t x,size_t y){return t.lanes[x]<t.lanes[y];});for(auto i:v)r.banks[i]=r.banks[leader];}std::vector<std::set<uint64_t>> distinct(g.bank_count);for(size_t i=0;i<n;++i)distinct[r.banks[i]].insert(t.byte_addresses[i]);r.conflict_degree=1;for(auto& s:distinct)r.conflict_degree=std::max(r.conflict_degree,static_cast<unsigned>(s.size()));return r;}
std::string canonical(const Mapping& m){std::ostringstream o;o<<"rotate="<<m.lane_rotation<<";stages=";for(size_t i=0;i<m.stages.size();++i){if(i)o<<',';o<<m.stages[i].source_bit<<'>'<<m.stages[i].destination_bit;}return o.str();}
}
