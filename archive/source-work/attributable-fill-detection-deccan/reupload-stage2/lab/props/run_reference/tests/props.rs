mod harness;
use harness::*;

fn sig(o: &Outcome) -> String {
    let mut pos: Vec<String> = o.positions.iter().map(|p| p.strategy_id.as_str().to_string()).collect();
    pos.sort();
    let mut s = format!("pos={:?};", pos);
    for m in ["ma", "mb"] {
        for v in [Venue::North, Venue::South] {
            let t: Vec<Qty> = o.takers_for(m, v).into_iter().map(|x| x.size).collect();
            let f: Qty = o.venue_orders.iter().filter(|x| x.strategy_id.as_str() == m && x.venue == v).map(|x| x.filled_qty).sum();
            s += &format!("{}/{:?}:takers={:?},filled={};", m, v, t, f);
        }
    }
    s
}

/// variant 0 = base; 1 = a redelivery appended to every step; 2 = a restart inserted at step k (k = seed % 10);
/// 3 = fills of the two mandates in one step swapped
fn scenario(seed: u64, variant: u8) -> Scenario {
    let mut prng = Prng::new(seed);
    let shared = prng.below(2) == 0;
    let south2 = if shared { "tA" } else { "tZ" };
    let mut sc = Scenario::new(vec![
        candidate("ma", "tA", "tB", 0.45, 0.52, 30, 30),
        candidate("mb", south2, "tC", 0.44, 0.53, 20, 20),
    ])
    .inventory(instrument("tB"), (prng.below(3) * 15) as Qty)
    .ask(instrument("tA"), 0.47).ask(instrument("tB"), 0.60).ask(instrument("tC"), 0.61).ask(instrument("tZ"), 0.49)
    .step(vec![]);
    let q = [30, 20, 12, 8];
    let k = (seed % 10) as usize;
    for index in 0..10 {
        let mut a = Vec::new();
        let pick = prng.below(7);
        let qa = q[prng.below(4)];
        let qb = q[prng.below(4)];
        match pick {
            0 => a.push(fill_resting("ma", Venue::North, qa)),
            1 => a.push(fill_resting("ma", Venue::South, qa)),
            2 => a.push(fill_resting("mb", Venue::North, qa)),
            3 => a.push(fill_resting("mb", Venue::South, qa)),
            4 => { // two mandates filled in one step: order swapped in variant 3
                if variant == 3 { a.push(fill_resting("mb", Venue::North, qb)); a.push(fill_resting("ma", Venue::North, qa)); }
                else { a.push(fill_resting("ma", Venue::North, qa)); a.push(fill_resting("mb", Venue::North, qb)); }
            }
            5 => a.push(if index % 2 == 0 { Action::WithholdFillEvents } else { Action::ReleaseFillEvents }),
            _ => a.push(Action::RedeliverAllFillEvents),
        }
        if prng.below(5) == 0 { a.push(Action::LoseNextAcknowledgement); }
        if variant == 1 { a.push(Action::RedeliverAllFillEvents); }
        if variant == 2 && index == k { a.push(Action::Restart); }
        sc = sc.step(a);
    }
    sc.step(vec![Action::UnfreezeInventory, Action::ReleaseFillEvents]).idle_steps(8)
}

#[test]
fn p1_redelivery_idempotence() { let mut bad = vec![]; for s in 0..24u64 { if sig(&run(scenario(s,0))) != sig(&run(scenario(s,1))) { bad.push(s); } } assert!(bad.is_empty(), "P1 differs on seeds {:?}", bad); }
#[test]
fn p2_restart_insertion() { let mut bad = vec![]; for s in 0..24u64 { if sig(&run(scenario(s,0))) != sig(&run(scenario(s,2))) { bad.push(s); } } assert!(bad.is_empty(), "P2 differs on seeds {:?}", bad); }
#[test]
fn p3_same_cycle_fill_order() { let mut bad = vec![]; for s in 0..24u64 { if sig(&run(scenario(s,0))) != sig(&run(scenario(s,3))) { bad.push(s); } } assert!(bad.is_empty(), "P3 differs on seeds {:?}", bad); }
