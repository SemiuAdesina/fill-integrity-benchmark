#!/usr/bin/env python3
"""Author-side experiment: root upper bounds from per-progeny and per-pair-of-progeny
column relaxations (columns free per group, shared within a group, paths continuous)."""
import json, math, sys, itertools, time
import numpy as np
STATES=[(0,0),(0,1),(0,2),(0,3),(1,1),(1,2),(1,3),(2,2),(2,3),(3,3)]
def cols(d): return [tuple((b>>h)&1 for h in range(4)) for b in range(16) if bin(b).count("1")==d]
def swc(s,t):
    r=list(t); sh=0
    for x in s:
        if x in r: r.remove(x); sh+=1
    return 2-sh
cross=json.load(open(sys.argv[1])); M=cross["markers"]; N=cross["offspring"]
gl=np.array(cross["child_gl"])  # N,M,5
dos=cross["parent_dosage"]; rec=cross["recombination"]; dr=cross["double_reduction"]
pairs=[[(a,b) for a in cols(dos["A"][m]) for b in cols(dos["B"][m])] for m in range(M)]
# dosage table: [m][pair][joint]
dose=[]
for m in range(M):
    t=np.zeros((len(pairs[m]),100),dtype=int)
    for k,(ca,cb) in enumerate(pairs[m]):
        for j,(ia,ib) in enumerate(itertools.product(range(10),range(10))):
            sa,sb=STATES[ia],STATES[ib]
            t[k,j]=ca[sa[0]]+ca[sa[1]]+cb[sb[0]]+cb[sb[1]]
    dose.append(t)
# dr term & legality per marker per joint
drt=np.zeros((M,100))
for m in range(M):
    for j,(ia,ib) in enumerate(itertools.product(range(10),range(10))):
        v=0.0
        for P,s in (("A",STATES[ia]),("B",STATES[ib])):
            if s[0]==s[1]:
                v+= math.log(dr[P]["rate"][m]) if dr[P]["allowed"][m] else -math.inf
            else: v+= math.log(1-dr[P]["rate"][m])
        drt[m,j]=v
# transition matrices per parent per interval (10x10)
TA=np.zeros((M-1,10,10)); TB=np.zeros((M-1,10,10))
for m in range(M-1):
    for i in range(10):
        for k in range(10):
            ka=swc(STATES[i],STATES[k]); kb=ka
            ra=rec["A"][m]; rb=rec["B"][m]
            TA[m,i,k]=ka*math.log(ra)+(2-ka)*math.log(1-ra)
            TB[m,i,k]=kb*math.log(rb)+(2-kb)*math.log(1-rb)
def adv(v,m):  # v: (...,100) max-plus advance over joint states (factorised)
    shp=v.shape[:-1]; x=v.reshape(*shp,10,10)  # (ia,ib)
    s1=np.max(x[...,:,None,:]+TA[m][None,:,:,None] if False else x[...,:,:,None]*0,axis=-3) if False else None
    # stage A: out1[na,ib]=max_ia x[ia,ib]+TA[ia,na]
    out1=np.max(x[...,:,:,None]+TA[m][(None,)*len(shp)+(slice(None),None,slice(None))],axis=-3)  # -> (ib,na)
    out1=np.swapaxes(out1,-1,-2)  # (na,ib)
    out2=np.max(out1[...,:,:,None]+TB[m][(None,)*len(shp)+(None,slice(None),slice(None))],axis=-2)  # (na,nb)
    return out2.reshape(*shp,100)
# single-progeny relaxation
def single(c):
    node=[np.max(gl[c,m][dose[m]] ,axis=0)+drt[m] for m in range(M)]  # max over pairs per joint
    f=node[0]
    for m in range(1,M): f=adv(f,m-1)+node[m]
    return f.max()
# pair-of-progeny relaxation: joint states (j1,j2) -> 10^4; node = max over pairs of gl1[dose]+gl2[dose]
def pairrelax(c1,c2):
    node=[]
    for m in range(M):
        e=gl[c1,m][dose[m]][:,:,None]+gl[c2,m][dose[m]][:,None,:]  # (pair,j1,j2)
        node.append(np.max(e,axis=0)+drt[m][:,None]+drt[m][None,:])
    f=node[0]  # (100,100)
    for m in range(1,M):
        g=adv(f,m-1)                 # advance progeny 1 (axis -1 is j1?) careful: adv acts on last axis
        g=adv(np.swapaxes(g,0,1),m-1).T if False else adv(g.T,m-1).T
        f=g+node[m]
    return f.max()
t0=time.time()
S=sum(single(c) for c in range(N))
t1=time.time()
P=sum(pairrelax(c1,c2) for c1,c2 in zip(range(0,N,2),range(1,N,2)))
t2=time.time()
exp=json.load(open(sys.argv[2]))["total_log_likelihood"] if len(sys.argv)>2 else float("nan")
print(f"M={M} N={N} optimum={exp:.4f} single_relax={S:.4f} (gap {S-exp:.3f}, {t1-t0:.1f}s)  pair_relax={P:.4f} (gap {P-exp:.3f}, {t2-t1:.1f}s)")
