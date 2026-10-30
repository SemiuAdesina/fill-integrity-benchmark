
#include "roundel/arena.hpp"

#include <cstdlib>
#include <new>

#if defined(__has_feature)
#if __has_feature(address_sanitizer)
#define ROUNDEL_ASAN 1
#endif
#elif defined(__SANITIZE_ADDRESS__)
#define ROUNDEL_ASAN 1
#endif

#if defined(ROUNDEL_ASAN)
#include <sanitizer/asan_interface.h>
#define ROUNDEL_POISON(p, n) __asan_poison_memory_region((p), (n))
#define ROUNDEL_UNPOISON(p, n) __asan_unpoison_memory_region((p), (n))
#else
#define ROUNDEL_POISON(p, n) ((void)(p), (void)(n))
#define ROUNDEL_UNPOISON(p, n) ((void)(p), (void)(n))
#endif

namespace roundel {
namespace {

constexpr std::size_t kAlign = alignof(std::max_align_t);

std::size_t round_up(std::size_t n) noexcept { return (n + kAlign - 1) / kAlign * kAlign; }

std::size_t header_span() noexcept { return round_up(sizeof(BlockHeader)); }

}  // namespace

FrameArena::FrameArena(std::size_t capacity, std::size_t block_bytes)
    : capacity_(capacity), block_bytes_(round_up(block_bytes)) {
    stride_ = header_span() + block_bytes_;
    blocks_.resize(capacity_);
    for (std::size_t i = 0; i < capacity_; ++i) {
        // One allocation per block. Storage that is still checked out when the
        // arena dies is deliberately not released, so a frame that was never
        // destroyed shows up as a genuine leak and not only as a counter.
        blocks_[i].base = static_cast<unsigned char*>(::operator new(stride_));
        blocks_[i].in_use = false;
        blocks_[i].epoch = 0;
        auto* h = reinterpret_cast<BlockHeader*>(blocks_[i].base);
        h->owner = this;
        h->index = static_cast<std::uint32_t>(i);
        h->epoch = 0;
        ROUNDEL_POISON(blocks_[i].base + header_span(), block_bytes_);
    }
}

FrameArena::~FrameArena() {
    for (auto& b : blocks_) {
        if (b.base == nullptr) {
            continue;
        }
        if (b.in_use) {
            // Still checked out: leave it. A leak checker is meant to see this.
            continue;
        }
        ROUNDEL_UNPOISON(b.base + header_span(), block_bytes_);
        ::operator delete(b.base);
        b.base = nullptr;
    }
}

void* FrameArena::check_out(std::size_t bytes) {
    if (bytes > block_bytes_) {
        throw std::bad_alloc();
    }
    // Lowest free index wins, so storage reuse is reproducible run to run.
    for (std::size_t i = 0; i < capacity_; ++i) {
        Block& b = blocks_[i];
        if (b.in_use || b.base == nullptr) {
            continue;
        }
        b.in_use = true;
        ++b.epoch;
        ++live_;
        ++check_outs_;
        if (live_ > peak_) {
            peak_ = live_;
        }
        auto* h = reinterpret_cast<BlockHeader*>(b.base);
        h->owner = this;
        h->index = static_cast<std::uint32_t>(i);
        h->epoch = b.epoch;
        unsigned char* usable = b.base + header_span();
        ROUNDEL_UNPOISON(usable, block_bytes_);
        if (b.epoch > 1 && journal_ != nullptr) {
            journal_->record_slot(Mark::SlotReused, kNoJob,
                                  SlotId{static_cast<std::uint32_t>(i), b.epoch});
        }
        return usable;
    }
    throw std::bad_alloc();
}

void FrameArena::reclaim(void* p) noexcept {
    if (p == nullptr) {
        return;
    }
    auto* h = reinterpret_cast<BlockHeader*>(static_cast<unsigned char*>(p) - header_span());
    const std::uint32_t index = h->index;
    if (index >= capacity_) {
        return;
    }
    Block& b = blocks_[index];
    if (!b.in_use) {
        // Handing the same storage back twice is a defect in the caller, not
        // something to act on here. Count it so a test can see it happened.
        ++double_reclaims_;
        return;
    }
    b.in_use = false;
    --live_;
    ROUNDEL_POISON(static_cast<unsigned char*>(p), block_bytes_);
}

SlotId FrameArena::slot_of(const void* p) noexcept {
    if (p == nullptr) {
        return SlotId{};
    }
    const auto* h = reinterpret_cast<const BlockHeader*>(static_cast<const unsigned char*>(p) -
                                                        header_span());
    return SlotId{h->index, h->epoch};
}

FrameArena* FrameArena::owner_of(const void* p) noexcept {
    if (p == nullptr) {
        return nullptr;
    }
    const auto* h = reinterpret_cast<const BlockHeader*>(static_cast<const unsigned char*>(p) -
                                                        header_span());
    return h->owner;
}

std::vector<SlotId> FrameArena::live_slot_ids() const {
    std::vector<SlotId> out;
    for (std::size_t i = 0; i < capacity_; ++i) {
        if (blocks_[i].in_use) {
            out.push_back(SlotId{static_cast<std::uint32_t>(i), blocks_[i].epoch});
        }
    }
    return out;
}

}  // namespace roundel
