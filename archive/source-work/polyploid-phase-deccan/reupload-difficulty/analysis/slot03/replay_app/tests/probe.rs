use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::solver::model::Model;
use polyploid_phase::solver::viterbi::total_all;

#[test]
fn probe_flat() {
    let text = std::fs::read_to_string("/tmp/flat12.json").unwrap();
    let cross = parse_cross(&text).unwrap();
    let model = Model::build(&cross);
    let m = model.m;
    println!("cols[0][0] = {:?}", model.cols[0][0]);
    println!("gl[0][0] = {:?}", model.gl[0][0]);
    let base = vec![(0usize, 0usize); m];
    println!("all first: {:.8}", total_all(&model, &base));
    for (ia, ib) in [(0usize, 0usize), (1, 1), (2, 2), (3, 3), (4, 4), (5, 5)] {
        let mut c = base.clone();
        c[5] = (ia, ib);
        println!("marker5 = ({ia},{ib}): {:.8}", total_all(&model, &c));
    }
    let mut c = base.clone();
    c[5] = (1, 0);
    println!("marker5 = (1,0): {:.8}", total_all(&model, &c));
}
