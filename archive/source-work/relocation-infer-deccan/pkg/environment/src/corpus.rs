//! Corpus loader (frozen).
//!
//! Reads `manifest.json`, every object and every linked output described in
//! `spec/elf-corpus-v1.md` into plain data. It resolves nothing about
//! relocation semantics: operands, pairs and programs are the solver's business
//! and are defined by the specification.

use crate::bits::read_word;
use crate::elf::{ElfFile, SHT_PROGBITS};
use crate::json::{self, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelocType {
    pub id: u32,
    pub name: String,
    pub container_bits: u32,
    pub pair_group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolResolution {
    /// Section name in `Link::bases`, or `None` for an absolute symbol.
    pub section: Option<String>,
    pub value: u64,
}

#[derive(Clone, Debug)]
pub enum LinkResult {
    Linked(ElfFile),
    Failed { overflow: Vec<(String, u64)> },
    Replay,
}

#[derive(Clone, Debug)]
pub struct Link {
    pub id: String,
    pub bases: BTreeMap<String, u64>,
    pub symbols: BTreeMap<String, SymbolResolution>,
    pub result: LinkResult,
}

impl Link {
    pub fn status(&self) -> &'static str {
        match self.result {
            LinkResult::Linked(_) => "linked",
            LinkResult::Failed { .. } => "failed",
            LinkResult::Replay => "replay",
        }
    }
    /// The legacy word of a site, when this link produced an output.
    pub fn legacy_word(&self, section: &str, offset: u64, container_bits: u32) -> Option<u64> {
        match &self.result {
            LinkResult::Linked(out) => out.section_named(section).and_then(|s| read_word(&s.data, offset, container_bits)),
            _ => None,
        }
    }
    /// For a failed link: whether the legacy linker rejected this site.
    pub fn rejected(&self, section: &str, offset: u64) -> Option<bool> {
        match &self.result {
            LinkResult::Failed { overflow } => Some(overflow.iter().any(|(s, o)| s == section && *o == offset)),
            _ => None,
        }
    }
}

/// One RELA entry of an object, with its container word as stored in the object.
#[derive(Clone, Debug)]
pub struct Site {
    pub section: String,
    pub section_index: usize,
    pub offset: u64,
    pub r_type: u32,
    pub container_bits: u32,
    /// Symbol table index referenced by the entry.
    pub symbol: usize,
    pub addend: i64,
    /// Position among the entries of the same relocation section (ascending offset order).
    pub entry: usize,
    pub object_word: u64,
}

#[derive(Clone, Debug)]
pub struct Object {
    pub id: String,
    pub file: String,
    pub elf: ElfFile,
    pub links: Vec<Link>,
    pub sites: Vec<Site>,
}

#[derive(Clone, Debug)]
pub struct Corpus {
    pub root: PathBuf,
    pub types: Vec<RelocType>,
    pub objects: Vec<Object>,
}

impl Corpus {
    pub fn reloc_type(&self, id: u32) -> Option<&RelocType> {
        self.types.iter().find(|t| t.id == id)
    }

    pub fn load(root: &Path) -> Result<Corpus, String> {
        let text = std::fs::read_to_string(root.join("manifest.json")).map_err(|e| format!("manifest.json: {}", e))?;
        let man = json::parse(&text).map_err(|e| format!("manifest.json: {}", e))?;
        if man.get("format_version").and_then(Value::as_int) != Some(1) {
            return Err("manifest.json: unsupported format_version".into());
        }
        let mut types = Vec::new();
        for t in man.get("relocation_types").and_then(Value::as_array).ok_or("manifest.json: relocation_types missing")? {
            let pg = t.get("pair_group").ok_or("relocation type without pair_group")?;
            types.push(RelocType {
                id: int_field(t, "type")? as u32,
                name: str_field(t, "name")?,
                container_bits: int_field(t, "container_bits")? as u32,
                pair_group: if pg.is_null() { None } else { Some(pg.as_str().ok_or("pair_group must be a string or null")?.to_string()) },
            });
        }
        let mut objects = Vec::new();
        for o in man.get("objects").and_then(Value::as_array).ok_or("manifest.json: objects missing")? {
            let id = str_field(o, "id")?;
            let file = str_field(o, "file")?;
            let bytes = std::fs::read(root.join(&file)).map_err(|e| format!("{}: {}", file, e))?;
            let elf = ElfFile::parse(&bytes).map_err(|e| format!("{}: {}", file, e))?;
            let mut links = Vec::new();
            for l in o.get("links").and_then(Value::as_array).ok_or("object without links")? {
                let lid = str_field(l, "id")?;
                let mut bases = BTreeMap::new();
                for (k, v) in l.get("bases").and_then(Value::as_object).ok_or("link without bases")? {
                    bases.insert(k.clone(), v.as_int().ok_or("base must be an integer")? as u64);
                }
                let mut symbols = BTreeMap::new();
                for (k, v) in l.get("symbols").and_then(Value::as_object).ok_or("link without symbols")? {
                    let sec = v.get("section").ok_or("symbol resolution without section")?;
                    symbols.insert(k.clone(), SymbolResolution {
                        section: if sec.is_null() { None } else { Some(sec.as_str().ok_or("section must be a string or null")?.to_string()) },
                        value: int_field(v, "value")? as u64,
                    });
                }
                let res = l.get("result").ok_or("link without result")?;
                let result = match str_field(res, "status")?.as_str() {
                    "linked" => {
                        let f = str_field(res, "file")?;
                        let b = std::fs::read(root.join(&f)).map_err(|e| format!("{}: {}", f, e))?;
                        LinkResult::Linked(ElfFile::parse(&b).map_err(|e| format!("{}: {}", f, e))?)
                    }
                    "failed" => {
                        let mut overflow = Vec::new();
                        for site in res.get("overflow").and_then(Value::as_array).ok_or("failed link without overflow list")? {
                            overflow.push((str_field(site, "section")?, int_field(site, "offset")? as u64));
                        }
                        LinkResult::Failed { overflow }
                    }
                    "replay" => LinkResult::Replay,
                    other => return Err(format!("unknown link status '{}'", other)),
                };
                links.push(Link { id: lid, bases, symbols, result });
            }
            let mut sites = Vec::new();
            for r in &elf.relocations {
                let sec = &elf.sections[r.section];
                let t = types.iter().find(|t| t.id == r.r_type).ok_or_else(|| format!("{}: relocation type {} not in manifest", id, r.r_type))?;
                if sec.sh_type != SHT_PROGBITS {
                    return Err(format!("{}: relocation against a non-content section", id));
                }
                let object_word = read_word(&sec.data, r.offset, t.container_bits)
                    .ok_or_else(|| format!("{}: site {}+{} outside its section", id, sec.name, r.offset))?;
                sites.push(Site {
                    section: sec.name.clone(), section_index: r.section, offset: r.offset, r_type: r.r_type,
                    container_bits: t.container_bits, symbol: r.symbol as usize, addend: r.addend, entry: r.entry, object_word,
                });
            }
            sites.sort_by(|a, b| (a.section_index, a.offset).cmp(&(b.section_index, b.offset)));
            objects.push(Object { id, file, elf, links, sites });
        }
        Ok(Corpus { root: root.to_path_buf(), types, objects })
    }
}

fn int_field(v: &Value, key: &str) -> Result<i128, String> {
    v.get(key).and_then(Value::as_int).ok_or_else(|| format!("'{}' missing or not an integer", key))
}

fn str_field(v: &Value, key: &str) -> Result<String, String> {
    v.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("'{}' missing or not a string", key))
}
