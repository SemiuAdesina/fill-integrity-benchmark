# Verifier check → public rule matrix (author-side)

| CHECK | PUBLIC RULE | SPEC | FIXTURES |
|---|---|---|---|
| A map documents decode and are valid | document shape; validity rules 1–3 | map-format §1–§2 | all 18 jobs |
| B vector-region positions match the backbone | surviving sites, torn windows, junction cuts | construct-model §4 | h_junction, public, h_cloning_sites |
| C every observed digest is reproduced | matching rule, tolerance, detection limit | construct-model §5–§7 | all 12 |
| D every fixture yields the least map | the five-part order | map-format §3 | all 12 |
| E fewer insert-region sites come first | order component 1 before 2 | map-format §3 | h_parsimony (least map 2 sites, truth 3) |
| F fragments below the detection limit are unseen | predicted bands | construct-model §5 | h_sub_detection, h_three_sites |
| G bands carry multiplicity | expanded form | construct-model §6 | h_comigration |
| H the cloning site is chosen among all listed | cloning sites | input-format §2, construct-model §3 | h_cloning_sites (4 sites, answer not first) |
| I ties resolve by position order, repeated runs agree | order component 3; determinism | map-format §3–§4 | h_ties (+ again) |
| J held-out digests are predicted within tolerance | held-out predictions vs the construct | map-format §1–§2, construct-model §6 | all 12 |
| K large constructs are recovered within budget | graded envelope, budget | input-format §4, instruction | s_large_a/b/c |
| L one percent sizing error is handled | tolerance formula | construct-model §6 | h_low_error |
| M answers follow the supplied backbone | validity, reproduction, order components 1–2 under rotation; determinism | all | public, h_low_error, h_three_sites rotated; public and s_large_a again |

Rules stated but unobservable on this corpus (mutations benign): a cut exactly at a junction being inside the insert region (least-squares optima land on a junction with negligible probability); a backbone site whose cut position equals the cloning site (impossible with offsets inside the window); ties broken by site and length before positions (every tie on the corpus is between maps at the same site and length); discarding a box whose bound equals the incumbent (no tie is lost that way on the corpus).
