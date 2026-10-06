import type { Brush, SwatchDraft } from '../bridge';
import { readPreference } from '../i18n';
import { toHex } from './BrushControls';
import { basicGradientSamples, webGradientSamples } from './gradientSamples';
export function swatchSeeds():SwatchDraft[]{
  const colors:Brush['color'][]=[[255,255,255],[0,0,0],[128,128,128],[255,0,0],[255,153,0],[255,255,0],[0,153,0],[0,204,255],[0,102,255],[153,51,204]];
  try {const old:unknown=JSON.parse(readPreference('color-swatches-v1')??'[]');if(Array.isArray(old))for(const c of old)if(typeof c==='string'&&/^#[0-9a-f]{6}$/i.test(c)){const rgb=c.slice(1).match(/../g)!.map(v=>parseInt(v,16)) as Brush['color'];if(!colors.some(v=>toHex(v)===toHex(rgb))&&colors.length<138)colors.push(rgb);}}catch{/* Keep valid defaults. */}
  return [{name:"[Registration]",paint:{kind:"color",color:[0,0,0],registration:true}},...colors.map(color=>({name:toHex(color).toUpperCase(),paint:{kind:'color' as const,color}})),...[...basicGradientSamples,...webGradientSamples].map(s=>({name:s.name,paint:{kind:'gradient' as const,gradient:s.gradient}}))];
}
