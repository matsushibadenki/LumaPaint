// Small UI color coordinates only; document pixels remain in Rust/GPU memory.
// CMYK uses a device-independent arithmetic approximation, not an ICC transform.
export type RgbColor = [number, number, number];
export type CmykColor = [number, number, number, number];

export function rgbToCmyk([r, g, b]: RgbColor): CmykColor {
  const max = Math.max(r, g, b) / 255;
  if (max === 0) return [0, 0, 0, 100];
  return [(1 - r / 255 / max) * 100, (1 - g / 255 / max) * 100, (1 - b / 255 / max) * 100, (1 - max) * 100];
}

export function cmykToRgb([c, m, y, k]: CmykColor): RgbColor {
  return [c, m, y].map(ink => Math.round(255 * (1 - ink / 100) * (1 - k / 100))) as RgbColor;
}

// CIE Lab D50, Bradford-adapted sRGB matrix (W3C CSS Color 4).
export type LabColor = [number, number, number];
const decode = (v:number)=>v<=0.04045?v/12.92:((v+0.055)/1.055)**2.4;
const encode = (v:number)=>v<=0.0031308?v*12.92:1.055*v**(1/2.4)-0.055;
export function rgbToLab(rgb:RgbColor):LabColor {
  const [r,g,b]=rgb.map(v=>decode(v/255));
  const xyz=[(.4360657428*r+.3851514688*g+.1430784544*b)/.9642956764,.2224931918*r+.7168870538*g+.0606197905*b,(.0139239045*r+.0970812857*g+.7140993584*b)/.8251046025];
  const [x,y,z]=xyz.map(v=>v>216/24389?Math.cbrt(v):(24389/27*v+16)/116);
  return [116*y-16,500*(x-y),200*(y-z)];
}
export function labToRgb([l,a,b]:LabColor):RgbColor {
  const y=(l+16)/116;
  const [x,yy,z]=[y+a/500,y,y-b/200].map(v=>v**3>216/24389?v**3:(116*v-16)/(24389/27));
  const X=x*.9642956764,Y=yy,Z=z*.8251046025;
  return [3.134135956*X-1.617386332*Y-.490661947*Z,-.978795502*X+1.916254568*Y+.033442731*Z,.071955379*X-.228976826*Y+1.405386058*Z].map(v=>Math.round(Math.max(0,Math.min(1,encode(v)))*255)) as RgbColor;
}
export function rgbToGray(rgb:RgbColor):number {
  const [r,g,b]=rgb.map(v=>decode(v/255));return Math.round(encode(.2126*r+.7152*g+.0722*b)*255);
}
