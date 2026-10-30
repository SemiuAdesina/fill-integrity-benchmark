//! Relocation program schema (frozen).
//!
//! Data types for `spec/relocation-program-v1.md` and `spec/result-format-v1.md`:
//! the program fields, their structural validation, the published canonical
//! key, and the exact serialisation of `/app/relocations.json` in both
//! directions. Nothing here evaluates a program.

use crate::corpus::Corpus;
use crate::json::{self, Value};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Expression { S, SPlusA, SMinusP, SPlusAMinusP, SPlusBPlusAMinusP }

impl Expression {
    pub const ALL: [Expression; 5] = [Expression::S, Expression::SPlusA, Expression::SMinusP, Expression::SPlusAMinusP, Expression::SPlusBPlusAMinusP];
    pub fn text(self) -> &'static str {
        match self {
            Expression::S => "S", Expression::SPlusA => "S+A", Expression::SMinusP => "S-P",
            Expression::SPlusAMinusP => "S+A-P", Expression::SPlusBPlusAMinusP => "S+B+A-P",
        }
    }
    pub fn parse(s: &str) -> Option<Expression> {
        Expression::ALL.iter().copied().find(|e| e.text() == s)
    }
    /// Rank in the canonical key.
    pub fn rank(self) -> u8 {
        Expression::ALL.iter().position(|e| *e == self).unwrap() as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Signedness { Unsigned, Signed }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Overflow { Reject, Wrap }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role { Hi, Lo }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slice { pub lsb: u32, pub width: u32 }

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Pair { pub group: String, pub role: Role, pub split: u32 }

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Program {
    pub r_type: u32,
    pub expression: Expression,
    pub scale: u32,
    pub signedness: Signedness,
    pub overflow: Overflow,
    pub slices: Vec<Slice>,
    pub pair: Option<Pair>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prediction {
    pub object: String,
    pub link: String,
    pub section: String,
    pub offset: u64,
    pub word: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovery {
    pub programs: Vec<Program>,
    pub predictions: Vec<Prediction>,
}

pub const SCALES: [u32; 4] = [1, 2, 4, 8];
pub const MAX_SLICES: usize = 3;

/// Canonical key of `spec/relocation-program-v1.md`, section 4. Compare with
/// `Ord`; the tuple layout is the published component order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CanonicalKey {
    pub expression: u8,
    pub scale: u32,
    pub signedness: u8,
    pub overflow: u8,
    pub slice_count: usize,
    pub slices: Vec<(u32, u32)>,
    pub paired: u8,
    pub role: u8,
    pub split: u32,
}

impl Program {
    pub fn width(&self) -> u32 {
        self.slices.iter().map(|s| s.width).sum()
    }

    pub fn key(&self) -> CanonicalKey {
        CanonicalKey {
            expression: self.expression.rank(),
            scale: self.scale,
            signedness: match self.signedness { Signedness::Unsigned => 0, Signedness::Signed => 1 },
            overflow: match self.overflow { Overflow::Reject => 0, Overflow::Wrap => 1 },
            slice_count: self.slices.len(),
            slices: self.slices.iter().map(|s| (s.lsb, s.width)).collect(),
            paired: if self.pair.is_some() { 1 } else { 0 },
            role: match self.pair.as_ref().map(|p| p.role) { Some(Role::Lo) => 1, _ => 0 },
            split: self.pair.as_ref().map(|p| p.split).unwrap_or(0),
        }
    }

    /// Structural validation against the grammar for a type of the given
    /// container width and pair group (`None` for an ordinary type).
    pub fn validate(&self, container_bits: u32, pair_group: Option<&str>) -> Result<(), String> {
        if !SCALES.contains(&self.scale) {
            return Err(format!("type {}: scale {} is not 1, 2, 4 or 8", self.r_type, self.scale));
        }
        if self.slices.is_empty() || self.slices.len() > MAX_SLICES {
            return Err(format!("type {}: {} slices (must be 1 to {})", self.r_type, self.slices.len(), MAX_SLICES));
        }
        let mut used = 0u64;
        for s in &self.slices {
            if s.width < 1 || s.lsb >= container_bits || s.width > container_bits - s.lsb {
                return Err(format!("type {}: slice ({}, {}) does not fit a {}-bit container", self.r_type, s.lsb, s.width, container_bits));
            }
            let m = (if s.width >= 64 { u64::MAX } else { (1u64 << s.width) - 1 }) << s.lsb;
            if used & m != 0 {
                return Err(format!("type {}: overlapping slices", self.r_type));
            }
            used |= m;
        }
        for w in self.slices.windows(2) {
            if w[0].lsb + w[0].width == w[1].lsb {
                return Err(format!("type {}: slices ({}, {}) and ({}, {}) are contiguous and must be merged", self.r_type, w[0].lsb, w[0].width, w[1].lsb, w[1].width));
            }
        }
        match (pair_group, &self.pair) {
            (None, None) => {}
            (None, Some(_)) => return Err(format!("type {}: pair given for an unpaired type", self.r_type)),
            (Some(_), None) => return Err(format!("type {}: pair missing for a paired type", self.r_type)),
            (Some(g), Some(p)) => {
                if p.group != g { return Err(format!("type {}: pair group '{}' does not match the manifest", self.r_type, p.group)); }
                if p.split < 1 || p.split > 63 { return Err(format!("type {}: split {} outside 1..=63", self.r_type, p.split)); }
            }
        }
        Ok(())
    }
}

fn cmp_prediction(a: &Prediction, b: &Prediction) -> Ordering {
    a.object.as_bytes().cmp(b.object.as_bytes())
        .then(a.link.as_bytes().cmp(b.link.as_bytes()))
        .then(a.section.as_bytes().cmp(b.section.as_bytes()))
        .then(a.offset.cmp(&b.offset))
}

impl Recovery {
    /// Sort programs and predictions into the order the result format requires.
    pub fn sort(&mut self) {
        self.programs.sort_by_key(|p| p.r_type);
        self.predictions.sort_by(cmp_prediction);
    }

    /// Validate the document against the manifest of `corpus`
    /// (`spec/result-format-v1.md`, sections 2 and 3, structural parts).
    pub fn validate(&self, corpus: &Corpus) -> Result<(), String> {
        let mut expected: Vec<u32> = corpus.types.iter().map(|t| t.id).collect();
        expected.sort();
        let got: Vec<u32> = self.programs.iter().map(|p| p.r_type).collect();
        if got != expected {
            return Err(format!("programs list types {:?}; the manifest lists {:?}", got, expected));
        }
        for p in &self.programs {
            let t = corpus.reloc_type(p.r_type).unwrap();
            p.validate(t.container_bits, t.pair_group.as_deref())?;
        }
        // pair members agree on expression, scale, split and have complementary roles
        for t in &corpus.types {
            if let Some(g) = &t.pair_group {
                let members: Vec<&Program> = self.programs.iter().filter(|p| p.pair.as_ref().map(|q| &q.group) == Some(g)).collect();
                if members.len() != 2 {
                    return Err(format!("pair group '{}' has {} members in the result", g, members.len()));
                }
                let (a, b) = (members[0], members[1]);
                let (pa, pb) = (a.pair.as_ref().unwrap(), b.pair.as_ref().unwrap());
                if a.expression != b.expression || a.scale != b.scale || pa.split != pb.split || pa.role == pb.role {
                    return Err(format!("pair group '{}': members disagree on expression, scale, split or roles", g));
                }
            }
        }
        // predictions: exactly one per site per link, in order
        let mut want: Vec<Prediction> = Vec::new();
        for o in &corpus.objects {
            for l in &o.links {
                for s in &o.sites {
                    want.push(Prediction { object: o.id.clone(), link: l.id.clone(), section: s.section.clone(), offset: s.offset, word: None });
                }
            }
        }
        want.sort_by(cmp_prediction);
        if self.predictions.len() != want.len() {
            return Err(format!("{} predictions; the manifest has {} sites across its links", self.predictions.len(), want.len()));
        }
        for (i, (g, w)) in self.predictions.iter().zip(&want).enumerate() {
            if (&g.object, &g.link, &g.section, g.offset) != (&w.object, &w.link, &w.section, w.offset) {
                return Err(format!("prediction {} is {}/{}/{}+{}; expected {}/{}/{}+{} (missing, duplicated or out of order)",
                    i, g.object, g.link, g.section, g.offset, w.object, w.link, w.section, w.offset));
            }
            let bits = corpus.objects.iter().find(|o| o.id == g.object).unwrap().sites.iter().find(|s| s.section == g.section && s.offset == g.offset).unwrap().container_bits;
            if let Some(word) = g.word {
                if bits < 64 && word >> bits != 0 {
                    return Err(format!("prediction {}/{}/{}+{} does not fit a {}-bit container", g.object, g.link, g.section, g.offset, bits));
                }
            }
        }
        Ok(())
    }

    /// Exact serialisation of `spec/result-format-v1.md`.
    pub fn to_json(&self) -> String {
        let mut programs = Vec::new();
        for p in &self.programs {
            let slices = p.slices.iter().map(|s| Value::Object(vec![("lsb".into(), Value::Int(s.lsb as i128)), ("width".into(), Value::Int(s.width as i128))])).collect();
            let pair = match &p.pair {
                None => Value::Null,
                Some(q) => Value::Object(vec![
                    ("group".into(), Value::Str(q.group.clone())),
                    ("role".into(), Value::Str(match q.role { Role::Hi => "hi", Role::Lo => "lo" }.into())),
                    ("split".into(), Value::Int(q.split as i128)),
                ]),
            };
            programs.push(Value::Object(vec![
                ("type".into(), Value::Int(p.r_type as i128)),
                ("expression".into(), Value::Str(p.expression.text().into())),
                ("scale".into(), Value::Int(p.scale as i128)),
                ("signedness".into(), Value::Str(match p.signedness { Signedness::Unsigned => "unsigned", Signedness::Signed => "signed" }.into())),
                ("overflow".into(), Value::Str(match p.overflow { Overflow::Reject => "reject", Overflow::Wrap => "wrap" }.into())),
                ("slices".into(), Value::Array(slices)),
                ("pair".into(), pair),
            ]));
        }
        let predictions = self.predictions.iter().map(|q| Value::Object(vec![
            ("object".into(), Value::Str(q.object.clone())),
            ("link".into(), Value::Str(q.link.clone())),
            ("section".into(), Value::Str(q.section.clone())),
            ("offset".into(), Value::Int(q.offset as i128)),
            ("word".into(), match q.word { None => Value::Null, Some(w) => Value::Int(w as i128) }),
        ])).collect();
        json::to_string(&Value::Object(vec![
            ("format_version".into(), Value::Int(1)),
            ("programs".into(), Value::Array(programs)),
            ("predictions".into(), Value::Array(predictions)),
        ]))
    }

    /// Strict parse of a result document: unknown members, wrong types and
    /// out-of-range numbers are errors. Structural validation against a
    /// manifest is `validate`.
    pub fn from_json(text: &str) -> Result<Recovery, String> {
        let doc = json::parse(text)?;
        only_members(&doc, &["format_version", "programs", "predictions"])?;
        if doc.get("format_version").and_then(Value::as_int) != Some(1) {
            return Err("format_version must be 1".into());
        }
        let mut programs = Vec::new();
        for p in doc.get("programs").and_then(Value::as_array).ok_or("programs must be an array")? {
            only_members(p, &["type", "expression", "scale", "signedness", "overflow", "slices", "pair"])?;
            let r_type = int_in(p, "type", 0, u32::MAX as i128)? as u32;
            let expression = Expression::parse(str_of(p, "expression")?).ok_or_else(|| format!("type {}: unknown expression", r_type))?;
            let scale = int_in(p, "scale", 1, 8)? as u32;
            let signedness = match str_of(p, "signedness")? { "unsigned" => Signedness::Unsigned, "signed" => Signedness::Signed, _ => return Err(format!("type {}: unknown signedness", r_type)) };
            let overflow = match str_of(p, "overflow")? { "reject" => Overflow::Reject, "wrap" => Overflow::Wrap, _ => return Err(format!("type {}: unknown overflow", r_type)) };
            let mut slices = Vec::new();
            for s in p.get("slices").and_then(Value::as_array).ok_or("slices must be an array")? {
                only_members(s, &["lsb", "width"])?;
                slices.push(Slice { lsb: int_in(s, "lsb", 0, 63)? as u32, width: int_in(s, "width", 1, 64)? as u32 });
            }
            let pair = match p.get("pair").ok_or("pair missing")? {
                Value::Null => None,
                q => {
                    only_members(q, &["group", "role", "split"])?;
                    Some(Pair {
                        group: str_of(q, "group")?.to_string(),
                        role: match str_of(q, "role")? { "hi" => Role::Hi, "lo" => Role::Lo, _ => return Err(format!("type {}: unknown role", r_type)) },
                        split: int_in(q, "split", 1, 63)? as u32,
                    })
                }
            };
            programs.push(Program { r_type, expression, scale, signedness, overflow, slices, pair });
        }
        let mut predictions = Vec::new();
        for q in doc.get("predictions").and_then(Value::as_array).ok_or("predictions must be an array")? {
            only_members(q, &["object", "link", "section", "offset", "word"])?;
            let word = match q.get("word").ok_or("word missing")? {
                Value::Null => None,
                Value::Int(i) if *i >= 0 && *i <= u64::MAX as i128 => Some(*i as u64),
                _ => return Err("word must be null or a non-negative integer below 2^64".into()),
            };
            predictions.push(Prediction {
                object: str_of(q, "object")?.to_string(), link: str_of(q, "link")?.to_string(),
                section: str_of(q, "section")?.to_string(), offset: int_in(q, "offset", 0, u64::MAX as i128)? as u64, word,
            });
        }
        Ok(Recovery { programs, predictions })
    }
}

fn only_members(v: &Value, allowed: &[&str]) -> Result<(), String> {
    let members = v.as_object().ok_or("expected an object")?;
    for (k, _) in members {
        if !allowed.contains(&k.as_str()) {
            return Err(format!("unknown member '{}'", k));
        }
    }
    for a in allowed {
        if v.get(a).is_none() {
            return Err(format!("member '{}' missing", a));
        }
    }
    Ok(())
}

fn int_in(v: &Value, key: &str, lo: i128, hi: i128) -> Result<i128, String> {
    let i = v.get(key).and_then(Value::as_int).ok_or_else(|| format!("'{}' must be an integer", key))?;
    if i < lo || i > hi {
        return Err(format!("'{}' = {} outside {}..={}", key, i, lo, hi));
    }
    Ok(i)
}

fn str_of<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v.get(key).and_then(Value::as_str).ok_or_else(|| format!("'{}' must be a string", key))
}
