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

results={}
for name in ['fit','200','400']:
 w,h,a=read('/tmp/complex-gpu-reference/'+name+'-cpu.png');_,_,b=read('/tmp/complex-gpu-reference/'+name+'-gpu.png')
 large=[i for i,(p,q) in enumerate(zip(a,b)) if max(abs(x-y) for x,y in zip(p,q))>32]
 offedge=[]
 for i in large:
  x,y=i%w,i//w
  ps=[a[yy*w+xx] for yy in range(max(0,y-1),min(h,y+2)) for xx in range(max(0,x-1),min(w,x+2))]
  contrast=max(max(v)-min(v) for v in zip(*ps))
  if contrast<=16:offedge.append(i)
 results[name]={'pixels_gt32':len(large),'pixels_gt32_outside_3x3_reference_edges':len(offedge),'edge_definition':'3x3 local range >16/255 in at least one premultiplied RGBA channel'}
print(json.dumps(results,indent=2))
Path('/tmp/complex-gpu-pixel-audit.json').write_text(json.dumps(results,indent=2)+'\n')
