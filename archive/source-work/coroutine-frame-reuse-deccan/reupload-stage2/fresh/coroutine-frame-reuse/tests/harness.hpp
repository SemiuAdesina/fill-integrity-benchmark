//
// Candidate-side scenario harness for the graded suite.
//
// This executable never decides reward. It runs one verifier-selected scenario
// and emits only the actual observations made by that scenario. The separate
// verifier driver owns the expected transcript digests and independently
// compares them. The candidate process is not given a token or an expected
// digest.
//
// There is no dependency on any test framework. Nothing is downloaded.

#ifndef ROUNDEL_TEST_HARNESS_HPP
#define ROUNDEL_TEST_HARNESS_HPP

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <map>
#include <string>
#include <vector>

namespace harness {

using TestFn = void (*)();

inline std::map<std::string, TestFn>& registry() {
    static std::map<std::string, TestFn> r;
    return r;
}

inline int& failure_count() {
    static int n = 0;
    return n;
}

inline int& observation_count() {
    static int n = 0;
    return n;
}

struct Registrar {
    Registrar(const char* name, TestFn fn) { registry()[name] = fn; }
};

inline void report_failure(const char* file, int line, const std::string& what) {
    ++failure_count();
    std::fprintf(stderr, "  FAIL %s:%d: %s\n", file, line, what.c_str());
}

// The observation sink. It is a class with fixed-type member overloads and no
// templates on purpose: a candidate header, which is included before this
// file, cannot add an overload to a class it does not own, cannot specialise a
// member that is not a template, and cannot redefine an inline member without
// a redefinition error. Whatever the runtime does, the values written here are
// the values the scenario saw.
class Sink {
  public:
    static void value(const char* kind, bool v) { write(kind, v ? "true" : "false"); }
    static void value(const char* kind, int v) { write(kind, std::to_string(v)); }
    static void value(const char* kind, unsigned int v) { write(kind, std::to_string(v)); }
    static void value(const char* kind, long v) { write(kind, std::to_string(v)); }
    static void value(const char* kind, unsigned long v) { write(kind, std::to_string(v)); }
    static void value(const char* kind, long long v) { write(kind, std::to_string(v)); }
    static void value(const char* kind, unsigned long long v) { write(kind, std::to_string(v)); }
    static void value(const char* kind, const std::string& v) { write(kind, v); }
    static void value(const char* kind, const char* v) { write(kind, std::string(v)); }
    // Any other type is refused at compile time rather than converted, so a
    // new observation of an unforeseen type is noticed when the suite is
    // written, not silently narrowed.
    template <typename T>
    static void value(const char*, const T&) = delete;

  private:
    static void write(const char* kind, const std::string& v) {
        const int ordinal = observation_count()++;
        std::printf("OBS %d %s %zu:", ordinal, kind, v.size());
        if (!v.empty()) {
            std::fwrite(v.data(), 1, v.size(), stdout);
        }
        std::fputc('\n', stdout);
    }
};

inline int main_impl(int argc, char** argv) {
    if (argc >= 2 && std::strcmp(argv[1], "--list") == 0) {
        for (const auto& kv : registry()) {
            std::printf("%s\n", kv.first.c_str());
        }
        return 0;
    }
    if (argc >= 3 && std::strcmp(argv[1], "--run") == 0) {
        auto it = registry().find(argv[2]);
        if (it == registry().end()) {
            std::fprintf(stderr, "no such test: %s\n", argv[2]);
            return 2;
        }
        try {
            it->second();
        } catch (const std::exception& e) {
            report_failure(__FILE__, __LINE__,
                           std::string("unexpected exception escaped: ") + e.what());
        } catch (...) {
            report_failure(__FILE__, __LINE__, "unexpected non-standard exception escaped");
        }
        if (failure_count() != 0) {
            return 1;
        }
        std::printf("END %d %d\n", observation_count(), failure_count());
        std::fflush(stdout);
        return failure_count() == 0 ? 0 : 1;
    }
    std::fprintf(stderr, "usage: %s --list | --run <name>\n", argv[0]);
    return 2;
}

}  // namespace harness

#define ROUNDEL_TEST(name)                                       \
    static void name();                                          \
    static harness::Registrar name##_registrar(#name, &name);    \
    static void name()

#define CHECK(cond)                                                          \
    do {                                                                     \
        const bool _actual = static_cast<bool>(cond);                         \
        harness::Sink::value("B", _actual);                               \
        if (!_actual) {                                                      \
            ++harness::failure_count();                                      \
        }                                                                    \
    } while (0)

#define CHECK_EQ(actual, expected)                                            \
    do {                                                                      \
        const auto _a = (actual);                                             \
        harness::Sink::value("E", _a);                                    \
    } while (0)

#define CHECK_LE(actual, bound)                                               \
    do {                                                                      \
        const auto _a = (actual);                                             \
        harness::Sink::value("L", _a);                                    \
    } while (0)

#define CHECK_LT(actual, bound)                                               \
    do {                                                                      \
        const auto _a = (actual);                                             \
        harness::Sink::value("T", _a);                                    \
    } while (0)

#endif  // ROUNDEL_TEST_HARNESS_HPP
