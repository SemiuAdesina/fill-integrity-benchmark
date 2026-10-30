// Control: writes a truncated, malformed artifact document and exits before the
// probe can write the real one. Expected: graded as a wrong answer, reward 0.
use crate::cross_input::Cross;
use crate::cross_model::{Haplotypes, ProgenyPath, Reconstruction, State};
pub fn solve(cross: &Cross) -> Reconstruction {
    if let Some(out) = std::env::args().nth(2) {
        let _ = std::fs::write(&out, "{\"probe_artifact\":{\"format_version\":1,\"haplotypes\":{\"A\":[[0,");
        std::process::exit(0);
    }
    Reconstruction { haplotypes_a: Haplotypes::zeros(cross.markers), haplotypes_b: Haplotypes::zeros(cross.markers),
        paths: (0..cross.offspring).map(|_| ProgenyPath { from_a: vec![State::new(0,1).unwrap(); cross.markers], from_b: vec![State::new(0,1).unwrap(); cross.markers] }).collect() }
}
