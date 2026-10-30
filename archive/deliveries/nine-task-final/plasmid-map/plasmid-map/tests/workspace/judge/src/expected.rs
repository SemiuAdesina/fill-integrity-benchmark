//! Verifier-owned expectations.
//!
//! These are the same properties the V1 graded harness asserted, moved out of
//! the candidate's process and into the judge. The candidate never links this
//! crate, and at run time the judge binary lives in a root-owned 0700 tree, so
//! the expectations can neither be linked nor read by the graded account.
use plasmid_model::record::Shape;

/// Properties a recovered map must have, per case, exactly as V1 asserted them.
#[derive(Clone,Copy,Debug,Default)]
pub struct MapChecks{
 /// V1: `r[0].position == 0`.
 pub first_position_zero:bool,
 /// V1: `r.iter().map(|s|s.position).min() == Some(0)`.
 pub min_position_zero:bool,
 /// V1: `assert_ne!(a.forward, b.forward)` for the named junction pair.
 pub differing_orientation:Option<(&'static str,&'static str)>,
 /// V1: `assert!(a.forward)` for the named site.
 pub forward_site:Option<&'static str>,
}

/// The complete expectation for one case.
#[derive(Clone,Copy,Debug)]
pub struct Expect{
 pub shape:Shape,
 /// Expected fragment multiset, when the case asserted one.
 pub frags:Option<&'static [u16]>,
 /// Map properties, when the case required a recovered map.
 pub map:Option<MapChecks>,
 /// A fully correct map, used only to stage records for the negative control.
 pub canonical_sites:&'static [(u16,&'static str,&'static str,bool)],
}

const BASE_MAP:&[(u16,&str,&str,bool)]=&[(0,"a1","A",true),(10,"a2","A",false),(15,"b1","B",false)];
const COMPOUND_MAP:&[(u16,&str,&str,bool)]=&[(0,"a1","A",true),(10,"a2","A",false),(16,"b1","B",false)];
const JUNCTION:Option<(&str,&str)>=Some(("a1","b1"));

fn frag(shape:Shape,frags:&'static [u16],map:Option<MapChecks>,sites:&'static [(u16,&'static str,&'static str,bool)])->Expect{
 Expect{shape,frags:Some(frags),map,canonical_sites:sites}
}

/// Expectation for a case id, or `None` if the id is not part of the population.
pub fn expect(id:&str)->Option<Expect>{
 let base=MapChecks::default();
 Some(match id{
  // Purpose: exercise a two-cut circular digest across the origin. Accepted
  // rule: intervals include the final-to-first wrap. A linear-only digest is
  // the plausible incorrect implementation this case rejects.
  "circular_wrap"=>frag(Shape::Fragments,&[5,15],None,&[]),
  // Purpose: cover the size-one cut boundary. Accepted rule: one circular cut
  // yields the whole molecule as one fragment. Returning zero or no fragments
  // is the plausible incorrect implementation this case rejects.
  "single_cut"=>frag(Shape::Fragments,&[20],None,&[]),
  // Purpose: make two observations select one shared reconstruction. Accepted
  // rule: single- and double-digest evidence is optimized jointly. Choosing a
  // separate best map per observation is the plausible incorrect approach.
  "joint_evidence"=>frag(Shape::Map{digest:true},&[5,5,10],Some(base),BASE_MAP),
  // Purpose: distinguish otherwise viable maps by an oriented junction.
  // Accepted rule: circular neighbors must have the requested orientation
  // parity. Ignoring orientation is the plausible incorrect implementation.
  "oriented_junction"=>Expect{shape:Shape::Map{digest:false},frags:None,map:Some(MapChecks{differing_orientation:JUNCTION,..base}),canonical_sites:BASE_MAP},
  // Purpose: grade rotation equivalence deterministically. Accepted rule: all
  // rotations participate in the canonical byte minimum. Keeping the first
  // encountered origin is the plausible incorrect implementation.
  "canonical_rotation"=>Expect{shape:Shape::Map{digest:false},frags:None,map:Some(MapChecks{min_position_zero:true,..base}),canonical_sites:BASE_MAP},
  // Purpose: grade reverse-equivalent circular maps. Accepted rule: reversal
  // reflects positions and flips every orientation before canonical choice.
  // Canonicalizing rotations only is the plausible incorrect implementation.
  "canonical_reversal"=>Expect{shape:Shape::Map{digest:false},frags:None,map:Some(MapChecks{forward_site:Some("a1"),..base}),canonical_sites:BASE_MAP},
  // Purpose: exercise the inclusive tolerance endpoint. Accepted rule: paired
  // absolute error equal to tolerance is accepted. Requiring strict inequality
  // is the plausible incorrect implementation this case rejects.
  "tolerance_endpoint"=>frag(Shape::Map{digest:true},&[10,10],Some(base),BASE_MAP),
  // Purpose: ensure reconstruction remains predictive beyond fitted output.
  // Accepted rule: the returned map must produce a withheld enzyme digest.
  // Hard-coding only supplied fragment observations is the plausible mistake.
  "withheld_digest"=>frag(Shape::Map{digest:true},&[20],Some(base),BASE_MAP),
  // Purpose: combine the public rules in one load-bearing reconstruction.
  // Accepted rules: circular wrap, repeated fragments, endpoint noise, junction
  // parity and canonical normalization all compose. Any local-only partial that
  // omits one interaction is the plausible incorrect implementation rejected.
  "compound_case"=>Expect{shape:Shape::Map{digest:true},frags:Some(&[4,6,10]),map:Some(MapChecks{first_position_zero:true,differing_orientation:JUNCTION,..base}),canonical_sites:COMPOUND_MAP},
  _=>return None,
 })
}
