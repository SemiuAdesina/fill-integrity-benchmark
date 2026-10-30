#!/usr/bin/env python3
"""Tracing for the lab copy of the branch-and-bound reference (lab/ref). Never applied to the shipped source."""
import os
R = os.path.expanduser('~/benchmark-suite-work/plasmid-map-deccan/lab/ref/src')
p = os.path.join(R, 'solver/bnb.rs'); s = open(p).read()
s = s.replace('''    fn recurse(&self, mut s: State, constraints: &mut Vec<Constraint>, best: &mut Option<Best>, warm: &mut Vec<f64>) {
        if !self.propagate''', '''    fn recurse(&self, mut s: State, constraints: &mut Vec<Constraint>, best: &mut Option<Best>, warm: &mut Vec<f64>) {
        NODES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if !self.propagate''')
s = s.replace('''    fn leaf(&self, s: &State, best: &mut Option<Best>) {
        let problem''', '''    fn leaf(&self, s: &State, best: &mut Option<Best>) {
        LEAVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let problem''')
s = s.replace('''/// Searches one distribution of insert-region sites at one cloning site,''', '''pub static NODES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static LEAVES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Searches one distribution of insert-region sites at one cloning site,''')
s = s.replace('''    let mut warm: Vec<f64> = Vec::new();
    search.recurse(state, &mut constraints, best, &mut warm);
}''', '''    let mut warm: Vec<f64> = Vec::new();
    let t0 = std::time::Instant::now();
    let n0 = NODES.load(std::sync::atomic::Ordering::Relaxed);
    let f0 = LEAVES.load(std::sync::atomic::Ordering::Relaxed);
    search.recurse(state, &mut constraints, best, &mut warm);
    eprintln!("  c={:<5} composition={:?} nodes={} leaves={} {} ms best={:?}", ctx.c, composition, NODES.load(std::sync::atomic::Ordering::Relaxed) - n0, LEAVES.load(std::sync::atomic::Ordering::Relaxed) - f0, t0.elapsed().as_millis(), best.as_ref().map(|b| (b.key.internal, b.key.deviation, b.key.insert_length)));
}''')
open(p, 'w').write(s)
p = os.path.join(R, 'solver.rs'); s = open(p).read()
s = s.replace('''    for level in start..=(n * k) {
        let mut best: Option<bnb::Best> = None;''', '''    for level in start..=(n * k) {
        eprintln!("LEVEL {} minima {:?}", level, contexts.iter().map(|x| (x.c, x.minimum.clone())).collect::<Vec<_>>());
        let mut best: Option<bnb::Best> = None;''')
open(p, 'w').write(s)
print("lab/ref traced")
