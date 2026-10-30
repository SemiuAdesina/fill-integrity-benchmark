
//! The payload the runner delivers on standard input.
//!
//! It carries two things: the certified answers and the maps the
//! probes produced. Both arrive on the one channel that
//! closes. The runner writes the payload and closes the pipe; the harness
//! consumes it to end of file before the check is dispatched, and nothing that
//! runs afterwards can recover it.
//!
//! The certified answers travel this way because a solver that can read them can
//! replay them instead of searching. They are deleted from every filesystem
//! before the first probe starts, so no process holding candidate code ever
//! shares a machine state with a readable copy.
//!
//! The artifacts travel this way for a different reason. A probe runs candidate
//! code with the runner's privileges, so a solver could write files the runner
//! did not ask it for. If the judge read artifacts off the filesystem, a probe
//! could plant an answer for a job it was not given — most usefully a copy of its
//! own answer under the identifier of the agreement check's second execution, which would
//! forge determinism it does not have. The runner instead captures each artifact
//! into memory the moment its probe exits, from a staging path it wipes before
//! every job, so a later probe cannot reach an earlier one's captured bytes.
//!
//! The format is framed blobs followed by an end marker:
//!
//! ```text
//! ##FIXTURE <stem> <byte-count>
//! <byte-count bytes of JSON>
//! ##ARTIFACT <job> <byte-count>
//! <byte-count bytes of JSON>
//! ##END
//! ```
//!
//! The end marker exists because the runner assembles the payload in a shell
//! variable, and command substitution strips trailing newlines. Without a final
//! line of its own, the last frame would arrive one byte shorter than its header
//! claims and the whole payload would be rejected as truncated.

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Frame header introducing one certified answer.
const FIXTURE_FRAME: &str = "##FIXTURE ";

/// Frame header introducing one probe's map.
const ARTIFACT_FRAME: &str = "##ARTIFACT ";

/// Marker closing the payload.
const END: &str = "##END";

/// What the runner delivered, parsed once at start up.
pub struct Payload {
    /// Certified answers by fixture stem.
    expectations: BTreeMap<String, String>,
    /// Probe artifacts by job identifier.
    artifacts: BTreeMap<String, String>,
}

impl Payload {
    /// The certified answer for one fixture.
    ///
    /// Absence is a fault in the runner rather than a candidate failure, so it
    /// panics: a check that silently skipped its ground truth would report a
    /// pass it had not earned.
    pub fn expectation(&self, stem: &str) -> &str {
        self.expectations
            .get(stem)
            .unwrap_or_else(|| panic!("the runner delivered no certified answer for {stem}"))
    }

    /// The artifact one probe job produced, if that job produced one.
    ///
    /// Absence is a candidate failure — a probe that crashed, timed out or was
    /// never reached — so it is returned rather than panicked, and the check
    /// that wanted it fails with a reason in the report.
    pub fn artifact(&self, job: &str) -> Option<&str> {
        self.artifacts.get(job).map(String::as_str)
    }
}

static PAYLOAD: OnceLock<Payload> = OnceLock::new();

/// Stores the parsed payload. Called once, before any check is dispatched.
pub fn install(payload: Payload) {
    PAYLOAD.set(payload).map_err(|_| ()).expect("the payload is installed once");
}

/// The installed payload.
pub fn get() -> &'static Payload {
    PAYLOAD.get().expect("the payload is installed before any check runs")
}

/// Parses the runner's payload.
///
/// Returns `Err` with a reason rather than a partial payload: a truncated frame
/// means the pipe was interfered with, and continuing with whatever arrived
/// would grade against incomplete ground truth.
pub fn parse(raw: &str) -> Result<Payload, String> {
    let mut rest = raw;

    let mut expectations = BTreeMap::new();
    let mut artifacts = BTreeMap::new();
    let mut closed = false;
    while !rest.is_empty() {
        let (header, after_header) = split_line(rest).ok_or("a frame header is truncated")?;
        if header.trim().is_empty() {
            rest = after_header;
            continue;
        }
        if header.trim() == END {
            closed = true;
            break;
        }

        let (into, spec) = if let Some(spec) = header.strip_prefix(FIXTURE_FRAME) {
            (&mut expectations, spec)
        } else if let Some(spec) = header.strip_prefix(ARTIFACT_FRAME) {
            (&mut artifacts, spec)
        } else {
            return Err(format!("expected a frame header, found {header:?}"));
        };

        let (name, body, after_body) = frame(spec, after_header)?;
        if into.insert(name.clone(), body).is_some() {
            return Err(format!("the payload carries two frames named {name}"));
        }
        rest = after_body;
    }

    if !closed {
        return Err("the payload has no end marker".to_string());
    }
    if expectations.is_empty() {
        return Err("the payload carries no certified answers".to_string());
    }

    Ok(Payload { expectations, artifacts })
}

/// Reads one frame's name and body, returning what follows it.
fn frame<'a>(spec: &str, after_header: &'a str) -> Result<(String, String, &'a str), String> {
    let mut parts = spec.split_whitespace();
    let name = parts.next().ok_or("a frame header names nothing")?.to_string();
    let len: usize = parts
        .next()
        .ok_or("a frame header carries no byte count")?
        .parse()
        .map_err(|_| "a frame header's byte count is not a number".to_string())?;

    if after_header.len() < len {
        return Err(format!("the frame for {name} is truncated"));
    }
    // Byte counts, on a string. A frame boundary landing mid-character means the
    // count was not produced by the runner over this body.
    if !after_header.is_char_boundary(len) {
        return Err(format!("the frame for {name} does not end on a character boundary"));
    }
    let (body, after_body) = after_header.split_at(len);
    Ok((name, body.to_string(), after_body.strip_prefix('\n').unwrap_or(after_body)))
}

/// Splits off the first line, returning it without its newline.
fn split_line(text: &str) -> Option<(&str, &str)> {
    match text.find('\n') {
        Some(at) => Some((&text[..at], &text[at + 1..])),
        None if text.is_empty() => None,
        None => Some((text, "")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(frames: &str) -> String {
        format!("{frames}##END\n")
    }

    #[test]
    fn reads_both_frame_kinds() {
        let parsed = parse(&payload("##FIXTURE base 2\n{}\n##ARTIFACT base 5\n[1,2]\n"))
            .expect("a well formed payload");
        assert_eq!(parsed.expectation("base"), "{}");
        assert_eq!(parsed.artifact("base"), Some("[1,2]"));
    }

    #[test]
    fn a_job_with_no_artifact_is_absent_rather_than_an_error() {
        // A probe that crashed produces no frame. The check that wanted it must
        // be able to fail with a reason instead of the payload being rejected.
        let parsed = parse(&payload("##FIXTURE base 2\n{}\n")).expect("a payload without artifacts");
        assert_eq!(parsed.artifact("base"), None);
    }

    #[test]
    fn rejects_a_payload_that_was_interfered_with() {
        assert!(parse("tok\n##FIXTURE base 2\n{}\n").is_err(), "missing end marker accepted");
        assert!(parse(&payload("##FIXTURE base 99\n{}\n")).is_err(), "truncated frame accepted");
        assert!(parse(&payload("##OTHER base 2\n{}\n")).is_err(), "unknown frame accepted");
        assert!(parse(&payload("")).is_err(), "payload with no certified answers accepted");
        assert!(
            parse(&payload("##FIXTURE base 2\n{}\n##FIXTURE base 2\n[]\n")).is_err(),
            "duplicate frame accepted"
        );
    }
}
