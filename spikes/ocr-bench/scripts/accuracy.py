import sys, collections, unicodedata, glob, os
S=sys.argv[1]
def norm(t):
    t=unicodedata.normalize('NFKC',t)
    return collections.Counter(c for c in t if not c.isspace())
pages={'privacy-policy.zh-1':'privacy-policy.zh-1','privacy-policy.zh-2':'privacy-policy.zh-2','privacy-policy.zh-3':'privacy-policy.zh-3',
'privacy-policy-1':'privacy-policy-1','privacy-policy-3':'privacy-policy-3','zh-scan-1':'zh-scan-1','zh-scan-2':'zh-scan-2'}
for run in sorted(d for d in os.listdir(S+'/out') if os.path.isdir(S+'/out/'+d)):
    tr=tp=tg=0; row=[]
    for p in pages:
        f=f'{S}/out/{run}/{p}.txt'
        if not os.path.exists(f): continue
        gt=norm(open(f'{S}/out/gt-{p}.txt',encoding='utf-8').read())
        oc=norm(open(f,encoding='utf-8').read())
        hit=sum((gt&oc).values()); tr+=hit; tg+=sum(gt.values()); tp+=sum(oc.values())
        row.append(f'{p.replace("privacy-policy","pp")}:{hit/sum(gt.values()):.3f}')
    print(f'{run:10s} recall={tr/tg:.4f} precision={tr/tp:.4f}  ', ' '.join(row))
