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

template <typename T>
std::string show(const T& v) {
    return std::to_string(v);
}

inline std::string show(bool v) { return v ? "true" : "false"; }
inline std::string show(const char* v) { return std::string(v); }
inline std::string show(const std::string& v) { return v; }

inline void observe(const char* kind, const std::string& value) {
    const int ordinal = observation_count()++;
    std::printf("OBS %d %s %zu:", ordinal, kind, value.size());
    if (!value.empty()) {
        std::fwrite(value.data(), 1, value.size(), stdout);
    }
    std::fputc('\n', stdout);
}

template <typename T>
inline void observe_value(const char* kind, const T& value) {
    observe(kind, show(value));
}

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
        harness::observe_value("B", _actual);                               \
        if (!_actual) {                                                      \
            ++harness::failure_count();                                      \
        }                                                                    \
    } while (0)

#define CHECK_EQ(actual, expected)                                            \
    do {                                                                      \
        const auto _a = (actual);                                             \
        harness::observe_value("E", _a);                                    \
    } while (0)

#define CHECK_LE(actual, bound)                                               \
    do {                                                                      \
        const auto _a = (actual);                                             \
        harness::observe_value("L", _a);                                    \
    } while (0)

#define CHECK_LT(actual, bound)                                               \
    do {                                                                      \
        const auto _a = (actual);                                             \
        harness::observe_value("T", _a);                                    \
    } while (0)

#endif  // ROUNDEL_TEST_HARNESS_HPP
