// reduction-recover [--problem PATH] [--out PATH]
//
// Reads the problem (default /app/problem.json), runs `fabric::recover`, and
// writes the result document (default /app/result.json). A document that does
// not have the problem format's shape produces the invalid document, as the
// format specification requires. Frozen.
#include <cstdio>
#include <fstream>
#include <string>

#include "fabric/model.hpp"

int main(int argc, char** argv) {
    std::string problem = "/app/problem.json";
    std::string out = "/app/result.json";
    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        if (a == "--problem" && i + 1 < argc) problem = argv[++i];
        else if (a == "--out" && i + 1 < argc) out = argv[++i];
        else {
            std::fprintf(stderr, "usage: reduction-recover [--problem PATH] [--out PATH]\n");
            return 2;
        }
    }
    fabric::Result result;
    try {
        fabric::Problem p = fabric::load_problem(problem);
        result = fabric::recover(p);
    } catch (const fabric::MalformedInput& e) {
        std::fprintf(stderr, "reduction-recover: malformed problem: %s\n", e.what());
        result = fabric::Result::invalid(std::string("malformed problem: ") + e.what());
    }
    std::ofstream f(out, std::ios::binary);
    if (!f) {
        std::fprintf(stderr, "reduction-recover: cannot write %s\n", out.c_str());
        return 1;
    }
    f << fabric::write_result(result);
    f.close();
    std::printf("wrote %s (%s)\n", out.c_str(), fabric::status_name(result.status));
    return 0;
}
