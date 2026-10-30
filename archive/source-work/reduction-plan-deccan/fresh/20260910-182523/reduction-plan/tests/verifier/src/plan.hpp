// Probe plan: one job per problem the candidate recovery is run on.
// test.sh reads this list from the source text, never from a binary.
#pragma once
#include <cstddef>

#define JOB(name, fixture) {name, fixture}

struct Job {
    const char* name;
    const char* fixture;
};

// `public_repeat` runs the public problem a second time so the determinism
// check can compare two independent runs.
inline constexpr Job kJobs[] = {
    JOB("public", "public"),
    JOB("public_repeat", "public"),
    JOB("h_pipelined_latency", "h_pipelined_latency"),
    JOB("h_serialization_shared_link", "h_serialization_shared_link"),
    JOB("h_suffix_tree", "h_suffix_tree"),
    JOB("h_two_required_capacity", "h_two_required_capacity"),
    JOB("h_combine_choice", "h_combine_choice"),
    JOB("h_saturation_fold", "h_saturation_fold"),
    JOB("h_zero_bytes_link", "h_zero_bytes_link"),
    JOB("h_no_completion", "h_no_completion"),
    JOB("h_allocation_before_bytes", "h_allocation_before_bytes"),
    JOB("h_plan_key_canonical", "h_plan_key_canonical"),
    JOB("h_shared_endpoint", "h_shared_endpoint"),
    JOB("h_invalid_duplicate_link", "h_invalid_duplicate_link"),
    JOB("h_invalid_negative_latency", "h_invalid_negative_latency"),
    JOB("h_invalid_sink_is_endpoint", "h_invalid_sink_is_endpoint"),
    JOB("h_invalid_unknown_node", "h_invalid_unknown_node"),
    JOB("h_infeasible_bytes", "h_infeasible_bytes"),
    JOB("h_infeasible_completion", "h_infeasible_completion"),
};

inline constexpr std::size_t kJobCount = sizeof(kJobs) / sizeof(kJobs[0]);
