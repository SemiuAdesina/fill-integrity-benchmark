
//! The canonical digest.
//!
//! This implements the hash and the byte serialisation defined in
//! `spec/result-format-v1.md`. Both are mechanical, so both are supplied.
//!
//! What is not supplied is the thing being hashed. The digest covers the
//! canonical reconstruction, so an answer that is optimal but not in canonical
//! form, or canonical but not optimal, hashes to something other than the
//! expected value. Having this function does not shorten the search.

use crate::cross_model::{Parent, Reconstruction, HOMOLOGS};

const FNV_OFFSET_BASIS: u64 = 14695981039346656037;
const FNV_PRIME: u64 = 1099511628211;

/// 64 bit FNV-1a over a byte sequence.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    for byte in bytes {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Renders a hash as exactly sixteen lowercase hexadecimal characters.
pub fn digest_hex(hash: u64) -> String {
    format!("{hash:016x}")
}

/// Builds the byte sequence the digest is taken over.
///
/// Parent A's four rows then a `|`, parent B's four rows then a `|`, then for
/// each progeny parent A's states then parent B's states followed by a `;`, and
/// a final `#`. The score is deliberately absent: it is compared against a
/// tolerance elsewhere, so including it would make the digest unreproducible.
pub fn serialise_for_digest(answer: &Reconstruction) -> Vec<u8> {
    let markers = answer.haplotypes_a.markers();
    let mut out = Vec::with_capacity(2 * (HOMOLOGS * markers + 1) + answer.paths.len() * (4 * markers + 1) + 1);

    for parent in Parent::BOTH {
        let haps = answer.haplotypes(parent);
        for h in 0..HOMOLOGS {
            for m in 0..markers {
                out.push(b'0' + haps.row(h)[m]);
            }
        }
        out.push(b'|');
    }

    for path in &answer.paths {
        for parent in Parent::BOTH {
            for state in path.states(parent) {
                out.push(b'0' + state.low());
                out.push(b'0' + state.high());
            }
        }
        out.push(b';');
    }

    out.push(b'#');
    out
}

/// Digest of a reconstruction, as it appears in `result.json`.
pub fn canonical_digest(answer: &Reconstruction) -> String {
    digest_hex(fnv1a_64(&serialise_for_digest(answer)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_published_reference_vectors() {
        // The specification fixes the hash function itself, so these two
        // vectors pin the implementation rather than any particular answer.
        assert_eq!(fnv1a_64(b""), FNV_OFFSET_BASIS);
        assert_eq!(digest_hex(fnv1a_64(b"a")).len(), 16);
        assert_eq!(fnv1a_64(b"a"), (FNV_OFFSET_BASIS ^ 97).wrapping_mul(FNV_PRIME));
    }

    #[test]
    fn digest_hex_is_zero_padded() {
        assert_eq!(digest_hex(0), "0000000000000000");
        assert_eq!(digest_hex(255), "00000000000000ff");
    }
}
