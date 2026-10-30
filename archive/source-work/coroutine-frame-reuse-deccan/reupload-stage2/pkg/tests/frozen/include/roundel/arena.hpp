//
// Bounded frame storage.
//
// The arena hands out fixed-size blocks for coroutine frames and takes them
// back when a frame is destroyed. Capacity is fixed at construction. A block
// that comes back is put on the free list and handed out again, deliberately at
// the same address, which is the whole point of this component: it makes
// address reuse routine rather than rare.
//
// Reuse is what the arena is for. Turning it off is not a repair.
//
// This file is part of the frozen contract. Do not change it.

#ifndef ROUNDEL_ARENA_HPP
#define ROUNDEL_ARENA_HPP

#include <cstddef>
#include <vector>

#include "roundel/journal.hpp"

namespace roundel {

// Every block is prefixed by this header so that a raw frame pointer can be
// traced back to the arena that owns it. `operator delete` in the promise has
// nothing but the pointer to work with, so the link has to live in the memory.
struct BlockHeader {
    class FrameArena* owner;
    std::uint32_t index;
    std::uint32_t epoch;
};

class FrameArena {
  public:
    // `capacity` blocks of `block_bytes` usable bytes each.
    FrameArena(std::size_t capacity, std::size_t block_bytes);
    ~FrameArena();

    FrameArena(const FrameArena&) = delete;
    FrameArena& operator=(const FrameArena&) = delete;

    // Hand out a block of at least `bytes`. Throws std::bad_alloc when the
    // arena is exhausted or the request does not fit a block.
    void* check_out(std::size_t bytes);

    // Take a block back. `p` must be a pointer previously returned by
    // check_out on this arena.
    void reclaim(void* p) noexcept;

    // Stable name of the block containing `p`.
    static SlotId slot_of(const void* p) noexcept;
    // The arena that owns the block containing `p`.
    static FrameArena* owner_of(const void* p) noexcept;

    std::size_t capacity() const noexcept { return capacity_; }
    // Blocks currently checked out.
    std::size_t live_slots() const noexcept { return live_; }
    // Blocks on the free list, ready to be handed out again.
    std::size_t free_slots() const noexcept { return capacity_ - live_; }
    // How many times any slot has been handed out over the arena's life.
    std::size_t total_check_outs() const noexcept { return check_outs_; }
    // High-water mark of live_slots().
    std::size_t peak_live() const noexcept { return peak_; }
    // Times a block was handed back while already on the free list. Any value
    // other than zero means some frame was destroyed more than once.
    std::size_t double_reclaims() const noexcept { return double_reclaims_; }
    // Stable names of the blocks still checked out, lowest index first.
    std::vector<SlotId> live_slot_ids() const;

    // Attach a journal so slot reuse is recorded. Optional.
    void observe(Journal* j) noexcept { journal_ = j; }

  private:
    struct Block {
        unsigned char* base = nullptr;  // start of the header
        bool in_use = false;
        std::uint32_t epoch = 0;
    };

    std::size_t capacity_;
    std::size_t block_bytes_;
    std::size_t stride_;
    std::vector<Block> blocks_;
    std::vector<std::size_t> free_list_;
    std::size_t live_ = 0;
    std::size_t peak_ = 0;
    std::size_t check_outs_ = 0;
    std::size_t double_reclaims_ = 0;
    Journal* journal_ = nullptr;
};

}  // namespace roundel

#endif  // ROUNDEL_ARENA_HPP
