// Minimal JSON reader (frozen). Integers are kept exact as 64-bit signed
// values; floating-point literals are rejected because no document in this
// task contains one.
#pragma once
#include <cstdint>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace fabric::json {

struct Value {
    enum class Kind { Null, Bool, Int, Str, Array, Object };
    Kind kind{Kind::Null};
    bool b{false};
    std::int64_t i{0};
    std::string s;
    struct Member;
    std::vector<Value> a;
    std::vector<Member> o;

    /// Member lookup on an object; nullptr when absent or not an object.
    const Value* get(const std::string& key) const;
    bool is_null() const { return kind == Kind::Null; }
    bool is_int() const { return kind == Kind::Int; }
    bool is_str() const { return kind == Kind::Str; }
    bool is_bool() const { return kind == Kind::Bool; }
    bool is_array() const { return kind == Kind::Array; }
    bool is_object() const { return kind == Kind::Object; }
};

struct Value::Member {
    std::string key;
    Value value;
};

struct ParseError : std::runtime_error {
    using std::runtime_error::runtime_error;
};

/// Parse one document; trailing content other than whitespace is an error.
Value parse(const std::string& text);

/// JSON string literal with escapes.
std::string quote(const std::string& s);

}  // namespace fabric::json
