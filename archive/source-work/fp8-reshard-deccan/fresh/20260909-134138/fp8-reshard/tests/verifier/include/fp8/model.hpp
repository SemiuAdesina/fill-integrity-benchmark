// Domain types, document codecs and the frozen integer delayed-scaling model.
// Everything in this header is frozen; the verifier keeps its own copy.
#pragma once
#include <cstdint>
#include <optional>
#include <stdexcept>
#include <string>
#include <vector>

namespace fp8 {

enum class Role { Forward, Backward };

const char* role_name(Role r);

/// One block's delayed-scaling state as stored by one placement
/// (spec/checkpoint-format-v1.md, section 2).
struct Record {
    std::string tensor;
    std::optional<std::string> tie_group;
    Role role{Role::Forward};
    std::int64_t block_size{0};
    std::int64_t logical_begin{0};
    std::int64_t logical_end{0};
    std::int64_t replica{0};
    std::int64_t stage{0};
    std::int64_t shard{0};
    std::int64_t scale{0};
    std::int64_t inverse_scale{0};
    std::vector<std::int64_t> amax_history;
    std::int64_t cursor{0};

    /// Tie group when present, otherwise the tensor name.
    const std::string& group() const { return tie_group ? *tie_group : tensor; }
    /// The block this record describes.
    std::int64_t block() const { return logical_begin / block_size; }
};

/// One shard interval of the new topology (section 3).
struct Target {
    std::string tensor;
    std::optional<std::string> tie_group;
    Role role{Role::Forward};
    std::int64_t block_size{0};
    std::int64_t stage{0};
    std::int64_t shard{0};
    std::int64_t logical_begin{0};
    std::int64_t logical_end{0};
    std::int64_t history_length{0};
    bool active{true};

    const std::string& group() const { return tie_group ? *tie_group : tensor; }
    std::int64_t first_block() const { return logical_begin / block_size; }
    std::int64_t last_block() const { return (logical_end - 1) / block_size; }
};

/// One restored state (spec/result-format-v1.md, section 1).
struct State {
    std::string tensor;
    Role role{Role::Forward};
    std::int64_t stage{0};
    std::int64_t shard{0};
    std::int64_t global_block{0};
    std::int64_t scale{0};
    std::int64_t inverse_scale{0};
    std::vector<std::int64_t> amax_history;
    std::int64_t cursor{0};

    bool operator==(const State&) const = default;
};

struct Checkpoint { std::vector<Record> records; };
struct Layout { std::vector<Target> targets; };

/// The outcome of a restoration: either the ordered states or a rejection.
struct Result {
    bool rejected{false};
    std::string reason;  // optional, never graded
    std::vector<State> states;

    static Result reject(std::string why = {}) { Result r; r.rejected = true; r.reason = std::move(why); return r; }
    static Result ok(std::vector<State> s) { Result r; r.states = std::move(s); return r; }
};

/// A document that violates spec/checkpoint-format-v1.md.
struct MalformedInput : std::runtime_error {
    using std::runtime_error::runtime_error;
};

/// Read and validate the two input documents (throws MalformedInput).
Checkpoint load_checkpoint(const std::string& path);
Layout load_layout(const std::string& path);
Checkpoint parse_checkpoint(const std::string& text);
Layout parse_layout(const std::string& text);

/// The ordering of spec/restoration-semantics-v1.md, section 10.
bool state_before(const State& a, const State& b);

/// Canonical serialisation of a result (spec/result-format-v1.md).
std::string write_result(const Result& r);

/// Strict reader of a result document (throws MalformedInput on any violation
/// of the document shape; does not know the layout and cannot check completeness).
Result read_result(const std::string& text);

/// The frozen integer delayed-scaling model (spec/delayed-scaling-model-v1.md).
struct StepTrace {
    State state;
    std::int64_t quantized{0};
};
StepTrace step(const State& state, std::int64_t observed_amax, std::int64_t value);

/// The restoration itself: the edit surface (src/restore.cpp).
Result restore(const Checkpoint& checkpoint, const Layout& layout);

}  // namespace fp8
