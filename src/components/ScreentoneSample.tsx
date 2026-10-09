import { useEffect, useRef } from 'react';
import type { Screentone } from '../screentone';

const fract = (n: number) => n - Math.floor(n);
function noise(x: number, y: number, seed: number) {
  let h = Math.imul(Math.floor(x), 1597334677) ^ Math.imul(Math.floor(y), 3812015801) ^ Math.imul(seed, 2798796415);
  h = Math.imul(h ^ (h >>> 16), 2246822519); h ^= h >>> 13;
  return (h & 65535) / 65536;
}
// Bounded UI swatches only; document rendering stays in Rust/wgpu.
export function ScreentoneSample({ tone, label }: { tone: Screentone; label: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const context = ref.current?.getContext('2d');
    if (!context) return;
    const width = 240, height = 100;
    const pixels = context.createImageData(width, height);
    const r = tone.angle * Math.PI / 180, c = Math.cos(r), s = Math.sin(r);
    // Show a 240 × 100 document-pixel crop at 600 dpi equivalent for legible swatches.
    const scale = tone.dpi / 600;
    for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
      const px = (x + .5) * scale - tone.offset[0], py = (y + .5) * scale - tone.offset[1];
      const u = (px * c + py * s) / (tone.dpi / tone.frequency), v = (-px * s + py * c) / (tone.dpi / tone.frequency);
      const a = Math.abs(fract(u) - .5), b = Math.abs(fract(v) - .5), n = tone.variant;
      let d = tone.density / 100;
      let score = n === 1 ? 4 * Math.max(a,b) ** 2 : n === 2 ? 2 * (a+b) ** 2 : n === 3 ? Math.min(a*a*2+b*b*6,1) : Math.PI*(a*a+b*b);
      if (tone.kind === 'gradient') {
        let t = (px*c+py*s)/tone.extent;
        if(n===1)t=Math.hypot(px,py)/tone.extent;
        if(n===2)t=Math.abs(px*c+py*s)/tone.extent;
        if(n===3)t=Math.sin(t*Math.PI)*.5+.5;
        d += (tone.gradientEnd/100-d)*Math.max(0,Math.min(1,t));
        score = Math.PI*(a*a+b*b);
      }
      if (tone.kind === 'line') score = n===1 ? Math.abs(2*fract(v+.2*Math.sin(u*1.5))-1) : n===2 ? Math.max(2*b,fract(u)<.65?0:1) : n===3 ? Math.abs(2*b-.45)*2 : 2*b;
      if (tone.kind === 'sand') score = noise(u*(n+1),v*(n+1),tone.seed);
      if (tone.kind === 'crosshatch') {
        const jitter = noise(u,v,tone.seed)*.3;
        score = n===1 ? 2*Math.min(a,b,Math.abs(a-b)) : n===2 ? Math.min(2*b+jitter,2*a+.3-jitter) : n===3 ? Math.min(Math.max(2*b,(Math.floor(u)&1)===0?0:1),Math.max(2*a,(Math.floor(v)&1)===1?0:1)) : 2*Math.min(a,b);
      }
      if (tone.kind === 'pattern') score = n===1 ? 2*Math.abs(a+b-.35) : n===2 ? Math.min(Math.hypot(a,b)/(.3+.12*Math.cos(Math.atan2(fract(v)-.5,fract(u)-.5)*5)),1) : n===3 ? (Math.floor(u)+Math.floor(v))&1 : 2*Math.min(a,b);
      if (tone.kind === 'cg') score = n===1 ? (Math.sin(u*.8)*Math.cos(v*.8)+1)*.5 : n===2 ? Math.sin(Math.hypot(u,v)*1.4)*.5+.5 : n===3 ? (Math.sin(u*.4)+Math.sin(v*.6)+2)*.25 : (Math.sin(u+v*.5)+1)*.5;
      if (tone.kind === 'effect') score = n===1 ? Math.abs(Math.sin(u*.8)) : n===2 ? Math.abs(Math.sin(Math.hypot(u,v))) : n===3 ? Math.min(Math.abs(Math.sin(Math.atan2(v,u)*12))+Math.hypot(u,v)*.01,1) : Math.abs(Math.sin(Math.atan2(v,u)*24));
      if (tone.kind === 'background') score = n===1 ? Math.min(2*b,Math.max(Math.abs(2*fract(u+((Math.floor(v)&1)===0?0:.5))-1),.2)) : n===2 ? (Math.sin(u*.2)+Math.sin(v*.3+Math.sin(u*.1))+2)*.25 : n===3 ? Math.abs(Math.sin(v*2+Math.sin(u*.3))) : Math.max(2*b,Math.abs(Math.sin(u*.3)));
      if (tone.kind === 'transfer') {
        const grain = noise(u,v,tone.seed);
        score = Math.min(n===1 ? score+grain*.4 : n===2 ? 2*b+grain*.5 : n===3 ? 2*Math.min(a,b)+grain*.35 : score*.7+grain*.6,1);
      }
      // A synthetic light-to-dark source makes luminance/copy settings visible.
      if(tone.luminance || tone.kind==='copy') d *= x/width;
      if(tone.inverted)d=1-d;
      const ink = d>=1 || (d>0 && score<d*tone.size*tone.size);
      const background = tone.kind==='white' ? 48 : 255;
      const color = ink ? tone.kind==='white' ? [255,255,255] : tone.color : tone.paper ? [255,255,255] : [background,background,background];
      const i=(y*width+x)*4;
      pixels.data.set([...color,255],i);
    }
    context.putImageData(pixels,0,0);
  }, [tone]);
  return <canvas ref={ref} width={240} height={100} role="img" aria-label={label} />;
}
