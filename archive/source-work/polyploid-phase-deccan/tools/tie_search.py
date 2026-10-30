"""Search for enumerable crosses (M=12,N=8, pinned dosage) whose optimal answer set
exposes coupled path ties and/or canonically distinct tied assignments.
Host-side authoring tool. Uses only the public spec semantics."""
import json, math, random, itertools, sys
M, N = 12, 8
STATES=[(0,0),(0,1),(0,2),(0,3),(1,1),(1,2),(1,3),(2,2),(2,3),(3,3)]
JOINT=[(a,b) for a in range(10) for b in range(10)]
def cols(d): return [tuple((bits>>h)&1 for h in range(4)) for bits in range(16) if bin(bits).count('1')==d]
def swc(s,t):
    r=list(t); sh=0
    for x in s:
        if x in r: r.remove(x); sh+=1
    return 2-sh
def make_cross(seed, free_a, free_b, dr_a=None, rec=0.2, coarse=0.7):
    rng=random.Random(seed)
    dos_a=[rng.choice([0,4]) for _ in range(M)]; dos_b=[rng.choice([0,4]) for _ in range(M)]
    for m in free_a: dos_a[m]=2
    for m in free_b: dos_b[m]=2
    allowed_a=[bool(dr_a and m in dr_a) for m in range(M)]
    rate_a=[0.3 if allowed_a[m] else 0.0 for m in range(M)]
    # coarse likelihoods: integer-distance penalties from a random truth dosage,
    # with occasional flat pairs, so exact ties are common
    gl=[]
    for c in range(N):
        row=[]
        for m in range(M):
            truth=rng.randint(0,4); slot=[-coarse*abs(d-truth) for d in range(5)]
            if rng.random()<0.35 and truth<4: slot[truth+1]=slot[truth]
            if rng.random()<0.25 and truth>0: slot[truth-1]=slot[truth]
            row.append(slot)
        gl.append(row)
    return {"format_version":1,"markers":M,"offspring":N,
            "parent_dosage":{"A":dos_a,"B":dos_b},"child_gl":gl,
            "recombination":{"A":[rec]*(M-1),"B":[rec]*(M-1)},
            "double_reduction":{"A":{"allowed":allowed_a,"rate":rate_a},
                                "B":{"allowed":[False]*M,"rate":[0.0]*M}}}
def node_terms(cross, hapA, hapB, c):
    dr=cross["double_reduction"]; out=[]
    for m in range(M):
        layer=[]
        for (ia,ib) in JOINT:
            sa,sb=STATES[ia],STATES[ib]
            if (sa[0]==sa[1] and not dr["A"]["allowed"][m]) or (sb[0]==sb[1] and not dr["B"]["allowed"][m]): layer.append(None); continue
            dos=hapA[sa[0]][m]+hapA[sa[1]][m]+hapB[sb[0]][m]+hapB[sb[1]][m]
            v=cross["child_gl"][c][m][dos]
            for P,s in (("A",sa),("B",sb)):
                r=dr[P]["rate"][m]; v+= math.log(r) if s[0]==s[1] else math.log(1-r)
            layer.append(v)
        out.append(layer)
    return out
TR={}
def trans(cross,ia,ib,ja,jb,m):
    k=(ia,ib,ja,jb,m)
    if k not in TR:
        ra=cross["recombination"]["A"][m]; rb=cross["recombination"]["B"][m]
        ka=swc(STATES[ia],STATES[ja]); kb=swc(STATES[ib],STATES[jb])
        TR[k]=ka*math.log(ra)+(2-ka)*math.log(1-ra)+kb*math.log(rb)+(2-kb)*math.log(1-rb)
    return TR[k]
def best_total(cross,node):
    best=[v if v is not None else -math.inf for v in node[0]]
    for m in range(1,M):
        nxt=[-math.inf]*100
        for j,(ja,jb) in enumerate(JOINT):
            if node[m][j] is None: continue
            b=-math.inf
            for i,(ia,ib) in enumerate(JOINT):
                if best[i]==-math.inf: continue
                v=best[i]+trans(cross,ia,ib,ja,jb,m-1)
                if v>b: b=v
            nxt[j]=b+node[m][j] if b>-math.inf else -math.inf
        best=nxt
    return max(best)
def hap_rows(colseq): return [[colseq[m][h] for m in range(M)] for h in range(4)]
def enumerate_optima(cross):
    TR.clear()
    A=[cols(d) for d in cross["parent_dosage"]["A"]]; B=[cols(d) for d in cross["parent_dosage"]["B"]]
    results=[]
    for ca in itertools.product(*A):
        hA=hap_rows(ca)
        for cb in itertools.product(*B):
            hB=hap_rows(cb)
            tot=sum(best_total(cross,node_terms(cross,hA,hB,c)) for c in range(N))
            results.append((tot,ca,cb))
    top=max(r[0] for r in results)
    return top,[r for r in results if abs(r[0]-top)<=1e-8]
def canon_rows(rows): return sorted(rows)
def forward_a_first(cross,node,eps=1e-7):
    """Public rule: smallest A sequence over all markers, then smallest B given A."""
    # back[m][j]: best obtainable strictly after m from joint state j
    back=[[0.0]*100 for _ in range(M)]
    for m in range(M-2,-1,-1):
        for i,(ia,ib) in enumerate(JOINT):
            b=-math.inf
            for j,(ja,jb) in enumerate(JOINT):
                if node[m+1][j] is None: continue
                v=trans(cross,ia,ib,ja,jb,m)+node[m+1][j]+back[m+1][j]
                if v>b: b=v
            back[m][i]=b
    opt=max(node[0][i]+back[0][i] for i in range(100) if node[0][i] is not None)
    # settle A
    chosenA=[]; reach=[-math.inf]*10  # by B state
    for m in range(M):
        for cand in range(10):
            nxt=[-math.inf]*10
            for ib in range(10):
                j=cand*10+ib
                if node[m][j] is None: continue
                if m==0: nxt[ib]=node[m][j]; continue
                b=-math.inf
                for pb in range(10):
                    if reach[pb]==-math.inf: continue
                    v=reach[pb]+trans(cross,chosenA[m-1],pb,cand,ib,m-1)
                    if v>b: b=v
                if b>-math.inf: nxt[ib]=b+node[m][j]
            if any(nxt[ib]>-math.inf and nxt[ib]+back[m][cand*10+ib]>=opt-eps for ib in range(10)):
                chosenA.append(cand); reach=nxt; break
    # settle B given A
    a=chosenA
    backB=[[0.0]*10 for _ in range(M)]
    for m in range(M-2,-1,-1):
        for ib in range(10):
            b=-math.inf
            for nb in range(10):
                j=a[m+1]*10+nb
                if node[m+1][j] is None: continue
                v=trans(cross,a[m],ib,a[m+1],nb,m)+node[m+1][j]+backB[m+1][nb]
                if v>b: b=v
            backB[m][ib]=b
    optB=max(node[0][a[0]*10+ib]+backB[0][ib] for ib in range(10) if node[0][a[0]*10+ib] is not None)
    chosenB=[]; running=0.0
    for m in range(M):
        for cand in range(10):
            j=a[m]*10+cand
            if node[m][j] is None: continue
            v=node[0][j] if m==0 else running+trans(cross,a[m-1],chosenB[m-1],a[m],cand,m-1)+node[m][j]
            if v+backB[m][cand]>=optB-eps: chosenB.append(cand); running=v; break
    return [(STATES[x]) for x in chosenA],[(STATES[x]) for x in chosenB]
def backward_traceback(cross,node):
    best=[v if v is not None else -math.inf for v in node[0]]; back=[[None]*100 for _ in range(M)]
    for m in range(1,M):
        nxt=[-math.inf]*100
        for j,(ja,jb) in enumerate(JOINT):
            if node[m][j] is None: continue
            for i,(ia,ib) in enumerate(JOINT):
                if best[i]==-math.inf: continue
                v=best[i]+trans(cross,ia,ib,ja,jb,m-1)+node[m][j]
                if v>nxt[j]: nxt[j]=v; back[m][j]=i
        best=nxt
    cur=max(range(100),key=lambda i:(best[i],-i))
    pa=[None]*M; pb=[None]*M
    for m in range(M-1,-1,-1):
        pa[m]=STATES[JOINT[cur][0]]; pb[m]=STATES[JOINT[cur][1]]
        if m>0: cur=back[m][cur]
    return pa,pb
def joint_greedy(cross,node,eps=1e-7):
    back=[[0.0]*100 for _ in range(M)]
    for m in range(M-2,-1,-1):
        for i,(ia,ib) in enumerate(JOINT):
            b=-math.inf
            for j,(ja,jb) in enumerate(JOINT):
                if node[m+1][j] is None: continue
                v=trans(cross,ia,ib,ja,jb,m)+node[m+1][j]+back[m+1][j]
                if v>b: b=v
            back[m][i]=b
    opt=max(node[0][i]+back[0][i] for i in range(100) if node[0][i] is not None)
    chosen=[]; running=0.0
    for m in range(M):
        for j in range(100):
            if node[m][j] is None: continue
            v=node[0][j] if m==0 else running+trans(cross,JOINT[chosen[-1]][0],JOINT[chosen[-1]][1],JOINT[j][0],JOINT[j][1],m-1)+node[m][j]
            if v+back[m][j]>=opt-eps: chosen.append(j); running=v; break
    return [STATES[JOINT[j][0]] for j in chosen],[STATES[JOINT[j][1]] for j in chosen]
if __name__=="__main__":
    want=sys.argv[1] if len(sys.argv)>1 else "paths"
    found=0
    for seed in range(int(sys.argv[2]) if len(sys.argv)>2 else 0, 100000):
        rng=random.Random(seed)
        free_a=sorted(rng.sample(range(M),2)); free_b=sorted(rng.sample(range(M),1))
        cross=make_cross(seed,free_a,free_b,dr_a=set(range(0,6)),rec=rng.choice([0.15,0.25,0.35]),coarse=rng.choice([0.5,0.7,1.0]))
        top,optima=enumerate_optima(cross)
        distinct={tuple(map(tuple,canon_rows(hap_rows(ca))))+tuple(map(tuple,canon_rows(hap_rows(cb)))) for _,ca,cb in optima}
        if want=="distinct":
            if len(distinct)>=2:
                print(f"SEED {seed} distinct_optima={len(distinct)} score={top:.6f}"); found+=1
        else:
            _,ca,cb=optima[0]; hA,hB=hap_rows(ca),hap_rows(cb); diffs=[]
            for c in range(N):
                node=node_terms(cross,hA,hB,c)
                fa=forward_a_first(cross,node); bt=backward_traceback(cross,node); jg=joint_greedy(cross,node)
                if fa!=bt: diffs.append(("backward",c))
                if fa!=jg: diffs.append(("joint",c))
            if diffs:
                kinds={d[0] for d in diffs}
                print(f"SEED {seed} coupled_ties={sorted(kinds)} progeny={sorted({d[1] for d in diffs})} distinct={len(distinct)} score={top:.6f}"); found+=1
        if found>=int(sys.argv[3]) if len(sys.argv)>3 else found>=3: break
