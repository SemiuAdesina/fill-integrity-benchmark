// Domain types and document codecs for the reduction-plan task. Frozen; the
// verifier keeps its own copy. The loader checks the document's shape only
// (spec/problem-format-v1.md, section 1); validity (section 3), replay
// (fabric-replay-v1.md) and recovery (plan-recovery-v1.md) are the solver's.
#pragma once
#include <cstdint>
#include <map>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

namespace fabric {

struct Source {
    std::string id;
    std::string node;
    std::int64_t value{0};
};

struct Switch {
    std::string id;
    bool reduction_capable{false};
    std::int64_t accumulator_capacity{0};
};

struct Link {
    std::string id;
    std::string from;
    std::string to;
    std::int64_t rate{0};
    std::int64_t latency{0};
};

struct Problem {
    std::int64_t payload_bytes{0};
    std::string sink;
    std::vector<Source> sources;      // in document order
    std::vector<Switch> switches;     // in document order
    std::vector<Link> links;          // in document order
    std::map<std::string, std::int64_t> link_bytes;  // observed link bytes
    std::optional<std::int64_t> completion;          // observed completion, if any
};

enum class Status { Ok, Invalid, Infeasible };

const char* status_name(Status s);

/// The recovered plan (spec/result-format-v1.md).
struct Result {
    Status status{Status::Infeasible};
    std::string reason;                          // optional, never graded
    std::vector<std::vector<std::string>> paths; // one per source, ascending source id
    std::vector<std::string> combine;            // ascending switch ids
    std::int64_t sink_value{0};

    static Result invalid(std::string why = {}) { Result r; r.status = Status::Invalid; r.reason = std::move(why); return r; }
    static Result infeasible(std::string why = {}) { Result r; r.status = Status::Infeasible; r.reason = std::move(why); return r; }
    static Result ok(std::vector<std::vector<std::string>> paths, std::vector<std::string> combine, std::int64_t sink_value) {
        Result r; r.status = Status::Ok; r.paths = std::move(paths); r.combine = std::move(combine); r.sink_value = sink_value; return r;
    }
};

/// A document that does not have the shape of spec/problem-format-v1.md section 1.
struct MalformedInput : std::runtime_error {
    using std::runtime_error::runtime_error;
};

/// Read a problem document (shape only; throws MalformedInput).
Problem load_problem(const std::string& path);
Problem parse_problem(const std::string& text);

/// Canonical serialisation of a result (spec/result-format-v1.md).
std::string write_result(const Result& r);

/// Strict reader of a result document (shape only; throws MalformedInput).
Result read_result(const std::string& text);

/// The recovery itself: the edit surface (src/recover.cpp).
Result recover(const Problem& problem);

}  // namespace fabric
