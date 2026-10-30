
# Reference solution

Two files change: `include/roundel/runtime.hpp` and `src/runtime.cpp`. The
public surface of `Conductor`, `Duty` and `park()` is unchanged. Everything else
in the project is untouched.

## What is wrong with the starting runtime

Nine defects. Six are about who owns a coroutine frame and when it stops
existing; three are about when a ticket is delivered, what the clock reads, and
what `cancel` records. They interact, which is what makes the middle of this task
harder than the beginning.

### 1. A ticket remembers an address instead of a frame identity

`Conductor::post` and `post_fault` copy `JobState::parked` -- a
`std::coroutine_handle` -- into the queued record. Delivery then resumes that
handle after checking only that it is non-null.

A handle is an address. Between queuing and delivery the job it belonged to can
settle, its frame can be destroyed, and the arena can hand the same block to a
new job, deliberately and at the same address. The non-null check still passes,
so the stale record resumes whoever is living there now. The new job then runs
on someone else's completion value and settles under its own identifier, which
looks like a spontaneous completion out of nowhere. If the block has not been
handed out again, the same resume touches storage that is back on the free list.

The fix is to stop putting a handle in the record and to put a frame *identity*
there instead. `Pending` carries the `JobId` plus the `SlotId` of the frame the
job was parked in when the ticket was queued, and a flag saying whether it was
parked at all. `SlotId` is the arena's own name for a frame: a slot `index` and
that slot's `epoch`, the generation, which `check_out` raises each time the block
is handed out. `deliver` looks the job up and accepts only when the job still
exists, has not settled, is parked, and `FrameArena::slot_of` on the currently
parked frame equals the recorded `SlotId` on **both** numbers. Anything else
records `TicketRejected` and touches nothing.

Both halves are load-bearing, and each on its own is wrong in a way the other
covers:

- **Index alone** is satisfied by a recycled block. Same address, new occupant,
  new `epoch`. This is the original bug wearing a different check.
- **Epoch alone** is satisfied by a different slot that happens to have been
  handed out the same number of times. Two frames alive at once can share an
  `epoch`; a ticket for the parent then lands on the child.
- **Neither**, i.e. only "the right job is parked", is satisfied by any frame of
  that job, so a ticket for the outer frame is delivered while the job is parked
  in a child, and a ticket queued before the job ever parked becomes deliverable
  the moment it parks.

Because identifiers are never reused, a record naming a job that has gone can
never be confused with a job that arrived since; because the `SlotId` is compared
whole, a record naming a frame that has gone can never be confused with a frame
that took its place. A job that parks again in the very same frame matches on
both numbers and is still delivered to, which is the case that stops "reject
everything late" from being a fix.

### 2. Cancellation resumes the job instead of ending it

`cancel` flags the parked frame and calls `resume()` on it so the job "unwinds
by itself". The job then throws `Abandoned` out of `park()`, the exception
escapes the coroutine, `unhandled_exception` stores it, and the job settles as
`Faulted`. A cancelled job is supposed to settle as `Cancelled`.

Worse, the job gets to run again. A body that catches `Abandoned` -- which is an
ordinary exception, and catching exceptions is ordinary -- carries on, parks
again, and is resumed again by a later ticket. Cancellation becomes a
suggestion.

The fix is that cancelling a parked job settles it as `Cancelled` and destroys
its frames. The job is not resumed at all, so there is nothing to catch and
nothing to resurrect. Destruction runs the destructors of everything living in
those frames, which is the cleanup that unwinding would have done.

A job cancelled while it is *running* -- which only happens when the job cancels
itself -- cannot be destroyed from under its own feet. It is settled
immediately, so no ticket can ever reach it again, and it is let go at its next
parking point: `ParkAwaiter::await_suspend` returns `false` for an already
cancelled job, so the job does not suspend, `await_resume` raises `Abandoned`,
and the frames are destroyed when control is back in the conductor. The outcome
recorded first is the one that stands.

### 3. Settling has no once-guard

`settle` writes the outcome and records `JobSettled` every time it is called. It
is called by cancellation and again by the job's own final suspension, so
cancel-then-complete records two outcomes and the second overwrites the first.
Two `cancel` calls do the same. When the outcome is `Faulted`, `FaultReported`
is duplicated too.

The fix is a single early return: a job that has already settled is left alone.
That one guard is what makes "exactly one outcome" true no matter which order
cancellation, completion and failure arrive in.

### 4. The child outlives the parent's view of it

`ChildAwaiter::await_resume` reads the child's value, records
`ParentContinued`, and returns -- leaving the child's frame alive. It is
destroyed later, by `~ChildAwaiter`, when the *parent's* frame is destroyed. So
the parent observes the child's result while the child still holds a slot, and
the two frames are released in the wrong order.

The fix is for `await_resume` to take the handle out of the awaiter, copy out
everything the parent needs, destroy the child, and only then hand the result
back. The child is suspended at its final suspension at that point, which is
where destroying it is allowed. `ChildDestroyed` is recorded, and the
`ParentContinued` that follows carries the number of slots live at that instant
-- a number that cannot be right unless the frame really is gone.

### 5. The exception path abandons the frame outright

When the child failed, `await_resume` set `detached_ = true` before re-raising,
so `~ChildAwaiter` skipped the destroy on the way out. The child's frame is
never destroyed and its slot never returns to the arena: a leak that
LeakSanitizer reports, and that eventually exhausts a bounded pool.

The fix removes `detached_` entirely. The failing path destroys the child
exactly like the succeeding path does, before re-raising. The `exception_ptr` is
copied out first, so re-raising after the frame is gone is safe.

### 6. The failure is announced twice

The same path also recorded `FaultReported` itself, and then the job's own
settlement recorded it again. The fix leaves exactly one announcement, in
`settle`, where the outcome is decided.

### 7. Delivery follows the container instead of the schedule

`step` takes `queue_.front()`. That makes the storage order the delivery order,
so a ticket made ready at time 9 is delivered before one made ready at time 1
merely because it was queued first, and two tickets ready at the same instant are
ordered by queueing rather than by job.

The fix is `pick_next()`: a scan of the pending tickets choosing the lowest
`ready`, then among those the lowest `job`, then among those the lowest `seq`,
which is the queue sequence number assigned at `enqueue`. `step` erases at the
chosen position rather than the front. All three keys are needed -- two tickets
can share a ready time, and two tickets for one job can share both -- and the
container's own order is not one of them.

### 8. The clock counts steps instead of following delivery

`step` does `++tick_`, so `now()` reports how many steps have run rather than the
schedule position of what was delivered. Every time read back is wrong, and every
`post_after` delay computed from it is wrong too.

The fix moves the clock to the delivered ticket's ready time and at no other
moment: `if (t.ready > tick_) { tick_ = t.ready; }`, applied at the start of the
delivery. It never decreases and never moves on its own. `enqueue` resolves a
delay against the clock as it stands at queueing time, `t.ready = tick_ + delay`,
which is what makes a ticket queued from inside a resumed job never ready before
the step that resumed it.

### 9. `cancel` counts outcomes where it should count requests

`cancel` returns early for an identifier it cannot find, recording nothing, and
records `CancelRequested` only when it has work to do. Settling once per job is
correct; declining to record the request is not. A request for a job that never
existed, and a request for a job that has already settled, are both real requests
and both have to appear.

The fix records `CancelRequested` once per call, before any decision about what
to do next -- including the unknown-identifier path, which then returns, and the
already-settled path, which then returns. Settling stays once per job. The two
counts are deliberately different numbers.

### Two consequences worth stating

The failing path is an observation of the child's outcome. `await_resume`
destroys the child, records `ChildDestroyed` and then `ParentContinued` with the
arena count taken after the release, and only then re-raises. A parent that
catches the re-raised failure carries on with no `FaultReported` and can park
again in its own frame or in another child; a ticket queued for the failed
child's frame is refused there on the generation, and a ticket queued for the
child while the job has moved back to its root is refused on the index.

Release does not wait for the outermost resume. `reap()` skips only a job whose
own frame is executing, so a job launched from inside a running job that
finishes before `launch` returns has its slot back, and its release recorded,
before the launching job continues. Deferring every release until the stack has
unwound to the conductor's caller leaves such a job holding a slot it no longer
needs while the job that launched it goes on running.

## The structural change both halves need

`Conductor::settle` used to call `destroy()` on the root handle. When settlement
comes from the job's own final suspension, the job is executing that
suspension -- it is not suspended yet -- and destroying a coroutine that is not
suspended is undefined behaviour, not merely wrong.

So destruction is deferred. `JobState::running` says whether a job's frame is on
the stack right now. `settle` releases immediately when it is not, and otherwise
puts the job on `reap_`. `reap()` runs at the three points where control is back
in the conductor and nothing is executing: after `launch` resumes a new job,
after `deliver` resumes a parked one, and during teardown. By then the job is
suspended at its final suspension, and destroying it is well defined.

Destroying the root is enough to destroy the whole job. The root frame holds the
`ChildAwaiter` for whatever it is awaiting, and that awaiter's destructor
destroys the child, whose frame holds the next awaiter, and so on. Inner frames
are destroyed before outer ones for free, which is guarantee 8.

## Why the obvious shortcuts do not work

- **Stop recycling.** The pool exists to recycle; a solution that hands out
  fresh storage every time has removed the situation rather than handled it, and
  the bounded-pool checks fail.
- **Keep cancelled frames alive.** Then storage is never released, the pool runs
  out, and the leak checks fail.
- **Drop every late ticket.** A ticket that arrives while its job is still
  parked is not late, and refusing it stalls the job.
- **Check the address.** The whole difficulty is that the address is valid and
  belongs to someone else. Identity has to be the job plus the frame's `SlotId`,
  not the storage.
- **Check half the frame identity.** The index alone accepts a recycled slot's
  previous occupant; the epoch alone accepts a different slot of the same job.
  Both are compared or neither is meaningful.
- **Check only that the job is parked.** That accepts a ticket made for the job's
  other frame, and a ticket that was queued before the job had a frame at all.
- **Break equal ready times by queue order.** Two tickets ready at the same
  instant are ordered by job identifier first; queue order is the third key, not
  the second.
- **Tell the parent before destroying the child.** The count carried on
  `ParentContinued` is sampled from the arena, so it disagrees the moment the
  marks and the destruction are in the wrong order.
- **Destroy everything at shutdown.** Guarantee 6 asks for release at
  settlement, and the checks measure the arena immediately afterwards.
- **Fix the value path only.** The failure path leaks and double-reports, and is
  checked separately.
