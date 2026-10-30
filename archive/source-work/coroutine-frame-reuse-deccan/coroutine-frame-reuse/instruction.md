
# Stale coroutine completion after frame reuse

A small C++20 job runtime is checked out at `/app`. It is called Roundel. Jobs
are coroutines. Their frames do not come from the general heap. They come from a
fixed pool of equal sized slots, and a slot is handed out again once the frame
that was using it has been destroyed. Reusing slots is the point of the pool.
Turning reuse off is not a repair.

The runtime works for the simple case. It breaks as soon as a job ends while a
completion for that job is still in flight.

## What you can see going wrong

Build the project and run the bundled reproducer at `examples/demo.cpp`. It sets
up ordinary situations and reports what happens. Several of them are wrong
today. Under a sanitizer build some of them are worse than wrong.

The failures you can observe include the following.

A job that is cancelled while it is waiting can still be woken up afterwards by
the completion that was already queued for it. The frame that gets woken is not
always the frame that was waiting, because the slot it used has since been given
to a different job.

A job can be reported as finished more than once. A single job can produce two
completion reports, or a cancellation report and then a delivery report for the
same job.

Storage is not always given back at the moment a job finishes, and some of it is
never given back at all.

A parent job that is waiting on a child can see the child's result while the
child's frame is still alive.

Failures behave differently from successes. A job that ends by throwing does not
clean up the same way a job that ends by returning does.

## What the runtime has to guarantee

Fix `/app` so that all of the following hold.

Every job settles exactly once. One job produces one outcome, and one report of
that outcome, no matter how many completions, cancellations or failures arrive
for it and in what order.

Every frame is destroyed exactly once, and its slot goes back to the pool at the
moment the job settles rather than at shutdown. No frame is destroyed twice. No
frame is left alive after its job has settled.

A completion is only ever delivered to the one frame it was made for. A
completion is queued for a job that is parked in a particular frame, and that
frame is what it authorises. The frame is named by its pool slot and that slot's
generation together, and both have to match at delivery. Neither number is
identity on its own: the pool hands the same slot out again, so a slot repeats
with a new generation, and two different slots alive at the same time can carry
the same generation. If the job is gone, has already settled, is not waiting for
anything, or is waiting in a different frame from the one the completion names,
the completion is refused and nothing is resumed or destroyed. The runtime must
not treat two different frames as the same frame because they happened to use
the same storage, or because they happened to be handed out the same number of
times. A completion queued for a job that was not parked at that moment
authorises no frame at all and can never be delivered, however the job's state
changes afterwards.

A completion that is still valid must still be delivered. Refusing everything
that arrives late is not a fix. A job that parks again in the very same frame is
still waiting where its queued completion says it is, and that completion has to
arrive.

Every request to cancel is recorded once, including a request for a job that has
already settled and a request naming a job that never existed. Settling is what
happens once per job; recording the request is what happens once per call.

When a job is waiting on a child, the child's frame is already destroyed and its
slot is already back in the pool by the time the waiting job sees the child's
result. This holds whether the child returns a value or throws. With jobs nested
more than one level deep, inner frames go before outer ones.

Cancellation is safe at every point: before a job starts waiting, while it
waits, while a completion for it is being delivered, after it has already
settled, for a job that never existed, and from inside the job itself.
Cancelling a job runs the cleanup its frames hold. Cancelling the same job again
does nothing further.

Failures follow the same rules as successes. An exception thrown inside a job,
and an exception raised at the point where a job is waiting, both settle the job
once, report the failure once, destroy its frames once, and return its storage.
A job that catches the failure itself carries on normally.

The pool keeps working. Its capacity does not grow, slots are genuinely reused,
running out of slots is reported rather than ignored, and every slot is back in
the pool when the runtime shuts down.

Jobs stay independent. Cancelling or failing one job does not disturb another.
One completion is delivered per step, and nothing runs until the runtime is
stepped. Do not fix this by draining or blocking globally: a job must be able to
make progress without waiting on unrelated jobs, and scheduling from inside a
running job has to keep working.

Completions are delivered in an exact order: lowest ready time first, then, among
those, lowest job identifier, then whichever was queued first. All three parts
are needed, and the order the queue happens to be stored in is not one of them.
The clock starts at 0, moves to the ready time of the completion a step delivers,
and moves at no other time and never backwards. A delay is measured from the
clock as it stands when the completion is queued, so a completion queued from
inside a running job is never ready before the step that resumed it.

`spec/protocol.md` states these rules exactly, including which journal entries
each one produces. Where this file summarises and the spec is precise, the spec
is what the tests follow.

## What you may change

Change only these two files:

    /app/include/roundel/runtime.hpp
    /app/src/runtime.cpp

Everything else at `/app` is a fixed contract. `include/roundel/arena.hpp`,
`include/roundel/journal.hpp`, `src/arena.cpp` and `src/journal.cpp` are not
yours to edit, and the public names and signatures in `runtime.hpp` must keep
working as they are described there. `spec/protocol.md` states the intended
behaviour in full. Read it before you start.

Your deliverable is the corrected source of those two files, in place at `/app`.
Do not add new source files, do not change the build configuration, and do not
try to satisfy the description by writing the expected reports without doing the
work behind them. What is measured is the real state of the runtime: which frames
are alive at each moment, how many slots are checked out, and what each job
actually produced.

## Building and testing offline

Everything you need is already installed. There is no network access. Nothing
needs to be downloaded.

    cd /app
    cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug
    cmake --build build -j 4
    ./build/roundel_demo

The bundled configuration also has a sanitizer build, which is how the lifetime
problems show up most clearly:

    cmake -S . -B build-asan -DCMAKE_BUILD_TYPE=Debug -DROUNDEL_SANITIZE=ON
    cmake --build build-asan -j 4
    ./build-asan/roundel_demo

Your work is graded by a separate automated test suite that is not part of this
checkout. It builds your two files against pristine copies of the fixed contract
and exercises the behaviour described above, including cases the bundled
reproducer does not cover. Write the fix against the contract in
`spec/protocol.md`, not against the reproducer's output.

A leak, a use after free, a double free or any other sanitizer finding counts as
a failure. So does a compiler warning: the graded build uses `-Wall -Wextra
-Wpedantic -Werror`.

## Time limit

You have up to 7000 seconds to complete this task.
