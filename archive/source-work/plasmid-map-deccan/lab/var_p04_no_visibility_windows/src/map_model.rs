
//! The map document as a value, its physical validity, and the objective.
//! **This module defines which map is the answer.**
//!
//! A map names a cloning site and an insert length and gives, for every panel
//! enzyme, every cut position on the construct circle of `V + L` bases. The
//! positions outside the closed insert region `[c, c + L]` must be exactly the
//! backbone's surviving sites for that opening; the positions inside it are
//! the insert-region sites, at most `max_internal_sites` per enzyme. The map
//! also carries the bands it predicts for every held-out digest, which must
//! be what its own positions produce.
//!
//! Among valid maps that reproduce every observed digest, the answer is the
//! least under the order: fewest insert-region sites in total; then least
//! total deviation over the observed digests; then the lexicographically least
//! list of `(enzyme, position)` pairs, enzymes ascending by name and positions
//! ascending; then the smaller cloning site; then the shorter insert.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::digest;
use crate::input::{Band, Problem};
use crate::sites;

/// A map document.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlasmidMap {
    pub cloning_site: usize,
    pub insert_length: usize,
    /// Enzyme name to ascending cut positions on the construct circle.
    pub cuts: BTreeMap<String, Vec<usize>>,
    /// Held-out digest id to predicted bands.
    pub held_out: BTreeMap<String, Vec<Band>>,
}

impl PlasmidMap {
    /// Construct length, `V + L`.
    pub fn n(&self, problem: &Problem) -> usize {
        problem.v() + self.insert_length
    }
}

/// The predicted bands of every held-out digest for these positions.
pub fn predictions(problem: &Problem, insert_length: usize, cuts: &BTreeMap<String, Vec<usize>>) -> BTreeMap<String, Vec<Band>> {
    let n = problem.v() + insert_length;
    problem
        .held_out
        .iter()
        .map(|h| (h.id.clone(), digest::group(&digest::predicted(cuts, &h.enzymes, n, problem.detection_limit))))
        .collect()
}

/// Checks physical validity. `Err` names the first rule the map breaks.
pub fn validate(problem: &Problem, map: &PlasmidMap) -> Result<(), String> {
    if !problem.cloning_sites.iter().any(|s| s.position == map.cloning_site) {
        return Err(format!("cloning site {} is not one of the backbone's", map.cloning_site));
    }
    if map.insert_length < problem.insert_min || map.insert_length > problem.insert_max {
        return Err(format!("insert length {} is outside the stated bounds", map.insert_length));
    }
    let c = map.cloning_site;
    let l = map.insert_length;
    let n = map.n(problem);
    let panel: Vec<&String> = problem.enzymes.iter().map(|e| &e.name).collect();
    let listed: Vec<&String> = map.cuts.keys().collect();
    if listed != panel {
        return Err("cuts must name exactly the panel enzymes".to_string());
    }
    for enzyme in &problem.enzymes {
        let positions = &map.cuts[&enzyme.name];
        for w in positions.windows(2) {
            if w[0] >= w[1] {
                return Err(format!("{} positions are not ascending and distinct", enzyme.name));
            }
        }
        if positions.iter().any(|&p| p >= n) {
            return Err(format!("{} has a position beyond the construct", enzyme.name));
        }
        let required = sites::required_positions(&problem.backbone.seq, enzyme, c, l);
        let outside: Vec<usize> = positions.iter().copied().filter(|&p| p < c || p > c + l).collect();
        if outside != required {
            return Err(format!("{} positions outside the insert region do not match the backbone's sites", enzyme.name));
        }
        let inside = positions.iter().filter(|&&p| p >= c && p <= c + l).count();
        if inside > problem.max_internal as usize {
            return Err(format!("{} has {} insert-region sites, more than allowed", enzyme.name, inside));
        }
    }
    let expected = predictions(problem, l, &map.cuts);
    if map.held_out != expected {
        return Err("held-out predictions are not what the map's own positions produce".to_string());
    }
    Ok(())
}

/// Total number of insert-region sites over the panel. Assumes a valid map.
pub fn internal_count(map: &PlasmidMap) -> u32 {
    let c = map.cloning_site;
    let l = map.insert_length;
    map.cuts.values().flat_map(|v| v.iter()).filter(|&&p| p >= c && p <= c + l).count() as u32
}

/// Total deviation over the observed digests, or the first digest the map
/// does not reproduce.
pub fn evaluate(problem: &Problem, map: &PlasmidMap) -> Result<u64, String> {
    let n = map.n(problem);
    let mut total = 0u64;
    for d in &problem.observed {
        let predicted = digest::predicted(&map.cuts, &d.enzymes, n, problem.detection_limit);
        match digest::deviation(&predicted, &d.bands, problem.error_percent) {
            Some(dev) => total += dev,
            None => return Err(format!("digest {} is not reproduced", d.id)),
        }
    }
    Ok(total)
}

/// The objective as a totally ordered key; smaller is better.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Key {
    pub internal: u32,
    pub deviation: u64,
    pub positions: Vec<(String, usize)>,
    pub cloning_site: usize,
    pub insert_length: usize,
}

impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        self.internal
            .cmp(&other.internal)
            .then(self.deviation.cmp(&other.deviation))
            .then_with(|| self.positions.cmp(&other.positions))
            .then(self.cloning_site.cmp(&other.cloning_site))
            .then(self.insert_length.cmp(&other.insert_length))
    }
}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The key of a valid map with the given total deviation.
pub fn key(map: &PlasmidMap, deviation: u64) -> Key {
    Key {
        internal: internal_count(map),
        deviation,
        positions: map.cuts.iter().flat_map(|(e, ps)| ps.iter().map(move |&p| (e.clone(), p))).collect(),
        cloning_site: map.cloning_site,
        insert_length: map.insert_length,
    }
}

/// Validity, reproduction of every observed digest, and the key, in one call.
pub fn key_of(problem: &Problem, map: &PlasmidMap) -> Result<Key, String> {
    validate(problem, map)?;
    let deviation = evaluate(problem, map)?;
    Ok(key(map, deviation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_order_lexicographically() {
        let a = Key { internal: 1, deviation: 50, positions: vec![("A".into(), 5)], cloning_site: 3, insert_length: 10 };
        let mut b = a.clone();
        b.deviation = 10;
        assert!(b < a);
        b.internal = 2;
        assert!(a < b);
        let mut c = a.clone();
        c.positions = vec![("A".into(), 4)];
        assert!(c < a);
        let mut d = a.clone();
        d.positions = vec![("A".into(), 5), ("B".into(), 1)];
        assert!(a < d, "a proper prefix sorts first");
    }
}
