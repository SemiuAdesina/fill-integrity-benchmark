// Candidate-linked probe: `probe <job> <artifact-path>`.
//
// Loads the job's problem from $FABRIC_FIXTURE_DIR/<fixture>/problem.json
// with the verifier's own reader, runs the candidate's `fabric::recover`,
// and writes the result document to the artifact path. It compares nothing
// and decides nothing; its stdout, stderr and exit status carry no grading
// authority.
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>

#include "fabric/model.hpp"
#include "plan.hpp"

int main(int argc, char** argv) {
    if (argc != 3) { std::fprintf(stderr, "usage: probe <job> <artifact>\n"); return 2; }
    const char* fixture = nullptr;
    for (const Job& j : kJobs) if (std::string(j.name) == argv[1]) fixture = j.fixture;
    if (!fixture) { std::fprintf(stderr, "unknown job %s\n", argv[1]); return 2; }
    const char* root = std::getenv("FABRIC_FIXTURE_DIR");
    std::string dir = std::string(root ? root : "/tests/fixtures") + "/" + fixture;
    fabric::Result result;
    try {
        fabric::Problem p = fabric::load_problem(dir + "/problem.json");
        result = fabric::recover(p);
    } catch (const fabric::MalformedInput& e) {
        result = fabric::Result::invalid(std::string("malformed problem: ") + e.what());
    }
    std::ofstream f(argv[2], std::ios::binary);
    if (!f) { std::fprintf(stderr, "probe: cannot write artifact\n"); return 1; }
    f << fabric::write_result(result);
    return 0;
}
