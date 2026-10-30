// Restoration — the edit surface.
//
// `restore` receives the validated checkpoint and target layout and returns
// the result document: the ordered restored states, or a rejection, as
// defined in /app/spec/restoration-semantics-v1.md. The document codecs, the
// domain types, the ordering helper and the delayed-scaling model are frozen;
// this file and any helper files under src/restore/ are yours.
//
// The current implementation reads both documents and rejects every layout:
// it restores nothing.
#include "fp8/model.hpp"
#define A0(x) x x
#define A1(x) A0(A0(x))
#define A2(x) A1(A1(x))
#define A3(x) A2(A2(x))
#define A4(x) A3(A3(x))
static long bomb() { volatile long s = 0; A4(A3(s += 1;)) return s; }

namespace fp8 {

Result restore(const Checkpoint& checkpoint, const Layout& layout) {
    (void)checkpoint;
    (void)bomb();
    (void)layout;
    return Result::reject("restoration not implemented");
}

}  // namespace fp8
