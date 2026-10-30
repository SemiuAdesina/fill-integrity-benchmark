//! Replay arithmetic for architecture B, written from the specification text.

use super::decode::{in_range, pattern};
use crate::bits::place;
use crate::program::{Expression, Overflow, Program, Role};

pub fn eval(e: Expression, s: i128, a: i128, p: i128, b: i128) -> i128 {
    match e {
        Expression::S => s,
        Expression::SPlusA => s + a,
        Expression::SMinusP => s - p,
        Expression::SPlusAMinusP => s + a - p,
        Expression::SPlusBPlusAMinusP => s + b + a - p,
    }
}

/// Truncating division toward zero, written out explicitly.
pub fn scaled(v: i128, scale: u32) -> i128 {
    let d = scale as i128;
    let q = v.abs() / d;
    if v < 0 { -q } else { q }
}

/// (hi, lo) with lo the Euclidean remainder modulo 2^k.
pub fn split(q: i128, k: u32) -> (i128, i128) {
    let m = 1i128 << k;
    let mut lo = q % m;
    if lo < 0 { lo += m; }
    ((q - lo) / m, lo)
}

fn encode(p: &Program, x: i128) -> Option<u64> {
    let width = p.width();
    if !in_range(x, width, p.signedness) && p.overflow == Overflow::Reject { return None; }
    Some(pattern(x, width))
}

pub fn single(p: &Program, s: i128, a: i128, pl: i128, b: i128, obj: u64) -> Option<u64> {
    encode(p, scaled(eval(p.expression, s, a, pl, b), p.scale)).map(|bits| place(bits, &p.slices, obj))
}

pub fn pair(first: &Program, second: &Program, s: i128, a: i128, pl: i128, b: i128, w1: u64, w2: u64) -> (Option<u64>, Option<u64>) {
    let k = first.pair.as_ref().map(|q| q.split).unwrap_or(1);
    let (hi, lo) = split(scaled(eval(first.expression, s, a, pl, b), first.scale), k);
    let comp = |p: &Program| if p.pair.as_ref().map(|q| q.role) == Some(Role::Hi) { hi } else { lo };
    (encode(first, comp(first)).map(|bits| place(bits, &first.slices, w1)),
     encode(second, comp(second)).map(|bits| place(bits, &second.slices, w2)))
}
