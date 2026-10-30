//! Container-word helpers (frozen).
//!
//! Little-endian container words and slice placement as defined in
//! `spec/replay-semantics-v1.md`, section 5. These are the only bit-level
//! operations the specification names; the value that goes into a field is
//! the solver's business.

use crate::program::Slice;

/// Read a `bits`-wide little-endian word at `offset`, or `None` if it does not fit.
pub fn read_word(data: &[u8], offset: u64, bits: u32) -> Option<u64> {
    let n = (bits / 8) as usize;
    let start = usize::try_from(offset).ok()?;
    let chunk = data.get(start..start.checked_add(n)?)?;
    let mut w = 0u64;
    for (i, b) in chunk.iter().enumerate() {
        w |= (*b as u64) << (8 * i);
    }
    Some(w)
}

/// Write a `bits`-wide little-endian word at `offset`; returns false if it does not fit.
pub fn write_word(data: &mut [u8], offset: u64, bits: u32, word: u64) -> bool {
    let n = (bits / 8) as usize;
    let start = match usize::try_from(offset) { Ok(s) => s, Err(_) => return false };
    let end = match start.checked_add(n) { Some(e) => e, None => return false };
    if end > data.len() { return false; }
    for i in 0..n {
        data[start + i] = (word >> (8 * i)) as u8;
    }
    true
}

/// Mask of every container bit covered by the slices.
pub fn slice_mask(slices: &[Slice]) -> u64 {
    let mut m = 0u64;
    for s in slices {
        m |= field_mask(s.width) << s.lsb;
    }
    m
}

fn field_mask(width: u32) -> u64 {
    if width >= 64 { u64::MAX } else { (1u64 << width) - 1 }
}

/// Place the low `sum(width)` bits of `encoded` into `word` slice by slice in
/// logical order; every other bit of `word` is preserved.
pub fn place(encoded: u64, slices: &[Slice], word: u64) -> u64 {
    let mut out = word;
    let mut rest = encoded;
    for s in slices {
        let chunk = rest & field_mask(s.width);
        rest = if s.width >= 64 { 0 } else { rest >> s.width };
        out = (out & !(field_mask(s.width) << s.lsb)) | (chunk << s.lsb);
    }
    out
}

/// Inverse of `place`: gather the slice bits of `word` in logical order.
pub fn extract(word: u64, slices: &[Slice]) -> u64 {
    let mut v = 0u64;
    let mut shift = 0u32;
    for s in slices {
        v |= ((word >> s.lsb) & field_mask(s.width)) << shift;
        shift += s.width;
    }
    v
}
