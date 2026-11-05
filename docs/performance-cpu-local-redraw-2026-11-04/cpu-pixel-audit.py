import struct,zlib,json
from pathlib import Path

def read(path):
 data=Path(path).read_bytes(); pos=8; payload=b''
 while pos<len(data):
  n=struct.unpack('>I',data[pos:pos+4])[0]; kind=data[pos+4:pos+8];chunk=data[pos+8:pos+8+n];pos+=n+12
  if kind==b'IHDR':w,h,depth,color,*_=struct.unpack('>IIBBBBB',chunk);assert(depth,color)==(8,6)
  if kind==b'IDAT':payload+=chunk
 raw=zlib.decompress(payload); stride=w*4;prev=bytearray(stride);out=[];idx=0
 for y in range(h):
  filt=raw[idx];idx+=1;row=bytearray(raw[idx:idx+stride]);idx+=stride
  for x in range(stride):
   a=row[x-4] if x>=4 else 0;b=prev[x];c=prev[x-4] if x>=4 else 0
   if filt==1:row[x]=(row[x]+a)&255
   elif filt==2:row[x]=(row[x]+b)&255
   elif filt==3:row[x]=(row[x]+((a+b)//2))&255
   elif filt==4:
    p=a+b-c;pa,pb,pc=abs(p-a),abs(p-b),abs(p-c)
    row[x]=(row[x]+(a if pa<=pb and pa<=pc else b if pb<=pc else c))&255
   else:assert filt==0
  out.extend(tuple((row[x+i]*row[x+3]+127)//255 for i in range(3))+(row[x+3],) for x in range(0,stride,4));prev=row
 return w,h,out


# Usage: python3 cpu-pixel-audit.py BEFORE_DIR AFTER_DIR
import sys
results={}
for zoom in ['fit','200','400']:
 w,h,ref=read(str(Path(sys.argv[2])/('combined-'+zoom+'-cpu.png')))
 for label,folder in [('before',sys.argv[1]),('after',sys.argv[2])]:
  ww,hh,img=read(str(Path(folder)/('combined-'+zoom+'-old-viewport.png')))
  assert (w,h)==(ww,hh)
  deltas=[abs(a-b) for p,q in zip(ref,img) for a,b in zip(p,q)]
  results[label+'_'+zoom]={'mean_rgba':sum(deltas)/len(deltas),'max_channel':max(deltas),'pixels_gt32':sum(max(abs(a-b) for a,b in zip(p,q))>32 for p,q in zip(ref,img))}
print(json.dumps(results,indent=2))
