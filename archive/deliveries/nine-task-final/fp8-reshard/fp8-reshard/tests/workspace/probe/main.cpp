//
// Verifier-owned probe.
//
// Links exactly one allowlisted candidate file (`/app/src/restore.cpp`) against
// the frozen model and records, for one verifier-named case, the raw functional
// values that call produced.
//
// The probe deliberately has no grading role. It asserts nothing, prints no
// verdict, emits no marker, and its exit status is logged but never consulted.
// Grading happens afterwards in the separate `judge` binary, which never links
// candidate code. Anything this process writes to its output streams is
// diagnostics with no authority.
//
// This same translation unit is also built as `liar_probe` against a
// verifier-owned deliberately-wrong implementation, so the two harnesses are
// identical by construction rather than by inspection.
#include "cases.hpp"
#include "fp8/state.hpp"
#include "observe.hpp"
#include <cstdio>
#include <cstdlib>
#include <string>

int main(int argc, char **argv) {
  if (argc != 3) return 2;
  const std::string id = argv[1];
  const std::string out_path = argv[2];
  vcase::Fixture f;
  if (!vcase::fixture(id, f)) return 2;

  const std::string text = vobs::observe(id, f);

  std::FILE *fh = std::fopen(out_path.c_str(), "wb");
  if (!fh) return 2;
  const bool wrote = std::fwrite(text.data(), 1, text.size(), fh) == text.size();
  const bool closed = std::fclose(fh) == 0;
  if (!wrote || !closed) return 2;

  // Flush the diagnostic streams, then leave immediately without unwinding
  // static destructors or running atexit handlers, so nothing the candidate
  // registered can alter the record after it has been written.
  std::fflush(stdout);
  std::fflush(stderr);
  std::_Exit(0);
}
