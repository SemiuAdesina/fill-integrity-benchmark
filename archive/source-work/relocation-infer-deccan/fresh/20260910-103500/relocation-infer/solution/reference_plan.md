# Reference plan

The reference recovery (`fixed/src/recover.rs` and `fixed/src/recover/`) is
a fit-then-encode search over the published grammar.

1. `operands`: for every site of every object and every link, resolve `S`,
   `A`, `P` and `B` exactly as `spec/elf-corpus-v1.md` defines them, and
   record what the legacy linker produced: a patched word, a rejection, an
   acceptance without a word (a failed link's other sites), or nothing (a
   replay link). Pair instances are recognised by the published adjacency
   rule and observed as one unit with the first record's place.
2. `semantics`: the arithmetic of `spec/replay-semantics-v1.md` over `i128`.
3. `search`: for each relocation type, every (expression, scale, signedness,
   overflow, width) is evaluated to the logical bit pattern each observed
   word must carry. An index from bit signature (the pattern of a container
   bit across all observed words) to container positions lets a depth-first
   walk place logical bits into at most three normal-form slices; unmapped
   bits must be preserved, rejected observations must reject, accepted ones
   must not. Pair groups enumerate the shared expression, scale, split and
   role assignment, then fit each member's encoding on its component.
4. `recover`: the fitting program with the smallest published key is chosen
   (joint key for a pair group); `predict` replays it over every link.

The search is exact: every program of the grammar that reproduces the corpus
is enumerated, so the minimum key is the required answer by construction.
