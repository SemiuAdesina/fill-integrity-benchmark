#include "fabric.hpp"
#include <algorithm>
#include <functional>
#include <limits>
#include <set>
#include <tuple>

namespace benchmark-suite_fabric {
namespace {
struct Payload {
  std::int64_t ready{}, bytes{}, value{};
  std::vector<std::string> origins;
};
bool payload_less(const Payload& a,const Payload& b){return std::tie(a.ready,a.origins)<std::tie(b.ready,b.origins);}

bool checked_add(std::int64_t a,std::int64_t b,std::int64_t& out){
  const auto v=static_cast<__int128>(a)+static_cast<__int128>(b);
  if(v<std::numeric_limits<std::int64_t>::min()||v>std::numeric_limits<std::int64_t>::max())return false;
  out=static_cast<std::int64_t>(v); return true;
}

struct Context {
  const Problem& p;
  std::map<std::string,Link> links;
  std::map<std::string,std::string> parent;
  std::map<std::string,std::vector<std::string>> children;
  std::map<std::string,std::vector<Source>> local_sources;
  std::set<std::string> combine;
  std::set<std::string> applied;
  std::map<std::string,std::int64_t> link_bytes;
  std::map<std::string,std::int64_t> accumulators;
  bool ok{true};

  explicit Context(const Problem& problem):p(problem){}

  std::vector<Payload> compute(const std::string& node){
    std::vector<Payload> payloads;
    if(const auto it=local_sources.find(node);it!=local_sources.end()){
      for(const auto& s:it->second)payloads.push_back(Payload{0,s.bytes,s.value,{s.id}});
    }
    auto kids=children[node]; std::sort(kids.begin(),kids.end());
    for(const auto& child:kids){
      auto child_payloads=compute(child); if(!ok)return {};
      std::sort(child_payloads.begin(),child_payloads.end(),payload_less);
      const auto lit=links.find(parent[child]); if(lit==links.end()){ok=false;return{};}
      const auto& link=lit->second; std::int64_t free_tick=0;
      for(auto payload:child_payloads){
        const auto sent_bytes=payload.bytes;
        const auto transfer=(payload.bytes+link.bytes_per_tick-1)/link.bytes_per_tick;
        const auto start=std::max(payload.ready,free_tick);
        std::int64_t finish{},with_latency{};
        if(!checked_add(start,link.latency,with_latency)||!checked_add(with_latency,transfer,finish)){ok=false;return{};}
        free_tick=finish; payload.ready=finish; payloads.push_back(std::move(payload));
        std::int64_t total{}; if(!checked_add(link_bytes[link.id],sent_bytes,total)){ok=false;return{};} link_bytes[link.id]=total;
      }
    }
    std::sort(payloads.begin(),payloads.end(),payload_less);
    if(combine.contains(node)){
      if(payloads.size()<2){ok=false;return{};}
      const auto bytes=payloads.front().bytes;
      if(std::any_of(payloads.begin(),payloads.end(),[&](const Payload& x){return x.bytes!=bytes;})){ok=false;return{};}
      const auto cap=p.accumulator_limits.find(node);
      if(cap==p.accumulator_limits.end()){ok=false;return{};}
      if(bytes<=0||payloads.size()>static_cast<std::size_t>(std::numeric_limits<std::int64_t>::max()/bytes)){ok=false;return{};}
      const auto need=bytes*static_cast<std::int64_t>(payloads.size());
      if(need>cap->second){ok=false;return{};}
      Payload merged; merged.bytes=bytes; merged.ready=0; merged.value=payloads.front().value;
      for(std::size_t i=0;i<payloads.size();++i){
        merged.ready=std::max(merged.ready,payloads[i].ready);
        merged.origins.insert(merged.origins.end(),payloads[i].origins.begin(),payloads[i].origins.end());
        if(i)merged.value=saturating_add(merged.value,payloads[i].value,p.saturation_min,p.saturation_max);
      }
      std::sort(merged.origins.begin(),merged.origins.end());
      accumulators[node]=need; applied.insert(node);
      return {merged};
    }
    return payloads;
  }
};

bool better(const Plan& a,const Plan& b){
  const auto asum=[](const auto& xs){std::int64_t s=0;for(const auto& x:xs)s+=x.second;return s;};
  return std::tuple{a.completion,asum(a.accumulators),asum(a.link_bytes),a.parents,a.combine_nodes}
       < std::tuple{b.completion,asum(b.accumulators),asum(b.link_bytes),b.parents,b.combine_nodes};
}

bool valid_problem(const Problem& p){
  if(p.sink.empty()||p.sources.empty()||p.saturation_min>p.saturation_max)return false;
  std::set<std::string> link_ids,source_ids;
  for(const auto& l:p.links)if(l.id.empty()||l.from.empty()||l.to.empty()||l.bytes_per_tick<=0||l.latency<0||!link_ids.insert(l.id).second)return false;
  for(const auto& s:p.sources)if(s.id.empty()||s.node.empty()||s.bytes<=0||!source_ids.insert(s.id).second)return false;
  return true;
}

void paths_from(const std::string& node,const Problem& p,const std::map<std::string,std::vector<Link>>& outgoing,
                std::set<std::string>& visited,std::vector<std::string>& path,std::vector<std::vector<std::string>>& out){
  if(node==p.sink){out.push_back(path);return;}
  if(!visited.insert(node).second)return;
  if(const auto it=outgoing.find(node);it!=outgoing.end())for(const auto& link:it->second){
    path.push_back(link.id); paths_from(link.to,p,outgoing,visited,path,out); path.pop_back();
  }
  visited.erase(node);
}
}

std::int64_t saturating_add(std::int64_t a,std::int64_t b,std::int64_t lo,std::int64_t hi){
  const auto v=static_cast<__int128>(a)+static_cast<__int128>(b);
  if(v<lo)return lo; if(v>hi)return hi; return static_cast<std::int64_t>(v);
}

std::optional<Plan> replay(const Problem& p,const std::vector<std::vector<std::string>>& source_paths,const std::vector<std::string>& combine_nodes){
  if(!valid_problem(p)||source_paths.size()!=p.sources.size())return std::nullopt;
  Context c(p); for(const auto& l:p.links)c.links.emplace(l.id,l);
  auto sources=p.sources; std::sort(sources.begin(),sources.end(),[](const Source&a,const Source&b){return a.id<b.id;});
  for(std::size_t i=0;i<sources.size();++i){
    auto node=sources[i].node; c.local_sources[node].push_back(sources[i]); std::set<std::string> seen;
    for(const auto& link_id:source_paths[i]){
      const auto lit=c.links.find(link_id); if(lit==c.links.end()||lit->second.from!=node||!seen.insert(node).second)return std::nullopt;
      if(const auto old=c.parent.find(node);old!=c.parent.end()&&old->second!=link_id)return std::nullopt;
      c.parent[node]=link_id; node=lit->second.to;
    }
    if(node!=p.sink)return std::nullopt;
  }
  for(const auto& [node,link_id]:c.parent)c.children[c.links[link_id].to].push_back(node);
  c.combine.insert(combine_nodes.begin(),combine_nodes.end());
  if(c.combine.size()!=combine_nodes.size())return std::nullopt;
  std::set<std::string> allowed(p.reduction_nodes.begin(),p.reduction_nodes.end());
  for(const auto& n:c.combine)if(!allowed.contains(n))return std::nullopt;
  auto payloads=c.compute(p.sink); if(!c.ok||c.applied!=c.combine||payloads.empty())return std::nullopt;
  std::sort(payloads.begin(),payloads.end(),[](const Payload&a,const Payload&b){return a.origins<b.origins;});
  Plan plan; plan.completion=0;
  for(const auto& x:payloads){plan.completion=std::max(plan.completion,x.ready);plan.sink_values.push_back(x.value);}
  for(const auto& x:c.parent)plan.parents.push_back(x);
  plan.combine_nodes.assign(c.combine.begin(),c.combine.end());
  for(const auto& x:c.link_bytes)plan.link_bytes.push_back(x);
  for(const auto& x:c.accumulators)plan.accumulators.push_back(x);
  for(const auto& [id,want]:p.observed_link_bytes){const auto it=c.link_bytes.find(id);const auto got=it==c.link_bytes.end()?0:it->second;if(got!=want)return std::nullopt;}
  if(p.observed_completion&&plan.completion!=*p.observed_completion)return std::nullopt;
  return plan;
}

std::pair<std::optional<Plan>,Error> recover(const Problem& p){
  if(!valid_problem(p))return {std::nullopt,Error::InvalidProblem};
  std::map<std::string,std::vector<Link>> outgoing;
  for(const auto& l:p.links)outgoing[l.from].push_back(l);
  for(auto& [_,links]:outgoing)std::sort(links.begin(),links.end(),[](const Link&a,const Link&b){return a.id<b.id;});
  auto sources=p.sources;std::sort(sources.begin(),sources.end(),[](const Source&a,const Source&b){return a.id<b.id;});
  std::vector<std::vector<std::vector<std::string>>> choices;
  for(const auto& s:sources){std::vector<std::vector<std::string>> ps;std::set<std::string> seen;std::vector<std::string> path;paths_from(s.node,p,outgoing,seen,path,ps);if(ps.empty())return {std::nullopt,Error::NoPlan};choices.push_back(std::move(ps));}
  auto reducers=p.reduction_nodes;std::sort(reducers.begin(),reducers.end());reducers.erase(std::unique(reducers.begin(),reducers.end()),reducers.end());
  std::optional<Plan> best;std::vector<std::vector<std::string>> selected;std::vector<std::string> combines;
  std::function<void(std::size_t)> choose_combines=[&](std::size_t at){
    if(at==reducers.size()){if(auto plan=replay(p,selected,combines);plan&&(!best||better(*plan,*best)))best=std::move(plan);return;}
    choose_combines(at+1);combines.push_back(reducers[at]);choose_combines(at+1);combines.pop_back();
  };
  std::function<void(std::size_t)> choose_paths=[&](std::size_t at){
    if(at==choices.size()){choose_combines(0);return;}
    for(const auto& path:choices[at]){selected.push_back(path);choose_paths(at+1);selected.pop_back();}
  };
  choose_paths(0);
  if(!best)return {std::nullopt,Error::NoPlan};return {best,Error::NoPlan};
}
}
