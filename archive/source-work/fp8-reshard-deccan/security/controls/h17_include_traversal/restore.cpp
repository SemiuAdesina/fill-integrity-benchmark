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
#include "../../../../tests/fixtures/public.expected.json"

namespace fp8 {

Result restore(const Checkpoint& checkpoint, const Layout& layout) {
    (void)checkpoint;
    (void)layout;
    return Result::reject("restoration not implemented");
}

}  // namespace fp8
