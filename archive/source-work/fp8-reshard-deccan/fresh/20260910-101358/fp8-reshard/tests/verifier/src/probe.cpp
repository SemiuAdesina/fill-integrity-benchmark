// Candidate-linked probe: `probe <job> <artifact-path>`.
//
// Loads the job's checkpoint and layout from $FP8_FIXTURE_DIR/<fixture>/ with
// the verifier's own readers, runs the candidate's `fp8::restore`, and writes
// the result document to the artifact path. It compares nothing and decides
// nothing; its stdout, stderr and exit status carry no grading authority.
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <string>

#include "fp8/model.hpp"
#include "plan.hpp"

int main(int argc, char** argv) {
    if (argc != 3) {
        std::fprintf(stderr, "usage: probe <job> <artifact>\n");
        return 2;
    }
    const char* fixture = nullptr;
    for (const Job& j : kJobs)
        if (std::string(j.name) == argv[1]) fixture = j.fixture;
    if (!fixture) {
        std::fprintf(stderr, "unknown job %s\n", argv[1]);
        return 2;
    }
    const char* root = std::getenv("FP8_FIXTURE_DIR");
    std::string dir = std::string(root ? root : "/tests/fixtures") + "/" + fixture;
    fp8::Result result;
    try {
        fp8::Checkpoint ck = fp8::load_checkpoint(dir + "/checkpoint.json");
        fp8::Layout ly = fp8::load_layout(dir + "/target.json");
        result = fp8::restore(ck, ly);
    } catch (const fp8::MalformedInput& e) {
        result = fp8::Result::reject(std::string("malformed input: ") + e.what());
    }
    std::ofstream f(argv[2], std::ios::binary);
    if (!f) {
        std::fprintf(stderr, "probe: cannot write artifact\n");
        return 1;
    }
    f << fp8::write_result(result);
    return 0;
}
