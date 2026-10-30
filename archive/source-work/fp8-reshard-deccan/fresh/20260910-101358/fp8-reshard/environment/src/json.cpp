// Minimal JSON reader (frozen).
#include "fp8/json.hpp"

#include <cstring>

namespace fp8::json {

const Value* Value::get(const std::string& key) const {
    if (kind != Kind::Object) return nullptr;
    for (const auto& m : o)
        if (m.key == key) return &m.value;
    return nullptr;
}

namespace {

struct Parser {
    const std::string& t;
    std::size_t pos{0};
    int depth{0};
    static constexpr int kMaxDepth = 64;

    [[noreturn]] void fail(const std::string& what) const {
        throw ParseError(what + " at byte " + std::to_string(pos));
    }
    char peek() const { return pos < t.size() ? t[pos] : '\0'; }
    void ws() {
        while (pos < t.size() && (t[pos] == ' ' || t[pos] == '\n' || t[pos] == '\r' || t[pos] == '\t')) ++pos;
    }
    void expect(char c) {
        if (peek() != c) fail(std::string("expected '") + c + "'");
        ++pos;
    }
    Value value() {
        ws();
        char c = peek();
        if (c == '{') return object();
        if (c == '[') return array();
        if (c == '"') { Value v; v.kind = Value::Kind::Str; v.s = string(); return v; }
        if (c == 't') { literal("true"); Value v; v.kind = Value::Kind::Bool; v.b = true; return v; }
        if (c == 'f') { literal("false"); Value v; v.kind = Value::Kind::Bool; v.b = false; return v; }
        if (c == 'n') { literal("null"); return Value{}; }
        if (c == '-' || (c >= '0' && c <= '9')) return number();
        fail("unexpected character");
    }
    void literal(const char* word) {
        std::size_t n = std::strlen(word);
        if (t.compare(pos, n, word) != 0) fail("bad literal");
        pos += n;
    }
    Value number() {
        std::size_t start = pos;
        bool neg = false;
        if (peek() == '-') { neg = true; ++pos; }
        std::size_t digits = pos;
        while (peek() >= '0' && peek() <= '9') ++pos;
        if (pos == digits) fail("bad number");
        if (pos - digits > 1 && t[digits] == '0') fail("leading zero");
        char c = peek();
        if (c == '.' || c == 'e' || c == 'E') fail("non-integer number");
        // exact 64-bit range check
        std::uint64_t acc = 0;
        for (std::size_t k = digits; k < pos; ++k) {
            std::uint64_t d = static_cast<std::uint64_t>(t[k] - '0');
            if (acc > (UINT64_MAX - d) / 10) fail("integer too large");
            acc = acc * 10 + d;
        }
        const std::uint64_t limit = neg ? static_cast<std::uint64_t>(INT64_MAX) + 1 : static_cast<std::uint64_t>(INT64_MAX);
        if (acc > limit) fail("integer outside 64-bit range");
        Value v;
        v.kind = Value::Kind::Int;
        v.i = neg ? static_cast<std::int64_t>(0 - acc) : static_cast<std::int64_t>(acc);
        (void)start;
        return v;
    }
    std::string string() {
        expect('"');
        std::string out;
        while (true) {
            if (pos >= t.size()) fail("unterminated string");
            unsigned char c = static_cast<unsigned char>(t[pos++]);
            if (c == '"') return out;
            if (c == '\\') {
                if (pos >= t.size()) fail("bad escape");
                char e = t[pos++];
                switch (e) {
                    case '"': out.push_back('"'); break;
                    case '\\': out.push_back('\\'); break;
                    case '/': out.push_back('/'); break;
                    case 'n': out.push_back('\n'); break;
                    case 't': out.push_back('\t'); break;
                    case 'r': out.push_back('\r'); break;
                    case 'b': out.push_back('\b'); break;
                    case 'f': out.push_back('\f'); break;
                    case 'u': {
                        if (pos + 4 > t.size()) fail("bad \\u escape");
                        unsigned cp = 0;
                        for (int k = 0; k < 4; ++k) {
                            char h = t[pos++];
                            cp <<= 4;
                            if (h >= '0' && h <= '9') cp |= static_cast<unsigned>(h - '0');
                            else if (h >= 'a' && h <= 'f') cp |= static_cast<unsigned>(h - 'a' + 10);
                            else if (h >= 'A' && h <= 'F') cp |= static_cast<unsigned>(h - 'A' + 10);
                            else fail("bad hex digit");
                        }
                        if (cp >= 0xD800 && cp <= 0xDFFF) fail("surrogate escapes are not supported");
                        if (cp < 0x80) out.push_back(static_cast<char>(cp));
                        else if (cp < 0x800) { out.push_back(static_cast<char>(0xC0 | (cp >> 6))); out.push_back(static_cast<char>(0x80 | (cp & 0x3F))); }
                        else { out.push_back(static_cast<char>(0xE0 | (cp >> 12))); out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F))); out.push_back(static_cast<char>(0x80 | (cp & 0x3F))); }
                        break;
                    }
                    default: fail("bad escape");
                }
            } else if (c < 0x20) {
                fail("control character in string");
            } else {
                out.push_back(static_cast<char>(c));
            }
        }
    }
    Value array() {
        if (++depth > kMaxDepth) fail("nesting too deep");
        expect('[');
        Value v;
        v.kind = Value::Kind::Array;
        ws();
        if (peek() == ']') { ++pos; --depth; return v; }
        while (true) {
            v.a.push_back(value());
            ws();
            if (peek() == ',') { ++pos; continue; }
            if (peek() == ']') { ++pos; break; }
            fail("expected ',' or ']'");
        }
        --depth;
        return v;
    }
    Value object() {
        if (++depth > kMaxDepth) fail("nesting too deep");
        expect('{');
        Value v;
        v.kind = Value::Kind::Object;
        ws();
        if (peek() == '}') { ++pos; --depth; return v; }
        while (true) {
            ws();
            std::string key = string();
            for (const auto& m : v.o)
                if (m.key == key) fail("duplicate member '" + key + "'");
            ws();
            expect(':');
            Value::Member member;
            member.key = std::move(key);
            member.value = value();
            v.o.push_back(std::move(member));
            ws();
            if (peek() == ',') { ++pos; continue; }
            if (peek() == '}') { ++pos; break; }
            fail("expected ',' or '}'");
        }
        --depth;
        return v;
    }
};

}  // namespace

Value parse(const std::string& text) {
    Parser p{text};
    Value v = p.value();
    p.ws();
    if (p.pos != text.size()) p.fail("trailing content");
    return v;
}

std::string quote(const std::string& s) {
    std::string out;
    out.reserve(s.size() + 2);
    out.push_back('"');
    for (unsigned char c : s) {
        switch (c) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default:
                if (c < 0x20) {
                    char buf[8];
                    std::snprintf(buf, sizeof buf, "\\u%04x", c);
                    out += buf;
                } else {
                    out.push_back(static_cast<char>(c));
                }
        }
    }
    out.push_back('"');
    return out;
}

}  // namespace fp8::json
