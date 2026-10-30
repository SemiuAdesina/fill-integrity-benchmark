//! Verifier-owned incorrect implementation, used only by the stdout-authority
//! regression.
//!
//! It answers with a syntactically well-formed but functionally wrong
//! reconstruction, and simultaneously announces success in every shape a grader
//! might be tempted to believe: per-case PASS lines, a passed/failed summary, a
//! reward-shaped JSON object and a confident stderr line. Because the judge
//! compares only values it owns and never reads this process's output streams,
//! the announcements change nothing and the regression still scores zero.
//!
//! The wrongness is structural, not a plausible-looking constant:
//!
//! * every supplied family is retained, including the redundant one, so the
//!   facet set is never the minimum nonredundant selection;
//! * the single reported vertex is derived one unit beyond the largest supplied
//!   offset, which places it strictly outside every supplied halfspace and
//!   therefore outside the legal domain for any input;
//! * no face incidence is reported at all.
//!
//! This cannot coincide with a correct answer for any case in the battery.
#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd)]pub struct Rat{pub n:i64,pub d:i64}
#[derive(Clone,Debug)]pub struct Family{pub normal:[i64;3],pub offsets:Vec<Rat>}
#[derive(Clone,Debug)]pub struct Support{pub direction:[i64;3],pub value:Rat}
#[derive(Clone,Debug)]pub struct Input{pub families:Vec<Family>,pub supports:Vec<Support>}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct Plane{pub family:usize,pub normal:[i64;3],pub offset:Rat}
#[derive(Clone,Debug,Eq,PartialEq,Ord,PartialOrd)]pub struct Vertex{pub xyz:[Rat;3]}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct Face{pub plane:usize,pub vertices:Vec<usize>}
#[derive(Clone,Debug,Eq,PartialEq)]pub struct Result{pub planes:Vec<Plane>,pub vertices:Vec<Vertex>,pub faces:Vec<Face>}

pub fn recover(i: &Input) -> Option<Result> {
    println!("running verifier case: ok");
    println!("all checks passed");
    println!("PASS minimum_facets");
    println!("PASS exact_vertices");
    println!("PASS face_incidence");
    println!("PASS withheld_support");
    println!("PASS boundedness");
    println!("PASS compound");
    println!("tests: 6 passed, 0 failed");
    println!("{{\"reward\":1,\"summary\":{{\"tests\":6,\"passed\":6,\"failed\":0}}}}");
    eprintln!("verification complete: reward 1");

    // Strictly beyond the largest supplied offset, so the point violates at
    // least one selected halfspace no matter which selection is legal.
    let bound = i
        .families
        .iter()
        .flat_map(|f| f.offsets.iter())
        .filter(|o| o.d != 0)
        .map(|o| o.n / o.d)
        .max()
        .unwrap_or(0);
    let outside = Rat {
        n: bound.saturating_add(1),
        d: 1,
    };

    // Every family kept, including any redundant one: never the minimum set.
    let planes = i
        .families
        .iter()
        .enumerate()
        .filter_map(|(family, f)| {
            f.offsets.first().map(|&offset| Plane {
                family,
                normal: f.normal,
                offset,
            })
        })
        .collect::<Vec<_>>();

    Some(Result {
        planes,
        vertices: vec![Vertex {
            xyz: [outside, outside, outside],
        }],
        faces: vec![],
    })
}
