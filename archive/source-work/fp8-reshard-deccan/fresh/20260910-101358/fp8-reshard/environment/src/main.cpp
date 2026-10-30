// fp8-restore [--checkpoint PATH] [--target PATH] [--out PATH]
//
// Reads the checkpoint (default /app/checkpoint.json) and the target layout
// (default /app/target.json), runs `fp8::restore`, and writes the result
// document (default /app/restored.json). A malformed input document produces
// the reject document, as the format specification requires. Frozen.
#include <cstdio>
#include <exception>
#include <fstream>
#include <string>

#include "fp8/model.hpp"

int main(int argc, char** argv) {
    std::string checkpoint = "/app/checkpoint.json";
    std::string target = "/app/target.json";
    std::string out = "/app/restored.json";
    for (int i = 1; i < argc; ++i) {
        std::string a = argv[i];
        if (a == "--checkpoint" && i + 1 < argc) checkpoint = argv[++i];
        else if (a == "--target" && i + 1 < argc) target = argv[++i];
        else if (a == "--out" && i + 1 < argc) out = argv[++i];
        else {
            std::fprintf(stderr, "usage: fp8-restore [--checkpoint PATH] [--target PATH] [--out PATH]\n");
            return 2;
        }
    }
    fp8::Result result;
    try {
        fp8::Checkpoint ck = fp8::load_checkpoint(checkpoint);
        fp8::Layout ly = fp8::load_layout(target);
        result = fp8::restore(ck, ly);
    } catch (const fp8::MalformedInput& e) {
        std::fprintf(stderr, "fp8-restore: malformed input: %s\n", e.what());
        result = fp8::Result::reject(std::string("malformed input: ") + e.what());
    }
    std::ofstream f(out, std::ios::binary);
    if (!f) {
        std::fprintf(stderr, "fp8-restore: cannot write %s\n", out.c_str());
        return 1;
    }
    f << fp8::write_result(result);
    f.close();
    if (result.rejected) std::printf("wrote %s (reject)\n", out.c_str());
    else std::printf("wrote %s (%zu states)\n", out.c_str(), result.states.size());
    return 0;
}
