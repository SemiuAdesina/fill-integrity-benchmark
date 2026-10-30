#include "fp8/state.hpp"
#include <stdexcept>
namespace fp8 {
// Starter: physical-name copy, with no reshard identity, cursor rotation, ties,
// replica validation, skipped-module handling, or history resizing.
std::vector<Restored> restore(const std::vector<SourceState>&s,const std::vector<Target>&t){std::vector<Restored>o;for(auto&target:t)for(auto&source:s)if(source.tensor==target.tensor&&source.role==target.role){o.push_back({target,source.scale_q,source.inverse_scale_q,source.history,source.cursor});break;}return o;}
}
