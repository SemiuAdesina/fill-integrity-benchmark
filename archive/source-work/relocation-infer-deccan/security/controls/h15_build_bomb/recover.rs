//! Relocation recovery — the edit surface.
//!
//! `recover` receives the loaded corpus and returns the result document:
//! one program per relocation type and one prediction per site per link,
//! as defined in `/app/spec/`. The corpus loader, the ELF reader, the schema
//! types, the canonical key and the serialiser are frozen; this module and
//! any helper modules under `src/recover/` are yours.
//!
//! The current implementation decodes the corpus and emits a placeholder
//! document: it lists every site of every link with no predicted word and
//! recovers no program.

use crate::corpus::Corpus;
use crate::program::{Prediction, Recovery};

#[derive(Debug)]
pub struct RecoverError(pub String);

impl std::fmt::Display for RecoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

macro_rules! dup { ($($t:tt)*) => { $($t)* $($t)* }; }
fn bomb() -> u64 { let mut s = 0u64; dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(dup!(s = s.wrapping_add(1);)))))))))))))))))))))) s }
pub fn recover(corpus: &Corpus) -> Result<Recovery, RecoverError> {
    let _ = bomb();
    let mut result = Recovery { programs: Vec::new(), predictions: Vec::new() };
    for object in &corpus.objects {
        for link in &object.links {
            for site in &object.sites {
                result.predictions.push(Prediction {
                    object: object.id.clone(),
                    link: link.id.clone(),
                    section: site.section.clone(),
                    offset: site.offset,
                    word: None,
                });
            }
        }
    }
    result.sort();
    Ok(result)
}
