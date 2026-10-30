//! Minimal JSON reader and writer (frozen).
//!
//! Integers are kept exact as `i128`, which covers every 64-bit container
//! word and every signed addend. Floating-point literals are rejected: no
//! document in this task contains one.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i128),
    Str(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_int(&self) -> Option<i128> {
        if let Value::Int(i) = self { Some(*i) } else { None }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Value::Str(s) = self { Some(s) } else { None }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        if let Value::Array(a) = self { Some(a) } else { None }
    }
    pub fn as_object(&self) -> Option<&[(String, Value)]> {
        if let Value::Object(o) = self { Some(o) } else { None }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

const MAX_DEPTH: usize = 64;

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\n' || c == b'\r' || c == b'\t' { self.pos += 1 } else { break }
        }
    }
    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.pos))
        }
    }
    fn value(&mut self) -> Result<Value, String> {
        self.skip_ws();
        match self.peek() {
            None => Err("unexpected end of document".into()),
            Some(b'{') => {
                self.depth += 1;
                if self.depth > MAX_DEPTH { return Err("nesting too deep".into()); }
                self.pos += 1;
                let mut members = Vec::new();
                self.skip_ws();
                if self.peek() == Some(b'}') { self.pos += 1; self.depth -= 1; return Ok(Value::Object(members)); }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.skip_ws();
                    self.expect(b':')?;
                    let v = self.value()?;
                    if members.iter().any(|(k, _): &(String, Value)| *k == key) {
                        return Err(format!("duplicate member '{}'", key));
                    }
                    members.push((key, v));
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => { self.pos += 1; }
                        Some(b'}') => { self.pos += 1; break; }
                        _ => return Err(format!("expected ',' or '}}' at byte {}", self.pos)),
                    }
                }
                self.depth -= 1;
                Ok(Value::Object(members))
            }
            Some(b'[') => {
                self.depth += 1;
                if self.depth > MAX_DEPTH { return Err("nesting too deep".into()); }
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.peek() == Some(b']') { self.pos += 1; self.depth -= 1; return Ok(Value::Array(items)); }
                loop {
                    items.push(self.value()?);
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => { self.pos += 1; }
                        Some(b']') => { self.pos += 1; break; }
                        _ => return Err(format!("expected ',' or ']' at byte {}", self.pos)),
                    }
                }
                self.depth -= 1;
                Ok(Value::Array(items))
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(format!("unexpected byte '{}' at {}", c as char, self.pos)),
        }
    }
    fn literal(&mut self, word: &str, v: Value) -> Result<Value, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(v)
        } else {
            Err(format!("bad literal at byte {}", self.pos))
        }
    }
    fn number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') { self.pos += 1; }
        let digits_start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() { self.pos += 1 } else { break }
        }
        if self.pos == digits_start { return Err(format!("bad number at byte {}", start)); }
        if let Some(c) = self.peek() {
            if c == b'.' || c == b'e' || c == b'E' { return Err(format!("non-integer number at byte {}", start)); }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|e| e.to_string())?;
        if self.pos - digits_start > 1 && self.bytes[digits_start] == b'0' {
            return Err(format!("leading zero at byte {}", start));
        }
        text.parse::<i128>().map(Value::Int).map_err(|e| format!("{} at byte {}", e, start))
    }
    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or("unterminated string")?;
            self.pos += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self.peek().ok_or("bad escape")?;
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            if self.pos + 4 > self.bytes.len() { return Err("bad \\u escape".into()); }
                            let hex = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4]).map_err(|e| e.to_string())?;
                            let cp = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
                            self.pos += 4;
                            out.push(char::from_u32(cp).ok_or("bad code point")?);
                        }
                        _ => return Err("bad escape".into()),
                    }
                }
                c if c < 0x20 => return Err("control character in string".into()),
                c => {
                    // copy one UTF-8 sequence
                    let len = if c < 0x80 { 1 } else if c >> 5 == 0b110 { 2 } else if c >> 4 == 0b1110 { 3 } else { 4 };
                    let end = self.pos - 1 + len;
                    if end > self.bytes.len() { return Err("truncated UTF-8".into()); }
                    let s = std::str::from_utf8(&self.bytes[self.pos - 1..end]).map_err(|e| e.to_string())?;
                    out.push_str(s);
                    self.pos = end;
                }
            }
        }
    }
}

/// Parse one JSON document; trailing content other than whitespace is an error.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser { bytes: text.as_bytes(), pos: 0, depth: 0 };
    let v = p.value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(format!("trailing content at byte {}", p.pos));
    }
    Ok(v)
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => { let _ = write!(out, "\\u{:04x}", c as u32); }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Serialise with the members in the order they were inserted; one line per
/// array element of the top-level arrays keeps the document diff-friendly.
pub fn write(v: &Value, out: &mut String, indent: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => { let _ = write!(out, "{}", i); }
        Value::Str(s) => out.push_str(&escape(s)),
        Value::Array(items) => {
            if items.is_empty() { out.push_str("[]"); return; }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 { out.push(','); }
                if indent < 2 {
                    out.push('\n');
                    for _ in 0..(indent + 1) { out.push(' '); }
                }
                write(item, out, indent + 1);
            }
            if indent < 2 { out.push('\n'); for _ in 0..indent { out.push(' '); } }
            out.push(']');
        }
        Value::Object(members) => {
            if members.is_empty() { out.push_str("{}"); return; }
            out.push('{');
            for (i, (k, item)) in members.iter().enumerate() {
                if i > 0 { out.push(','); }
                if indent < 1 {
                    out.push('\n');
                    for _ in 0..(indent + 1) { out.push(' '); }
                }
                out.push_str(&escape(k));
                out.push(':');
                write(item, out, indent + 1);
            }
            if indent < 1 { out.push('\n'); }
            out.push('}');
        }
    }
}

pub fn to_string(v: &Value) -> String {
    let mut s = String::new();
    write(v, &mut s, 0);
    s.push('\n');
    s
}
