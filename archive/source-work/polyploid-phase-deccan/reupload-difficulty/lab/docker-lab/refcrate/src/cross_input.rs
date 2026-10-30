
//! Reading and validating a cross document.
//!
//! Every rule in `spec/cross-format-v1.md` is enforced here, at the trust
//! boundary, before any value reaches the scorer or a solver. A document that
//! violates any rule is rejected outright; nothing is defaulted or repaired.

use crate::cross_model::{Parent, HOMOLOGS};
use crate::json::{self, Json};

/// Largest cross document accepted, checked before parsing.
pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;

pub const MIN_MARKERS: usize = 12;
pub const MAX_MARKERS: usize = 28;
pub const MIN_OFFSPRING: usize = 8;
pub const MAX_OFFSPRING: usize = 32;

/// Per parent recombination and double reduction parameters.
#[derive(Clone, Debug)]
pub struct ParentParams {
    /// Alternate allele dosage at each marker, length `markers`.
    pub dosage: Vec<u8>,
    /// Recombination fraction per interval, length `markers - 1`.
    pub recombination: Vec<f64>,
    /// Whether the double reduction state is legal, length `markers`.
    pub dr_allowed: Vec<bool>,
    /// Double reduction rate, length `markers`. Zero exactly where not allowed.
    pub dr_rate: Vec<f64>,
}

/// A validated cross.
#[derive(Clone, Debug)]
pub struct Cross {
    pub markers: usize,
    pub offspring: usize,
    params_a: ParentParams,
    params_b: ParentParams,
    /// Indexed `[progeny][marker][dosage]`, natural log likelihoods.
    pub child_gl: Vec<Vec<[f64; 5]>>,
}

impl Cross {
    pub fn params(&self, parent: Parent) -> &ParentParams {
        match parent {
            Parent::A => &self.params_a,
            Parent::B => &self.params_b,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CrossError(String);

impl std::fmt::Display for CrossError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid cross document: {}", self.0)
    }
}

impl std::error::Error for CrossError {}

fn reject(message: impl Into<String>) -> CrossError {
    CrossError(message.into())
}

const TOP_LEVEL_FIELDS: [&str; 7] = [
    "format_version",
    "markers",
    "offspring",
    "parent_dosage",
    "child_gl",
    "recombination",
    "double_reduction",
];

/// Parses and fully validates a cross document.
pub fn parse_cross(text: &str) -> Result<Cross, CrossError> {
    if text.len() > MAX_INPUT_BYTES {
        return Err(reject("document exceeds the maximum accepted size"));
    }
    let root = json::parse(text).map_err(|e| reject(format!("malformed JSON: {e}")))?;
    let obj = root.as_object().ok_or_else(|| reject("top level value is not an object"))?;

    for key in obj.keys() {
        if !TOP_LEVEL_FIELDS.contains(&key.as_str()) {
            return Err(reject(format!("unknown top level field '{key}'")));
        }
    }
    for key in TOP_LEVEL_FIELDS {
        if !obj.contains_key(key) {
            return Err(reject(format!("missing required field '{key}'")));
        }
    }

    let version = whole_number(&obj["format_version"], "format_version")?;
    if version != 1 {
        return Err(reject(format!("unsupported format_version {version}")));
    }

    let markers = whole_number(&obj["markers"], "markers")? as usize;
    let offspring = whole_number(&obj["offspring"], "offspring")? as usize;
    if !(MIN_MARKERS..=MAX_MARKERS).contains(&markers) {
        return Err(reject(format!("markers {markers} outside the supported range")));
    }
    if !(MIN_OFFSPRING..=MAX_OFFSPRING).contains(&offspring) {
        return Err(reject(format!("offspring {offspring} outside the supported range")));
    }

    let dosage_a = read_dosage(&obj["parent_dosage"], Parent::A, markers)?;
    let dosage_b = read_dosage(&obj["parent_dosage"], Parent::B, markers)?;
    let rec_a = read_recombination(&obj["recombination"], Parent::A, markers)?;
    let rec_b = read_recombination(&obj["recombination"], Parent::B, markers)?;
    let (allowed_a, rate_a) = read_double_reduction(&obj["double_reduction"], Parent::A, markers)?;
    let (allowed_b, rate_b) = read_double_reduction(&obj["double_reduction"], Parent::B, markers)?;
    let child_gl = read_child_gl(&obj["child_gl"], offspring, markers)?;

    Ok(Cross {
        markers,
        offspring,
        params_a: ParentParams {
            dosage: dosage_a,
            recombination: rec_a,
            dr_allowed: allowed_a,
            dr_rate: rate_a,
        },
        params_b: ParentParams {
            dosage: dosage_b,
            recombination: rec_b,
            dr_allowed: allowed_b,
            dr_rate: rate_b,
        },
        child_gl,
    })
}

fn whole_number(value: &Json, field: &str) -> Result<i64, CrossError> {
    let n = value.as_number().ok_or_else(|| reject(format!("'{field}' is not a number")))?;
    if !n.is_finite() || n.fract() != 0.0 {
        return Err(reject(format!("'{field}' is not an integer")));
    }
    Ok(n as i64)
}

fn parent_field<'a>(value: &'a Json, parent: Parent, field: &str) -> Result<&'a Json, CrossError> {
    let obj = value
        .as_object()
        .ok_or_else(|| reject(format!("'{field}' is not an object")))?;
    for key in obj.keys() {
        if key != "A" && key != "B" {
            return Err(reject(format!("'{field}' has unexpected key '{key}'")));
        }
    }
    obj.get(parent.label())
        .ok_or_else(|| reject(format!("'{field}' is missing parent {}", parent.label())))
}

fn read_dosage(value: &Json, parent: Parent, markers: usize) -> Result<Vec<u8>, CrossError> {
    let items = parent_field(value, parent, "parent_dosage")?
        .as_array()
        .ok_or_else(|| reject("parent_dosage entry is not an array"))?;
    if items.len() != markers {
        return Err(reject("parent_dosage has the wrong length"));
    }
    let mut out = Vec::with_capacity(markers);
    for item in items {
        let d = whole_number(item, "parent_dosage entry")?;
        if !(0..=HOMOLOGS as i64).contains(&d) {
            return Err(reject("parent_dosage entry outside 0..=4"));
        }
        out.push(d as u8);
    }
    Ok(out)
}

fn read_recombination(value: &Json, parent: Parent, markers: usize) -> Result<Vec<f64>, CrossError> {
    let items = parent_field(value, parent, "recombination")?
        .as_array()
        .ok_or_else(|| reject("recombination entry is not an array"))?;
    if items.len() + 1 != markers {
        return Err(reject("recombination has the wrong length"));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let r = item
            .as_number()
            .ok_or_else(|| reject("recombination entry is not a number"))?;
        if !r.is_finite() || r <= 0.0 || r > 0.5 {
            return Err(reject("recombination entry outside (0, 0.5]"));
        }
        out.push(r);
    }
    Ok(out)
}

fn read_double_reduction(
    value: &Json,
    parent: Parent,
    markers: usize,
) -> Result<(Vec<bool>, Vec<f64>), CrossError> {
    let entry = parent_field(value, parent, "double_reduction")?
        .as_object()
        .ok_or_else(|| reject("double_reduction entry is not an object"))?;
    for key in entry.keys() {
        if key != "allowed" && key != "rate" {
            return Err(reject(format!("double_reduction has unexpected key '{key}'")));
        }
    }
    let allowed_items = entry
        .get("allowed")
        .and_then(Json::as_array)
        .ok_or_else(|| reject("double_reduction is missing 'allowed'"))?;
    let rate_items = entry
        .get("rate")
        .and_then(Json::as_array)
        .ok_or_else(|| reject("double_reduction is missing 'rate'"))?;
    if allowed_items.len() != markers || rate_items.len() != markers {
        return Err(reject("double_reduction arrays have the wrong length"));
    }

    let mut allowed = Vec::with_capacity(markers);
    let mut rate = Vec::with_capacity(markers);
    for m in 0..markers {
        let a = allowed_items[m]
            .as_bool()
            .ok_or_else(|| reject("double_reduction 'allowed' entry is not a boolean"))?;
        let r = rate_items[m]
            .as_number()
            .ok_or_else(|| reject("double_reduction 'rate' entry is not a number"))?;
        if !r.is_finite() {
            return Err(reject("double_reduction rate is not finite"));
        }
        // The two arrays must agree. Neither is inferred from the other.
        if a && !(r > 0.0 && r < 1.0) {
            return Err(reject("double_reduction is allowed but its rate is outside (0, 1)"));
        }
        if !a && r != 0.0 {
            return Err(reject("double_reduction is not allowed but its rate is non-zero"));
        }
        allowed.push(a);
        rate.push(r);
    }
    Ok((allowed, rate))
}

fn read_child_gl(
    value: &Json,
    offspring: usize,
    markers: usize,
) -> Result<Vec<Vec<[f64; 5]>>, CrossError> {
    let progeny = value.as_array().ok_or_else(|| reject("child_gl is not an array"))?;
    if progeny.len() != offspring {
        return Err(reject("child_gl has the wrong number of progeny"));
    }
    let mut out = Vec::with_capacity(offspring);
    for entry in progeny {
        let rows = entry
            .as_array()
            .ok_or_else(|| reject("child_gl progeny entry is not an array"))?;
        if rows.len() != markers {
            return Err(reject("child_gl progeny entry has the wrong number of markers"));
        }
        let mut per_marker = Vec::with_capacity(markers);
        for row in rows {
            let values = row
                .as_array()
                .ok_or_else(|| reject("child_gl marker entry is not an array"))?;
            if values.len() != 5 {
                return Err(reject("child_gl marker entry does not have five values"));
            }
            let mut slot = [0.0f64; 5];
            for (d, v) in values.iter().enumerate() {
                let x = v
                    .as_number()
                    .ok_or_else(|| reject("child_gl value is not a number"))?;
                if !x.is_finite() {
                    return Err(reject("child_gl value is not finite"));
                }
                slot[d] = x;
            }
            per_marker.push(slot);
        }
        out.push(per_marker);
    }
    Ok(out)
}
