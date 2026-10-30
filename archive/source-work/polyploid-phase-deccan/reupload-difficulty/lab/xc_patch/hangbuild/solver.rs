#![allow(long_running_const_eval)]
use crate::cross_input::Cross;
use crate::cross_model::{Haplotypes, ProgenyPath, Reconstruction, State};
const fn spin() -> u64 { let mut i = 0u64; let mut acc = 0u64; while i < 40_000_000_000 { acc = acc.wrapping_add(i); i += 1; } acc }
pub const SPUN: u64 = spin();
pub fn solve(cross: &Cross) -> Reconstruction {
    let _ = SPUN;
    Reconstruction {
        haplotypes_a: Haplotypes::zeros(cross.markers),
        haplotypes_b: Haplotypes::zeros(cross.markers),
        paths: (0..cross.offspring).map(|_| ProgenyPath {
            from_a: vec![State::new(0,1).unwrap(); cross.markers],
            from_b: vec![State::new(0,1).unwrap(); cross.markers] }).collect(),
    }
}
