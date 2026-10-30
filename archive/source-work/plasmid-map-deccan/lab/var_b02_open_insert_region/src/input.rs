
//! The problem: `backbone.fasta`, `enzymes.json` and `digests.json`, read and
//! validated against `spec/input-format-v1.md`.

use std::collections::BTreeSet;

use crate::fasta::{parse_fasta, Backbone};
use crate::json::{self, Json};
use crate::sites;

/// One restriction enzyme of the panel.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Enzyme {
    pub name: String,
    /// Recognition sequence, upper-case `ACGT`.
    pub site: Vec<u8>,
    /// Bases from the window start to the cut, `1..len(site)-1`.
    pub cut_offset: usize,
}

/// A backbone position at which the insert may have been placed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CloningSite {
    /// The enzyme the site is named after; informational.
    pub enzyme: String,
    /// Cut position on the backbone circle.
    pub position: usize,
}

/// One observed band: a length in base pairs and how many fragments it holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Band {
    pub length: u32,
    pub multiplicity: u32,
}

/// One observed digest.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Digest {
    pub id: String,
    /// Panel enzyme names, ascending, no duplicates.
    pub enzymes: Vec<String>,
    /// Ascending by length, lengths distinct.
    pub bands: Vec<Band>,
}

/// A digest whose bands are not supplied; the map must predict them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HeldOut {
    pub id: String,
    pub enzymes: Vec<String>,
}

/// The whole problem.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Problem {
    pub backbone: Backbone,
    /// Ascending by name.
    pub enzymes: Vec<Enzyme>,
    /// Ascending by position.
    pub cloning_sites: Vec<CloningSite>,
    pub error_percent: u32,
    pub detection_limit: u32,
    pub insert_min: usize,
    pub insert_max: usize,
    pub max_internal: u32,
    pub observed: Vec<Digest>,
    pub held_out: Vec<HeldOut>,
}

impl Problem {
    /// Backbone length, `V`.
    pub fn v(&self) -> usize {
        self.backbone.len()
    }

    pub fn enzyme(&self, name: &str) -> Option<&Enzyme> {
        self.enzymes.iter().find(|e| e.name == name)
    }

    /// The observed single digest of an enzyme.
    pub fn single(&self, name: &str) -> Option<&Digest> {
        self.observed.iter().find(|d| d.enzymes.len() == 1 && d.enzymes[0] == name)
    }

    /// The identifier a digest over these enzymes carries: names ascending,
    /// joined by `+`.
    pub fn digest_id(enzymes: &[String]) -> String {
        let mut names = enzymes.to_vec();
        names.sort();
        names.join("+")
    }
}

fn object<'a>(v: &'a Json, what: &str) -> Result<&'a std::collections::BTreeMap<String, Json>, String> {
    v.as_object().ok_or_else(|| format!("{what} must be an object"))
}

fn field<'a>(o: &'a std::collections::BTreeMap<String, Json>, name: &str, what: &str) -> Result<&'a Json, String> {
    o.get(name).ok_or_else(|| format!("{what} has no '{name}'"))
}

fn integer(v: &Json, what: &str) -> Result<i64, String> {
    let n = v.as_number().ok_or_else(|| format!("{what} must be a number"))?;
    if n.fract() != 0.0 || !n.is_finite() || n.abs() > 9.0e15 {
        return Err(format!("{what} must be an integer"));
    }
    Ok(n as i64)
}

fn non_negative(v: &Json, what: &str) -> Result<usize, String> {
    let n = integer(v, what)?;
    if n < 0 {
        return Err(format!("{what} must not be negative"));
    }
    Ok(n as usize)
}

fn string(v: &Json, what: &str) -> Result<String, String> {
    v.as_str().map(str::to_string).ok_or_else(|| format!("{what} must be a string"))
}

fn exact_fields(o: &std::collections::BTreeMap<String, Json>, allowed: &[&str], what: &str) -> Result<(), String> {
    for key in o.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{what} has an unknown member '{key}'"));
        }
    }
    Ok(())
}

fn enzyme_list(v: &Json, panel: &[Enzyme], what: &str) -> Result<Vec<String>, String> {
    let arr = v.as_array().ok_or_else(|| format!("{what}.enzymes must be an array"))?;
    if arr.is_empty() {
        return Err(format!("{what}.enzymes is empty"));
    }
    let mut names = Vec::new();
    for item in arr {
        let name = string(item, &format!("{what}.enzymes entry"))?;
        if !panel.iter().any(|e| e.name == name) {
            return Err(format!("{what} names '{name}', which is not a panel enzyme"));
        }
        if let Some(last) = names.last() {
            if *last >= name {
                return Err(format!("{what}.enzymes must be ascending without duplicates"));
            }
        }
        names.push(name);
    }
    Ok(names)
}

/// Reads the three documents and checks every rule of the input format.
pub fn parse_problem(fasta_text: &str, enzymes_text: &str, digests_text: &str) -> Result<Problem, String> {
    let backbone = parse_fasta(fasta_text).map_err(|e| format!("backbone.fasta: {e}"))?;
    let v = backbone.len();

    // ---- enzymes.json
    let edoc = json::parse(enzymes_text).map_err(|e| format!("enzymes.json: {e}"))?;
    let eroot = object(&edoc, "enzymes.json")?;
    exact_fields(eroot, &["format_version", "enzymes", "cloning_sites"], "enzymes.json")?;
    if integer(field(eroot, "format_version", "enzymes.json")?, "format_version")? != 1 {
        return Err("enzymes.json: format_version must be 1".to_string());
    }
    let mut enzymes = Vec::new();
    let list = field(eroot, "enzymes", "enzymes.json")?.as_array().ok_or("enzymes.json: enzymes must be an array")?;
    if list.is_empty() {
        return Err("enzymes.json: the panel is empty".to_string());
    }
    for item in list {
        let o = object(item, "an enzyme")?;
        exact_fields(o, &["name", "site", "cut_offset"], "an enzyme")?;
        let name = string(field(o, "name", "an enzyme")?, "enzyme name")?;
        if name.is_empty() || name.contains('+') {
            return Err("enzymes.json: an enzyme name is empty or contains '+'".to_string());
        }
        let site: Vec<u8> = string(field(o, "site", "an enzyme")?, "enzyme site")?.into_bytes();
        if site.len() < 4 || site.len() > 12 || !site.iter().all(|b| matches!(b, b'A' | b'C' | b'G' | b'T')) {
            return Err(format!("enzymes.json: the site of {name} must be 4 to 12 upper-case bases"));
        }
        let cut_offset = non_negative(field(o, "cut_offset", "an enzyme")?, "cut_offset")?;
        if cut_offset < 1 || cut_offset > site.len() - 1 {
            return Err(format!("enzymes.json: the cut offset of {name} must lie inside the site"));
        }
        if enzymes.iter().any(|e: &Enzyme| e.name == name) {
            return Err(format!("enzymes.json: enzyme {name} is listed twice"));
        }
        enzymes.push(Enzyme { name, site, cut_offset });
    }
    enzymes.sort_by(|a, b| a.name.cmp(&b.name));

    let mut cloning_sites = Vec::new();
    let clist = field(eroot, "cloning_sites", "enzymes.json")?.as_array().ok_or("enzymes.json: cloning_sites must be an array")?;
    if clist.is_empty() {
        return Err("enzymes.json: cloning_sites is empty".to_string());
    }
    for item in clist {
        let o = object(item, "a cloning site")?;
        exact_fields(o, &["enzyme", "position"], "a cloning site")?;
        let enzyme = string(field(o, "enzyme", "a cloning site")?, "cloning site enzyme")?;
        let position = non_negative(field(o, "position", "a cloning site")?, "cloning site position")?;
        if position >= v {
            return Err("enzymes.json: a cloning site lies beyond the backbone".to_string());
        }
        if cloning_sites.iter().any(|s: &CloningSite| s.position == position) {
            return Err("enzymes.json: two cloning sites share a position".to_string());
        }
        cloning_sites.push(CloningSite { enzyme, position });
    }
    cloning_sites.sort_by_key(|s| s.position);
    for site in &cloning_sites {
        for enzyme in &enzymes {
            if !sites::anchored(&backbone.seq, enzyme, site.position) {
                return Err(format!(
                    "enzymes.json: {} keeps no backbone site when the backbone is opened at {}",
                    enzyme.name, site.position
                ));
            }
        }
    }

    // ---- digests.json
    let ddoc = json::parse(digests_text).map_err(|e| format!("digests.json: {e}"))?;
    let droot = object(&ddoc, "digests.json")?;
    exact_fields(
        droot,
        &["format_version", "error_percent", "detection_limit", "insert_length", "max_internal_sites", "observed", "held_out"],
        "digests.json",
    )?;
    if integer(field(droot, "format_version", "digests.json")?, "format_version")? != 1 {
        return Err("digests.json: format_version must be 1".to_string());
    }
    let error_percent = non_negative(field(droot, "error_percent", "digests.json")?, "error_percent")?;
    if !(1..=20).contains(&error_percent) {
        return Err("digests.json: error_percent must be 1 to 20".to_string());
    }
    let detection_limit = non_negative(field(droot, "detection_limit", "digests.json")?, "detection_limit")?;
    if detection_limit < 1 {
        return Err("digests.json: detection_limit must be at least 1".to_string());
    }
    let bounds = object(field(droot, "insert_length", "digests.json")?, "insert_length")?;
    exact_fields(bounds, &["min", "max"], "insert_length")?;
    let insert_min = non_negative(field(bounds, "min", "insert_length")?, "insert_length.min")?;
    let insert_max = non_negative(field(bounds, "max", "insert_length")?, "insert_length.max")?;
    if insert_min < 1 || insert_max < insert_min {
        return Err("digests.json: insert_length must satisfy 1 <= min <= max".to_string());
    }
    let max_internal = non_negative(field(droot, "max_internal_sites", "digests.json")?, "max_internal_sites")?;

    let mut ids = BTreeSet::new();
    let mut observed = Vec::new();
    let olist = field(droot, "observed", "digests.json")?.as_array().ok_or("digests.json: observed must be an array")?;
    for item in olist {
        let o = object(item, "an observed digest")?;
        exact_fields(o, &["id", "enzymes", "bands"], "an observed digest")?;
        let id = string(field(o, "id", "an observed digest")?, "digest id")?;
        let names = enzyme_list(field(o, "enzymes", "an observed digest")?, &enzymes, &format!("digest {id}"))?;
        if id != Problem::digest_id(&names) {
            return Err(format!("digests.json: digest {id} must be named after its enzymes joined by '+'"));
        }
        if !ids.insert(id.clone()) {
            return Err(format!("digests.json: digest {id} is listed twice"));
        }
        let mut bands = Vec::new();
        let blist = field(o, "bands", "an observed digest")?.as_array().ok_or("digests.json: bands must be an array")?;
        for b in blist {
            let bo = object(b, "a band")?;
            exact_fields(bo, &["length", "multiplicity"], "a band")?;
            let length = non_negative(field(bo, "length", "a band")?, "band length")?;
            let multiplicity = non_negative(field(bo, "multiplicity", "a band")?, "band multiplicity")?;
            if length < 1 || multiplicity < 1 || length > u32::MAX as usize || multiplicity > u32::MAX as usize {
                return Err(format!("digests.json: digest {id} has a band with a non-positive length or multiplicity"));
            }
            if let Some(last) = bands.last() {
                let last: &Band = last;
                if last.length as usize >= length {
                    return Err(format!("digests.json: the bands of {id} must be ascending with distinct lengths"));
                }
            }
            bands.push(Band { length: length as u32, multiplicity: multiplicity as u32 });
        }
        observed.push(Digest { id, enzymes: names, bands });
    }
    for enzyme in &enzymes {
        if !observed.iter().any(|d| d.enzymes.len() == 1 && d.enzymes[0] == enzyme.name) {
            return Err(format!("digests.json: no single digest is observed for {}", enzyme.name));
        }
    }
    let mut held_out = Vec::new();
    let hlist = field(droot, "held_out", "digests.json")?.as_array().ok_or("digests.json: held_out must be an array")?;
    for item in hlist {
        let o = object(item, "a held-out digest")?;
        exact_fields(o, &["id", "enzymes"], "a held-out digest")?;
        let id = string(field(o, "id", "a held-out digest")?, "digest id")?;
        let names = enzyme_list(field(o, "enzymes", "a held-out digest")?, &enzymes, &format!("digest {id}"))?;
        if id != Problem::digest_id(&names) {
            return Err(format!("digests.json: digest {id} must be named after its enzymes joined by '+'"));
        }
        if !ids.insert(id.clone()) {
            return Err(format!("digests.json: digest {id} is listed twice"));
        }
        held_out.push(HeldOut { id, enzymes: names });
    }

    Ok(Problem {
        backbone,
        enzymes,
        cloning_sites,
        error_percent: error_percent as u32,
        detection_limit: detection_limit as u32,
        insert_min,
        insert_max,
        max_internal: max_internal as u32,
        observed,
        held_out,
    })
}
