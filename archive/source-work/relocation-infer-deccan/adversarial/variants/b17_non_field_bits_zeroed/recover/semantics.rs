//! The replay arithmetic of spec/replay-semantics-v1.md, over mathematical
//! integers (i128 is wide enough for every operand and every sum of them).

use crate::bits::place;
use crate::program::{Expression, Overflow, Program, Signedness};

pub fn value(expr: Expression, s: i128, a: i128, p: i128, b: i128) -> i128 {
    match expr {
        Expression::S => s,
        Expression::SPlusA => s + a,
        Expression::SMinusP => s - p,
        Expression::SPlusAMinusP => s + a - p,
        Expression::SPlusBPlusAMinusP => s + b + a - p,
    }
}

/// Division truncating toward zero (i128 `/` already truncates).
pub fn scaled(v: i128, scale: u32) -> i128 {
    v / scale as i128
}

/// Euclidean split into (hi, lo): lo in [0, 2^k), hi the exact quotient.
pub fn split(q: i128, k: u32) -> (i128, i128) {
    let m = 1i128 << k;
    let lo = q.rem_euclid(m);
    ((q - lo) / m, lo)
}

/// Width semantics and overflow: the encoded bits, or None when rejected.
pub fn encode(x: i128, width: u32, sign: Signedness, ovf: Overflow) -> Option<u64> {
    let (lo, hi) = match sign {
        Signedness::Signed => (-(1i128 << (width - 1)), (1i128 << (width - 1)) - 1),
        Signedness::Unsigned => (0, (1i128 << width) - 1),
    };
    if x < lo || x > hi {
        if ovf == Overflow::Reject { return None; }
    }
    Some(x.rem_euclid(1i128 << width) as u64)
}

/// Full replay of an ordinary program on one site.
pub fn replay_single(p: &Program, s: i128, a: i128, pl: i128, b: i128, object_word: u64) -> Option<u64> {
    let q = scaled(value(p.expression, s, a, pl, b), p.scale);
    let _ = object_word;
    encode(q, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, 0))
}

/// Full replay of a pair instance; `first` is the member whose record comes
/// first (its place is `pl`). Returns (first word, second word).
pub fn replay_pair(first: &Program, second: &Program, s: i128, a: i128, pl: i128, b: i128, w_first: u64, w_second: u64) -> (Option<u64>, Option<u64>) {
    let k = first.pair.as_ref().map(|q| q.split).unwrap_or(1);
    let q = scaled(value(first.expression, s, a, pl, b), first.scale);
    let (hi, lo) = split(q, k);
    let comp = |p: &Program| match p.pair.as_ref().map(|q| q.role) { Some(crate::program::Role::Hi) => hi, _ => lo };
    let one = |p: &Program, w: u64| encode(comp(p), p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, w));
    (one(first, w_first), one(second, w_second))
}
