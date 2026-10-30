//! Raw functional-result codec.
//!
//! A record can express exactly one thing: the values the candidate's `recover`,
//! `translate` and `predicts_dma` returned. There is no status field, no
//! pass/fail flag, no case name, no test count, no marker and no nonce, so a
//! record carries no authority of its own — the judge alone decides whether the
//! values are correct. Decoding is strict (fixed field order, exact field set,
//! exact separator arity, bounded length), so a malformed or padded record is a
//! decode error and therefore a failure rather than a channel.
//!
//! Identifiers are normalised on the way out: every byte outside
//! `[A-Za-z0-9_.-]` becomes `?`, which is itself outside that set. Normalisation
//! can only ever turn a legal identifier into itself or an illegal one into
//! something that matches no expected value, so it removes separator ambiguity
//! without ever converting a wrong answer into a right one.
use crate::{Decoder, Observed, Outcome, Route, TargetOffset};

/// Hard ceiling on a record. A correct record for these fixtures is a few
/// hundred bytes; anything beyond this is malformed by definition.
pub const MAX_RECORD_BYTES: usize = 65536;

/// Hard ceiling on how many items any one collection may contain.
const MAX_ITEMS: usize = 4096;

/// Longest identifier the codec will carry. The fixtures use ids of at most a
/// few characters.
const MAX_ID_CHARS: usize = 64;

/// Longest error text the codec will carry.
const MAX_MSG_CHARS: usize = 256;

fn id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'
}

/// Normalise an identifier into the codec's alphabet.
fn id(s: &str) -> String {
    s.chars()
        .take(MAX_ID_CHARS)
        .map(|c| if id_char(c) { c } else { '?' })
        .collect()
}

/// Normalise an error message into printable ASCII on a single line.
fn msg(s: &str) -> String {
    s.chars()
        .take(MAX_MSG_CHARS)
        .map(|c| if (' '..='~').contains(&c) { c } else { '?' })
        .collect()
}

fn decoder(d: &Decoder) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        id(&d.host),
        d.start,
        d.end,
        id(&d.device),
        d.device_offset
    )
}

fn route(r: &Route) -> String {
    format!(
        "{}:{}:{}:{}",
        id(&r.host),
        id(&r.device),
        r.cost,
        r.hops.iter().map(|h| id(h)).collect::<Vec<_>>().join("|")
    )
}

fn target(t: &Option<TargetOffset>) -> String {
    match t {
        None => "-".to_string(),
        Some(t) => format!("{}:{}", id(&t.device), t.offset),
    }
}

/// Serialize the raw values a probe observed.
///
/// A collection larger than the codec's ceiling is not truncated — truncation
/// could invent a shorter, accidentally-correct answer. It is encoded as an
/// unparseable record instead, which the judge scores as a failure.
pub fn encode(o: &Observed) -> String {
    match o {
        Observed::Failed(e) => format!("ERR\nmsg={}\n", msg(e)),
        Observed::Recovered(r) => {
            if r.decoders.len() > MAX_ITEMS
                || r.routes.len() > MAX_ITEMS
                || r.translate.len() > MAX_ITEMS
                || r.dma.len() > MAX_ITEMS
                || r.routes.iter().any(|x| x.hops.len() > MAX_ITEMS)
            {
                return "BAD\n".to_string();
            }
            let ds = r.decoders.iter().map(decoder).collect::<Vec<_>>().join(";");
            let rs = r.routes.iter().map(route).collect::<Vec<_>>().join(";");
            let ts = r.translate.iter().map(target).collect::<Vec<_>>().join(";");
            let ms = r
                .dma
                .iter()
                .map(|b| if *b { "1" } else { "0" })
                .collect::<Vec<_>>()
                .join(";");
            format!("OK\ndecoders={ds}\nroutes={rs}\ntranslate={ts}\ndma={ms}\n")
        }
    }
}

fn u64v(s: &str) -> Result<u64, String> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("bad integer {s:?}"));
    }
    s.parse::<u64>().map_err(|_| format!("bad integer {s:?}"))
}

fn ident(s: &str) -> Result<String, String> {
    if s.chars().count() > MAX_ID_CHARS || !s.chars().all(id_char) {
        return Err(format!("bad identifier {s:?}"));
    }
    Ok(s.to_string())
}

/// Split a `;`-separated list. An empty payload is an empty list; every other
/// form must yield non-empty items, so stray separators are a decode error.
fn items(s: &str) -> Result<Vec<&str>, String> {
    if s.is_empty() {
        return Ok(vec![]);
    }
    let v: Vec<&str> = s.split(';').collect();
    if v.len() > MAX_ITEMS {
        return Err(format!("list of {} items exceeds ceiling", v.len()));
    }
    if v.iter().any(|x| x.is_empty()) {
        return Err("empty item in list".into());
    }
    Ok(v)
}

fn field<'a>(lines: &[&'a str], i: usize, key: &str) -> Result<&'a str, String> {
    let l = lines.get(i).ok_or_else(|| format!("missing field {key}"))?;
    l.strip_prefix(key)
        .and_then(|r| r.strip_prefix('='))
        .ok_or_else(|| format!("expected field {key}, found {l:?}"))
}

fn parse_decoders(s: &str) -> Result<Vec<Decoder>, String> {
    items(s)?
        .into_iter()
        .map(|d| {
            let p: Vec<&str> = d.split(':').collect();
            if p.len() != 5 {
                return Err(format!("expected 5 decoder fields, found {}", p.len()));
            }
            Ok(Decoder {
                host: ident(p[0])?,
                start: u64v(p[1])?,
                end: u64v(p[2])?,
                device: ident(p[3])?,
                device_offset: u64v(p[4])?,
            })
        })
        .collect()
}

fn parse_routes(s: &str) -> Result<Vec<Route>, String> {
    items(s)?
        .into_iter()
        .map(|r| {
            let p: Vec<&str> = r.split(':').collect();
            if p.len() != 4 {
                return Err(format!("expected 4 route fields, found {}", p.len()));
            }
            let hops = if p[3].is_empty() {
                vec![]
            } else {
                let h: Vec<&str> = p[3].split('|').collect();
                if h.len() > MAX_ITEMS {
                    return Err("hop list exceeds ceiling".into());
                }
                h.into_iter().map(ident).collect::<Result<Vec<_>, _>>()?
            };
            Ok(Route {
                host: ident(p[0])?,
                device: ident(p[1])?,
                cost: u64v(p[2])?,
                hops,
            })
        })
        .collect()
}

fn parse_translate(s: &str) -> Result<Vec<Option<TargetOffset>>, String> {
    items(s)?
        .into_iter()
        .map(|t| {
            if t == "-" {
                return Ok(None);
            }
            let p: Vec<&str> = t.split(':').collect();
            if p.len() != 2 {
                return Err(format!("expected 2 target fields, found {}", p.len()));
            }
            Ok(Some(TargetOffset {
                device: ident(p[0])?,
                offset: u64v(p[1])?,
            }))
        })
        .collect()
}

fn parse_dma(s: &str) -> Result<Vec<bool>, String> {
    items(s)?
        .into_iter()
        .map(|b| match b {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(format!("bad boolean {b:?}")),
        })
        .collect()
}

/// Strictly decode a record. Any extra, missing, reordered or oversized content
/// is malformed.
pub fn decode(s: &str) -> Result<Observed, String> {
    if s.len() > MAX_RECORD_BYTES {
        return Err(format!("record exceeds {MAX_RECORD_BYTES} bytes"));
    }
    let lines: Vec<&str> = s.lines().collect();
    match lines.first() {
        Some(&"ERR") => {
            if lines.len() != 2 {
                return Err(format!("expected 2 lines, found {}", lines.len()));
            }
            let m = field(&lines, 1, "msg")?;
            if m.chars().count() > MAX_MSG_CHARS || !m.chars().all(|c| (' '..='~').contains(&c)) {
                return Err("error text outside the codec alphabet".into());
            }
            Ok(Observed::Failed(m.to_string()))
        }
        Some(&"OK") => {
            if lines.len() != 5 {
                return Err(format!("expected 5 lines, found {}", lines.len()));
            }
            Ok(Observed::Recovered(Outcome {
                decoders: parse_decoders(field(&lines, 1, "decoders")?)?,
                routes: parse_routes(field(&lines, 2, "routes")?)?,
                translate: parse_translate(field(&lines, 3, "translate")?)?,
                dma: parse_dma(field(&lines, 4, "dma")?)?,
            }))
        }
        _ => Err("record must begin with OK or ERR".into()),
    }
}
