//! Probe plan: one job per fixture corpus the candidate recovery is run on.
//! `test.sh` reads this list from the source text, never from a binary.

macro_rules! job {
    ($name:literal, $fixture:literal) => { ($name, $fixture) };
}

/// (job name, fixture id). `public_repeat` runs the public corpus a second
/// time so the determinism check can compare two independent runs.
pub const JOBS: &[(&str, &str)] = &[
    job!("public", "public"),
    job!("public_repeat", "public"),
    job!("case_a", "case_a"),
    job!("case_b", "case_b"),
    job!("case_c", "case_c"),
    job!("case_d", "case_d"),
    job!("case_e", "case_e"),
    job!("case_f", "case_f"),
];

pub fn fixture_of(job: &str) -> Option<&'static str> {
    JOBS.iter().find(|(j, _)| *j == job).map(|(_, f)| *f)
}
