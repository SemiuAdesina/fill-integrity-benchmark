
//! The backbone record.
//!
//! `backbone.fasta` holds one sequence record: a header line naming the
//! source record, then the bases. The sequence is circular; base `V - 1` is
//! followed by base `0`. Only the four unambiguous bases are accepted, so a
//! recognition site either occurs at a window or does not.

/// One circular DNA record.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Backbone {
    /// The header line without its leading `>`.
    pub header: String,
    /// Upper-case bases, `A`, `C`, `G` or `T`.
    pub seq: Vec<u8>,
}

impl Backbone {
    /// Number of bases, `V` in the specification.
    pub fn len(&self) -> usize {
        self.seq.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seq.is_empty()
    }
}

/// Parses a single-record FASTA text.
///
/// A second record, an empty sequence, or a character other than `ACGTacgt`
/// in the sequence lines is an error; lower-case bases are folded to upper.
pub fn parse_fasta(text: &str) -> Result<Backbone, String> {
    let mut header: Option<String> = None;
    let mut seq = Vec::new();
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('>') {
            if header.is_some() {
                return Err(format!("line {}: a second record starts here; the backbone is one record", number + 1));
            }
            header = Some(rest.trim().to_string());
            continue;
        }
        if header.is_none() {
            return Err(format!("line {}: sequence before the header line", number + 1));
        }
        for byte in line.trim().bytes() {
            match byte.to_ascii_uppercase() {
                b @ (b'A' | b'C' | b'G' | b'T') => seq.push(b),
                other => {
                    return Err(format!("line {}: '{}' is not one of the four bases", number + 1, other as char))
                }
            }
        }
    }
    let header = header.ok_or_else(|| "no header line".to_string())?;
    if seq.is_empty() {
        return Err("the record carries no bases".to_string());
    }
    Ok(Backbone { header, seq })
}

/// Writes a record in the layout `backbone.fasta` uses: the header, then the
/// bases in lines of seventy.
pub fn write_fasta(backbone: &Backbone) -> String {
    let mut out = String::with_capacity(backbone.seq.len() + backbone.header.len() + 64);
    out.push('>');
    out.push_str(&backbone.header);
    out.push('\n');
    for chunk in backbone.seq.chunks(70) {
        out.push_str(std::str::from_utf8(chunk).expect("bases are ASCII"));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_one_record_and_folds_case() {
        let b = parse_fasta(">x rec\nACgt\nTT\n").unwrap();
        assert_eq!(b.header, "x rec");
        assert_eq!(b.seq, b"ACGTTT");
        assert_eq!(write_fasta(&b), ">x rec\nACGTTT\n");
    }

    #[test]
    fn rejects_what_is_not_a_single_clean_record() {
        assert!(parse_fasta("ACGT\n").is_err());
        assert!(parse_fasta(">a\nACGT\n>b\nACGT\n").is_err());
        assert!(parse_fasta(">a\nACGN\n").is_err());
        assert!(parse_fasta(">a\n").is_err());
    }
}
