//! Verifier-private judge: `judge --run <check> --fixtures <dir> --artifacts <dir>`.
//!
//! Loads every fixture corpus and its certified answer from the private
//! fixture directory, loads the artifacts the probe phase captured, and runs
//! one named check. The judge links no candidate code: every submitted program
//! is parsed with the verifier's own schema code and replayed with the
//! verifier's own engine below. It prints `##RELOC-PASS <check>` and exits 0
//! only when the check holds on every fixture it covers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use verifier_core::bits::place;
use verifier_core::corpus::{Corpus, Link, LinkResult, Object, Site, SymbolResolution};
use verifier_core::elf::{SHN_ABS, SHN_UNDEF};
use verifier_core::json::{self, Value};
use verifier_core::plan::JOBS;
use verifier_core::program::{Expression, Overflow, Program, Recovery, Role, Signedness};

const PASS_MARKER: &str = "##RELOC-PASS";

macro_rules! check {
    ($name:ident) => { stringify!($name) };
}

/// The graded checks, in the order they are run. `test.sh` counts this list.
const CHECKS: &[&str] = &[
    check!(result_documents_well_formed),
    check!(predictions_follow_submitted_programs),
    check!(predictions_match_legacy_outcomes),
    check!(hidden_perturbations_replay),
    check!(scalar_expression_and_scale),
    check!(section_base_versus_place),
    check!(negative_truncation_and_split_fields),
    check!(pair_coupling_and_carry),
    check!(canonical_equivalence),
    check!(reject_wrap_boundaries),
    check!(field_layout_recovery),
    check!(cross_object_consistency),
    check!(deterministic_output),
];
const CONTROL: &str = "negative_control_must_fail";

// ------------------------------------------------------------ certified data
struct HiddenLink {
    object: String,
    link: Link,
    sites: Vec<(String, u64, Option<u64>)>,
}

struct Fixture {
    id: String,
    corpus: Corpus,
    programs: BTreeMap<u32, Program>,
    predictions: BTreeMap<(String, String, String, u64), Option<u64>>,
    hidden: Vec<HiddenLink>,
}

fn load_expected(path: &Path) -> Result<(BTreeMap<u32, Program>, BTreeMap<(String, String, String, u64), Option<u64>>, Vec<HiddenLink>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let doc = json::parse(&text)?;
    // reuse the strict schema parser for programs and predictions
    let sub = Value::Object(vec![
        ("format_version".into(), Value::Int(1)),
        ("programs".into(), doc.get("programs").cloned().ok_or("programs missing")?),
        ("predictions".into(), doc.get("predictions").cloned().ok_or("predictions missing")?),
    ]);
    let rec = Recovery::from_json(&json::to_string(&sub))?;
    let programs = rec.programs.into_iter().map(|p| (p.r_type, p)).collect();
    let predictions = rec.predictions.into_iter().map(|q| ((q.object, q.link, q.section, q.offset), q.word)).collect();
    let mut hidden = Vec::new();
    for h in doc.get("hidden_links").and_then(Value::as_array).ok_or("hidden_links missing")? {
        let mut bases = BTreeMap::new();
        for (k, v) in h.get("bases").and_then(Value::as_object).ok_or("hidden link bases")? {
            bases.insert(k.clone(), v.as_int().ok_or("base")? as u64);
        }
        let mut symbols = BTreeMap::new();
        for (k, v) in h.get("symbols").and_then(Value::as_object).ok_or("hidden link symbols")? {
            let sec = v.get("section").ok_or("section")?;
            symbols.insert(k.clone(), SymbolResolution {
                section: if sec.is_null() { None } else { Some(sec.as_str().ok_or("section")?.to_string()) },
                value: v.get("value").and_then(Value::as_int).ok_or("value")? as u64,
            });
        }
        let mut sites = Vec::new();
        for s in h.get("sites").and_then(Value::as_array).ok_or("hidden link sites")? {
            let w = s.get("word").ok_or("word")?;
            sites.push((
                s.get("section").and_then(Value::as_str).ok_or("section")?.to_string(),
                s.get("offset").and_then(Value::as_int).ok_or("offset")? as u64,
                if w.is_null() { None } else { Some(w.as_int().ok_or("word")? as u64) },
            ));
        }
        hidden.push(HiddenLink {
            object: h.get("object").and_then(Value::as_str).ok_or("object")?.to_string(),
            link: Link { id: h.get("id").and_then(Value::as_str).ok_or("id")?.to_string(), bases, symbols, result: LinkResult::Replay },
            sites,
        });
    }
    Ok((programs, predictions, hidden))
}

fn load_fixtures(dir: &Path) -> Result<BTreeMap<String, Fixture>, String> {
    let mut out = BTreeMap::new();
    let mut ids: Vec<&str> = JOBS.iter().map(|(_, f)| *f).collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        let corpus = Corpus::load(&dir.join(id).join("corpus")).map_err(|e| format!("fixture {}: {}", id, e))?;
        let (programs, predictions, hidden) = load_expected(&dir.join(format!("{}.expected.json", id))).map_err(|e| format!("fixture {}: {}", id, e))?;
        out.insert(id.to_string(), Fixture { id: id.to_string(), corpus, programs, predictions, hidden });
    }
    Ok(out)
}

// ---------------------------------------------------------- verifier replay
#[derive(Clone, Copy)]
enum Unit { Single(usize), Pair(usize, usize) }

fn units(corpus: &Corpus, o: &Object) -> Result<Vec<Unit>, String> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < o.sites.len() {
        let s = &o.sites[i];
        match corpus.reloc_type(s.r_type).and_then(|t| t.pair_group.clone()) {
            Some(g) => {
                let ok = o.sites.get(i + 1).map(|n| {
                    corpus.reloc_type(n.r_type).and_then(|t| t.pair_group.clone()).as_deref() == Some(g.as_str())
                        && n.r_type != s.r_type && n.symbol == s.symbol && n.addend == s.addend && n.section == s.section
                }).unwrap_or(false);
                if !ok { return Err(format!("{}: unpaired pair-group site {}+{}", o.id, s.section, s.offset)); }
                v.push(Unit::Pair(i, i + 1));
                i += 2;
            }
            None => { v.push(Unit::Single(i)); i += 1; }
        }
    }
    Ok(v)
}

fn operands(o: &Object, link: &Link, site: &Site) -> Option<(i128, i128, i128)> {
    let sym = o.elf.symbols.get(site.symbol)?;
    let p = *link.bases.get(&site.section)? as i128 + site.offset as i128;
    let (s, b) = if sym.shndx == SHN_ABS {
        (sym.value as i128, 0i128)
    } else if sym.shndx == SHN_UNDEF {
        let r = link.symbols.get(&sym.name)?;
        (r.value as i128, match &r.section { None => 0, Some(sec) => *link.bases.get(sec)? as i128 })
    } else {
        let sec = o.elf.sections.get(sym.shndx as usize)?;
        (sym.value as i128, *link.bases.get(&sec.name)? as i128)
    };
    Some((s, p, b))
}

fn value(e: Expression, s: i128, a: i128, p: i128, b: i128) -> i128 {
    match e {
        Expression::S => s,
        Expression::SPlusA => s + a,
        Expression::SMinusP => s - p,
        Expression::SPlusAMinusP => s + a - p,
        Expression::SPlusBPlusAMinusP => s + b + a - p,
    }
}

fn encode(x: i128, width: u32, sign: Signedness, ovf: Overflow) -> Option<u64> {
    let (lo, hi) = match sign {
        Signedness::Signed => (-(1i128 << (width - 1)), (1i128 << (width - 1)) - 1),
        Signedness::Unsigned => (0i128, (1i128 << width) - 1),
    };
    if (x < lo || x > hi) && ovf == Overflow::Reject { return None; }
    Some(x.rem_euclid(1i128 << width) as u64)
}

fn member_word(p: &Program, x: i128, object_word: u64) -> Option<u64> {
    encode(x, p.width(), p.signedness, p.overflow).map(|bits| place(bits, &p.slices, object_word))
}

/// Replay `programs` over one link of one object: (section, offset, word).
fn replay_link(corpus: &Corpus, o: &Object, link: &Link, programs: &BTreeMap<u32, Program>) -> Result<Vec<(String, u64, Option<u64>)>, String> {
    let mut out = Vec::new();
    for u in units(corpus, o)? {
        match u {
            Unit::Single(i) => {
                let site = &o.sites[i];
                let p = programs.get(&site.r_type).ok_or_else(|| format!("no program for type {}", site.r_type))?;
                let (s, pl, b) = operands(o, link, site).ok_or_else(|| format!("{}/{}: unresolvable site {}+{}", o.id, link.id, site.section, site.offset))?;
                let q = value(p.expression, s, site.addend as i128, pl, b) / p.scale as i128;
                out.push((site.section.clone(), site.offset, member_word(p, q, site.object_word)));
            }
            Unit::Pair(i, j) => {
                let (first, second) = (&o.sites[i], &o.sites[j]);
                let pf = programs.get(&first.r_type).ok_or_else(|| format!("no program for type {}", first.r_type))?;
                let ps = programs.get(&second.r_type).ok_or_else(|| format!("no program for type {}", second.r_type))?;
                let (s, pl, b) = operands(o, link, first).ok_or_else(|| format!("{}/{}: unresolvable pair at {}+{}", o.id, link.id, first.section, first.offset))?;
                let k = pf.pair.as_ref().map(|q| q.split).ok_or("pair member without pair data")?;
                let q = value(pf.expression, s, first.addend as i128, pl, b) / pf.scale as i128;
                let lo = q.rem_euclid(1i128 << k);
                let hi = (q - lo) / (1i128 << k);
                let comp = |p: &Program| if p.pair.as_ref().map(|x| x.role) == Some(Role::Hi) { hi } else { lo };
                out.push((first.section.clone(), first.offset, member_word(pf, comp(pf), first.object_word)));
                out.push((second.section.clone(), second.offset, member_word(ps, comp(ps), second.object_word)));
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------- artifacts
struct Artifact {
    job: String,
    fixture: String,
    text: Option<String>,
    parsed: Result<Recovery, String>,
}

fn load_artifacts(dir: &Path) -> Vec<Artifact> {
    JOBS.iter().map(|(job, fixture)| {
        let path = dir.join(format!("{}.json", job));
        let text = std::fs::read_to_string(&path).ok();
        let parsed = match &text {
            None => Err("no artifact was captured for this job".to_string()),
            Some(t) => Recovery::from_json(t),
        };
        Artifact { job: job.to_string(), fixture: fixture.to_string(), text, parsed }
    }).collect()
}

struct Ctx {
    fixtures: BTreeMap<String, Fixture>,
    artifacts: Vec<Artifact>,
}

impl Ctx {
    /// The validated document of a job, or the reason it is unusable.
    fn doc(&self, job: &str) -> Result<(&Fixture, &Recovery), String> {
        let a = self.artifacts.iter().find(|a| a.job == job).ok_or_else(|| format!("{}: unknown job", job))?;
        let fx = self.fixtures.get(&a.fixture).ok_or_else(|| format!("{}: fixture missing", job))?;
        let r = a.parsed.as_ref().map_err(|e| format!("{}: {}", job, e))?;
        r.validate(&fx.corpus).map_err(|e| format!("{}: {}", job, e))?;
        Ok((fx, r))
    }
    fn programs_of(&self, job: &str) -> Result<(&Fixture, BTreeMap<u32, Program>), String> {
        let (fx, r) = self.doc(job)?;
        Ok((fx, r.programs.iter().map(|p| (p.r_type, p.clone())).collect()))
    }
}

fn program_line(p: &Program) -> String {
    format!("{} scale {} {} {} slices {:?} pair {:?}", p.expression.text(), p.scale,
        match p.signedness { Signedness::Signed => "signed", Signedness::Unsigned => "unsigned" },
        match p.overflow { Overflow::Reject => "reject", Overflow::Wrap => "wrap" },
        p.slices.iter().map(|s| (s.lsb, s.width)).collect::<Vec<_>>(),
        p.pair.as_ref().map(|q| (q.group.clone(), match q.role { Role::Hi => "hi", Role::Lo => "lo" }, q.split)))
}

/// Programs of `job` equal the certified programs for the given types (all types when `types` is empty).
fn programs_equal(ctx: &Ctx, job: &str, types: &dyn Fn(&Program) -> bool, out: &mut Vec<String>) -> bool {
    let (fx, got) = match ctx.programs_of(job) { Ok(x) => x, Err(e) => { out.push(e); return false; } };
    let mut ok = true;
    for (t, want) in &fx.programs {
        if !types(want) { continue; }
        match got.get(t) {
            Some(g) if g == want => {}
            Some(g) => { ok = false; out.push(format!("{}: type {} recovered as [{}] but the certified program is [{}]", job, t, program_line(g), program_line(want))); }
            None => { ok = false; out.push(format!("{}: type {} has no program", job, t)); }
        }
    }
    ok
}

// ---------------------------------------------------------------- checks
fn run_check(name: &str, ctx: &Ctx) -> Result<(), Vec<String>> {
    let mut out = Vec::new();
    let mut ok = true;
    let jobs: Vec<&str> = JOBS.iter().map(|(j, _)| *j).collect();
    match name {
        "result_documents_well_formed" => {
            for job in &jobs {
                if let Err(e) = ctx.doc(job) { ok = false; out.push(e); }
            }
        }
        "predictions_follow_submitted_programs" => {
            for job in &jobs {
                let (fx, r) = match ctx.doc(job) { Ok(x) => x, Err(e) => { ok = false; out.push(e); continue } };
                let programs: BTreeMap<u32, Program> = r.programs.iter().map(|p| (p.r_type, p.clone())).collect();
                let submitted: BTreeMap<(String, String, String, u64), Option<u64>> = r.predictions.iter().map(|q| ((q.object.clone(), q.link.clone(), q.section.clone(), q.offset), q.word)).collect();
                let mut bad = 0;
                for o in &fx.corpus.objects {
                    for l in &o.links {
                        match replay_link(&fx.corpus, o, l, &programs) {
                            Ok(words) => for (sec, off, w) in words {
                                if submitted.get(&(o.id.clone(), l.id.clone(), sec.clone(), off)) != Some(&w) { bad += 1; }
                            },
                            Err(e) => { bad += 1; if bad < 4 { out.push(format!("{}: {}", job, e)); } }
                        }
                    }
                }
                if bad > 0 { ok = false; out.push(format!("{}: {} predictions differ from the verifier's replay of the submitted programs", job, bad)); }
            }
        }
        "predictions_match_legacy_outcomes" => {
            for job in &jobs {
                let (fx, r) = match ctx.doc(job) { Ok(x) => x, Err(e) => { ok = false; out.push(e); continue } };
                let mut bad = 0;
                for q in &r.predictions {
                    let key = (q.object.clone(), q.link.clone(), q.section.clone(), q.offset);
                    if fx.predictions.get(&key) != Some(&q.word) { bad += 1; }
                }
                if bad > 0 { ok = false; out.push(format!("{}: {} of {} predictions differ from the legacy linker's outcome", job, bad, r.predictions.len())); }
            }
        }
        "hidden_perturbations_replay" => {
            for job in &jobs {
                let (fx, programs) = match ctx.programs_of(job) { Ok(x) => x, Err(e) => { ok = false; out.push(e); continue } };
                let mut bad = 0; let mut total = 0;
                for h in &fx.hidden {
                    let o = fx.corpus.objects.iter().find(|o| o.id == h.object).unwrap();
                    match replay_link(&fx.corpus, o, &h.link, &programs) {
                        Ok(words) => {
                            let got: BTreeMap<(String, u64), Option<u64>> = words.into_iter().map(|(s, o, w)| ((s, o), w)).collect();
                            for (sec, off, want) in &h.sites {
                                total += 1;
                                if got.get(&(sec.clone(), *off)) != Some(want) { bad += 1; }
                            }
                        }
                        Err(e) => { bad += h.sites.len(); total += h.sites.len(); out.push(format!("{}: {}", job, e)); }
                    }
                }
                if bad > 0 { ok = false; out.push(format!("{}: {} of {} verifier-owned perturbation words differ from the legacy outcome", job, bad, total)); }
            }
        }
        "scalar_expression_and_scale" => {
            for job in ["public", "case_a", "case_e"] {
                let (fx, got) = match ctx.programs_of(job) { Ok(x) => x, Err(e) => { ok = false; out.push(e); continue } };
                for (t, want) in &fx.programs {
                    if want.pair.is_some() { continue; }
                    match got.get(t) {
                        Some(g) if g.expression == want.expression && g.scale == want.scale => {}
                        Some(g) => { ok = false; out.push(format!("{}: type {} recovered {} scale {}; certified {} scale {}", job, t, g.expression.text(), g.scale, want.expression.text(), want.scale)); }
                        None => { ok = false; out.push(format!("{}: type {} has no program", job, t)); }
                    }
                }
            }
        }
        "section_base_versus_place" => { ok &= programs_equal(ctx, "case_a", &|_| true, &mut out); }
        "negative_truncation_and_split_fields" => {
            ok &= programs_equal(ctx, "case_b", &|_| true, &mut out);
            ok &= hidden_ok(ctx, "case_b", &mut out);
        }
        "pair_coupling_and_carry" => {
            for job in ["public", "case_c", "case_f"] {
                ok &= programs_equal(ctx, job, &|p| p.pair.is_some(), &mut out);
            }
            ok &= hidden_ok(ctx, "case_c", &mut out);
        }
        "canonical_equivalence" => { ok &= programs_equal(ctx, "case_d", &|_| true, &mut out); }
        "reject_wrap_boundaries" => {
            ok &= programs_equal(ctx, "case_e", &|_| true, &mut out);
            ok &= hidden_ok(ctx, "case_e", &mut out);
        }
        "field_layout_recovery" => {
            for job in ["public", "case_b", "case_f"] {
                ok &= programs_equal(ctx, job, &|p| p.slices.len() > 1, &mut out);
            }
        }
        "cross_object_consistency" => {
            for job in ["public", "case_f"] {
                ok &= programs_equal(ctx, job, &|_| true, &mut out);
            }
        }
        "deterministic_output" => {
            let a = ctx.artifacts.iter().find(|a| a.job == "public").and_then(|a| a.text.clone());
            let b = ctx.artifacts.iter().find(|a| a.job == "public_repeat").and_then(|a| a.text.clone());
            match (a, b) {
                (Some(x), Some(y)) if x == y => { if let Err(e) = ctx.doc("public") { ok = false; out.push(e); } }
                (Some(_), Some(_)) => { ok = false; out.push("two runs over the public corpus produced different documents".into()); }
                _ => { ok = false; out.push("an artifact of the public corpus is missing".into()); }
            }
        }
        CONTROL => {
            // Deliberately wrong expectation: the public type 2 program with its
            // scale doubled. A correct candidate never matches it, so this check
            // must fail; a pass means the pipeline cannot see failure.
            let (fx, got) = match ctx.programs_of("public") { Ok(x) => x, Err(e) => { return Err(vec![e]); } };
            let mut want = fx.programs.get(&2).cloned().ok_or_else(|| vec!["public fixture has no type 2".to_string()])?;
            want.scale *= 2;
            match got.get(&2) {
                Some(g) if *g == want => {}
                _ => { ok = false; out.push("control: candidate does not match the perturbed program (expected)".into()); }
            }
        }
        _ => { ok = false; out.push(format!("unknown check {}", name)); }
    }
    if ok { Ok(()) } else { Err(out) }
}

fn hidden_ok(ctx: &Ctx, job: &str, out: &mut Vec<String>) -> bool {
    let (fx, programs) = match ctx.programs_of(job) { Ok(x) => x, Err(e) => { out.push(e); return false; } };
    let mut bad = 0;
    for h in &fx.hidden {
        let o = fx.corpus.objects.iter().find(|o| o.id == h.object).unwrap();
        match replay_link(&fx.corpus, o, &h.link, &programs) {
            Ok(words) => {
                let got: BTreeMap<(String, u64), Option<u64>> = words.into_iter().map(|(s, o, w)| ((s, o), w)).collect();
                for (sec, off, want) in &h.sites { if got.get(&(sec.clone(), *off)) != Some(want) { bad += 1; } }
            }
            Err(e) => { bad += 1; out.push(format!("{}: {}", job, e)); }
        }
    }
    if bad > 0 { out.push(format!("{}: {} verifier-owned perturbation words differ", job, bad)); }
    bad == 0
}

/// Self-check of the certified data: the verifier's own replay of the
/// certified programs must reproduce every certified prediction and every
/// hidden word. Run before grading so expectations cannot drift.
fn selfcheck(fixtures: &BTreeMap<String, Fixture>) -> Result<(), String> {
    for fx in fixtures.values() {
        for o in &fx.corpus.objects {
            for l in &o.links {
                let words = replay_link(&fx.corpus, o, l, &fx.programs).map_err(|e| format!("{}: {}", fx.id, e))?;
                for (sec, off, w) in words {
                    if fx.predictions.get(&(o.id.clone(), l.id.clone(), sec.clone(), off)) != Some(&w) {
                        return Err(format!("{}: certified prediction {}/{} {}+{} does not follow from the certified programs", fx.id, o.id, l.id, sec, off));
                    }
                }
            }
        }
        for h in &fx.hidden {
            let o = fx.corpus.objects.iter().find(|o| o.id == h.object).ok_or("hidden link names a missing object")?;
            let words = replay_link(&fx.corpus, o, &h.link, &fx.programs).map_err(|e| format!("{}: {}", fx.id, e))?;
            let got: BTreeMap<(String, u64), Option<u64>> = words.into_iter().map(|(s, o, w)| ((s, o), w)).collect();
            for (sec, off, want) in &h.sites {
                if got.get(&(sec.clone(), *off)) != Some(want) {
                    return Err(format!("{}: certified hidden word {}/{} {}+{} does not follow from the certified programs", fx.id, o.id, h.link.id, sec, off));
                }
            }
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut check = None;
    let mut fixtures_dir = PathBuf::from("/tests/fixtures");
    let mut artifacts_dir = PathBuf::from("/tmp/artifacts");
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--run" if i + 1 < args.len() => { check = Some(args[i + 1].clone()); i += 2; }
            "--fixtures" if i + 1 < args.len() => { fixtures_dir = PathBuf::from(&args[i + 1]); i += 2; }
            "--artifacts" if i + 1 < args.len() => { artifacts_dir = PathBuf::from(&args[i + 1]); i += 2; }
            "--list" => { for c in CHECKS { println!("{}", c); } return; }
            _ => { eprintln!("usage: judge --run <check> --fixtures <dir> --artifacts <dir>"); std::process::exit(2); }
        }
    }
    let name = match check { Some(c) => c, None => { eprintln!("--run required"); std::process::exit(2); } };
    let fixtures = match load_fixtures(&fixtures_dir) { Ok(f) => f, Err(e) => { println!("FAIL {}: cannot load certified fixtures: {}", name, e); std::process::exit(1); } };
    if let Err(e) = selfcheck(&fixtures) {
        println!("FAIL {}: certified data inconsistent: {}", name, e);
        std::process::exit(1);
    }
    let ctx = Ctx { fixtures, artifacts: load_artifacts(&artifacts_dir) };
    match run_check(&name, &ctx) {
        Ok(()) => { println!("{} {}", PASS_MARKER, name); }
        Err(reasons) => {
            println!("FAIL {}", name);
            for r in reasons.iter().take(12) { println!("  {}", r); }
            std::process::exit(1);
        }
    }
}
