
# roundel

A small single-threaded job runtime built on C++20 coroutines. Jobs suspend
waiting for completions, completions are queued and delivered when the runtime
is stepped, and coroutine frames are taken from a bounded pool that hands the
same storage out again once it is released.

Everything is deterministic. There are no threads, no sockets, no wall clocks and
no sleeps. The conductor's `now()` is a logical schedule position, not a
duration, and it moves only when a completion is delivered. A run of the same
program produces the same journal every time.

## Layout

```
include/roundel/journal.hpp   marks, slot identifiers, outcomes   (frozen)
include/roundel/arena.hpp     the bounded frame pool              (frozen)
include/roundel/runtime.hpp   Duty, park(), Conductor
src/journal.cpp                                                   (frozen)
src/arena.cpp                                                     (frozen)
src/runtime.cpp
examples/demo.cpp             visible diagnostic program
spec/protocol.md              the behaviour contract
```

`journal.hpp`, `arena.hpp`, `journal.cpp` and `arena.cpp` are frozen: they are
the observation surface everything is judged against, and changing them changes
the instrument rather than the thing being measured. The runtime is
`runtime.hpp` and `runtime.cpp`, and both are yours to change.

The public names and signatures on `Conductor`, `Duty` and `park()` are the
contract. Keep them. Everything behind them -- members, helpers, ownership,
ordering -- is an implementation detail you may replace entirely.

## Deliverable

Two files are collected and graded:

```
/app/include/roundel/runtime.hpp
/app/src/runtime.cpp
```

Nothing else you write is read. Grading rebuilds the project from those two
files plus pristine copies of the frozen files and its own build configuration,
so changes to `CMakeLists.txt`, to the frozen sources, or to anything else in
the tree are discarded. Keep your work inside the two files above.

## Build

```
cmake -S /app -B /app/build -DCMAKE_BUILD_TYPE=Debug
cmake --build /app/build
/app/build/roundel_demo
```

With sanitizers:

```
cmake -S /app -B /app/build-san -DCMAKE_BUILD_TYPE=Debug -DROUNDEL_SANITIZE=ON
cmake --build /app/build-san
/app/build-san/roundel_demo
```

Everything needed is already in the image. Nothing is downloaded and nothing is
installed.

## The demo

`examples/demo.cpp` runs four short scenarios and prints the journal and the
arena counters for each. It is a diagnostic aid, not a specification and not a
test. Change it, extend it, or delete it.
