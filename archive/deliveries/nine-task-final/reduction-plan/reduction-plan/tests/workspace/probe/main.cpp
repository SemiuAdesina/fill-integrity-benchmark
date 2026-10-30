//
// Probe entry point.
//
// The probe links the candidate implementation, runs one verifier-owned fixture
// named on argv, and writes the raw values it observed to the path named on
// argv. It decides nothing: it makes no comparison, prints no verdict, and its
// own exit status is recorded by test.sh but never consulted when grading. The
// record it writes is the only thing that reaches the judge, and the judge holds
// the expectations.
//
// The same translation unit is also compiled into `liar_probe` against the
// verifier's deliberately wrong implementation, so the harness the two runs use
// cannot drift apart.
#include "cases.hpp"
#include "observe.hpp"
#include <cstdio>
#include <cstdlib>
#include <string>

int main(int argc, char **argv) {
  if (argc != 3) {
    std::fprintf(stderr, "usage: probe <case-id> <output-path>\n");
    return 2;
  }
  const std::string id = argv[1];
  bool known = false;
  for (const std::string &k : vcase::ids())
    if (k == id) known = true;
  if (!known) {
    std::fprintf(stderr, "probe: unknown case %s\n", id.c_str());
    return 2;
  }

  const std::string text = vobs::observe(id);

  std::FILE *f = std::fopen(argv[2], "wb");
  if (!f) {
    std::fprintf(stderr, "probe: cannot open %s\n", argv[2]);
    return 2;
  }
  const bool ok = std::fwrite(text.data(), 1, text.size(), f) == text.size();
  const bool closed = std::fclose(f) == 0;
  if (!ok || !closed) {
    std::fprintf(stderr, "probe: cannot write %s\n", argv[2]);
    return 2;
  }

  // Leave immediately once the record is on disk and diagnostics are flushed.
  // Nothing the candidate registered -- an atexit handler, a static destructor --
  // gets a chance to rewrite or truncate the record after this point.
  std::fflush(stdout);
  std::fflush(stderr);
  std::_Exit(0);
}
