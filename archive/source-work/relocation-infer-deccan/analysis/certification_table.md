# Fixture certification table (author-side; generation-and-rejection)

| fixture | seed/attempt | target | visible linked obs | latent program | oracle A fits | oracle B fits | fit sets equal | canonical (A=B) | changed bits = field | held-out disagreeing fits | retained |
|---|---|---|---|---|---|---|---|---|---|---|---|
| public | 1000/0 | type 1 | 30 | S+A /1 uns rej [(0, 32)] | 1 | 1 | True | S+A /1 uns rej [(0, 32)] (True) | True | 0 | True |
| public | 1000/0 | type 2 | 30 | S+A-P /4 sig rej [(12, 20)] | 1 | 1 | True | S+A-P /4 sig rej [(12, 20)] (True) | True | 0 | True |
| public | 1000/0 | type 3 | 30 | S+B+A-P /1 sig wra [(0, 12), (20, 12)] | 2 | 2 | True | S+B+A-P /1 uns wra [(0, 12), (20, 12)] (True) | True | 0 | True |
| public | 1000/0 | type 4 | 30 | S-P /2 sig rej [(8, 4), (25, 6), (7, 1)] | 1 | 1 | True | S-P /2 sig rej [(8, 4), (25, 6), (7, 1)] (True) | True | 0 | True |
| public | 1000/0 | pair G1 (types 5,6) | 30 | S+B+A-P /1 sig rej [(12, 20)] pair(G1,hi,k=12) + S+B+A-P /1 uns rej [(20, 12)] pair(G1,lo,k=12) | 3 | 3 | True | S+B+A-P /1 sig rej [(12, 20)] pair(G1,hi,k=12) + S+B+A-P /1 uns rej [(20, 12)] pair(G1,lo,k=12) (True) | True | 0 | True |
| case_a | 2001/1 | type 1 | 33 | S+A-P /1 sig rej [(0, 16)] | 1 | 1 | True | S+A-P /1 sig rej [(0, 16)] (True) | True | 0 | True |
| case_a | 2001/1 | type 2 | 33 | S+B+A-P /1 sig rej [(16, 16)] | 1 | 1 | True | S+B+A-P /1 sig rej [(16, 16)] (True) | True | 0 | True |
| case_a | 2001/1 | type 3 | 33 | S-P /4 sig rej [(10, 22)] | 1 | 1 | True | S-P /4 sig rej [(10, 22)] (True) | True | 0 | True |
| case_b | 3000/0 | type 1 | 33 | S+A-P /4 sig rej [(21, 5), (7, 6), (28, 3)] | 1 | 1 | True | S+A-P /4 sig rej [(21, 5), (7, 6), (28, 3)] (True) | True | 0 | True |
| case_b | 3000/0 | type 2 | 33 | S+B+A-P /2 sig wra [(0, 8), (16, 8)] | 2 | 2 | True | S+B+A-P /2 uns wra [(0, 8), (16, 8)] (True) | True | 0 | True |
| case_b | 3000/0 | type 3 | 33 | S-P /8 sig rej [(12, 12)] | 1 | 1 | True | S-P /8 sig rej [(12, 12)] (True) | True | 0 | True |
| case_c | 4000/0 | pair G2 (types 1,2) | 36 | S+B+A-P /1 uns rej [(0, 16)] pair(G2,lo,k=16) + S+B+A-P /1 sig rej [(16, 16)] pair(G2,hi,k=16) | 3 | 3 | True | S+B+A-P /1 uns rej [(0, 16)] pair(G2,lo,k=16) + S+B+A-P /1 sig rej [(16, 16)] pair(G2,hi,k=16) (True) | True | 0 | True |
| case_c | 4000/0 | pair G3 (types 3,4) | 36 | S+A-P /4 sig rej [(12, 20)] pair(G3,hi,k=10) + S+A-P /4 uns rej [(20, 10)] pair(G3,lo,k=10) | 3 | 3 | True | S+A-P /4 sig rej [(12, 20)] pair(G3,hi,k=10) + S+A-P /4 uns rej [(20, 10)] pair(G3,lo,k=10) (True) | True | 0 | True |
| case_d | 5000/0 | type 1 | 33 | S+A /1 uns rej [(0, 24)] | 2 | 2 | True | S /1 uns rej [(0, 24)] (True) | True | 0 | True |
| case_d | 5000/0 | type 2 | 33 | S+B+A-P /1 sig rej [(8, 16)] | 3 | 3 | True | S+B+A-P /1 uns wra [(8, 16)] (True) | True | 0 | True |
| case_d | 5000/0 | type 3 | 33 | S+A-P /1 sig rej [(0, 20)] | 1 | 1 | True | S+A-P /1 sig rej [(0, 20)] (True) | True | 0 | True |
| case_d | 5000/0 | type 4 | 33 | S+A-P /1 sig wra [(4, 12)] | 2 | 2 | True | S+A-P /1 uns wra [(4, 12)] (True) | True | 0 | True |
| case_e | 6000/0 | type 1 | 22 | S+A-P /1 sig rej [(0, 12)] | 1 | 1 | True | S+A-P /1 sig rej [(0, 12)] (True) | True | 0 | True |
| case_e | 6000/0 | type 2 | 22 | S+A /1 uns wra [(0, 16)] | 2 | 2 | True | S+A /1 uns wra [(0, 16)] (True) | True | 0 | True |
| case_e | 6000/0 | type 3 | 22 | S+B+A-P /1 sig wra [(4, 12)] | 2 | 2 | True | S+B+A-P /1 uns wra [(4, 12)] (True) | True | 0 | True |
| case_e | 6000/0 | type 4 | 22 | S /1 uns rej [(24, 8)] | 1 | 1 | True | S /1 uns rej [(24, 8)] (True) | True | 0 | True |
| case_f | 7000/0 | type 7 | 32 | S-P /2 sig rej [(8, 4), (25, 6), (7, 1)] | 1 | 1 | True | S-P /2 sig rej [(8, 4), (25, 6), (7, 1)] (True) | True | 0 | True |
| case_f | 7000/0 | type 9 | 32 | S+A /1 uns rej [(0, 32)] | 1 | 1 | True | S+A /1 uns rej [(0, 32)] (True) | True | 0 | True |
| case_f | 7000/0 | type 11 | 32 | S+A-P /4 sig wra [(16, 16)] | 2 | 2 | True | S+A-P /4 uns wra [(16, 16)] (True) | True | 0 | True |
| case_f | 7000/0 | pair G4 (types 3,4) | 32 | S+B+A-P /1 sig rej [(12, 20)] pair(G4,hi,k=12) + S+B+A-P /1 uns rej [(20, 12)] pair(G4,lo,k=12) | 3 | 3 | True | S+B+A-P /1 sig rej [(12, 20)] pair(G4,hi,k=12) + S+B+A-P /1 uns rej [(20, 12)] pair(G4,lo,k=12) (True) | True | 0 | True |

Rejected attempts during construction (recorded in the generator log): case_a attempt 0 (an alternative fit predicted different held-out words: underdetermined), case_d attempt 0 (oracle B found 1 fit where oracle A found 3: a field bit never toggled).
