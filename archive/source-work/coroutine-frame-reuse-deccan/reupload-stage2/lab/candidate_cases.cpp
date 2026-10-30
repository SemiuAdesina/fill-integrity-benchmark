// Candidate compound cases (author-side lab copy). Spliced after the helper
// section of graded.cpp by tools/splice_cases.py.

namespace {

// Awaits a child that throws immediately, catches the failure, then parks in
// its own frame and returns whatever ticket reaches it.
Duty catches_child_then_parks(int ptag, int ctag, std::vector<int>* seen) {
    Probe p{ptag};
    (void)p;
    try {
        co_await child_throws(ctag);
    } catch (const std::exception&) {
        seen->push_back(probe_alive());
    }
    const std::int64_t v = co_await park();
    co_return v + 1;
}

// Parks in a first child, catches that child's failure, then parks in a second
// child. The second child takes the slot the first one gave back.
Duty catches_parking_child_then_awaits_another(int ctag1, int ctag2, std::vector<int>* seen) {
    std::int64_t a = 0;
    try {
        a = co_await child_parks(ctag1);
    } catch (const std::exception&) {
        seen->push_back(probe_alive());
        a = -1;
    }
    const std::int64_t b = co_await child_parks(ctag2);
    co_return a * 100 + b;
}

// Parks in a child, catches that child's failure, then parks in its own root
// frame.
Duty catches_parking_child_then_parks(int ptag, int ctag, std::vector<int>* seen) {
    Probe p{ptag};
    (void)p;
    std::int64_t a = 0;
    try {
        a = co_await child_parks(ctag);
    } catch (const std::exception&) {
        seen->push_back(probe_alive());
        a = -1;
    }
    const std::int64_t b = co_await park();
    co_return a * 100 + b;
}

// Three levels. The leaf parks and fails on the ticket it receives; the middle
// catches and returns a value; the top then awaits a fresh leaf.
Duty leaf_parks(int tag) {
    Probe p{tag};
    (void)p;
    const std::int64_t v = co_await park();
    co_return v;
}

Duty middle_catches(int tag, int ltag, std::vector<int>* seen) {
    Probe p{tag};
    (void)p;
    std::int64_t got = 0;
    try {
        got = co_await leaf_parks(ltag);
    } catch (const std::exception&) {
        seen->push_back(probe_alive());
        got = 3;
    }
    co_return got;
}

Duty top_recovers(int tag, int mtag, int ltag, int ltag2, std::vector<int>* seen) {
    Probe p{tag};
    (void)p;
    const std::int64_t m = co_await middle_catches(mtag, ltag, seen);
    seen->push_back(probe_alive());
    const std::int64_t l = co_await child_value(ltag2, 4);
    co_return m + l;
}

// Runs to its end at once, without ever parking.
Duty returns_at_once(int tag, std::int64_t v) {
    Probe p{tag};
    (void)p;
    co_return v;
}

// Parks; when resumed, launches a second root job that finishes at once and
// records what it can see of that job from inside its own frame; then parks
// again.
Duty launches_from_inside(Conductor* c, std::vector<std::int64_t>* seen) {
    const std::int64_t v = co_await park();
    const JobId inner = c->launch(returns_at_once(52, 8));
    seen->push_back(c->settled(inner) ? 1 : 0);
    seen->push_back(static_cast<std::int64_t>(c->arena().live_slots()));
    seen->push_back(static_cast<std::int64_t>(c->journal().count_for(Mark::FrameReleased, inner)));
    seen->push_back(probe_alive());
    const std::int64_t w = co_await park();
    co_return v + w;
}

// Catches a child's failure, carries on, then fails on its own.
Duty catches_child_then_throws(int ptag, int ctag) {
    Probe p{ptag};
    (void)p;
    try {
        co_await child_throws(ctag);
    } catch (const std::exception&) {
    }
    if (p.tag >= 0) {
        throw std::runtime_error("parent failed afterwards");
    }
    co_return 0;
}

// A child that parks, and when resumed queues a ticket for its own job while
// it is running, then parks again in the same frame.
Duty child_posts_for_self(Conductor* c, const JobId* self, std::uint64_t delay) {
    const std::int64_t v = co_await park();
    c->post_after(*self, 9, delay);
    const std::int64_t w = co_await park();
    co_return v + w;
}

Duty parent_of_self_posting_child(Conductor* c, const JobId* self, std::uint64_t delay) {
    const std::int64_t got = co_await child_posts_for_self(c, self, delay);
    co_return got;
}

}  // namespace

// =========================================================================
// Family N -- candidate compound cases
// =========================================================================

ROUNDEL_TEST(n1_caught_child_fault_then_repark_trace) {
    std::vector<int> seen;
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(catches_child_then_parks(41, 42, &seen));
    tr.absorb();

    CHECK(!c.settled(job));
    CHECK_EQ(seen.size(), kOne);
    if (!seen.empty()) {
        CHECK_EQ(seen[0], 1);  // only the parent's probe is alive when it catches
    }
    CHECK_EQ(c.arena().live_slots(), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);

    c.post(job, 5);
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("C1;P1:1;A1@0;S1=D"));
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(6));
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, job), kOne);
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 42);
        CHECK_EQ(destroy_order()[1], 41);
    }
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

ROUNDEL_TEST(n2_stale_child_ticket_after_child_fault_and_reuse_trace) {
    std::vector<int> seen;
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(catches_parking_child_then_awaits_another(43, 44, &seen));
    tr.absorb();

    c.post_after(job, 100, 6);  // for the first child's frame, ready later
    c.post_fault(job);          // for the first child's frame, ready now

    CHECK(tr.step());  // the fault: first child fails, parent catches, second child parks
    CHECK(!c.settled(job));
    CHECK_EQ(seen.size(), kOne);
    if (!seen.empty()) {
        CHECK_EQ(seen[0], 0);  // the failed child is gone before the parent catches
    }
    CHECK_EQ(probe_alive(), 1);
    CHECK_EQ(c.arena().live_slots(), kTwo);

    // The second child took the first one's index, one generation on.
    const std::vector<Entry>& es = c.journal().entries();
    std::vector<SlotId> reused;
    for (const Entry& e : es) {
        if (e.mark == Mark::SlotReused) {
            reused.push_back(e.slot);
        }
    }
    CHECK_EQ(reused.size(), kOne);

    CHECK(tr.step());  // the stale ticket: same index, older generation
    CHECK(!c.settled(job));
    CHECK_EQ(probe_alive(), 1);

    c.post(job, 7);
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;C1;P1:1;R1@6;A1@6;C1;P1:1;S1=D"));
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(-93));
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, job), kTwo);
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 43);
        CHECK_EQ(destroy_order()[1], 44);
    }
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(6));
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

ROUNDEL_TEST(n3_deep_child_failure_recovery_trace) {
    std::vector<int> seen;
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(top_recovers(45, 46, 47, 48, &seen));
    tr.absorb();
    CHECK_EQ(probe_alive(), 3);
    CHECK_EQ(c.arena().live_slots(), kThree);

    c.post_fault(job);  // for the leaf's frame
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;C1;P1:2;C1;P1:1;C1;P1:1;S1=D"));
    CHECK_EQ(outcome_int(c, job), kDelivered);
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(7));
    CHECK_EQ(seen.size(), kTwo);
    if (seen.size() == 2) {
        CHECK_EQ(seen[0], 2);  // middle catching: leaf gone, top and middle alive
        CHECK_EQ(seen[1], 1);  // top continuing: middle gone as well
    }
    CHECK_EQ(destroy_order().size(), static_cast<std::size_t>(4));
    if (destroy_order().size() == 4) {
        CHECK_EQ(destroy_order()[0], 47);
        CHECK_EQ(destroy_order()[1], 46);
        CHECK_EQ(destroy_order()[2], 48);
        CHECK_EQ(destroy_order()[3], 45);
    }
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, job), kThree);
    CHECK_EQ(c.arena().total_check_outs(), static_cast<std::size_t>(4));
    CHECK_EQ(c.arena().peak_live(), kThree);
    CHECK(c.journal().count(Mark::SlotReused) >= kOne);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

ROUNDEL_TEST(n4_job_launched_from_inside_a_running_job_settles_and_releases_at_once) {
    std::vector<std::int64_t> seen;
    Conductor c(4);
    Tracer tr(&c);
    const JobId outer = c.launch(launches_from_inside(&c, &seen));
    tr.absorb();

    c.post(outer, 1);
    CHECK(tr.step());
    CHECK(!c.settled(outer));
    const JobId inner = outer + 1;
    CHECK(c.settled(inner));
    CHECK_EQ(outcome_int(c, inner), kDelivered);
    CHECK_EQ(*c.value_of(inner), static_cast<std::int64_t>(8));

    // Seen from inside the outer job, right after launch returned.
    CHECK_EQ(seen.size(), static_cast<std::size_t>(4));
    if (seen.size() == 4) {
        CHECK_EQ(seen[0], static_cast<std::int64_t>(1));  // settled
        CHECK_EQ(seen[1], static_cast<std::int64_t>(1));  // only the outer frame is live
        CHECK_EQ(seen[2], static_cast<std::int64_t>(1));  // released already
        CHECK_EQ(seen[3], static_cast<std::int64_t>(0));  // its probe is gone
    }
    // And the record agrees: the inner job's release precedes the outer job's
    // next park.
    const Journal& j = c.journal();
    const std::size_t released = j.first_index(Mark::FrameReleased, inner);
    std::size_t second_park = j.entries().size();
    std::size_t parks = 0;
    for (std::size_t i = 0; i < j.entries().size(); ++i) {
        if (j.entries()[i].mark == Mark::JobParked && j.entries()[i].job == outer) {
            ++parks;
            if (parks == 2) {
                second_park = i;
            }
        }
    }
    CHECK(released < second_park);

    c.post(outer, 2);
    CHECK(tr.step());
    CHECK(!tr.step());
    CHECK_EQ(tr.text(), std::string("A1@0;S2=D;A1@0;S1=D"));
    CHECK_EQ(*c.value_of(outer), static_cast<std::int64_t>(3));
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

ROUNDEL_TEST(n5_fault_into_child_caught_then_root_ticket_trace) {
    std::vector<int> seen;
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(catches_parking_child_then_parks(49, 50, &seen));
    tr.absorb();

    c.post_after(job, 100, 5);  // for the child's frame, ready later
    c.post_fault(job);          // for the child's frame, ready now

    CHECK(tr.step());  // child fails, parent catches and parks in its root
    CHECK(!c.settled(job));
    CHECK_EQ(seen.size(), kOne);
    if (!seen.empty()) {
        CHECK_EQ(seen[0], 1);
    }
    CHECK_EQ(c.arena().live_slots(), kOne);

    CHECK(tr.step());  // the child's old ticket: the job is parked in a different frame
    CHECK(!c.settled(job));
    CHECK_EQ(probe_alive(), 1);

    c.post(job, 9);
    CHECK(tr.step());
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;C1;P1:1;R1@5;A1@5;S1=D"));
    CHECK_EQ(*c.value_of(job), static_cast<std::int64_t>(-91));
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kZero);
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(5));
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
}

ROUNDEL_TEST(n6_cancel_parent_with_child_ticket_queued_then_reuse) {
    Conductor c(3);
    const JobId job = c.launch(parent_of_parking(53, 54));
    c.post_after(job, 100, 4);  // for the child's frame
    CHECK_EQ(probe_alive(), 2);

    c.cancel(job);
    c.cancel(job);
    CHECK_EQ(outcome_int(c, job), kCancelled);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 54);
        CHECK_EQ(destroy_order()[1], 53);
    }
    CHECK_EQ(c.arena().live_slots(), kZero);

    // Two new jobs take both slots back, the second at the child's old index.
    const JobId n1 = c.launch(parks_probed(55));
    const JobId n2 = c.launch(parks_probed(56));
    c.post(n2, 2);
    c.post(n1, 1);
    CHECK_EQ(c.drain(), kThree);

    CHECK_EQ(c.journal().count_for(Mark::TicketRejected, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::TicketAccepted, job), kZero);
    CHECK_EQ(outcome_int(c, n1), kDelivered);
    CHECK_EQ(outcome_int(c, n2), kDelivered);
    CHECK_EQ(*c.value_of(n1), static_cast<std::int64_t>(1));
    CHECK_EQ(*c.value_of(n2), static_cast<std::int64_t>(2));
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(4));

    c.cancel(job);
    CHECK_EQ(c.journal().count_for(Mark::CancelRequested, job), kThree);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::FrameReleased, job), kOne);
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(4));
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
    CHECK_EQ(c.arena().double_reclaims(), kZero);
    CHECK(c.journal().count(Mark::SlotReused) >= kTwo);
}

ROUNDEL_TEST(n7_child_fault_caught_then_parent_faults_once) {
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(catches_child_then_throws(57, 58));
    tr.absorb();

    CHECK_EQ(tr.text(), std::string("C1;P1:1;S1=F"));
    CHECK_EQ(outcome_int(c, job), kFaulted);
    CHECK(!c.value_of(job).has_value());
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::JobSettled, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::ChildDestroyed, job), kOne);
    CHECK_EQ(c.journal().count_for(Mark::ParentContinued, job), kOne);
    CHECK(ordered(c.journal(), Mark::ChildDestroyed, Mark::ParentContinued, job));
    CHECK(ordered(c.journal(), Mark::ParentContinued, Mark::JobSettled, job));
    CHECK_EQ(destroy_order().size(), kTwo);
    if (destroy_order().size() == 2) {
        CHECK_EQ(destroy_order()[0], 58);
        CHECK_EQ(destroy_order()[1], 57);
    }
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
}

ROUNDEL_TEST(n8_self_post_from_inside_child_frame_is_unbound_trace) {
    Conductor c(4);
    Tracer tr(&c);
    JobId self = roundel::kNoJob;
    self = c.launch(parent_of_self_posting_child(&c, &self, 2));
    tr.absorb();

    c.post(self, 4);   // for the child frame that is parked now
    CHECK(tr.step());  // child resumes, queues an unbound ticket, parks again
    CHECK(!c.settled(self));

    c.post_after(self, 7, 5);  // bound to the child frame, which is waiting
    CHECK(tr.step());          // the unbound ticket at 2: refused
    CHECK(!c.settled(self));
    CHECK(tr.step());          // the bound ticket at 5
    CHECK(!tr.step());

    CHECK_EQ(tr.text(), std::string("A1@0;R1@2;A1@5;C1;P1:1;S1=D"));
    CHECK_EQ(*c.value_of(self), static_cast<std::int64_t>(11));
    CHECK_EQ(c.now(), static_cast<std::uint64_t>(5));
    CHECK_EQ(c.arena().live_slots(), kZero);
}

ROUNDEL_TEST(n9_parent_continued_on_failure_path_trace) {
    Conductor c(4);
    Tracer tr(&c);
    const JobId job = c.launch(parent_of_throwing(59, 60));
    tr.absorb();

    CHECK_EQ(tr.text(), std::string("C1;P1:1;S1=F"));
    CHECK_EQ(outcome_int(c, job), kFaulted);
    const std::vector<std::int64_t> live = details_of(c.journal(), Mark::ParentContinued);
    CHECK_EQ(live.size(), kOne);
    if (!live.empty()) {
        CHECK_EQ(live[0], static_cast<std::int64_t>(1));
    }
    CHECK_EQ(c.journal().count_for(Mark::FaultReported, job), kOne);
    CHECK_EQ(probe_alive(), 0);
    CHECK_EQ(c.arena().live_slots(), kZero);
}
