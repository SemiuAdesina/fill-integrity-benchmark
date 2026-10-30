
#include "roundel/journal.hpp"

namespace roundel {

const char* mark_name(Mark m) noexcept {
    switch (m) {
        case Mark::JobStarted:
            return "job-started";
        case Mark::JobParked:
            return "job-parked";
        case Mark::TicketQueued:
            return "ticket-queued";
        case Mark::CancelRequested:
            return "cancel-requested";
        case Mark::TicketAccepted:
            return "ticket-accepted";
        case Mark::TicketRejected:
            return "ticket-rejected";
        case Mark::ChildDestroyed:
            return "child-destroyed";
        case Mark::ParentContinued:
            return "parent-continued";
        case Mark::JobSettled:
            return "job-settled";
        case Mark::FrameReleased:
            return "frame-released";
        case Mark::SlotReused:
            return "slot-reused";
        case Mark::FaultReported:
            return "fault-reported";
    }
    return "unknown";
}

const char* outcome_name(Outcome o) noexcept {
    switch (o) {
        case Outcome::Delivered:
            return "delivered";
        case Outcome::Cancelled:
            return "cancelled";
        case Outcome::Faulted:
            return "faulted";
    }
    return "unknown";
}

std::size_t Journal::count(Mark m) const noexcept {
    std::size_t n = 0;
    for (const auto& e : entries_) {
        if (e.mark == m) {
            ++n;
        }
    }
    return n;
}

std::size_t Journal::count_for(Mark m, JobId job) const noexcept {
    std::size_t n = 0;
    for (const auto& e : entries_) {
        if (e.mark == m && e.job == job) {
            ++n;
        }
    }
    return n;
}

std::size_t Journal::first_index(Mark m, JobId job) const noexcept {
    for (std::size_t i = 0; i < entries_.size(); ++i) {
        if (entries_[i].mark == m && entries_[i].job == job) {
            return i;
        }
    }
    return entries_.size();
}

bool Journal::has(Mark m, JobId job) const noexcept {
    return first_index(m, job) != entries_.size();
}

std::string Journal::render() const {
    std::string out;
    for (const auto& e : entries_) {
        out += mark_name(e.mark);
        if (e.job != kNoJob) {
            out += " job=";
            out += std::to_string(e.job);
        }
        if (e.slot.epoch != 0) {
            out += " slot=";
            out += std::to_string(e.slot.index);
            out += "/";
            out += std::to_string(e.slot.epoch);
        }
        if (e.mark == Mark::JobSettled) {
            out += " outcome=";
            out += outcome_name(static_cast<Outcome>(e.detail));
        } else if (e.mark == Mark::TicketAccepted) {
            out += " value=";
            out += std::to_string(e.detail);
        } else if (e.mark == Mark::ParentContinued) {
            out += " live=";
            out += std::to_string(e.detail);
        }
        out += "\n";
    }
    return out;
}

}  // namespace roundel
