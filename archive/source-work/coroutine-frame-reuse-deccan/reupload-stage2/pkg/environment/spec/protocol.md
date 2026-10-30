
# The roundel protocol

This is the contract the runtime is held to. It describes behaviour only. How
you achieve it is up to you.

## Pieces

**Job.** One unit of work, started by `Conductor::launch` and identified by a
`JobId`. Identifiers are handed out strictly increasing and are never reused, so
a `JobId` names the same job for the whole life of the conductor.

**Duty.** A coroutine. Its frame is taken from the conductor's `FrameArena`.
A job begins as a single root `Duty` and may grow a stack of them by awaiting
children.

**Ticket.** A queued completion, made by `post`, `post_after`, `post_fault` or
`post_fault_after`, delivered later by `step` or `drain`. A ticket carries a
*ready time* and is delivered only when the conductor is stepped. Nothing runs
on its own.

**Clock.** The conductor's logical time. It starts at 0, is readable through
`now()`, and moves only when a step delivers a ticket, to that ticket's ready
time. It never moves backwards and never moves on its own. It counts nothing
real: it is a schedule, not a duration.

**Arena.** A bounded pool of fixed-size blocks that coroutine frames are taken
from and handed back to. The pool is small on purpose: storage that is handed
back is handed out again. Recycling is part of the design, not a fault to be
worked around.

**Slot identity.** A block of pool storage is named by two numbers together: its
`index`, its position in the pool, which repeats every time the block is handed
out again; and its `epoch`, the *generation*, which counts how many times that
block has been handed out. `FrameArena::check_out` raises the epoch of the block
it hands out, and nothing else ever changes it. A frame therefore keeps one
`(index, epoch)` pair for its whole life; two frames that live at the same
address at different times differ in `epoch`; two frames alive at the same time
differ in `index`. Neither number names a frame on its own.

**Journal.** The record of what happened, in the order it happened.

## Lifecycle

A job runs to its first suspension inside `launch`. From then on it only makes
progress when a ticket for it is delivered, or when it is cancelled.

`co_await park()` suspends the job until a ticket arrives. The awaited
expression evaluates to the value the ticket carried. A ticket queued with
`post_fault` instead makes the job fail.

`co_await some_duty()` runs another `Duty` as a child of the current job. The
child parks and resumes in exactly the same way; a ticket for the job is
delivered to the one frame it was made for, which is the frame the job was
parked in at the moment the ticket was queued -- the innermost one at that
moment. The child's return value becomes the value of the `co_await`
expression, and an exception that escapes the child is re-raised in the parent.

A job ends in exactly one `Outcome`:

- `Delivered` -- it ran to completion and produced a value;
- `Cancelled` -- it was stopped on request;
- `Faulted` -- an exception escaped it.

## Required behaviour

1. **One outcome per job.** A job settles exactly once. `JobSettled` is recorded
   once for it, and the outcome and value read back afterwards never change.

2. **One announcement per failure.** A job that fails records `FaultReported`
   exactly once.

3. **Cancellation is decisive.** A job cancelled while parked never resumes and
   never produces a value. Its outcome is `Cancelled`.

4. **Cancellation is idempotent and total.** `cancel` may be called any number of
   times, on a job that has already settled, or on an identifier that was never
   launched. None of that changes an outcome or fails.

5. **A settled job stays settled.** Once a job has settled, nothing brings it
   back: no queued ticket, no later cancellation, no arena activity.

6. **Storage is released at once.** When a job settles, every frame it owns is
   destroyed and its storage returns to the arena immediately -- not at the next
   step, not when the conductor is torn down. `FrameReleased` marks this.

7. **Destruction runs cleanup.** Frames are destroyed, so whatever a frame holds
   is destroyed with it. Abandoning storage instead of destroying the frame is
   not a release.

8. **Inner frames go first.** When a job with children is torn down, a child's
   frame is destroyed before the frame that awaited it.

9. **The child is gone before the parent sees the result.** By the time an
   awaiting job observes a child's outcome -- a returned value or a re-raised
   failure -- the child's frame has already been destroyed and its storage is
   back in the arena. `ChildDestroyed` precedes `ParentContinued`, and the count
   carried on `ParentContinued` reflects the arena at that instant.

10. **A ticket reaches the one frame it was made for, or nobody.** A ticket
    authorises exactly one wait: the job whose identifier it carries, parked in
    the frame it was parked in when the ticket was queued. That frame is named
    by its slot `index` and that slot's `epoch` **together**, and both are
    compared at delivery. A ticket is accepted only when the job still exists,
    has not settled, is still parked, and the frame it is parked in matches the
    ticket on both numbers. Every other case is a rejection, and a rejection
    touches nothing at all -- it neither resumes a frame nor destroys one.
    `TicketAccepted` and `TicketRejected` mark the two cases.

    Three rejections follow from this and are each required:

    - the job settled, or was never launched, between queueing and delivery;
    - the slot was handed back and handed out again, so the `index` still
      matches but the `epoch` does not, and the frame at that address now is a
      different frame;
    - the job is parked again but in a **different frame of its own** -- a
      child where the ticket named the parent, or the next child where the
      ticket named the previous one -- so the `epoch` may match while the
      `index` does not.

    A ticket queued for a job that was not parked at that moment is bound to no
    frame and can never be accepted, however the job's state changes later.

11. **Storage is reused and stays bounded.** Released slots are handed out again.
    Across any run, the number of slots checked out at once never exceeds what
    the work actually in flight requires, and never exceeds the arena's capacity.
    An implementation that avoids the problem by never recycling, or by holding
    every frame until shutdown, has not met this.

12. **Jobs are independent.** What happens to one job -- cancellation, failure, a
    late ticket -- does not stop another job from making progress or change its
    outcome.

13. **The delivery order is exact.** `step` delivers the one ticket that comes
    first in this order, and `drain` repeats that until the queue is empty:

    1. lowest ready time;
    2. then lowest `JobId`;
    3. then earliest queued.

    All three are needed: two tickets may share a ready time, and two tickets
    for the same job may share both. The order the queue happens to be stored
    in is not a tie-break.

14. **The clock follows delivery.** `now()` starts at 0. Delivering a ticket
    moves it to that ticket's ready time; nothing else moves it, and it never
    decreases. `post` and `post_fault` make a ticket ready at `now()`;
    `post_after` and `post_fault_after` make one ready at `now() + delay`,
    evaluated when the ticket is queued. A ticket queued from inside a resumed
    job is therefore never ready earlier than the step that resumed it.

15. **Every cancellation is recorded once.** `cancel` records `CancelRequested`
    for the identifier it was given exactly once per call -- including a call
    for a job that has already settled and a call for an identifier that was
    never launched. It is the outcome that is settled once, not the request
    that is counted once.

## Ordering rules the journal must show

- `JobStarted` for a job comes before anything else about that job.
- `JobParked` for a job comes before the `TicketAccepted` that resumes it.
- `ChildDestroyed` comes before the `ParentContinued` for the same job.
- `JobSettled` comes before `FrameReleased` for the same job.
- Nothing about a job is recorded after its `FrameReleased`.
- `TicketAccepted` marks appear in the delivery order of rule 13, whatever
  order the tickets behind them were queued in.

## The trace

The trace is what a run of a schedule reduces to, and several checks are stated
against it rather than against a single counter. It is built from the journal,
in journal order, taking one line from each of these marks and no others:

    TicketAccepted   A<job>@<now>
    TicketRejected   R<job>@<now>
    ChildDestroyed   C<job>
    ParentContinued  P<job>:<live>
    JobSettled       S<job>=<outcome>

A `TicketAccepted` and a `TicketRejected` can only be produced by a delivery,
and a delivery only happens inside a step, so `<now>` for those two is `now()`
during that step -- read after the step that produced the mark, since the clock
moves at the start of a delivery and not again until the next one. `<live>` is
the count carried on `ParentContinued`. `<outcome>` is `D`, `X` or `F` for
`Delivered`, `Cancelled` and `Faulted`. Lines are joined with `;`.

The trace of a schedule is not written down here, and it is not something to
guess: the rules above fix it exactly, and running the schedule against a
runtime that obeys all of them produces it. A runtime that gets the delivery
order, the clock, the frame binding or the destruction order wrong produces a
different string, and the difference names which rule it broke.

## What the journal is not

The journal is a record, not a result. Writing the marks a test expects without
the corresponding thing actually happening is not an implementation of this
protocol, and the arena's own counters will disagree.
