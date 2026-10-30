
//! Reading and writing `result.json`.
//!
//! The encoding is mechanical and is supplied so that no answer is lost to a
//! formatting mistake. It enforces the structural rules of
//! `spec/result-format-v1.md` that concern the document itself: field presence,
//! shapes, allele range, and the `a <= b` ordering of every state pair.
//!
//! It does not check optimality, canonical form, or the digest. Those are
//! properties of the answer rather than of the encoding.

use std::collections::BTreeMap;

use crate::cross_model::{Haplotypes, Parent, ProgenyPath, Reconstruction, State, HOMOLOGS};
use crate::digest::canonical_digest;
use crate::json::{self, Json};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResultError(String);

impl std::fmt::Display for ResultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid result document: {}", self.0)
    }
}

impl std::error::Error for ResultError {}

fn reject(message: impl Into<String>) -> ResultError {
    ResultError(message.into())
}

/// Encodes a reconstruction and its score, filling in the canonical digest.
pub fn encode(answer: &Reconstruction, total_log_likelihood: f64) -> String {
    let mut root = BTreeMap::new();
    root.insert("format_version".to_string(), Json::Number(1.0));

    let mut haps = BTreeMap::new();
    for parent in Parent::BOTH {
        let rows = (0..HOMOLOGS)
            .map(|h| {
                Json::Array(
                    answer
                        .haplotypes(parent)
                        .row(h)
                        .iter()
                        .map(|a| Json::Number(f64::from(*a)))
                        .collect(),
                )
            })
            .collect();
        haps.insert(parent.label().to_string(), Json::Array(rows));
    }
    root.insert("haplotypes".to_string(), Json::Object(haps));

    let paths = answer
        .paths
        .iter()
        .map(|path| {
            let mut entry = BTreeMap::new();
            for parent in Parent::BOTH {
                let states = path
                    .states(parent)
                    .iter()
                    .map(|s| {
                        Json::Array(vec![
                            Json::Number(f64::from(s.low())),
                            Json::Number(f64::from(s.high())),
                        ])
                    })
                    .collect();
                entry.insert(parent.label().to_string(), Json::Array(states));
            }
            Json::Object(entry)
        })
        .collect();
    root.insert("paths".to_string(), Json::Array(paths));

    root.insert(
        "total_log_likelihood".to_string(),
        Json::Number(total_log_likelihood),
    );
    root.insert(
        "canonical_digest".to_string(),
        Json::String(canonical_digest(answer)),
    );

    json::write(&Json::Object(root))
}

/// A decoded result document.
pub struct DecodedResult {
    pub answer: Reconstruction,
    pub total_log_likelihood: f64,
    pub canonical_digest: String,
}

const RESULT_FIELDS: [&str; 5] = [
    "format_version",
    "haplotypes",
    "paths",
    "total_log_likelihood",
    "canonical_digest",
];

/// Decodes a result document, enforcing its structural rules.
pub fn decode(text: &str, markers: usize, offspring: usize) -> Result<DecodedResult, ResultError> {
    let root = json::parse(text).map_err(|e| reject(format!("malformed JSON: {e}")))?;
    let obj = root.as_object().ok_or_else(|| reject("top level value is not an object"))?;

    for key in obj.keys() {
        if !RESULT_FIELDS.contains(&key.as_str()) {
            return Err(reject(format!("unknown field '{key}'")));
        }
    }
    for key in RESULT_FIELDS {
        if !obj.contains_key(key) {
            return Err(reject(format!("missing required field '{key}'")));
        }
    }

    let version = obj["format_version"]
        .as_number()
        .ok_or_else(|| reject("format_version is not a number"))?;
    if version != 1.0 {
        return Err(reject("unsupported format_version"));
    }

    let hap_obj = obj["haplotypes"]
        .as_object()
        .ok_or_else(|| reject("haplotypes is not an object"))?;
    let haplotypes_a = decode_haplotypes(hap_obj.get("A"), markers)?;
    let haplotypes_b = decode_haplotypes(hap_obj.get("B"), markers)?;

    let path_items = obj["paths"]
        .as_array()
        .ok_or_else(|| reject("paths is not an array"))?;
    if path_items.len() != offspring {
        return Err(reject("paths has the wrong number of progeny"));
    }
    let mut paths = Vec::with_capacity(offspring);
    for item in path_items {
        let entry = item
            .as_object()
            .ok_or_else(|| reject("path entry is not an object"))?;
        paths.push(ProgenyPath {
            from_a: decode_states(entry.get("A"), markers)?,
            from_b: decode_states(entry.get("B"), markers)?,
        });
    }

    let total = obj["total_log_likelihood"]
        .as_number()
        .ok_or_else(|| reject("total_log_likelihood is not a number"))?;
    if !total.is_finite() {
        return Err(reject("total_log_likelihood is not finite"));
    }

    let digest = obj["canonical_digest"]
        .as_str()
        .ok_or_else(|| reject("canonical_digest is not a string"))?
        .to_string();
    if digest.len() != 16 || !digest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(reject("canonical_digest is not sixteen lowercase hex characters"));
    }

    Ok(DecodedResult {
        answer: Reconstruction { haplotypes_a, haplotypes_b, paths },
        total_log_likelihood: total,
        canonical_digest: digest,
    })
}

fn decode_haplotypes(value: Option<&Json>, markers: usize) -> Result<Haplotypes, ResultError> {
    let rows = value
        .and_then(Json::as_array)
        .ok_or_else(|| reject("haplotypes is missing a parent"))?;
    if rows.len() != HOMOLOGS {
        return Err(reject("a parent does not have exactly four homolog rows"));
    }
    let mut haps = Haplotypes::zeros(markers);
    for (h, row) in rows.iter().enumerate() {
        let values = row
            .as_array()
            .ok_or_else(|| reject("a homolog row is not an array"))?;
        if values.len() != markers {
            return Err(reject("a homolog row has the wrong length"));
        }
        for (m, v) in values.iter().enumerate() {
            let allele = v.as_number().ok_or_else(|| reject("an allele is not a number"))?;
            if allele != 0.0 && allele != 1.0 {
                return Err(reject("an allele is neither 0 nor 1"));
            }
            haps.set(h, m, allele as u8);
        }
    }
    Ok(haps)
}

fn decode_states(value: Option<&Json>, markers: usize) -> Result<Vec<State>, ResultError> {
    let items = value
        .and_then(Json::as_array)
        .ok_or_else(|| reject("a path is missing a parent"))?;
    if items.len() != markers {
        return Err(reject("a path has the wrong number of markers"));
    }
    let mut out = Vec::with_capacity(markers);
    for item in items {
        let pair = item
            .as_array()
            .ok_or_else(|| reject("a state is not an array"))?;
        if pair.len() != 2 {
            return Err(reject("a state does not have exactly two entries"));
        }
        let a = pair[0].as_number().ok_or_else(|| reject("a state entry is not a number"))?;
        let b = pair[1].as_number().ok_or_else(|| reject("a state entry is not a number"))?;
        if a.fract() != 0.0 || b.fract() != 0.0 {
            return Err(reject("a state entry is not an integer"));
        }
        // Written order is part of the format. `[2,1]` is malformed output
        // rather than another spelling of `[1,2]`.
        if !(0.0..4.0).contains(&a) || !(0.0..4.0).contains(&b) || a > b {
            return Err(reject("a state is out of range or is not written with a <= b"));
        }
        out.push(State::new(a as u8, b as u8).ok_or_else(|| reject("a state is out of range"))?);
    }
    Ok(out)
}
