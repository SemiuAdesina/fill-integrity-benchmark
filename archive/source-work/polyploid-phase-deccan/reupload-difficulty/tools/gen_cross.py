#!/usr/bin/env python3
"""Author-side simulation of a biparental autotetraploid cross under the accepted
scoring model (spec/scoring-model.md) with a standard low-depth read model.
Everything produced is a valid cross-format-v1 document inside the accepted ranges."""
import json, math, random, sys
STATES=[(0,0),(0,1),(0,2),(0,3),(1,1),(1,2),(1,3),(2,2),(2,3),(3,3)]
def switch_count(s,t):
    r=list(t); sh=0
    for x in s:
        if x in r: r.remove(x); sh+=1
    return 2-sh
def log_binom_pmf(k,n,p):
    if n==0: return 0.0
    return math.lgamma(n+1)-math.lgamma(k+1)-math.lgamma(n-k+1)+k*math.log(p)+(n-k)*math.log(1-p)
def simulate(seed, M, N, *, depth_mean=3.0, depth_spread=0.5, missing=0.05, err=0.01,
             dosage_weights=(1,4,6,4,1), rec_mean_morgan=0.10, rec_parent_scale=(1.0,1.0),
             dr_distal_from=0.4, dr_rate_range=(0.03,0.25), ibd_rows=(0,0), ibd_span=1.0,
             unlinked=0, rounding=6):
    """dosage_weights: relative weights of parental dosage 0..4 at a marker (both parents
    independently, but a marker uninformative in both parents is redrawn).
    ibd_rows: number of identical (identity-by-descent) homolog pairs per parent (0,1,2).
    dr_distal_from: fraction of the chromosome (from the proximal end) where DR is not
    admissible; beyond it DR is allowed with a rate growing toward the distal end."""
    rng=random.Random(seed)
    # ---- parental haplotypes: 4 rows, allele frequency per marker from a beta-ish draw
    def draw_parent():
        rows=[[0]*M for _ in range(4)]
        for m in range(M):
            d=rng.choices(range(5),weights=dosage_weights)[0]
            ones=rng.sample(range(4),d)
            for h in ones: rows[h][m]=1
        return rows
    hapA=draw_parent(); hapB=draw_parent()
    for m in range(M):
        tries=0
        while sum(hapA[h][m] for h in range(4)) in (0,4) and sum(hapB[h][m] for h in range(4)) in (0,4) and tries<50:
            for rows in (hapA,hapB):
                d=rng.choices(range(5),weights=dosage_weights)[0]
                for h in range(4): rows[h][m]=0
                for h in rng.sample(range(4),d): rows[h][m]=1
            tries+=1
    # identity by descent: copy one homolog onto another over a span of markers
    for rows,k in ((hapA,ibd_rows[0]),(hapB,ibd_rows[1])):
        pairs=[(0,1),(2,3)][:k]
        for (h1,h2) in pairs:
            span=max(1,int(round(ibd_span*M))); start=rng.randint(0,M-span)
            for m in range(start,start+span): rows[h2][m]=rows[h1][m]
    dosA=[sum(hapA[h][m] for h in range(4)) for m in range(M)]
    dosB=[sum(hapB[h][m] for h in range(4)) for m in range(M)]
    # ---- map: interval lengths in Morgans, Haldane to recombination fraction, per parent scale
    d_int=[rng.expovariate(1.0/rec_mean_morgan) for _ in range(M-1)]
    def haldane(d): return max(1e-3, min(0.5, 0.5*(1-math.exp(-2*d))))
    recA=[round(haldane(d*rec_parent_scale[0]*rng.uniform(0.7,1.3)),rounding) for d in d_int]
    recB=[round(haldane(d*rec_parent_scale[1]*rng.uniform(0.7,1.3)),rounding) for d in d_int]
    recA=[max(0.001,min(0.5,r)) for r in recA]; recB=[max(0.001,min(0.5,r)) for r in recB]
    # Unlinked intervals: r = 0.5 exactly, which the format permits and which a
    # marker pair on opposite arms really has.
    if unlinked:
        for m in rng.sample(range(M-1), min(unlinked, M-1)):
            recA[m]=0.5; recB[m]=0.5
    # ---- double reduction: proximal markers forbid it; distal allow with rising rate
    pos=[0.0]+[sum(d_int[:i+1]) for i in range(M-1)]; L=pos[-1] if pos[-1]>0 else 1.0
    def dr_parent():
        allowed=[]; rate=[]
        for m in range(M):
            frac=pos[m]/L
            if frac>=dr_distal_from and rng.random()<0.85:
                lo,hi=dr_rate_range
                r=lo+(hi-lo)*((frac-dr_distal_from)/(1-dr_distal_from+1e-9))*rng.uniform(0.6,1.2)
                r=round(max(0.005,min(0.6,r)),rounding); allowed.append(True); rate.append(r)
            else: allowed.append(False); rate.append(0.0)
        return allowed,rate
    alA,rtA=dr_parent(); alB,rtB=dr_parent()
    cross={"format_version":1,"markers":M,"offspring":N,"parent_dosage":{"A":dosA,"B":dosB},
           "child_gl":[], "recombination":{"A":recA,"B":recB},
           "double_reduction":{"A":{"allowed":alA,"rate":rtA},"B":{"allowed":alB,"rate":rtB}}}
    # ---- progeny paths sampled from the scoring model itself
    def sample_path(allowed,rate,rec):
        def dr_w(m,s):
            if s[0]==s[1]: return rate[m] if allowed[m] else 0.0
            return (1-rate[m])
        ws=[dr_w(0,s) for s in STATES]; cur=rng.choices(range(10),weights=ws)[0]; path=[cur]
        for m in range(1,M):
            r=rec[m-1]; ws=[]
            for j,s in enumerate(STATES):
                k=switch_count(STATES[cur],s); ws.append((r**k)*((1-r)**(2-k))*dr_w(m,s))
            cur=rng.choices(range(10),weights=ws)[0]; path.append(cur)
        return path
    truth=[]
    for c in range(N):
        pa=sample_path(alA,rtA,recA); pb=sample_path(alB,rtB,recB)
        gls=[]
        for m in range(M):
            a,b=STATES[pa[m]]; d_,e_=STATES[pb[m]]
            dose=hapA[a][m]+hapA[b][m]+hapB[d_][m]+hapB[e_][m]
            if rng.random()<missing: depth=0
            else:
                lam=max(0.2, rng.gauss(depth_mean, depth_mean*depth_spread)); depth=0
                # Poisson draw
                Lp=math.exp(-lam); k=0; p=1.0
                while True:
                    p*=rng.random()
                    if p<Lp: break
                    k+=1
                depth=k
            palt=lambda dd: (dd/4)*(1-err)+(1-dd/4)*err
            alt=sum(1 for _ in range(depth) if rng.random()<palt(dose))
            gl=[round(log_binom_pmf(alt,depth,palt(dd)),rounding) for dd in range(5)]
            gl=[min(0.0,g) for g in gl]
            gls.append(gl)
        cross["child_gl"].append(gls); truth.append((pa,pb))
    return cross, {"hapA":hapA,"hapB":hapB,"paths":truth}
if __name__=="__main__":
    import argparse
    ap=argparse.ArgumentParser(); ap.add_argument("--seed",type=int,required=True); ap.add_argument("--M",type=int,default=16); ap.add_argument("--N",type=int,default=12)
    ap.add_argument("--depth",type=float,default=3.0); ap.add_argument("--missing",type=float,default=0.05); ap.add_argument("--rec",type=float,default=0.10)
    ap.add_argument("--w",type=str,default="1,4,6,4,1"); ap.add_argument("--ibd",type=str,default="0,0"); ap.add_argument("--ibd_span",type=float,default=1.0)
    ap.add_argument("--dr_from",type=float,default=0.4); ap.add_argument("--dr_rate",type=str,default="0.03,0.25"); ap.add_argument("--out",required=True)
    a=ap.parse_args()
    cross,_=simulate(a.seed,a.M,a.N,depth_mean=a.depth,missing=a.missing,rec_mean_morgan=a.rec,dosage_weights=tuple(int(x) for x in a.w.split(",")),ibd_rows=tuple(int(x) for x in a.ibd.split(",")),ibd_span=a.ibd_span,dr_distal_from=a.dr_from,dr_rate_range=tuple(float(x) for x in a.dr_rate.split(",")))
    json.dump(cross,open(a.out,"w"),separators=(",",":"))
