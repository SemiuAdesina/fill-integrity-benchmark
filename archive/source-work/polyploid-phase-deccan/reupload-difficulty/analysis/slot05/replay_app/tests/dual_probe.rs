// scratch: is the splitting relaxation tight, and does the subgradient find it?
use polyploid_phase::cross_input::parse_cross;
use polyploid_phase::solver::branch::{assignment_value, local_search};
use polyploid_phase::solver::dual;
use polyploid_phase::solver::model::Model;

const JSON: &str = include_str!("/tmp/tiny.json");

#[test]
fn probe() {
    let cross = parse_cross(JSON).unwrap();
    let model = Model::new(&cross);
    let (lb, _) = local_search(&model, 64);

    // Brute force: every column pair per marker.
    let m = model.markers;
    let mut total_counts = 1usize;
    for mm in 0..m {
        total_counts *= model.na[mm] * model.nb[mm];
    }
    let mut best = f64::NEG_INFINITY;
    let mut idx = vec![0usize; m];
    for _ in 0..total_counts {
        let assign: Vec<(usize, usize)> = (0..m)
            .map(|mm| (idx[mm] / model.nb[mm], idx[mm] % model.nb[mm]))
            .collect();
        let v = assignment_value(&model, &assign);
        if v > best {
            best = v;
        }
        for mm in (0..m).rev() {
            idx[mm] += 1;
            if idx[mm] < model.na[mm] * model.nb[mm] {
                break;
            }
            idx[mm] = 0;
        }
    }

    for iters in 1..=20usize {
        let out = dual::optimise(&model, lb, iters);
        println!("iters={iters} dual={:.6}", out.bound);
    }
    println!("opt={best:.6} lb={lb:.6}");
}
