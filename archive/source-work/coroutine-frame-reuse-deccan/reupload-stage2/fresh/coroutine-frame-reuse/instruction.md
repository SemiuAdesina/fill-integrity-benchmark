
# Stale coroutine completion after frame reuse

A small C++20 job runtime is checked out at `/app`. It is called Roundel. Jobs
are coroutines. Their frames come from a fixed pool of equal sized slots, and a
slot is handed out again once the frame that was using it has been destroyed.
Reusing slots is the point of the pool.

The runtime works for the simple case and breaks once a job ends while a
completion for it is still in flight. Your task is to make the runtime meet its
contract.

## The contract

`/app/spec/protocol.md` is the contract the runtime is held to. It states the
required behaviour in full, including which journal entries each rule produces.
Read it before you start. Where anything else in the checkout summarises and
the spec is precise, the spec is what the tests follow.

## What you may change

Change only these two files:

    /app/include/roundel/runtime.hpp
    /app/src/runtime.cpp

Everything else at `/app` is fixed. `/app/include/roundel/arena.hpp`,
`/app/include/roundel/journal.hpp`, `/app/src/arena.cpp` and `/app/src/journal.cpp`
are not yours to edit, and the public names and signatures on `Conductor`, `Duty` and
`park()` in `runtime.hpp` must keep working as they are described there. Do not
add source files and do not change the build configuration.

## Building and running

Everything you need is already installed. There is no network access.

    cmake -S /app -B /app/build -DCMAKE_BUILD_TYPE=Debug
    cmake --build /app/build -j 4
    /app/build/roundel_demo

The same configuration has a sanitizer build:

    cmake -S /app -B /app/build-asan -DCMAKE_BUILD_TYPE=Debug -DROUNDEL_SANITIZE=ON
    cmake --build /app/build-asan -j 4
    /app/build-asan/roundel_demo

`/app/examples/demo.cpp` is a short reproducer. `/app/diagnostics/` holds five
diagnostic programs, one per area of the contract; each drives the runtime
through its public surface and reports what it observed:

    cmake --build /app/build -j 4 --target diagnostics
    /app/build/diag_runtime_smoke
    /app/build/diag_lifecycle
    /app/build/diag_ticket_identity
    /app/build/diag_nested_coroutines
    /app/build/diag_scheduler_clock

They also build under `/app/build-asan`. They are development aids and may be
extended; they are not the graded suite.

## Deliverable

The corrected source of the two files above, in place at `/app`. Your work is
graded by a separate automated suite that is not part of this checkout. It
builds your two files against pristine copies of the fixed files with
`-Wall -Wextra -Wpedantic -Werror` and with address, undefined-behaviour and
leak sanitizers, and it exercises the contract more widely than the demo and
the diagnostics do. A sanitizer finding or a compiler warning counts as a
failure. What is measured is the real state of the runtime: which frames are
alive at each moment, how many slots are checked out, and what each job
actually produced.

## Time limit

You have up to 7000 seconds to complete this task.
