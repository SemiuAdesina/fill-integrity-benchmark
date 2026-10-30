//! `reloc-diag [--corpus DIR] [--result FILE] [--group NAME]...`
//!
//! Public diagnostics. Each group is independently runnable and reports
//! symptoms observed by comparing the result document with the corpus: which
//! sites, links, objects, bits or pair members disagree with the legacy
//! linker's recorded outcome. It compares; it does not evaluate programs and
//! it holds no expected answer. Frozen.

use reloc_recover::corpus::{Corpus, LinkResult, Object, Site};
use reloc_recover::elf::{ET_EXEC, ET_REL, EM_VX, SHN_ABS, SHN_UNDEF, SHT_PROGBITS};
use reloc_recover::program::Recovery;
use std::collections::BTreeMap;
use std::path::PathBuf;

const GROUPS: [&str; 7] = ["corpus_integrity", "result_schema", "scalar_replay", "field_replay", "pair_replay", "cross_object_consistency", "replay_links"];

struct Report {
    lines: Vec<String>,
    ok: bool,
}

impl Report {
    fn new() -> Report { Report { lines: Vec::new(), ok: true } }
    fn note(&mut self, s: String) { self.lines.push(format!("    {}", s)); }
    fn fail(&mut self, s: String) { self.ok = false; self.lines.push(format!("    ! {}", s)); }
}

fn hex(bits: u32, w: u64) -> String {
    if bits == 64 { format!("0x{:016x}", w) } else { format!("0x{:08x}", w) }
}

fn bit_list(mut m: u64) -> String {
    let mut v = Vec::new();
    let mut i = 0;
    while m != 0 {
        if m & 1 == 1 { v.push(i.to_string()); }
        m >>= 1;
        i += 1;
    }
    if v.is_empty() { "none".into() } else { v.join(",") }
}

fn symbol_kind(o: &Object, s: &Site) -> String {
    match o.elf.symbols.get(s.symbol) {
        Some(sym) if sym.shndx == SHN_ABS => format!("{} (absolute)", sym.name),
        Some(sym) if sym.shndx == SHN_UNDEF => format!("{} (undefined, resolved by the link map)", sym.name),
        Some(sym) => format!("{} (defined in {})", sym.name, o.elf.sections.get(sym.shndx as usize).map(|x| x.name.as_str()).unwrap_or("?")),
        None => "<missing symbol>".into(),
    }
}

/// Prediction lookup: (object, link, section, offset) -> word.
type Preds = BTreeMap<(String, String, String, u64), Option<u64>>;

fn preds_of(r: &Recovery) -> Preds {
    r.predictions.iter().map(|p| ((p.object.clone(), p.link.clone(), p.section.clone(), p.offset), p.word)).collect()
}

/// Every observed (visible) outcome: Some(word) for linked, None for a rejected site.
struct Obs<'a> {
    object: &'a Object,
    link_id: &'a str,
    site: &'a Site,
    legacy: Option<Option<u64>>, // Some(Some(w)) linked word; Some(None) rejected; None accepted-without-word
}

fn observations(corpus: &Corpus) -> Vec<Obs<'_>> {
    let mut v = Vec::new();
    for o in &corpus.objects {
        for l in &o.links {
            for s in &o.sites {
                let legacy = match &l.result {
                    LinkResult::Linked(_) => Some(l.legacy_word(&s.section, s.offset, s.container_bits)),
                    LinkResult::Failed { .. } => if l.rejected(&s.section, s.offset) == Some(true) { Some(None) } else { None },
                    LinkResult::Replay => continue,
                };
                if let Some(Some(None)) = Some(legacy.as_ref().map(|x| x.as_ref())) {
                    // linked but the word could not be read: treat as absent
                }
                v.push(Obs { object: o, link_id: &l.id, site: s, legacy });
            }
        }
    }
    v
}

fn group_corpus_integrity(corpus: &Corpus, rep: &mut Report) {
    let mut sites_per_type: BTreeMap<u32, usize> = BTreeMap::new();
    for o in &corpus.objects {
        if o.elf.e_type != ET_REL { rep.fail(format!("{}: not a relocatable object", o.id)); }
        if o.elf.e_machine != EM_VX { rep.fail(format!("{}: unexpected e_machine 0x{:x}", o.id, o.elf.e_machine)); }
        for s in &o.sites {
            *sites_per_type.entry(s.r_type).or_default() += 1;
            if corpus.reloc_type(s.r_type).is_none() { rep.fail(format!("{}: type {} is not in the manifest", o.id, s.r_type)); }
            if o.elf.symbols.get(s.symbol).is_none() { rep.fail(format!("{}: site {}+{} references symbol index {} which does not exist", o.id, s.section, s.offset, s.symbol)); }
        }
        // pair partners
        for (i, s) in o.sites.iter().enumerate() {
            let t = match corpus.reloc_type(s.r_type) { Some(t) => t, None => continue };
            if let Some(g) = &t.pair_group {
                let partner = |x: &Site| corpus.reloc_type(x.r_type).and_then(|u| u.pair_group.clone()).as_ref() == Some(g) && x.r_type != s.r_type && x.symbol == s.symbol && x.addend == s.addend && x.section == s.section;
                let prev = if i > 0 { partner(&o.sites[i - 1]) } else { false };
                let next = if i + 1 < o.sites.len() { partner(&o.sites[i + 1]) } else { false };
                if !prev && !next { rep.fail(format!("{}: pair-group site {}+{} (type {}) has no adjacent partner", o.id, s.section, s.offset, s.r_type)); }
            }
        }
        for l in &o.links {
            for sec in o.elf.sections.iter().filter(|s| s.sh_type == SHT_PROGBITS) {
                if !l.bases.contains_key(&sec.name) { rep.fail(format!("{}/{}: no base for section {}", o.id, l.id, sec.name)); }
            }
            for sym in o.elf.symbols.iter().filter(|s| s.shndx == SHN_UNDEF && !s.name.is_empty()) {
                match l.symbols.get(&sym.name) {
                    None => rep.fail(format!("{}/{}: undefined symbol {} is not resolved by the link map", o.id, l.id, sym.name)),
                    Some(r) => if let Some(sec) = &r.section { if !l.bases.contains_key(sec) { rep.fail(format!("{}/{}: symbol {} resolves into section {} which has no base", o.id, l.id, sym.name, sec)); } },
                }
            }
            match &l.result {
                LinkResult::Linked(out) => {
                    if out.e_type != ET_EXEC { rep.fail(format!("{}/{}: output is not an executable", o.id, l.id)); }
                    for sec in o.elf.sections.iter().filter(|s| s.sh_type == SHT_PROGBITS) {
                        match out.section_named(&sec.name) {
                            None => rep.fail(format!("{}/{}: output lacks section {}", o.id, l.id, sec.name)),
                            Some(os) => {
                                if os.data.len() != sec.data.len() { rep.fail(format!("{}/{}: section {} size differs between object and output", o.id, l.id, sec.name)); }
                                if Some(&os.addr) != l.bases.get(&sec.name) { rep.fail(format!("{}/{}: section {} address {} differs from the manifest base", o.id, l.id, sec.name, os.addr)); }
                            }
                        }
                    }
                    for s in &o.sites {
                        if l.legacy_word(&s.section, s.offset, s.container_bits).is_none() { rep.fail(format!("{}/{}: legacy word of {}+{} unreadable", o.id, l.id, s.section, s.offset)); }
                    }
                }
                LinkResult::Failed { overflow } => {
                    for (sec, off) in overflow {
                        if !o.sites.iter().any(|s| &s.section == sec && s.offset == *off) { rep.fail(format!("{}/{}: overflow entry {}+{} is not a site", o.id, l.id, sec, off)); }
                    }
                }
                LinkResult::Replay => {}
            }
        }
    }
    let links: usize = corpus.objects.iter().map(|o| o.links.len()).sum();
    rep.note(format!("{} relocation types, {} objects, {} links", corpus.types.len(), corpus.objects.len(), links));
    for t in &corpus.types {
        rep.note(format!("type {} ({}): {} sites per object set, container {} bits{}", t.id, t.name, sites_per_type.get(&t.id).copied().unwrap_or(0), t.container_bits,
            t.pair_group.as_ref().map(|g| format!(", pair group {}", g)).unwrap_or_default()));
    }
}

fn group_result_schema(corpus: &Corpus, result: &Recovery, rep: &mut Report) {
    match result.validate(corpus) {
        Ok(()) => rep.note(format!("{} programs, {} predictions, document is well formed and complete", result.programs.len(), result.predictions.len())),
        Err(e) => rep.fail(e),
    }
}

fn group_scalar_replay(corpus: &Corpus, preds: &Preds, rep: &mut Report) {
    let obs = observations(corpus);
    for t in corpus.types.iter().filter(|t| t.pair_group.is_none()) {
        let mut total = 0;
        let mut agree = 0;
        let mut shown = 0;
        for ob in obs.iter().filter(|o| o.site.r_type == t.id) {
            let key = (ob.object.id.clone(), ob.link_id.to_string(), ob.site.section.clone(), ob.site.offset);
            let pred = match preds.get(&key) { Some(p) => *p, None => continue };
            match ob.legacy {
                Some(legacy) => {
                    total += 1;
                    if pred == legacy { agree += 1; continue; }
                    if shown < 6 {
                        shown += 1;
                        let link = ob.object.links.iter().find(|l| l.id == ob.link_id).unwrap();
                        let ctx = format!("{}/{} {}+{} symbol {} addend {} site-section base {}", ob.object.id, ob.link_id, ob.site.section, ob.site.offset,
                            symbol_kind(ob.object, ob.site), ob.site.addend, link.bases.get(&ob.site.section).copied().unwrap_or(0));
                        match (pred, legacy) {
                            (Some(p), Some(l)) => rep.fail(format!("{}: predicted {} legacy {} differing bits {}", ctx, hex(ob.site.container_bits, p), hex(ob.site.container_bits, l), bit_list(p ^ l))),
                            (Some(p), None) => rep.fail(format!("{}: predicted {} but the legacy linker rejected this site", ctx, hex(ob.site.container_bits, p))),
                            (None, Some(l)) => rep.fail(format!("{}: predicted a rejection but the legacy word is {}", ctx, hex(ob.site.container_bits, l))),
                            (None, None) => {}
                        }
                    } else { rep.ok = false; }
                }
                None => {
                    // accepted without a word: a null prediction contradicts the record
                    total += 1;
                    if pred.is_some() { agree += 1; } else if shown < 6 { shown += 1; rep.fail(format!("{}/{} {}+{}: predicted a rejection but the legacy linker accepted this site", ob.object.id, ob.link_id, ob.site.section, ob.site.offset)); } else { rep.ok = false; }
                }
            }
        }
        rep.note(format!("type {}: {}/{} observed outcomes reproduced", t.id, agree, total));
    }
}

fn group_field_replay(corpus: &Corpus, preds: &Preds, rep: &mut Report) {
    let obs = observations(corpus);
    for t in &corpus.types {
        let mut legacy_changed = 0u64;
        let mut cand_changed = 0u64;
        let mut inside = 0usize;
        let mut outside = 0usize;
        let mut n = 0usize;
        for ob in obs.iter().filter(|o| o.site.r_type == t.id) {
            let legacy = match ob.legacy { Some(Some(w)) => w, _ => continue };
            legacy_changed |= legacy ^ ob.site.object_word;
            let key = (ob.object.id.clone(), ob.link_id.to_string(), ob.site.section.clone(), ob.site.offset);
            if let Some(Some(p)) = preds.get(&key) {
                n += 1;
                cand_changed |= p ^ ob.site.object_word;
                let diff = p ^ legacy;
                if diff != 0 {
                    if diff & !legacy_changed != 0 { outside += 1; } else { inside += 1; }
                }
            }
        }
        let only_legacy = legacy_changed & !cand_changed;
        let only_cand = cand_changed & !legacy_changed;
        rep.note(format!("type {}: legacy linker modified bits {{{}}}; predictions modified bits {{{}}}", t.id, bit_list(legacy_changed), bit_list(cand_changed)));
        if n == 0 {
            if legacy_changed != 0 { rep.fail(format!("type {}: no prediction for an observed site carries a patched word", t.id)); }
            continue;
        }
        if only_cand != 0 { rep.fail(format!("type {}: predictions modify bits {{{}}} the legacy linker never modified", t.id, bit_list(only_cand))); }
        if only_legacy != 0 { rep.fail(format!("type {}: the legacy linker modifies bits {{{}}} that no prediction modifies", t.id, bit_list(only_legacy))); }
        if inside + outside > 0 {
            rep.fail(format!("type {}: {} sites differ only inside the legacy-modified bits, {} sites differ in bits the legacy linker left alone", t.id, inside, outside));
        }
    }
}

fn group_pair_replay(corpus: &Corpus, preds: &Preds, rep: &mut Report) {
    let mut groups: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for t in &corpus.types { if let Some(g) = &t.pair_group { groups.entry(g.clone()).or_default().push(t.id); } }
    if groups.is_empty() { rep.note("no pair groups in the manifest".into()); return; }
    for (g, members) in &groups {
        let mut per_member: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
        let mut both = 0; let mut one = 0; let mut instances = 0;
        let mut by_first: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
        for o in &corpus.objects {
            for l in &o.links {
                if matches!(l.result, LinkResult::Replay) { continue; }
                let mut i = 0;
                while i + 1 < o.sites.len() {
                    let (a, b) = (&o.sites[i], &o.sites[i + 1]);
                    if members.contains(&a.r_type) && members.contains(&b.r_type) && a.r_type != b.r_type && a.symbol == b.symbol && a.addend == b.addend && a.section == b.section {
                        instances += 1;
                        let mut bad = 0;
                        for s in [a, b] {
                            let legacy = match &l.result {
                                LinkResult::Linked(_) => Some(l.legacy_word(&s.section, s.offset, s.container_bits)),
                                LinkResult::Failed { .. } => if l.rejected(&s.section, s.offset) == Some(true) { Some(None) } else { None },
                                LinkResult::Replay => None,
                            };
                            let pred = preds.get(&(o.id.clone(), l.id.clone(), s.section.clone(), s.offset)).copied().flatten();
                            let e = per_member.entry(s.r_type).or_default();
                            let agree = match legacy { Some(lw) => pred == lw, None => pred.is_some() };
                            e.1 += 1;
                            if agree { e.0 += 1; } else { bad += 1; }
                        }
                        let f = by_first.entry(a.r_type).or_default();
                        f.1 += 1;
                        if bad == 0 { f.0 += 1; }
                        if bad == 2 { both += 1; } else if bad == 1 { one += 1; }
                        i += 2;
                    } else { i += 1; }
                }
            }
        }
        rep.note(format!("pair group {}: {} observed instances", g, instances));
        for (tid, (ok, n)) in &per_member { rep.note(format!("  member type {}: {}/{} reproduced", tid, ok, n)); }
        for (tid, (ok, n)) in &by_first { rep.note(format!("  instances whose first record is type {}: {}/{} fully reproduced", tid, ok, n)); }
        if both + one > 0 { rep.fail(format!("pair group {}: {} instances wrong in both members, {} instances wrong in exactly one member", g, both, one)); }
    }
}

fn group_cross_object(corpus: &Corpus, preds: &Preds, rep: &mut Report) {
    let obs = observations(corpus);
    for t in &corpus.types {
        let mut per_object: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut per_link: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for ob in obs.iter().filter(|o| o.site.r_type == t.id) {
            let key = (ob.object.id.clone(), ob.link_id.to_string(), ob.site.section.clone(), ob.site.offset);
            let pred = match preds.get(&key) { Some(p) => *p, None => continue };
            let agree = match ob.legacy { Some(l) => pred == l, None => pred.is_some() };
            let eo = per_object.entry(ob.object.id.clone()).or_default();
            eo.1 += 1; if agree { eo.0 += 1; }
            let el = per_link.entry(ob.link_id.to_string()).or_default();
            el.1 += 1; if agree { el.0 += 1; }
        }
        let full: Vec<&String> = per_object.iter().filter(|(_, (a, n))| a == n).map(|(k, _)| k).collect();
        let partial: Vec<String> = per_object.iter().filter(|(_, (a, n))| a != n).map(|(k, (a, n))| format!("{} {}/{}", k, a, n)).collect();
        if partial.is_empty() {
            rep.note(format!("type {}: consistent across all {} objects", t.id, per_object.len()));
        } else if full.is_empty() {
            rep.fail(format!("type {}: reproduced in no object ({})", t.id, partial.join(", ")));
        } else {
            rep.fail(format!("type {}: fully reproduced in {:?} but not in {}", t.id, full, partial.join(", ")));
        }
        let bad_links: Vec<String> = per_link.iter().filter(|(_, (a, n))| a != n).map(|(k, (a, n))| format!("{} {}/{}", k, a, n)).collect();
        if !bad_links.is_empty() && !full.is_empty() { rep.note(format!("type {}: links with differences: {}", t.id, bad_links.join(", "))); }
    }
}

fn group_replay_links(corpus: &Corpus, preds: &Preds, rep: &mut Report) {
    let mut replay_sites = 0; let mut replay_null = 0; let mut bad = 0;
    for o in &corpus.objects {
        for l in &o.links {
            for s in &o.sites {
                let key = (o.id.clone(), l.id.clone(), s.section.clone(), s.offset);
                let pred = match preds.get(&key) { Some(p) => *p, None => { bad += 1; continue } };
                match &l.result {
                    LinkResult::Replay => { replay_sites += 1; if pred.is_none() { replay_null += 1; } }
                    LinkResult::Failed { .. } => {
                        let rejected = l.rejected(&s.section, s.offset) == Some(true);
                        if rejected && pred.is_some() { bad += 1; if bad <= 6 { rep.fail(format!("{}/{} {}+{}: the legacy linker rejected this site; predicted {}", o.id, l.id, s.section, s.offset, hex(s.container_bits, pred.unwrap()))); } }
                        if !rejected && pred.is_none() { bad += 1; if bad <= 6 { rep.fail(format!("{}/{} {}+{}: predicted a rejection where the legacy linker accepted", o.id, l.id, s.section, s.offset)); } }
                    }
                    LinkResult::Linked(_) => { if pred.is_none() { bad += 1; if bad <= 6 { rep.fail(format!("{}/{} {}+{}: predicted a rejection where the legacy linker produced a word", o.id, l.id, s.section, s.offset)); } } }
                }
            }
        }
    }
    if bad > 6 { rep.fail(format!("{} further rejection/acceptance disagreements", bad - 6)); }
    rep.note(format!("{} replay-link sites predicted, {} of them as rejections", replay_sites, replay_null));
}

fn main() {
    let mut corpus_dir = PathBuf::from("/app/corpus");
    let mut result_path = PathBuf::from("/app/relocations.json");
    let mut wanted: Vec<String> = Vec::new();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--corpus" if i + 1 < args.len() => { corpus_dir = PathBuf::from(&args[i + 1]); i += 2; }
            "--result" if i + 1 < args.len() => { result_path = PathBuf::from(&args[i + 1]); i += 2; }
            "--group" if i + 1 < args.len() => { wanted.push(args[i + 1].clone()); i += 2; }
            "--list" => { for g in GROUPS { println!("{}", g); } return; }
            other => { eprintln!("usage: reloc-diag [--corpus DIR] [--result FILE] [--group NAME]... [--list] (unexpected '{}')", other); std::process::exit(2); }
        }
    }
    for w in &wanted {
        if !GROUPS.contains(&w.as_str()) { eprintln!("unknown group '{}'; --list shows the groups", w); std::process::exit(2); }
    }
    let run = |g: &str| wanted.is_empty() || wanted.iter().any(|w| w == g);
    let corpus = match Corpus::load(&corpus_dir) {
        Ok(c) => c,
        Err(e) => { println!("GROUP corpus_integrity: FAIL\n    ! cannot load corpus: {}", e); std::process::exit(1); }
    };
    let mut all_ok = true;
    if run("corpus_integrity") {
        let mut rep = Report::new();
        group_corpus_integrity(&corpus, &mut rep);
        print_report("corpus_integrity", &rep); all_ok &= rep.ok;
    }
    let needs_result = GROUPS[1..].iter().any(|g| run(g));
    if !needs_result { std::process::exit(if all_ok { 0 } else { 1 }); }
    let result = match std::fs::read_to_string(&result_path).map_err(|e| e.to_string()).and_then(|t| Recovery::from_json(&t)) {
        Ok(r) => r,
        Err(e) => {
            println!("GROUP result_schema: FAIL\n    ! {}: {}", result_path.display(), e);
            for g in &GROUPS[2..] { if run(g) { println!("GROUP {}: SKIPPED (no readable result document)", g); } }
            std::process::exit(1);
        }
    };
    if run("result_schema") { let mut rep = Report::new(); group_result_schema(&corpus, &result, &mut rep); print_report("result_schema", &rep); all_ok &= rep.ok; }
    let preds = preds_of(&result);
    if run("scalar_replay") { let mut rep = Report::new(); group_scalar_replay(&corpus, &preds, &mut rep); print_report("scalar_replay", &rep); all_ok &= rep.ok; }
    if run("field_replay") { let mut rep = Report::new(); group_field_replay(&corpus, &preds, &mut rep); print_report("field_replay", &rep); all_ok &= rep.ok; }
    if run("pair_replay") { let mut rep = Report::new(); group_pair_replay(&corpus, &preds, &mut rep); print_report("pair_replay", &rep); all_ok &= rep.ok; }
    if run("cross_object_consistency") { let mut rep = Report::new(); group_cross_object(&corpus, &preds, &mut rep); print_report("cross_object_consistency", &rep); all_ok &= rep.ok; }
    if run("replay_links") { let mut rep = Report::new(); group_replay_links(&corpus, &preds, &mut rep); print_report("replay_links", &rep); all_ok &= rep.ok; }
    std::process::exit(if all_ok { 0 } else { 1 });
}

fn print_report(name: &str, rep: &Report) {
    println!("GROUP {}: {}", name, if rep.ok { "PASS" } else { "FAIL" });
    for l in &rep.lines { println!("{}", l); }
}
