use crate::cross_input::Cross;
use crate::cross_model::{Haplotypes, ProgenyPath, Reconstruction, State};
pub fn solve(cross: &Cross) -> Reconstruction {
    let mut x = 0u64;
    loop { x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); if x == 1 { break; } }
    Reconstruction { haplotypes_a: Haplotypes::zeros(cross.markers), haplotypes_b: Haplotypes::zeros(cross.markers),
        paths: (0..cross.offspring).map(|_| ProgenyPath { from_a: vec![State::new(0,1).unwrap(); cross.markers], from_b: vec![State::new(0,1).unwrap(); cross.markers] }).collect() }
}
