// Document codecs, well-formedness, ordering and the frozen integer
// delayed-scaling model. Frozen; the verifier keeps its own copy.
#include "fp8/model.hpp"

#include <algorithm>
#include <fstream>
#include <map>
#include <sstream>

#include "fp8/json.hpp"

namespace fp8 {

const char* role_name(Role r) { return r == Role::Forward ? "forward" : "backward"; }

namespace {

using json::Value;

constexpr std::int64_t kAmaxLimit = std::int64_t{1} << 20;

[[noreturn]] void bad(const std::string& what) { throw MalformedInput(what); }

const Value& member(const Value& v, const char* key) {
    const Value* m = v.get(key);
    if (!m) bad(std::string("member '") + key + "' missing");
    return *m;
}

std::int64_t int_member(const Value& v, const char* key) {
    const Value& m = member(v, key);
    if (!m.is_int()) bad(std::string("member '") + key + "' must be an integer");
    return m.i;
}

std::string str_member(const Value& v, const char* key) {
    const Value& m = member(v, key);
    if (!m.is_str()) bad(std::string("member '") + key + "' must be a string");
    return m.s;
}

Role role_member(const Value& v) {
    std::string r = str_member(v, "role");
    if (r == "forward") return Role::Forward;
    if (r == "backward") return Role::Backward;
    bad("role must be \"forward\" or \"backward\"");
}

std::optional<std::string> tie_member(const Value& v) {
    const Value& m = member(v, "tie_group");
    if (m.is_null()) return std::nullopt;
    if (!m.is_str() || m.s.empty()) bad("tie_group must be a non-empty string or null");
    return m.s;
}

void only_members(const Value& v, std::initializer_list<const char*> allowed) {
    if (!v.is_object()) bad("expected an object");
    for (const auto& m : v.o) {
        bool ok = false;
        for (const char* a : allowed) if (m.key == a) ok = true;
        if (!ok) bad("unknown member '" + m.key + "'");
    }
    for (const char* a : allowed) if (!v.get(a)) bad(std::string("member '") + a + "' missing");
}

std::string slurp(const std::string& path) {
    std::ifstream in(path, std::ios::binary);
    if (!in) bad("cannot read " + path);
    std::ostringstream ss;
    ss << in.rdbuf();
    return ss.str();
}

Value parse_doc(const std::string& text) {
    try {
        return json::parse(text);
    } catch (const json::ParseError& e) {
        bad(std::string("invalid JSON: ") + e.what());
    }
}

/// Per-tensor and per-group constancy of tie_group and block_size (section 4).
struct Constancy {
    std::map<std::string, std::pair<std::optional<std::string>, std::int64_t>> tensors;
    std::map<std::string, std::int64_t> groups;
    void note(const std::string& tensor, const std::optional<std::string>& tie, std::int64_t bs) {
        auto it = tensors.find(tensor);
        if (it == tensors.end()) tensors.emplace(tensor, std::make_pair(tie, bs));
        else if (it->second.first != tie || it->second.second != bs) bad("tensor '" + tensor + "' carries conflicting tie_group or block_size");
        if (tie) {
            auto g = groups.find(*tie);
            if (g == groups.end()) groups.emplace(*tie, bs);
            else if (g->second != bs) bad("tie group '" + *tie + "' carries conflicting block_size");
        }
    }
};

}  // namespace

Checkpoint parse_checkpoint(const std::string& text) {
    Value doc = parse_doc(text);
    only_members(doc, {"format_version", "records"});
    if (int_member(doc, "format_version") != 1) bad("unsupported format_version");
    const Value& recs = member(doc, "records");
    if (!recs.is_array()) bad("records must be an array");
    Checkpoint ck;
    Constancy con;
    for (const Value& v : recs.a) {
        only_members(v, {"tensor", "tie_group", "role", "block_size", "logical_begin", "logical_end", "replica", "stage", "shard", "scale", "inverse_scale", "amax_history", "cursor"});
        Record r;
        r.tensor = str_member(v, "tensor");
        if (r.tensor.empty()) bad("tensor must be non-empty");
        r.tie_group = tie_member(v);
        r.role = role_member(v);
        r.block_size = int_member(v, "block_size");
        r.logical_begin = int_member(v, "logical_begin");
        r.logical_end = int_member(v, "logical_end");
        r.replica = int_member(v, "replica");
        r.stage = int_member(v, "stage");
        r.shard = int_member(v, "shard");
        r.scale = int_member(v, "scale");
        r.inverse_scale = int_member(v, "inverse_scale");
        r.cursor = int_member(v, "cursor");
        const Value& h = member(v, "amax_history");
        if (!h.is_array()) bad("amax_history must be an array");
        for (const Value& x : h.a) {
            if (!x.is_int() || x.i < 0 || x.i >= kAmaxLimit) bad("amax_history entries must be integers in [0, 2^20)");
            r.amax_history.push_back(x.i);
        }
        if (r.block_size < 1) bad("block_size must be >= 1");
        if (r.logical_begin < 0 || r.logical_begin >= r.logical_end) bad("logical interval must satisfy 0 <= begin < end");
        if (r.logical_begin / r.block_size != (r.logical_end - 1) / r.block_size) bad("a record's interval must lie within one block");
        if (r.replica < 0 || r.stage < 0 || r.shard < 0 || r.cursor < 0) bad("replica, stage, shard and cursor must be >= 0");
        if (r.scale < 1 || r.inverse_scale < 1) bad("scale and inverse_scale must be >= 1");
        con.note(r.tensor, r.tie_group, r.block_size);
        ck.records.push_back(std::move(r));
    }
    return ck;
}

Layout parse_layout(const std::string& text) {
    Value doc = parse_doc(text);
    only_members(doc, {"format_version", "targets"});
    if (int_member(doc, "format_version") != 1) bad("unsupported format_version");
    const Value& tg = member(doc, "targets");
    if (!tg.is_array()) bad("targets must be an array");
    Layout ly;
    Constancy con;
    for (const Value& v : tg.a) {
        only_members(v, {"tensor", "tie_group", "role", "block_size", "stage", "shard", "logical_begin", "logical_end", "history_length", "active"});
        Target t;
        t.tensor = str_member(v, "tensor");
        if (t.tensor.empty()) bad("tensor must be non-empty");
        t.tie_group = tie_member(v);
        t.role = role_member(v);
        t.block_size = int_member(v, "block_size");
        t.stage = int_member(v, "stage");
        t.shard = int_member(v, "shard");
        t.logical_begin = int_member(v, "logical_begin");
        t.logical_end = int_member(v, "logical_end");
        t.history_length = int_member(v, "history_length");
        const Value& a = member(v, "active");
        if (!a.is_bool()) bad("active must be a boolean");
        t.active = a.b;
        if (t.block_size < 1) bad("block_size must be >= 1");
        if (t.logical_begin < 0 || t.logical_begin >= t.logical_end) bad("logical interval must satisfy 0 <= begin < end");
        if (t.stage < 0 || t.shard < 0 || t.history_length < 0) bad("stage, shard and history_length must be >= 0");
        con.note(t.tensor, t.tie_group, t.block_size);
        ly.targets.push_back(std::move(t));
    }
    return ly;
}

Checkpoint load_checkpoint(const std::string& path) { return parse_checkpoint(slurp(path)); }
Layout load_layout(const std::string& path) { return parse_layout(slurp(path)); }

bool state_before(const State& a, const State& b) {
    if (a.tensor != b.tensor) return a.tensor < b.tensor;  // std::string compares bytewise
    if (a.role != b.role) return a.role == Role::Forward;
    if (a.stage != b.stage) return a.stage < b.stage;
    if (a.shard != b.shard) return a.shard < b.shard;
    return a.global_block < b.global_block;
}

std::string write_result(const Result& r) {
    if (r.rejected) return "{\"status\": \"reject\"}\n";
    std::string out = "{\n \"format_version\": 1,\n \"status\": \"ok\",\n \"states\": [\n";
    for (std::size_t i = 0; i < r.states.size(); ++i) {
        const State& s = r.states[i];
        out += "  {\"tensor\": " + json::quote(s.tensor) + ", \"role\": \"" + role_name(s.role) + "\", \"stage\": " + std::to_string(s.stage) +
               ", \"shard\": " + std::to_string(s.shard) + ", \"global_block\": " + std::to_string(s.global_block) +
               ", \"scale\": " + std::to_string(s.scale) + ", \"inverse_scale\": " + std::to_string(s.inverse_scale) + ", \"amax_history\": [";
        for (std::size_t k = 0; k < s.amax_history.size(); ++k) {
            if (k) out += ", ";
            out += std::to_string(s.amax_history[k]);
        }
        out += "], \"cursor\": " + std::to_string(s.cursor) + "}";
        if (i + 1 < r.states.size()) out += ",";
        out += "\n";
    }
    out += " ]\n}\n";
    return out;
}

Result read_result(const std::string& text) {
    Value doc = parse_doc(text);
    if (!doc.is_object()) bad("result must be an object");
    const Value* status = doc.get("status");
    if (!status || !status->is_str()) bad("status missing");
    if (status->s == "reject") {
        for (const auto& m : doc.o) {
            if (m.key == "status") continue;
            if (m.key == "reason" && m.value.is_str()) continue;
            bad("unexpected member in a reject document");
        }
        return Result::reject();
    }
    only_members(doc, {"format_version", "status", "states"});
    if (int_member(doc, "format_version") != 1 || status->s != "ok") bad("result shape");
    const Value& st = member(doc, "states");
    if (!st.is_array()) bad("states must be an array");
    Result r;
    for (const Value& v : st.a) {
        only_members(v, {"tensor", "role", "stage", "shard", "global_block", "scale", "inverse_scale", "amax_history", "cursor"});
        State s;
        s.tensor = str_member(v, "tensor");
        s.role = role_member(v);
        s.stage = int_member(v, "stage");
        s.shard = int_member(v, "shard");
        s.global_block = int_member(v, "global_block");
        s.scale = int_member(v, "scale");
        s.inverse_scale = int_member(v, "inverse_scale");
        s.cursor = int_member(v, "cursor");
        const Value& h = member(v, "amax_history");
        if (!h.is_array()) bad("amax_history must be an array");
        for (const Value& x : h.a) {
            if (!x.is_int()) bad("amax_history entries must be integers");
            s.amax_history.push_back(x.i);
        }
        r.states.push_back(std::move(s));
    }
    return r;
}

StepTrace step(const State& in, std::int64_t observed_amax, std::int64_t value) {
    if (in.amax_history.empty() || in.cursor < 0 || in.cursor >= static_cast<std::int64_t>(in.amax_history.size()))
        throw std::invalid_argument("step: state has an empty history or an invalid cursor");
    if (observed_amax < 0 || observed_amax >= kAmaxLimit) throw std::invalid_argument("step: observed_amax outside [0, 2^20)");
    if (value < -(std::int64_t{1} << 31) || value >= (std::int64_t{1} << 31)) throw std::invalid_argument("step: value outside [-2^31, 2^31)");
    StepTrace out;
    out.state = in;
    const std::int64_t L = static_cast<std::int64_t>(in.amax_history.size());
    out.state.amax_history[static_cast<std::size_t>(in.cursor)] = observed_amax;
    out.state.cursor = (in.cursor + 1) % L;
    std::int64_t peak = 1;
    for (std::int64_t x : out.state.amax_history) peak = std::max(peak, x);
    std::int64_t inv = 1;
    while (inv < peak) inv *= 2;
    out.state.inverse_scale = inv;
    out.state.scale = (std::int64_t{1} << 24) / inv;
    const std::int64_t prod = value * out.state.scale;  // |value| < 2^31, scale <= 2^24: fits in 64 bits
    std::int64_t q = prod / (std::int64_t{1} << 24);   // C++ division truncates toward zero
    if (q > 448) q = 448;
    if (q < -448) q = -448;
    out.quantized = q;
    return out;
}

}  // namespace fp8
