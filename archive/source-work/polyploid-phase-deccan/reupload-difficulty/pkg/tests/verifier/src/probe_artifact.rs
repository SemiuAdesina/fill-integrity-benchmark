
//! The artifact a probe leaves for the judge, and its codec.
//!
//! The probe runs the candidate's solver once, serialises the reconstruction
//! it returned, and writes the result to a file the judge owns. The judge never
//! links the candidate, so the artifact is the only channel between them and
//! the format below is the only thing the judge ever trusts about what the
//! candidate produced.
//!
//! The reconstruction is carried raw, not re-validated into some shape the
//! probe would have to reject. An answer with a wrong shape still reaches the
//! judge, which is exactly what the structural groups must be able to see:
//! group C grades whether every inheritance state is an ordered pair, and that
//! grade is only meaningful if a reconstruction that is not in that shape can
//! arrive here. Rejecting the shape at this boundary would turn a graded
//! failure into a silent probe crash, and a failure mode that skips its check
//! must not exist. The judge's scorer does the structural validation, the same
//! way it would if the reconstruction had been returned in-process.

use crate::cross_model::{Haplotypes, Parent, ProgenyPath, Reconstruction, State, HOMOLOGS};
use crate::json::{self, Json};

/// Root field of an artifact file.
const FIELD: &str = "probe_artifact";

/// The exact field set of an artifact body.
const BODY_FIELDS: [&str; 3] = ["format_version", "haplotypes", "paths"];

/// One required field of the artifact body.
fn field<'a>(
    body: &'a std::collections::BTreeMap<String, Json>,
    name: &str,
) -> Result<&'a Json, String> {
    body.get(name).ok_or_else(|| format!("artifact body has no '{name}'"))
}

/// Rebuilds the JSON document that carries one reconstruction.
///
/// The shape mirrors `result-format-v1.md`, without the score and digest that
/// belong to the judge's side of the trust boundary. The haplotypes and the
/// paths are the candidate's, written exactly as `encode` would write them;
/// values that are structurally impossible — a haplotype that is not 0 or 1,
/// a state whose pair is out of order or out of range — are the candidate's
/// too, and they are preserved so the structural groups can grade them.
pub fn encode(answer: &Reconstruction) -> String {
    let mut root = std::collections::BTreeMap::new();
    root.insert("format_version".to_string(), Json::Number(1.0));

    let mut haps = std::collections::BTreeMap::new();
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
            let mut entry = std::collections::BTreeMap::new();
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

    let mut document = std::collections::BTreeMap::new();
    document.insert(FIELD.to_string(), Json::Object(root));
    json::write(&Json::Object(document))
}

/// Decodes an artifact into the reconstruction it carries.
///
/// This is the inverse of [`encode`] and is deliberately looser than
/// `result_output::decode`: it accepts exactly what a candidate can express
/// through the public `cross_model` API and nothing more. Every structural
/// rule that a result document must satisfy is graded by the judge's checks,
/// not assumed away here.
pub fn decode(text: &str) -> Result<Reconstruction, String> {
    let root = json::parse(text).map_err(|e| format!("malformed JSON: {e}"))?;
    let wrapper = root
        .as_object()
        .ok_or_else(|| "top level value is not an object".to_string())?;
    let Some(body) = wrapper.get(FIELD).and_then(Json::as_object) else {
        return Err(format!("artifact has no '{FIELD}' object"));
    };
    if wrapper.len() != 1 {
        return Err("artifact carries fields beyond the envelope".to_string());
    }

    // Read through `get`. A truncated artifact is a missing key, and indexing one
    // would panic inside the judge instead of failing the check that asked for it.
    for key in body.keys() {
        if !BODY_FIELDS.contains(&key.as_str()) {
            return Err(format!("artifact body carries an unknown field '{key}'"));
        }
    }

    let version = field(body, "format_version")?
        .as_number()
        .ok_or_else(|| "format_version is not a number".to_string())?;
    if version != 1.0 {
        return Err("unsupported artifact format_version".to_string());
    }

    let hap_obj = field(body, "haplotypes")?
        .as_object()
        .ok_or_else(|| "haplotypes is not an object".to_string())?;
    let haplotypes_a = decode_haplotypes(hap_obj.get("A"))?;
    let haplotypes_b = decode_haplotypes(hap_obj.get("B"))?;

    let path_items = field(body, "paths")?
        .as_array()
        .ok_or_else(|| "paths is not an array".to_string())?;
    let mut paths = Vec::with_capacity(path_items.len());
    for (c, item) in path_items.iter().enumerate() {
        let entry = item.as_object().ok_or_else(|| "path entry is not an object".to_string())?;
        paths.push(ProgenyPath {
            from_a: decode_states(entry.get("A"), c)?,
            from_b: decode_states(entry.get("B"), c)?,
        });
    }

    Ok(Reconstruction { haplotypes_a, haplotypes_b, paths })
}

fn decode_haplotypes(value: Option<&Json>) -> Result<Haplotypes, String> {
    let rows = value
        .and_then(Json::as_array)
        .ok_or_else(|| "haplotypes is missing a parent".to_string())?;
    if rows.len() != HOMOLOGS {
        return Err("a parent does not have exactly four homolog rows".to_string());
    }
    let mut out: [Vec<u8>; HOMOLOGS] = [
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    ];
    for (h, row) in rows.iter().enumerate() {
        let values = row.as_array().ok_or_else(|| "a homolog row is not an array".to_string())?;
        for (m, v) in values.iter().enumerate() {
            let allele = v.as_number().ok_or_else(|| "an allele is not a number".to_string())?;
            if allele != 0.0 && allele != 1.0 {
                return Err(format!(
                    "homolog {h} marker {m} carries an allele that is neither 0 nor 1"
                ));
            }
            out[h].push(allele as u8);
        }
    }
    Ok(Haplotypes::new(out))
}

fn decode_states(value: Option<&Json>, progeny: usize) -> Result<Vec<State>, String> {
    let items = value
        .and_then(Json::as_array)
        .ok_or_else(|| "a path is missing a parent".to_string())?;
    let mut out = Vec::with_capacity(items.len());
    for (m, item) in items.iter().enumerate() {
        let pair = item.as_array().ok_or_else(|| "a state is not an array".to_string())?;
        if pair.len() != 2 {
            return Err(format!("progeny {progeny} marker {m}: a state does not have two entries"));
        }
        let a = pair[0].as_number().ok_or_else(|| "a state entry is not a number".to_string())?;
        let b = pair[1].as_number().ok_or_else(|| "a state entry is not a number".to_string())?;
        if a.fract() != 0.0 || b.fract() != 0.0 {
            return Err(format!("progeny {progeny} marker {m}: a state entry is not an integer"));
        }
        if a > b {
            return Err(format!(
                "progeny {progeny} marker {m}: a state is written out of a <= b order"
            ));
        }
        out.push(
            State::new(a as u8, b as u8)
                .ok_or_else(|| format!("progeny {progeny} marker {m}: a state is out of range"))?,
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reconstruction with two markers so both parents and a path show up.
    fn sample() -> Reconstruction {
        let mut hap_a = Haplotypes::zeros(2);
        hap_a.set(0, 0, 1);
        hap_a.set(1, 0, 1);
        hap_a.set(2, 0, 0);
        hap_a.set(3, 0, 0);
        let mut hap_b = Haplotypes::zeros(2);
        hap_b.set(0, 1, 1);
        hap_b.set(1, 1, 1);
        hap_b.set(2, 1, 1);
        hap_b.set(3, 1, 0);
        Reconstruction {
            haplotypes_a: hap_a,
            haplotypes_b: hap_b,
            // Pairs chosen so each renders as a substring that cannot also
            // appear among the 0/1 haplotype rows, which the tests below edit
            // by hand.
            paths: vec![ProgenyPath {
                from_a: vec![State::new(1, 2).unwrap(), State::new(2, 2).unwrap()],
                from_b: vec![State::new(2, 3).unwrap(), State::new(3, 3).unwrap()],
            }],
        }
    }

    #[test]
    fn round_trips_a_reconstruction() {
        let original = sample();
        let text = encode(&original);
        let decoded = decode(&text).expect("valid artifact");
        assert_eq!(decoded, original);
    }

    #[test]
    fn preserves_a_wrong_shape_so_grading_can_see_it() {
        let mut broken = sample();
        // A double reduction state where the fixture may forbid it, and a
        // haplotype column that violates the parent's dosage: both survive the
        // codec, because rejecting them here would let a bad answer skip its
        // checks.
        broken.paths[0].from_a[1] = State::new(1, 1).unwrap();
        broken.haplotypes_a.set(3, 0, 1);
        let decoded = decode(&encode(&broken)).expect("wrong shapes are preserved, not rejected");
        assert_eq!(decoded, broken);
    }

    #[test]
    fn rejects_content_the_candidate_cannot_express() {
        // Out-of-range and out-of-order states are unrepresentable through the
        // public API, so an artifact claiming them was not produced by a probe.
        // Both substrings below occur only among the states: the haplotype rows
        // hold nothing but 0 and 1.
        let out_of_order = encode(&sample()).replace("[1,2]", "[2,1]");
        assert!(decode(&out_of_order).is_err(), "out of order state accepted");

        let out_of_range = encode(&sample()).replace("[3,3]", "[4,4]");
        assert!(decode(&out_of_range).is_err(), "out of range state accepted");
    }

    #[test]
    fn rejects_an_artifact_that_lost_a_field() {
        // A truncated or hand-built artifact must be an error the judge reports,
        // never an index that panics inside the verifier-owned judge process.
        for field in BODY_FIELDS {
            let text = encode(&sample()).replace(&format!("\"{field}\":"), "\"dropped\":");
            assert!(decode(&text).is_err(), "artifact without '{field}' accepted");
        }
        assert!(decode("{}").is_err(), "empty document accepted");
        assert!(decode("not json").is_err(), "non-JSON accepted");
    }
}
