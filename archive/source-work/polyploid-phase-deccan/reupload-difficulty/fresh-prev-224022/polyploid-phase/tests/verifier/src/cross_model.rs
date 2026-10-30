
//! Domain types for an autotetraploid biparental cross.
//!
//! The types here make the two rules that are easiest to get wrong structural
//! rather than conventional: an inheritance state is an unordered pair, and a
//! parent always has exactly four homologs.

/// Number of homologs carried by an autotetraploid parent.
pub const HOMOLOGS: usize = 4;

/// Number of distinct unordered inheritance states, `C(4,2) + 4`.
pub const STATES: usize = 10;

/// The two parents of the cross. They are never exchanged with each other.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Parent {
    A,
    B,
}

impl Parent {
    /// Both parents in canonical order. `A` always precedes `B`.
    pub const BOTH: [Parent; 2] = [Parent::A, Parent::B];

    pub fn index(self) -> usize {
        match self {
            Parent::A => 0,
            Parent::B => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Parent::A => "A",
            Parent::B => "B",
        }
    }
}

/// An unordered pair of homolog indices: the two homologs a progeny received
/// from one parent at one marker.
///
/// The invariant `a <= b` is established at construction and cannot be broken
/// afterwards, because the fields are private. `(1,2)` and `(2,1)` are the same
/// state and both produce the same value.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub struct State {
    a: u8,
    b: u8,
}

impl State {
    /// Builds a state from two homolog indices in either order.
    ///
    /// Returns `None` if either index is outside `0..4`.
    pub fn new(x: u8, y: u8) -> Option<State> {
        if x >= HOMOLOGS as u8 || y >= HOMOLOGS as u8 {
            return None;
        }
        let (a, b) = if x <= y { (x, y) } else { (y, x) };
        Some(State { a, b })
    }

    pub fn low(self) -> u8 {
        self.a
    }

    pub fn high(self) -> u8 {
        self.b
    }

    /// True when the same homolog was received twice: the double reduction
    /// form. Legal only where the cross permits it for that parent and marker.
    pub fn is_double_reduction(self) -> bool {
        self.a == self.b
    }

    /// Position of this state in the canonical ordering documented in
    /// `spec/scoring-model.md`.
    pub fn canonical_index(self) -> usize {
        ALL_STATES
            .iter()
            .position(|s| *s == self)
            .expect("every constructible state appears in ALL_STATES")
    }

    /// Number of inherited homologs that differ between two states, treating
    /// each state as a multiset of size two.
    ///
    /// This is the `k` of the transition term. The multiset treatment is what
    /// makes `{1,1}` to `{1,2}` count as one change rather than two.
    pub fn switch_count(self, next: State) -> u32 {
        let mut shared = 0u32;
        let mut right = [next.a, next.b];
        for left in [self.a, self.b] {
            for slot in right.iter_mut() {
                if *slot == left {
                    *slot = u8::MAX;
                    shared += 1;
                    break;
                }
            }
        }
        2 - shared
    }

    /// Relabels both homologs through `perm`, where `perm[h]` is the new index
    /// of the homolog currently labelled `h`, and re-sorts the pair.
    pub fn remap(self, perm: &[u8; HOMOLOGS]) -> State {
        State::new(perm[self.a as usize], perm[self.b as usize])
            .expect("a permutation of 0..4 stays within 0..4")
    }
}

/// The ten legal states in canonical order.
pub const ALL_STATES: [State; STATES] = [
    State { a: 0, b: 0 },
    State { a: 0, b: 1 },
    State { a: 0, b: 2 },
    State { a: 0, b: 3 },
    State { a: 1, b: 1 },
    State { a: 1, b: 2 },
    State { a: 1, b: 3 },
    State { a: 2, b: 2 },
    State { a: 2, b: 3 },
    State { a: 3, b: 3 },
];

/// One parent's four homolog rows, each of length `markers`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Haplotypes {
    rows: [Vec<u8>; HOMOLOGS],
}

impl Haplotypes {
    pub fn new(rows: [Vec<u8>; HOMOLOGS]) -> Haplotypes {
        Haplotypes { rows }
    }

    pub fn zeros(markers: usize) -> Haplotypes {
        Haplotypes {
            rows: [vec![0; markers], vec![0; markers], vec![0; markers], vec![0; markers]],
        }
    }

    pub fn row(&self, homolog: usize) -> &[u8] {
        &self.rows[homolog]
    }

    pub fn rows(&self) -> &[Vec<u8>; HOMOLOGS] {
        &self.rows
    }

    pub fn markers(&self) -> usize {
        self.rows[0].len()
    }

    pub fn set(&mut self, homolog: usize, marker: usize, allele: u8) {
        self.rows[homolog][marker] = allele;
    }

    /// Alternate allele count across the four homologs at one marker.
    pub fn dosage_at(&self, marker: usize) -> u8 {
        (0..HOMOLOGS).map(|h| self.rows[h][marker]).sum()
    }

    /// Reorders rows so that the row currently at `h` lands at `perm[h]`.
    pub fn remap(&self, perm: &[u8; HOMOLOGS]) -> Haplotypes {
        let mut out = Haplotypes::zeros(self.markers());
        for (h, target) in perm.iter().enumerate() {
            out.rows[usize::from(*target)] = self.rows[h].clone();
        }
        out
    }
}

/// One progeny's inheritance state path through both parents.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProgenyPath {
    pub from_a: Vec<State>,
    pub from_b: Vec<State>,
}

impl ProgenyPath {
    pub fn states(&self, parent: Parent) -> &[State] {
        match parent {
            Parent::A => &self.from_a,
            Parent::B => &self.from_b,
        }
    }

    pub fn states_mut(&mut self, parent: Parent) -> &mut Vec<State> {
        match parent {
            Parent::A => &mut self.from_a,
            Parent::B => &mut self.from_b,
        }
    }
}

/// A complete answer: both parents' haplotypes and every progeny's path.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reconstruction {
    pub haplotypes_a: Haplotypes,
    pub haplotypes_b: Haplotypes,
    pub paths: Vec<ProgenyPath>,
}

impl Reconstruction {
    pub fn haplotypes(&self, parent: Parent) -> &Haplotypes {
        match parent {
            Parent::A => &self.haplotypes_a,
            Parent::B => &self.haplotypes_b,
        }
    }

    pub fn haplotypes_mut(&mut self, parent: Parent) -> &mut Haplotypes {
        match parent {
            Parent::A => &mut self.haplotypes_a,
            Parent::B => &mut self.haplotypes_b,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_is_unordered() {
        assert_eq!(State::new(2, 1), State::new(1, 2));
    }

    #[test]
    fn all_states_are_in_canonical_index_order() {
        for (i, s) in ALL_STATES.iter().enumerate() {
            assert_eq!(s.canonical_index(), i);
        }
    }

    #[test]
    fn switch_count_treats_states_as_multisets() {
        let repeated = State::new(1, 1).unwrap();
        let mixed = State::new(1, 2).unwrap();
        assert_eq!(repeated.switch_count(mixed), 1);
        assert_eq!(mixed.switch_count(repeated), 1);
        assert_eq!(repeated.switch_count(repeated), 0);
        assert_eq!(repeated.switch_count(State::new(2, 3).unwrap()), 2);
    }
}
