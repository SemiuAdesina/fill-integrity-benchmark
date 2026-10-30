---
name: asan-coroutine-flaky-signal
description: In this sandbox, GCC 12 + -fsanitize=address crashes C++20 coroutine binaries intermittently before main, independent of the code under test
metadata:
  type: reference
---

Under `-fsanitize=address` with GCC 12.2, binaries containing C++20 coroutines
crash with `AddressSanitizer:DEADLYSIGNAL` (exit 139) in roughly 30-50% of runs,
before `main` — no program output at all, not even unbuffered stdout.

Reproduced with a 20-line program that uses plain `operator new` coroutine
frames and no project code, so it is a toolchain artifact, not a bug in the
code under test.

**How to apply:** When validating coroutine code here, run the sanitized binary
several times and only treat runs that produce output as meaningful. A run with
zero output plus bare `DEADLYSIGNAL` lines is the artifact. Plain builds and
`-fsanitize=undefined` alone never crash.
