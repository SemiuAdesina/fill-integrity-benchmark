//! Raw functional-result codec.
//!
//! A record can express exactly one thing: the values `recover` returned. There
//! is no status field, no pass/fail flag, no case name, no test count, no marker
//! and no nonce, so a record carries no authority of its own — the judge alone
//! decides whether the values are correct. Decoding is strict (fixed field
//! order, exact field set, bounded length), so a malformed or padded record is
//! a decode error and therefore a failure rather than a channel.
use crate::{Rat, RawFace, RawPlane, RawResult, RawVertex};

/// Hard ceiling on a record. A correct record for these fixtures is a few
/// hundred bytes; anything beyond this is malformed by definition.
pub const MAX_RECORD_BYTES: usize = 65536;

fn rat(q: Rat) -> String {
    format!("{}/{}", q.n, q.d)
}

fn plane(p: &RawPlane) -> String {
    format!(
        "{}:{},{},{}:{}",
        p.family, p.normal[0], p.normal[1], p.normal[2], rat(p.offset)
    )
}

fn vertex(v: &RawVertex) -> String {
    format!("{},{},{}", rat(v.xyz[0]), rat(v.xyz[1]), rat(v.xyz[2]))
}

fn face(f: &RawFace) -> String {
    format!(
        "{}:{}",
        f.plane,
        f.vertices
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// Serialize the raw outcome of `recover`.
pub fn encode(r: Option<&RawResult>) -> String {
    match r {
        None => "NONE\n".to_string(),
        Some(r) => {
            let ps = r.planes.iter().map(plane).collect::<Vec<_>>().join(";");
            let vs = r.vertices.iter().map(vertex).collect::<Vec<_>>().join(";");
            let fs = r.faces.iter().map(face).collect::<Vec<_>>().join(";");
            format!("SOME\nplanes={ps}\nvertices={vs}\nfaces={fs}\n")
        }
    }
}

fn int(s: &str) -> Result<i64, String> {
    s.parse::<i64>().map_err(|_| format!("bad integer {s:?}"))
}

fn idx(s: &str) -> Result<u64, String> {
    s.parse::<u64>().map_err(|_| format!("bad index {s:?}"))
}

fn parse_rat(s: &str) -> Result<Rat, String> {
    let mut it = s.split('/');
    let n = int(it.next().ok_or("missing numerator")?)?;
    let d = int(it.next().ok_or("missing denominator")?)?;
    if it.next().is_some() {
        return Err(format!("trailing content in rational {s:?}"));
    }
    Ok(Rat { n, d })
}

fn parse_triple(s: &str) -> Result<[Rat; 3], String> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 3 {
        return Err(format!("expected 3 coordinates, found {}", parts.len()));
    }
    Ok([
        parse_rat(parts[0])?,
        parse_rat(parts[1])?,
        parse_rat(parts[2])?,
    ])
}

/// Split a `;`-separated list. An empty payload is an empty list; every other
/// form must yield non-empty items, so stray separators are a decode error.
fn items(s: &str) -> Vec<&str> {
    if s.is_empty() {
        vec![]
    } else {
        s.split(';').collect()
    }
}

fn field<'a>(lines: &[&'a str], i: usize, key: &str) -> Result<&'a str, String> {
    let l = lines.get(i).ok_or_else(|| format!("missing field {key}"))?;
    l.strip_prefix(key)
        .and_then(|r| r.strip_prefix('='))
        .ok_or_else(|| format!("expected field {key}, found {l:?}"))
}

fn parse_planes(s: &str) -> Result<Vec<RawPlane>, String> {
    items(s)
        .into_iter()
        .map(|p| {
            let parts: Vec<&str> = p.split(':').collect();
            if parts.len() != 3 {
                return Err(format!("expected 3 plane fields, found {}", parts.len()));
            }
            let ns: Vec<&str> = parts[1].split(',').collect();
            if ns.len() != 3 {
                return Err(format!("expected 3 normal components, found {}", ns.len()));
            }
            Ok(RawPlane {
                family: idx(parts[0])?,
                normal: [int(ns[0])?, int(ns[1])?, int(ns[2])?],
                offset: parse_rat(parts[2])?,
            })
        })
        .collect()
}

fn parse_faces(s: &str) -> Result<Vec<RawFace>, String> {
    items(s)
        .into_iter()
        .map(|f| {
            let parts: Vec<&str> = f.split(':').collect();
            if parts.len() != 2 {
                return Err(format!("expected 2 face fields, found {}", parts.len()));
            }
            let vs = if parts[1].is_empty() {
                vec![]
            } else {
                parts[1]
                    .split(',')
                    .map(idx)
                    .collect::<Result<Vec<u64>, String>>()?
            };
            Ok(RawFace {
                plane: idx(parts[0])?,
                vertices: vs,
            })
        })
        .collect()
}

/// Strictly decode a record. Any extra, missing, reordered or oversized content
/// is malformed.
pub fn decode(s: &str) -> Result<Option<RawResult>, String> {
    if s.len() > MAX_RECORD_BYTES {
        return Err(format!("record exceeds {MAX_RECORD_BYTES} bytes"));
    }
    let lines: Vec<&str> = s.lines().collect();
    match lines.first() {
        Some(&"NONE") => {
            if lines.len() != 1 {
                return Err("trailing content after NONE".into());
            }
            Ok(None)
        }
        Some(&"SOME") => {
            if lines.len() != 4 {
                return Err(format!("expected 4 lines, found {}", lines.len()));
            }
            Ok(Some(RawResult {
                planes: parse_planes(field(&lines, 1, "planes")?)?,
                vertices: items(field(&lines, 2, "vertices")?)
                    .into_iter()
                    .map(|v| Ok(RawVertex { xyz: parse_triple(v)? }))
                    .collect::<Result<Vec<RawVertex>, String>>()?,
                faces: parse_faces(field(&lines, 3, "faces")?)?,
            }))
        }
        _ => Err("record must begin with SOME or NONE".into()),
    }
}
