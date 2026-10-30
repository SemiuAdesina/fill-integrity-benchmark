
//! Diagnostics: the frozen model on hand-built cases. Nothing here is graded;
//! these show how the pieces behave so the search can be built on them.

use std::collections::BTreeMap;

use plasmid_map::digest::{deviation, expand, fragments, group, predicted, tolerance};
use plasmid_map::input::{parse_problem, Band};
use plasmid_map::map_model::{evaluate, internal_count, key, validate, PlasmidMap};
use plasmid_map::result_output::{decode, encode};
use plasmid_map::sites::{required_positions, scan, spans_junction};

/// A 60-base backbone with one EcoRI site (window at 10) and one PstI site (window at 40).
const BACKBONE: &str = ">toy backbone\nACGTACGTACGAATTCACGTACGTACGTACGTACGTACGTCTGCAGACGTACGTACGTAC\n";
const ENZYMES: &str = r#"{"format_version":1,"enzymes":[{"name":"EcoRI","site":"GAATTC","cut_offset":1},{"name":"PstI","site":"CTGCAG","cut_offset":5}],"cloning_sites":[{"enzyme":"toy","position":30}]}"#;

fn digests(observed: &str) -> String {
    format!(
        r#"{{"format_version":1,"error_percent":5,"detection_limit":4,"insert_length":{{"min":5,"max":20}},"max_internal_sites":1,"observed":[{observed}],"held_out":[{{"id":"EcoRI+PstI","enzymes":["EcoRI","PstI"]}}]}}"#
    )
}

#[test]
fn sites_are_found_on_the_circle_and_cut_at_the_offset() {
    let seq = b"ACGTACGTACGAATTCACGTACGTACGTACGTACGTACGTCTGCAGACGTACGTACGTAC";
    let ecori = scan(seq, b"GAATTC", 1);
    assert_eq!(ecori.len(), 1);
    assert_eq!((ecori[0].start, ecori[0].cut), (10, 11));
    let psti = scan(seq, b"CTGCAG", 5);
    assert_eq!((psti[0].start, psti[0].cut), (40, 45));
    // a window that wraps past the last base is still a site
    let wrapped = scan(b"TTCAAAAAAAGAA", b"GAATTC", 1);
    assert_eq!(wrapped, vec![plasmid_map::sites::Site { start: 10, cut: 11 }]);
}

#[test]
fn backbone_sites_move_with_the_insert_and_torn_windows_are_dropped() {
    let seq = b"ACGTACGTACGAATTCACGTACGTACGTACGTACGTACGTCTGCAGACGTACGTACGTAC";
    let ecori = plasmid_map::input::Enzyme { name: "EcoRI".into(), site: b"GAATTC".to_vec(), cut_offset: 1 };
    assert_eq!(required_positions(seq, &ecori, 30, 100), vec![11], "a site before the opening keeps its position");
    assert_eq!(required_positions(seq, &ecori, 5, 100), vec![111], "a site after the opening moves by the insert length");
    assert!(spans_junction(10, 6, 13, 60), "opening inside the window tears it");
    assert_eq!(required_positions(seq, &ecori, 13, 100), Vec::<usize>::new());
}

#[test]
fn fragments_bands_and_the_matching_rule() {
    assert_eq!(fragments(&[], 100), vec![100]);
    assert_eq!(fragments(&[11, 45], 100), vec![34, 66]);
    let mut cuts = BTreeMap::new();
    cuts.insert("A".to_string(), vec![10, 50]);
    cuts.insert("B".to_string(), vec![50, 80]);
    assert_eq!(predicted(&cuts, &["A".into(), "B".into()], 100, 1), vec![30, 30, 40], "coincident cuts count once");
    assert_eq!(predicted(&cuts, &["A".into(), "B".into()], 100, 31), vec![40], "fragments below the limit are unseen");
    assert_eq!(group(&[30, 30, 40]), vec![Band { length: 30, multiplicity: 2 }, Band { length: 40, multiplicity: 1 }]);
    assert_eq!(expand(&[Band { length: 30, multiplicity: 2 }]), vec![30, 30]);
    assert_eq!(tolerance(1000, 2), 20);
    assert_eq!(tolerance(1001, 2), 21, "the tolerance rounds up");
    let bands = vec![Band { length: 100, multiplicity: 2 }, Band { length: 300, multiplicity: 1 }];
    assert_eq!(deviation(&[99, 101, 302], &bands, 1), Some(6), "pairs in ascending order; squared differences");
    assert_eq!(deviation(&[99, 101], &bands, 1), None, "counts must agree");
    assert_eq!(deviation(&[97, 101, 302], &bands, 1), None, "outside tolerance");
}

#[test]
fn a_map_is_validated_evaluated_and_ordered() {
    let observed = r#"{"id":"EcoRI","enzymes":["EcoRI"],"bands":[{"length":70,"multiplicity":1}]},{"id":"PstI","enzymes":["PstI"],"bands":[{"length":70,"multiplicity":1}]}"#;
    let problem = parse_problem(BACKBONE, ENZYMES, &digests(observed)).expect("a valid toy problem");
    let mut cuts = BTreeMap::new();
    cuts.insert("EcoRI".to_string(), vec![11]);
    cuts.insert("PstI".to_string(), vec![55]);
    let held_out = plasmid_map::map_model::predictions(&problem, 10, &cuts);
    let map = PlasmidMap { cloning_site: 30, insert_length: 10, cuts: cuts.clone(), held_out };
    validate(&problem, &map).expect("required positions only, insert of 10 at 30");
    assert_eq!(internal_count(&map), 0);
    assert_eq!(evaluate(&problem, &map), Ok(0), "both single digests give one band of 70");
    let text = encode(&map);
    assert_eq!(decode(&text).unwrap(), map);

    let mut wrong = map.clone();
    wrong.cuts.get_mut("PstI").unwrap().push(60);
    assert!(validate(&problem, &wrong).is_err(), "a position that is neither a surviving site nor inside the insert region");

    let a = key(&map, 5);
    let mut b = a.clone();
    b.internal = 1;
    b.deviation = 0;
    assert!(a < b, "fewer insert-region sites come before a closer fit");
}

#[test]
fn the_reader_enforces_the_input_rules() {
    let observed = r#"{"id":"EcoRI","enzymes":["EcoRI"],"bands":[{"length":70,"multiplicity":1}]}"#;
    assert!(parse_problem(BACKBONE, ENZYMES, &digests(observed)).is_err(), "every enzyme needs a single digest");
    let both = r#"{"id":"EcoRI","enzymes":["EcoRI"],"bands":[{"length":70,"multiplicity":1}]},{"id":"PstI","enzymes":["PstI"],"bands":[]}"#;
    assert!(parse_problem(BACKBONE, ENZYMES, &digests(both)).is_ok());
    let torn = ENZYMES.replace("\"position\":30", "\"position\":13");
    assert!(parse_problem(BACKBONE, &torn, &digests(both)).is_err(), "a cloning site that tears an enzyme's only site is rejected");
}
