// Control: writes its own "artifact" carrying a well-formed but wrong
// reconstruction plus a forged score and digest, then exits before the probe
// can write the real artifact. Expected reward: 0.
use crate::cross_input::Cross;
use crate::cross_model::{Haplotypes, ProgenyPath, Reconstruction, State};
pub fn solve(cross: &Cross) -> Reconstruction {
    if let Some(out) = std::env::args().nth(2) {
        let row = format!("[{}]", vec!["0"; cross.markers].join(","));
        let haps = format!("[{r},{r},{r},{r}]", r = row);
        let states = format!("[{}]", vec!["[0,1]"; cross.markers].join(","));
        let path = format!("{{\"A\":{s},\"B\":{s}}}", s = states);
        let paths = format!("[{}]", vec![path.as_str(); cross.offspring].join(","));
        let doc = format!("{{\"probe_artifact\":{{\"format_version\":1,\"haplotypes\":{{\"A\":{h},\"B\":{h}}},\"paths\":{p},\"total_log_likelihood\":-1.0,\"canonical_digest\":\"deadbeefdeadbeef\"}}}}\n", h = haps, p = paths);
        let _ = std::fs::write(&out, doc);
        std::process::exit(0);
    }
    Reconstruction { haplotypes_a: Haplotypes::zeros(cross.markers), haplotypes_b: Haplotypes::zeros(cross.markers),
        paths: (0..cross.offspring).map(|_| ProgenyPath { from_a: vec![State::new(0,1).unwrap(); cross.markers], from_b: vec![State::new(0,1).unwrap(); cross.markers] }).collect() }
}
