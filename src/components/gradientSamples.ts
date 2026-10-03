import type { Gradient } from '../bridge';
// Color palettes adapted from WebGradients by Itmeo: https://webgradients.com/
// Angles use LumaPaint's coordinate convention; these are editable palette samples.
export const webGradientSamples: {name: string; gradient: Gradient}[] = [
  ['Warm Flame', '#ff9a9e', '#fad0c4', 45],
  ['Night Fade', '#a18cd1', '#fbc2eb', 90],
  ['Juicy Peach', '#ffecd2', '#fcb69f', 0],
  ['Sunny Morning', '#f6d365', '#fda085', -30],
  ['Winter Neva', '#a1c4fd', '#c2e9fb', 0],
  ['Dusty Grass', '#d4fc79', '#96e6a1', -30],
  ['Tempting Azure', '#84fab0', '#8fd3f4', -30],
  ['Mean Fruit', '#fccb90', '#d57eeb', -30],
  ['Malibu Beach', '#4facfe', '#00f2fe', 0],
  ['New Life', '#43e97b', '#38f9d7', 0],
  ['True Sunset', '#fa709a', '#fee140', 0],
  ['Morpheus Den', '#30cfd0', '#330867', 90],
].map(([name, a, b, angle]) => ({name: String(name), gradient: {
  kind:'linear', angle:Number(angle), aspect:1, dither:false, method:'classic',
  stops:[a,b].map((hex,i)=>({position:i, midpoint:.5, color:[...String(hex).slice(1).match(/../g)!.map(v=>parseInt(v,16)),255] as [number,number,number,number]})),
}}));

const gradients: Gradient[] = [
  [[255,220,0,255],[242,66,24,255]], [[255,255,255,255],[0,0,0,255]], [[255,210,0,255],[255,54,38,255]], [[52,160,220,255],[255,255,255,0]], [[255,255,255,255],[0,0,0,255]],
].map((colors,i) => ({kind:i===4?'radial':'linear', angle:0, aspect:1, dither:false, method:'classic', stops:colors.map((color,j)=>({position:j,color:color as [number,number,number,number],midpoint:.5}))}));

export const basicGradientSamples = gradients.map((gradient,i)=>({name:['Sunrise','Black & White','Golden Orange','Blue Fade','Radial Black & White'][i],gradient}));
