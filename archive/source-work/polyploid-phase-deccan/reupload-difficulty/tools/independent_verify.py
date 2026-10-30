import json, math, itertools, sys, glob, os
FIX="/Users/macbok/benchmark-suite-work/polyploid-phase-deccan/polyploid-phase/tests/fixtures"
RES="/tmp/poly-gate/results"
STATES=[(0,0),(0,1),(0,2),(0,3),(1,1),(1,2),(1,3),(2,2),(2,3),(3,3)]

def switch_count(s,t):
    # multiset intersection per spec
    right=list(t); shared=0
    for x in s:
        if x in right: right.remove(x); shared+=1
    return 2-shared

def score(cross, rec):
    """Independent scorer from spec/scoring-model.md, fixed summation order."""
    M,N=cross["markers"],cross["offspring"]
    hap={P:rec["haplotypes"][P] for P in "AB"}
    dos=cross["parent_dosage"]; rec_=cross["recombination"]; dr=cross["double_reduction"]
    # feasibility
    for P in "AB":
        assert len(hap[P])==4, "4 rows"
        for h in range(4):
            assert len(hap[P][h])==M and all(v in (0,1) for v in hap[P][h]), "binary/len"
        for m in range(M):
            assert sum(hap[P][h][m] for h in range(4))==dos[P][m], f"dosage {P}@{m}"
    assert len(rec["paths"])==N
    total=0.0
    for c in range(N):
        pa,pb=rec["paths"][c]["A"],rec["paths"][c]["B"]
        assert len(pa)==M and len(pb)==M
        for m in range(M):
            a,b=pa[m]; d,e=pb[m]
            assert 0<=a<=b<4 and 0<=d<=e<4, "state order"
            if a==b: assert dr["A"]["allowed"][m], f"illegal DR A c{c} m{m}"
            if d==e: assert dr["B"]["allowed"][m], f"illegal DR B c{c} m{m}"
            dosage=hap["A"][a][m]+hap["A"][b][m]+hap["B"][d][m]+hap["B"][e][m]
            total+=cross["child_gl"][c][m][dosage]
            for P,(x,y) in (("A",(a,b)),("B",(d,e))):
                r=dr[P]["rate"][m]
                total+= math.log(r) if x==y else math.log(1-r)
            if m+1<M:
                for P,path in (("A",pa),("B",pb)):
                    r=rec_[P][m]; k=switch_count(tuple(path[m]),tuple(path[m+1]))
                    total+= k*math.log(r)+(2-k)*math.log(1-r)
    return total

def digest(rec):
    """Independent FNV-1a from spec/result-format-v1.md."""
    b=bytearray()
    for P in "AB":
        for h in range(4):
            for v in rec["haplotypes"][P][h]: b.append(ord('0')+v)
        b.append(ord('|'))
    for p in rec["paths"]:
        for P in "AB":
            for a,c in p[P]: b.append(ord('0')+a); b.append(ord('0')+c)
        b.append(ord(';'))
    b.append(ord('#'))
    h=14695981039346656037
    for x in b:
        h^=x; h=(h*1099511628211)&0xFFFFFFFFFFFFFFFF
    return f"{h:016x}"

def canonical_parent(rows, states_per_progeny):
    """Apply spec steps 2-6 for one parent; return (rows, remapped states) — p: old->new."""
    best=None
    for p in itertools.permutations(range(4)):
        newrows=[None]*4
        for h in range(4): newrows[p[h]]=rows[h]
        if not all(newrows[i]<=newrows[i+1] for i in range(3)): continue   # step 3
        sig=[]
        for prog in states_per_progeny:
            for a,b in prog:
                x,y=sorted((p[a],p[b])); sig+= [x,y]                        # step 4
        key=(tuple(map(tuple,newrows)), tuple(sig), tuple(p))               # step 5
        if best is None or key<best[0]: best=(key,p,newrows)
    _,p,newrows=best
    newstates=[[sorted((p[a],p[b])) for a,b in prog] for prog in states_per_progeny]
    return newrows,newstates

def is_canonical(rec):
    for P in "AB":
        rows=[list(r) for r in rec["haplotypes"][P]]
        st=[[list(s) for s in prog[P]] for prog in rec["paths"]]
        nr,ns=canonical_parent(rows,st)
        if nr!=rows or ns!=st: return False
    return True

stems=sorted(os.path.basename(f)[:-len(".cross.json")] for f in glob.glob(f"{FIX}/*.cross.json"))
allok=True
print(f"{'fixture':14s} {'M':>3s} {'N':>3s}  {'expected':>18s}  {'reproduced':>18s}  {'|diff|':>9s}  struct canon digest indep_score indep_digest determ  RESULT")
for s in stems:
    cross=json.load(open(f"{FIX}/{s}.cross.json")); exp=json.load(open(f"{FIX}/{s}.expected.json"))
    r1=json.load(open(f"{RES}/{s}.run1.json")); r2=json.load(open(f"{RES}/{s}.run2.json"))
    determ = open(f"{RES}/{s}.run1.json","rb").read()==open(f"{RES}/{s}.run2.json","rb").read()
    struct = r1["haplotypes"]==exp["haplotypes"] and r1["paths"]==exp["paths"]
    dmatch = r1["canonical_digest"]==exp["canonical_digest"]
    diff = abs(r1["total_log_likelihood"]-exp["total_log_likelihood"])
    try:
        ind_exp=score(cross,exp); ind_ok = abs(ind_exp-exp["total_log_likelihood"])<=1e-8
        ind_rep=score(cross,r1);  rep_ok = abs(ind_rep-r1["total_log_likelihood"])<=1e-8
    except AssertionError as e:
        ind_ok=rep_ok=False; print("  FEASIBILITY:",e)
    canon = is_canonical(exp) and is_canonical(r1)
    idig = digest(exp)==exp["canonical_digest"] and digest(r1)==r1["canonical_digest"]
    ok = struct and dmatch and diff<=1e-8 and ind_ok and rep_ok and canon and idig and determ
    allok &= ok
    print(f"{s:14s} {cross['markers']:>3d} {cross['offspring']:>3d}  {exp['total_log_likelihood']:>18.10f}  {r1['total_log_likelihood']:>18.10f}  {diff:>9.2e}  {str(struct):6s} {str(canon):5s} {str(dmatch):6s} {str(ind_ok and rep_ok):11s} {str(idig):12s} {str(determ):6s}  {'PASS' if ok else 'FAIL'}")
print()
print("ALL SEVEN CERTIFIED:", "YES" if allok else "NO")
sys.exit(0 if allok else 1)
