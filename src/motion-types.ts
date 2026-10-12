export type Curve = {kind:string;[parameter:string]:unknown};
export type MotionPreset = {id:string;category:string;names:Record<string,string>;durationMs:number;curve:Curve;operations:string[];stagger:string;exit:boolean;cyclic:boolean;actors?:{keys:{offset:number;values:Record<string,number>}[]}[]};
export type MotionCatalog = {reference:string;categories:{id:string;names:Record<string,string>}[];presets:MotionPreset[]};
export type MotionClip = {
  id:string;preset:string;enabled:boolean;start:number;duration:number;repeats:number;reverse:boolean;yoyo:boolean;
  strength:number;distance:number;angle:number;frequency:number;stagger:number;order:string;pivot:[number,number];seed:number;
  curve:Curve|null;objects:string[];particleCount:number;copies:number;blur:number;glow:number;color:[number,number,number,number];
  alternateText:string;numberFrom:number;numberTo:number;path:[number,number][];morphTarget:string;meshDepth:number;cameraDistance:number;rotationX:number;rotationY:number;rotationZ:number;variableFont:string;fontAxes:{tag:string;from:number;to:number}[];strokeGapMs:number;strokeWidth:number;
};
export type MotionLayer = {id:string;name:string;pixel:boolean;locked:boolean;motions:MotionClip[]};
export type MotionData = {documentId:number;selectedLayer:string;selectedObjects:string[];fps:number;duration:number;looping:boolean;layers:MotionLayer[]};
export type MotionState = {frame:number;playing:boolean;preview:boolean;data:MotionData|null};
export const curveDefaults:Record<string,Curve> = {
  linear:{kind:'linear'},power:{kind:'power',exponent:3,mode:'out'},sine:{kind:'sine',mode:'inOut'},expo:{kind:'expo',mode:'out'},circular:{kind:'circular',mode:'out'},
  back:{kind:'back',overshoot:1.70158,mode:'out'},elastic:{kind:'elastic',amplitude:1,period:.3},bounce:{kind:'bounce',restitution:.55},
  bezier:{kind:'bezier',points:[.25,.1,.25,1]},spring:{kind:'spring',stiffness:170,damping:26,mass:1},steps:{kind:'steps',count:6},
  rough:{kind:'rough',strength:.7,points:20},slowMo:{kind:'slowMo',plateau:.7},expoScale:{kind:'expoScale',ratio:8},wiggle:{kind:'wiggle',cycles:6},gravity:{kind:'gravity'},inertia:{kind:'inertia',decay:7},
};
export function newMotion(preset:MotionPreset,fps:number,start:number):MotionClip {
  return {id:crypto.randomUUID(),preset:preset.id,enabled:true,start,duration:Math.max(1,Math.round(preset.durationMs*fps/1000)),repeats:1,reverse:false,yoyo:false,strength:1,distance:80,angle:preset.operations.includes('cube')?360:preset.operations.includes('coverflow')?60:preset.id==='tx-layered-depth'?150:180,frequency:preset.operations.includes('fontAxes')?1:2,stagger:.08,order:preset.stagger,pivot:[.5,.5],seed:1,curve:null,objects:[],particleCount:24,copies:preset.id==='tx-layered-depth'?12:4,blur:12,glow:12,color:[100,140,255,255],alternateText:'',numberFrom:0,numberTo:100,path:[],morphTarget:'',meshDepth:preset.id==='tx-layered-depth'?12:80,cameraDistance:600,rotationX:preset.category==='tx-depth'?0:-20,rotationY:preset.category==='tx-depth'?0:25,rotationZ:0,variableFont:'',fontAxes:preset.operations.includes('fontAxes')?defaultAxes(preset.id):[],strokeGapMs:100,strokeWidth:4};
}

export type VariableFont = {family:string;postscript:string;japanese:boolean;latin:boolean;axes:{tag:string;minimum:number;default:number;maximum:number}[]};
export function defaultAxes(id:string):MotionClip['fontAxes'] {if(id.includes('width-slant'))return [{tag:'wdth',from:75,to:125},{tag:'slnt',from:0,to:-10}];if(id.includes('width')||id.includes('pressure'))return [{tag:'wdth',from:75,to:125}];if(id.includes('slant'))return [{tag:'slnt',from:0,to:-10}];return [{tag:'wght',from:200,to:900}];}
