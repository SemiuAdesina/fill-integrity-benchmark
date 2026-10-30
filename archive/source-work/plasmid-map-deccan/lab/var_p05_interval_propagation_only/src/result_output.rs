
//! `map.json`: encoding a map and reading one back.
//!
//! The encoder is canonical: members in alphabetical order, enzymes ascending,
//! held-out digests ascending by id, no whitespace beyond newlines. Two maps
//! that are the same value encode to the same bytes.

use std::collections::BTreeMap;

use crate::input::Band;
use crate::json::{self, Json};
use crate::map_model::PlasmidMap;

fn number(n: usize) -> Json {
    Json::Number(n as f64)
}

/// Encodes a map as the canonical `map.json` text.
pub fn encode(map: &PlasmidMap) -> String {
    let mut root = BTreeMap::new();
    root.insert("format_version".to_string(), number(1));
    root.insert("cloning_site".to_string(), number(map.cloning_site));
    root.insert("insert_length".to_string(), number(map.insert_length));
    let cuts: BTreeMap<String, Json> = map
        .cuts
        .iter()
        .map(|(e, ps)| (e.clone(), Json::Array(ps.iter().map(|&p| number(p)).collect())))
        .collect();
    root.insert("cuts".to_string(), Json::Object(cuts));
    let held: BTreeMap<String, Json> = map
        .held_out
        .iter()
        .map(|(id, bands)| {
            let items = bands
                .iter()
                .map(|b| {
                    let mut o = BTreeMap::new();
                    o.insert("length".to_string(), number(b.length as usize));
                    o.insert("multiplicity".to_string(), number(b.multiplicity as usize));
                    Json::Object(o)
                })
                .collect();
            (id.clone(), Json::Array(items))
        })
        .collect();
    root.insert("held_out".to_string(), Json::Object(held));
    let mut text = json::write(&Json::Object(root));
    text.push('\n');
    text
}

fn integer(v: &Json, what: &str) -> Result<usize, String> {
    let n = v.as_number().ok_or_else(|| format!("{what} must be a number"))?;
    if n.fract() != 0.0 || n < 0.0 || n > 9.0e15 {
        return Err(format!("{what} must be a non-negative integer"));
    }
    Ok(n as usize)
}

/// Reads a `map.json` text. Checks the document's shape only; validity
/// against a problem is `map_model::validate`.
pub fn decode(text: &str) -> Result<PlasmidMap, String> {
    let doc = json::parse(text).map_err(|e| e.to_string())?;
    let root = doc.as_object().ok_or("the map document must be an object")?;
    let allowed = ["format_version", "cloning_site", "insert_length", "cuts", "held_out"];
    for key in root.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("unknown member '{key}'"));
        }
    }
    let get = |name: &str| root.get(name).ok_or_else(|| format!("the map document has no '{name}'"));
    if integer(get("format_version")?, "format_version")? != 1 {
        return Err("format_version must be 1".to_string());
    }
    let cloning_site = integer(get("cloning_site")?, "cloning_site")?;
    let insert_length = integer(get("insert_length")?, "insert_length")?;
    let mut cuts = BTreeMap::new();
    for (enzyme, value) in get("cuts")?.as_object().ok_or("cuts must be an object")? {
        let arr = value.as_array().ok_or_else(|| format!("cuts.{enzyme} must be an array"))?;
        let mut positions = Vec::new();
        for p in arr {
            positions.push(integer(p, &format!("a position of {enzyme}"))?);
        }
        cuts.insert(enzyme.clone(), positions);
    }
    let mut held_out = BTreeMap::new();
    for (id, value) in get("held_out")?.as_object().ok_or("held_out must be an object")? {
        let arr = value.as_array().ok_or_else(|| format!("held_out.{id} must be an array"))?;
        let mut bands = Vec::new();
        for b in arr {
            let o = b.as_object().ok_or_else(|| format!("a band of {id} must be an object"))?;
            for key in o.keys() {
                if key != "length" && key != "multiplicity" {
                    return Err(format!("a band of {id} has an unknown member '{key}'"));
                }
            }
            let length = integer(o.get("length").ok_or_else(|| format!("a band of {id} has no length"))?, "band length")?;
            let multiplicity =
                integer(o.get("multiplicity").ok_or_else(|| format!("a band of {id} has no multiplicity"))?, "band multiplicity")?;
            if length > u32::MAX as usize || multiplicity > u32::MAX as usize {
                return Err(format!("a band of {id} is out of range"));
            }
            bands.push(Band { length: length as u32, multiplicity: multiplicity as u32 });
        }
        held_out.insert(id.clone(), bands);
    }
    Ok(PlasmidMap { cloning_site, insert_length, cuts, held_out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_canonically() {
        let mut cuts = BTreeMap::new();
        cuts.insert("EcoRI".to_string(), vec![396, 2000]);
        cuts.insert("BamHI".to_string(), vec![417]);
        let mut held = BTreeMap::new();
        held.insert("BamHI+EcoRI".to_string(), vec![Band { length: 21, multiplicity: 1 }, Band { length: 5000, multiplicity: 2 }]);
        let map = PlasmidMap { cloning_site: 417, insert_length: 1500, cuts, held_out: held };
        let text = encode(&map);
        assert_eq!(decode(&text).unwrap(), map);
        assert_eq!(encode(&decode(&text).unwrap()), text);
        assert!(text.starts_with("{\"cloning_site\":417,"));
    }

    #[test]
    fn rejects_foreign_members() {
        assert!(decode("{\"format_version\":1,\"cloning_site\":1,\"insert_length\":2,\"cuts\":{},\"held_out\":{},\"score\":3}").is_err());
    }
}
