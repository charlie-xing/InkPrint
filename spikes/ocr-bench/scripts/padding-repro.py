import sys, numpy as np, onnxruntime as ort
from PIL import Image
S=sys.argv[1]
d=['<b>']+[l for l in open(S+'/models/ppocrv5_dict.txt',encoding='utf-8').read().split('\n') if l]+[' ']
s=ort.InferenceSession(S+'/models/pp-ocrv5_mobile_rec.onnx')
im=Image.open(S+'/py/line.png').convert('RGB'); h=48; w=int(im.width*h/im.height)
a=(np.asarray(im.resize((w,h),Image.BILINEAR),dtype=np.float32)/255.-0.5)/0.5
for W in (w, 1600, 2400):
  for name,val in (('paddle(0 after norm)',0.0),('black(-1)',-1.0)):
    x=np.full((h,W,3),val,np.float32); x[:,:w]=a
    o=s.run(None,{s.get_inputs()[0].name:x.transpose(2,0,1)[None]})[0][0].argmax(-1)
    t=[];p=0
    for i in o:
      if i and i!=p: t.append(d[i])
      p=i
    print(W,name,''.join(t))
