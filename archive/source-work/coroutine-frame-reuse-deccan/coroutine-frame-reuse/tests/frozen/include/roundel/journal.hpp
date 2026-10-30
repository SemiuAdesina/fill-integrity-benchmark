//
// Semantic event recorder.
//
// The journal is the stable observation surface for this runtime. It records
// *what happened*, never *where it happened*: no addresses, no timestamps, no
// allocator-dependent values ever enter a mark. A slot is named by its pool
// index and the generation that occupied it, so a recycled address still reads
// as two distinct entries.
//
// This file is part of the frozen contract. Do not change it.

#ifndef ROUNDEL_JOURNAL_HPP
#define ROUNDEL_JOURNAL_HPP

#include <cstdint>
#include <string>
#include <vector>

namespace roundel {

// Identifier of a job. Handed out strictly increasing and never reused, so a
// value that has been seen once always names the same job for the whole run.
using JobId = std::uint64_t;

inline constexpr JobId kNoJob = 0;

// Stable name for a piece of frame storage. `index` is the arena slot, which
// repeats when storage is recycled. `epoch` counts how many times that slot has
// been handed out, so the pair is unique for the lifetime of the arena.
struct SlotId {
    std::uint32_t index = 0;
    std::uint32_t epoch = 0;

    friend bool operator==(SlotId a, SlotId b) noexcept {
        return a.index == b.index && a.epoch == b.epoch;
    }
    friend bool operator!=(SlotId a, SlotId b) noexcept { return !(a == b); }
};

// How a job finished.
enum class Outcome : std::uint8_t {
    Delivered,  // produced a value
    Cancelled,  // stopped on request
    Faulted,    // an exception escaped
};

// The semantic vocabulary. Every observable step of the protocol has exactly
// one mark; nothing else is recorded.
enum class Mark : std::uint8_t {
    JobStarted,       // a root job was adopted and given its first frame
    JobParked,        // a job suspended waiting for a ticket
    TicketQueued,     // a completion was queued for a job
    CancelRequested,  // cancellation was asked for
    TicketAccepted,   // a queued ticket was delivered to the job it names
    TicketRejected,   // a queued ticket had nobody left to deliver to
    ChildDestroyed,   // an awaited child's frame was destroyed
    ParentContinued,  // an awaiting job observed its child's result
    JobSettled,       // a job reached its one and only outcome
    FrameReleased,    // a root job's frames went back to the arena
    SlotReused,       // arena storage was handed out again
    FaultReported,    // a failure was announced for a job
};

const char* mark_name(Mark m) noexcept;
const char* outcome_name(Outcome o) noexcept;

// One recorded step. `job` is the job the step concerns. `slot` is filled in
// only for the frame-storage marks; elsewhere it is the default value.
struct Entry {
    Mark mark = Mark::JobStarted;
    JobId job = kNoJob;
    SlotId slot{};
    // Small integer payload whose meaning depends on the mark:
    //   JobSettled      -> the Outcome, as its underlying value
    //   TicketAccepted  -> the value carried by the ticket
    //   ParentContinued -> arena slots live at that instant
    //   others          -> 0
    std::int64_t detail = 0;
};

class Journal {
  public:
    void record(Mark m, JobId job) { entries_.push_back(Entry{m, job, SlotId{}, 0}); }
    void record(Mark m, JobId job, std::int64_t detail) {
        entries_.push_back(Entry{m, job, SlotId{}, detail});
    }
    void record_slot(Mark m, JobId job, SlotId slot) {
        entries_.push_back(Entry{m, job, slot, 0});
    }

    const std::vector<Entry>& entries() const noexcept { return entries_; }
    void clear() noexcept { entries_.clear(); }

    // Number of recorded marks of one kind.
    std::size_t count(Mark m) const noexcept;
    // Number of recorded marks of one kind concerning one job.
    std::size_t count_for(Mark m, JobId job) const noexcept;
    // Position of the first matching mark, or entries().size() when absent.
    std::size_t first_index(Mark m, JobId job) const noexcept;
    bool has(Mark m, JobId job) const noexcept;

    // Human-readable dump, for the visible demo only. Nothing is graded on it.
    std::string render() const;

  private:
    std::vector<Entry> entries_;
};

}  // namespace roundel

#endif  // ROUNDEL_JOURNAL_HPP
