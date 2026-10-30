
//! Recognition sites on a circle, and where the backbone's sites end up on the
//! construct.
//!
//! Positions are cut positions: a cut at `p` lies between base `p - 1` and
//! base `p`, so a circle of `n` bases has cut positions `0..n`. A recognition
//! window that starts at base `i` covers bases `i, i + 1, ..., i + w - 1`,
//! wrapping past the last base; the enzyme cuts `cut_offset` bases into the
//! window, at `(i + cut_offset) mod n`.
//!
//! The construct is the backbone opened at cloning site `c` with `l` insert
//! bases placed between backbone base `c - 1` and backbone base `c`. A
//! backbone window that contains both of those bases is torn apart by the
//! insertion and says nothing about the construct; every other backbone site
//! survives, at its old position when it lies before the opening and `l`
//! further along when it lies at or after it.

use crate::input::Enzyme;

/// One occurrence of a recognition sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Site {
    /// Index of the window's first base.
    pub start: usize,
    /// Cut position on the same circle.
    pub cut: usize,
}

/// Every occurrence of `site` on the circular sequence, ascending by start.
pub fn scan(seq: &[u8], site: &[u8], cut_offset: usize) -> Vec<Site> {
    let n = seq.len();
    let w = site.len();
    let mut out = Vec::new();
    if n == 0 || w == 0 || w > n {
        return out;
    }
    for start in 0..n {
        let hit = (0..w).all(|k| seq[(start + k) % n] == site[k]);
        if hit {
            out.push(Site { start, cut: (start + cut_offset) % n });
        }
    }
    out
}

/// Whether a window of `window` bases starting at `start` on a circle of `v`
/// bases contains both base `(c - 1) mod v` and base `c mod v`.
pub fn spans_junction(start: usize, window: usize, c: usize, v: usize) -> bool {
    let before = (c + v - 1) % v;
    let after = c % v;
    let covers = |base: usize| (0..window).any(|k| (start + k) % v == base);
    covers(before) && covers(after)
}

/// The construct positions of the backbone sites that survive an insertion of
/// `l` bases at cloning site `c`, ascending and without duplicates.
pub fn required_from_sites(sites: &[Site], window: usize, v: usize, c: usize, l: usize) -> Vec<usize> {
    let mut out: Vec<usize> = sites
        .iter()
        .filter(|s| !spans_junction(s.start, window, c, v))
        .map(|s| if s.cut < c { s.cut } else { s.cut + l })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// [`required_from_sites`] for one enzyme of a backbone.
pub fn required_positions(backbone: &[u8], enzyme: &Enzyme, c: usize, l: usize) -> Vec<usize> {
    let sites = scan(backbone, &enzyme.site, enzyme.cut_offset);
    required_from_sites(&sites, enzyme.site.len(), backbone.len(), c, l)
}

/// Whether the enzyme keeps at least one surviving backbone site when the
/// backbone is opened at `c`. The input format requires this of every panel
/// enzyme at every cloning site.
pub fn anchored(backbone: &[u8], enzyme: &Enzyme, c: usize) -> bool {
    !required_positions(backbone, enzyme, c, 0).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enzyme(site: &str, offset: usize) -> Enzyme {
        Enzyme { name: "E".to_string(), site: site.as_bytes().to_vec(), cut_offset: offset }
    }

    #[test]
    fn scans_across_the_origin() {
        // GAATTC wraps: ...TTC at the end, GAA at the start would be "GAATTC" only if
        // the sequence is circular. Here: seq = "ATTCxxxxGA" -> window starts at 8.
        let seq = b"ATTCACGTGA";
        let hits = scan(seq, b"GAATTC", 1);
        assert_eq!(hits, vec![Site { start: 8, cut: 9 }]);
    }

    #[test]
    fn junction_windows_are_dropped_and_later_sites_shift() {
        // Two sites: window at 0..6 (cut 1) and window at 10..16 (cut 11), v = 20.
        let sites = vec![Site { start: 0, cut: 1 }, Site { start: 10, cut: 11 }];
        assert_eq!(required_from_sites(&sites, 6, 20, 8, 100), vec![1, 111]);
        // Opening inside the second window (between base 12 and 13) tears it.
        assert_eq!(required_from_sites(&sites, 6, 20, 13, 100), vec![1]);
        // Opening at 0 tears a window that wraps around the origin.
        let wrapping = vec![Site { start: 17, cut: 18 }];
        assert_eq!(required_from_sites(&wrapping, 6, 20, 0, 5), Vec::<usize>::new());
        assert_eq!(required_from_sites(&wrapping, 6, 20, 5, 5), vec![23]);
    }

    #[test]
    fn anchoring_needs_a_surviving_site() {
        let seq = b"GAATTCAAAAAAAAAA";
        let e = enzyme("GAATTC", 1);
        assert!(anchored(seq, &e, 8));
        assert!(!anchored(seq, &e, 3));
    }
}
