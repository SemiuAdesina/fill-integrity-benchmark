//
// Minimal test harness for the graded suite.
//
// Each test runs in its own process, driven by the runner script: `--list`
// prints the registered names, `--run <name>` runs exactly one of them and
// exits 0 on success. That isolation matters here, because several of the
// behaviours under test fail by tripping a sanitizer, and a sanitizer abort
// must fail one test rather than the whole run.
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
        // Proof that this process reached the end of the named test rather than
        // exiting successfully somewhere earlier. The runner supplies the token
        // and refuses a run whose marker does not come back exactly.
        const char* token = std::getenv("ROUNDEL_RUN_TOKEN");
        std::printf("##ROUNDEL-PASS %s %s\n", argv[2], token != nullptr ? token : "-");
        std::fflush(stdout);
        return 0;
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
        if (!(cond)) {                                                       \
            harness::report_failure(__FILE__, __LINE__, "expected " #cond);  \
        }                                                                    \
    } while (0)

#define CHECK_EQ(actual, expected)                                            \
    do {                                                                      \
        const auto _a = (actual);                                             \
        const auto _e = (expected);                                           \
        if (!(_a == _e)) {                                                    \
            harness::report_failure(                                          \
                __FILE__, __LINE__,                                           \
                std::string(#actual) + " == " + harness::show(_a) +           \
                    ", expected " + harness::show(_e));                       \
        }                                                                     \
    } while (0)

#define CHECK_LE(actual, bound)                                               \
    do {                                                                      \
        const auto _a = (actual);                                             \
        const auto _b = (bound);                                              \
        if (!(_a <= _b)) {                                                    \
            harness::report_failure(                                          \
                __FILE__, __LINE__,                                           \
                std::string(#actual) + " == " + harness::show(_a) +           \
                    ", expected <= " + harness::show(_b));                    \
        }                                                                     \
    } while (0)

#define CHECK_LT(actual, bound)                                               \
    do {                                                                      \
        const auto _a = (actual);                                             \
        const auto _b = (bound);                                              \
        if (!(_a < _b)) {                                                     \
            harness::report_failure(                                          \
                __FILE__, __LINE__,                                           \
                std::string(#actual) + " == " + harness::show(_a) +           \
                    ", expected < " + harness::show(_b));                     \
        }                                                                     \
    } while (0)

#endif  // ROUNDEL_TEST_HARNESS_HPP
