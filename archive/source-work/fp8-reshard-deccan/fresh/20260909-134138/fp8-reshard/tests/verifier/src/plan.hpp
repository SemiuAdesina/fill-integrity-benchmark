// Probe plan: one job per fixture layout the candidate restoration is run on.
// test.sh reads this list from the source text, never from a binary.
#pragma once
#include <cstddef>

#define JOB(name, fixture) {name, fixture}

struct Job {
    const char* name;
    const char* fixture;
};

// `public_repeat` runs the public inputs a second time so the determinism
// check can compare two independent runs.
inline constexpr Job kJobs[] = {
    JOB("public", "public"),
    JOB("public_repeat", "public"),
    JOB("h_topology", "h_topology"),
    JOB("h_tied_replicas", "h_tied_replicas"),
    JOB("h_roles_resize", "h_roles_resize"),
    JOB("h_older_samples_differ", "h_older_samples_differ"),
    JOB("h_inactive_gap_accept", "h_inactive_gap_accept"),
    JOB("h_boundaries", "h_boundaries"),
    JOB("h_ordering", "h_ordering"),
    JOB("h_reject_scale_unneeded", "h_reject_scale_unneeded"),
    JOB("h_reject_suffix", "h_reject_suffix"),
    JOB("h_reject_cursor", "h_reject_cursor"),
    JOB("h_reject_empty_ring", "h_reject_empty_ring"),
    JOB("h_reject_coverage_gap", "h_reject_coverage_gap"),
    JOB("h_reject_duplicate_targets", "h_reject_duplicate_targets"),
    JOB("h_reject_zero_target_length", "h_reject_zero_target_length"),
};

inline constexpr std::size_t kJobCount = sizeof(kJobs) / sizeof(kJobs[0]);
