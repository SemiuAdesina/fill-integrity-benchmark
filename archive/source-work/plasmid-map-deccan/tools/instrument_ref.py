#!/usr/bin/env python3
"""Patches the lab copy of the reference (lab/ref) with stderr tracing. Never
applied to the shipped source."""
import os, re
R = os.path.expanduser('~/benchmark-suite-work/plasmid-map-deccan/lab/ref/src')
p = os.path.join(R, 'solver/candidates.rs'); s = open(p).read()
s = s.replace('''pub fn candidates_at(problem: &Problem, ctx: &EnzymeContext, c: usize, t: usize) -> HashMap<usize, Vec<Candidate>> {''', '''pub fn candidates_at(problem: &Problem, ctx: &EnzymeContext, c: usize, t: usize) -> HashMap<usize, Vec<Candidate>> {
    let started = std::time::Instant::now();
    let out = candidates_inner(problem, ctx, c, t);
    eprintln!("  cand {:<10} c={:<5} t={} lengths={:<4} sets={:<8} {} ms", ctx.name, c, t, out.len(), out.values().map(|v| v.len()).sum::<usize>(), started.elapsed().as_millis());
    out
}

fn candidates_inner(problem: &Problem, ctx: &EnzymeContext, c: usize, t: usize) -> HashMap<usize, Vec<Candidate>> {''')
open(p, 'w').write(s)
p = os.path.join(R, 'solver/join.rs'); s = open(p).read()
s = s.replace('''pub fn join(problem: &Problem, contexts: &[EnzymeContext], c: usize, lists: &[&HashMap<usize, Vec<Candidate>>], best: &mut Option<Best>) {
    let names: Vec<String> = contexts.iter().map(|x| x.name.clone()).collect();''', '''pub static NODES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub fn join(problem: &Problem, contexts: &[EnzymeContext], c: usize, lists: &[&HashMap<usize, Vec<Candidate>>], best: &mut Option<Best>) {
    let started = std::time::Instant::now();
    let before = NODES.load(std::sync::atomic::Ordering::Relaxed);
    join_inner(problem, contexts, c, lists, best);
    let common = lists[0].keys().filter(|l| lists.iter().all(|m| m.contains_key(l))).count();
    eprintln!("  join c={:<5} sizes={:?} common_lengths={} nodes={} {} ms best={:?}", c, lists.iter().map(|m| m.values().map(|v| v.len()).sum::<usize>()).collect::<Vec<_>>(), common, NODES.load(std::sync::atomic::Ordering::Relaxed) - before, started.elapsed().as_millis(), best.as_ref().map(|b| (b.key.internal, b.key.deviation, b.key.cloning_site, b.key.insert_length)));
}

fn join_inner(problem: &Problem, contexts: &[EnzymeContext], c: usize, lists: &[&HashMap<usize, Vec<Candidate>>], best: &mut Option<Best>) {
    let names: Vec<String> = contexts.iter().map(|x| x.name.clone()).collect();''')
s = s.replace('''    fn dfs(&mut self, i: usize, count: u32, dev: u64, best: &mut Option<Best>) {
        if let Some(b) = best {''', '''    fn dfs(&mut self, i: usize, count: u32, dev: u64, best: &mut Option<Best>) {
        NODES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(b) = best {''')
open(p, 'w').write(s)
p = os.path.join(R, 'solver.rs'); s = open(p).read()
s = s.replace('''    for level in start..=(n * k) {
        let mut best: Option<join::Best> = None;''', '''    for level in start..=(n * k) {
        eprintln!("LEVEL {} (minimum per site: {:?})", level, states.iter().map(|s| (s.c, s.minimum.clone())).collect::<Vec<_>>());
        let mut best: Option<join::Best> = None;''')
open(p, 'w').write(s)
print("lab/ref instrumented")
