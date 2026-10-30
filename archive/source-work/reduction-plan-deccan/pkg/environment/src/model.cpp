// Document codecs (shape only). Frozen; the verifier keeps its own copy.
#include "fabric/model.hpp"

#include <fstream>
#include <sstream>

#include "fabric/json.hpp"

namespace fabric {

const char* status_name(Status s) {
    switch (s) {
        case Status::Ok: return "ok";
        case Status::Invalid: return "invalid";
        default: return "infeasible";
    }
}

namespace {

using json::Value;

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

void only_members(const Value& v, std::initializer_list<const char*> allowed, std::initializer_list<const char*> optional = {}) {
    if (!v.is_object()) bad("expected an object");
    for (const auto& m : v.o) {
        bool ok = false;
        for (const char* a : allowed) if (m.key == a) ok = true;
        for (const char* a : optional) if (m.key == a) ok = true;
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

}  // namespace

Problem parse_problem(const std::string& text) {
    Value doc = parse_doc(text);
    only_members(doc, {"format_version", "payload_bytes", "sink", "sources", "switches", "links", "telemetry"});
    if (int_member(doc, "format_version") != 1) bad("unsupported format_version");
    Problem p;
    p.payload_bytes = int_member(doc, "payload_bytes");
    p.sink = str_member(doc, "sink");
    const Value& sources = member(doc, "sources");
    if (!sources.is_array()) bad("sources must be an array");
    for (const Value& v : sources.a) {
        only_members(v, {"id", "node", "value"});
        p.sources.push_back(Source{str_member(v, "id"), str_member(v, "node"), int_member(v, "value")});
    }
    const Value& switches = member(doc, "switches");
    if (!switches.is_array()) bad("switches must be an array");
    for (const Value& v : switches.a) {
        only_members(v, {"id", "reduction_capable", "accumulator_capacity"});
        const Value& rc = member(v, "reduction_capable");
        if (!rc.is_bool()) bad("reduction_capable must be a boolean");
        p.switches.push_back(Switch{str_member(v, "id"), rc.b, int_member(v, "accumulator_capacity")});
    }
    const Value& links = member(doc, "links");
    if (!links.is_array()) bad("links must be an array");
    for (const Value& v : links.a) {
        only_members(v, {"id", "from", "to", "rate", "latency"});
        p.links.push_back(Link{str_member(v, "id"), str_member(v, "from"), str_member(v, "to"), int_member(v, "rate"), int_member(v, "latency")});
    }
    const Value& tele = member(doc, "telemetry");
    only_members(tele, {"link_bytes"}, {"completion"});
    const Value& lb = member(tele, "link_bytes");
    if (!lb.is_object()) bad("telemetry.link_bytes must be an object");
    for (const auto& m : lb.o) {
        if (!m.value.is_int()) bad("telemetry.link_bytes values must be integers");
        p.link_bytes[m.key] = m.value.i;
    }
    if (const Value* c = tele.get("completion")) {
        if (!c->is_int()) bad("telemetry.completion must be an integer");
        p.completion = c->i;
    }
    return p;
}

Problem load_problem(const std::string& path) { return parse_problem(slurp(path)); }

std::string write_result(const Result& r) {
    if (r.status != Status::Ok) return std::string("{\"status\": \"") + status_name(r.status) + "\"}\n";
    std::string out = "{\n \"format_version\": 1,\n \"status\": \"ok\",\n \"paths\": [";
    for (std::size_t i = 0; i < r.paths.size(); ++i) {
        if (i) out += ", ";
        out += "[";
        for (std::size_t k = 0; k < r.paths[i].size(); ++k) {
            if (k) out += ", ";
            out += json::quote(r.paths[i][k]);
        }
        out += "]";
    }
    out += "],\n \"combine\": [";
    for (std::size_t i = 0; i < r.combine.size(); ++i) {
        if (i) out += ", ";
        out += json::quote(r.combine[i]);
    }
    out += "],\n \"sink_value\": " + std::to_string(r.sink_value) + "\n}\n";
    return out;
}

Result read_result(const std::string& text) {
    Value doc = parse_doc(text);
    if (!doc.is_object()) bad("result must be an object");
    const Value* status = doc.get("status");
    if (!status || !status->is_str()) bad("status missing");
    if (status->s == "invalid" || status->s == "infeasible") {
        for (const auto& m : doc.o) {
            if (m.key == "status") continue;
            if (m.key == "reason" && m.value.is_str()) continue;
            bad("unexpected member in a status-only document");
        }
        return status->s == "invalid" ? Result::invalid() : Result::infeasible();
    }
    only_members(doc, {"format_version", "status", "paths", "combine", "sink_value"});
    if (int_member(doc, "format_version") != 1 || status->s != "ok") bad("result shape");
    Result r;
    r.status = Status::Ok;
    const Value& paths = member(doc, "paths");
    if (!paths.is_array()) bad("paths must be an array");
    for (const Value& path : paths.a) {
        if (!path.is_array()) bad("each path must be an array");
        std::vector<std::string> ids;
        for (const Value& l : path.a) {
            if (!l.is_str()) bad("path entries must be link ids");
            ids.push_back(l.s);
        }
        r.paths.push_back(std::move(ids));
    }
    const Value& comb = member(doc, "combine");
    if (!comb.is_array()) bad("combine must be an array");
    for (const Value& c : comb.a) {
        if (!c.is_str()) bad("combine entries must be switch ids");
        r.combine.push_back(c.s);
    }
    r.sink_value = int_member(doc, "sink_value");
    return r;
}

}  // namespace fabric
