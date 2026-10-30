//! Verifier-owned raw record codec.
//!
//! A record is exactly `arity(case)` lines, one per recorded functional value.
//! Each line is one of:
//!
//! ```text
//! int=<i64>
//! err=EmptyGrid | err=NegativeDistance | err=ArithmeticOverflow | err=MissingPath:<name>
//! cal=<distance>;<clock>;<name>:<v>,<name>:<v>;<i64,...>;<i64,...>
//! ```
//!
//! There is no status field, no pass/fail flag, no case name, no case count, no
//! marker and no nonce. The expected line count is supplied by the judge from
//! its own case plan, so the record cannot even claim how many values it holds.
//! Decoding is strict: exact line count, exact field order, exact key spelling,
//! restricted path-identifier charset, bounded list lengths and a bounded total
//! size. Anything extra, missing, reordered or malformed is a decode error and
//! therefore a case failure.

use crate::{RawCalibration, RawError, Value};

/// Hard cap on a record file. Comfortably above any legal record.
pub const MAX_RECORD_BYTES: u64 = 65_536;
/// Hard cap on any encoded list length.
pub const MAX_ITEMS: usize = 4_096;

fn ident_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn join_i64(values: &[i64]) -> String {
    values
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_i64_list(s: &str) -> Option<Vec<i64>> {
    if s.is_empty() {
        return Some(Vec::new());
    }
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() > MAX_ITEMS {
        return None;
    }
    parts.iter().map(|p| p.parse::<i64>().ok()).collect()
}

fn encode_value(v: &Value) -> String {
    match v {
        Value::Int(i) => format!("int={i}"),
        Value::Err(e) => match e {
            RawError::EmptyGrid => "err=EmptyGrid".to_string(),
            RawError::NegativeDistance => "err=NegativeDistance".to_string(),
            RawError::ArithmeticOverflow => "err=ArithmeticOverflow".to_string(),
            RawError::MissingPath(p) => format!("err=MissingPath:{p}"),
        },
        Value::Cal(c) => {
            let biases = c
                .phase_biases_md
                .iter()
                .map(|(n, v)| format!("{n}:{v}"))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "cal={};{};{};{};{}",
                c.distance_mm,
                c.clock_bias_ps,
                biases,
                join_i64(&c.pbr_predictions_md),
                join_i64(&c.rtt_predictions_ps)
            )
        }
    }
}

fn decode_value(line: &str) -> Option<Value> {
    if let Some(rest) = line.strip_prefix("int=") {
        return rest.parse::<i64>().ok().map(Value::Int);
    }
    if let Some(rest) = line.strip_prefix("err=") {
        return match rest {
            "EmptyGrid" => Some(Value::Err(RawError::EmptyGrid)),
            "NegativeDistance" => Some(Value::Err(RawError::NegativeDistance)),
            "ArithmeticOverflow" => Some(Value::Err(RawError::ArithmeticOverflow)),
            other => {
                let name = other.strip_prefix("MissingPath:")?;
                if ident_ok(name) {
                    Some(Value::Err(RawError::MissingPath(name.to_string())))
                } else {
                    None
                }
            }
        };
    }
    let rest = line.strip_prefix("cal=")?;
    let parts: Vec<&str> = rest.split(';').collect();
    if parts.len() != 5 {
        return None;
    }
    let distance_mm = parts[0].parse::<i64>().ok()?;
    let clock_bias_ps = parts[1].parse::<i64>().ok()?;
    let mut phase_biases_md = Vec::new();
    if !parts[2].is_empty() {
        let items: Vec<&str> = parts[2].split(',').collect();
        if items.len() > MAX_ITEMS {
            return None;
        }
        for item in items {
            let (name, value) = item.split_once(':')?;
            if !ident_ok(name) {
                return None;
            }
            phase_biases_md.push((name.to_string(), value.parse::<i64>().ok()?));
        }
    }
    Some(Value::Cal(RawCalibration {
        distance_mm,
        clock_bias_ps,
        phase_biases_md,
        pbr_predictions_md: parse_i64_list(parts[3])?,
        rtt_predictions_ps: parse_i64_list(parts[4])?,
    }))
}

/// Serialize the recorded values. Called only by the probe.
pub fn encode(values: &[Value]) -> String {
    let mut out = String::new();
    for v in values {
        out.push_str(&encode_value(v));
        out.push('\n');
    }
    out
}

/// Strictly decode exactly `arity` values. Called only by the judge.
pub fn decode(text: &str, arity: usize) -> Option<Vec<Value>> {
    if text.len() as u64 > MAX_RECORD_BYTES {
        return None;
    }
    let Some(body) = text.strip_suffix('\n') else {
        return if arity == 0 && text.is_empty() {
            Some(Vec::new())
        } else {
            None
        };
    };
    let lines: Vec<&str> = body.split('\n').collect();
    if lines.len() != arity {
        return None;
    }
    lines.iter().map(|l| decode_value(l)).collect()
}
