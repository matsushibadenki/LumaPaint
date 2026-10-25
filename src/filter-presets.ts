import type {Locale} from './i18n';
import type {DocumentSnapshot} from './bridge';
export type FilterCategory='pixel'|'vector'|'animation';
export type FilterPreset='grayscale'|'invert'|'contrast'|'linear'|'hold'|'smooth';
export const filterLabels={
 ja:{pixel:'ピクセル',vector:'ベクター',animation:'アニメーション',grayscale:'グレースケール',invert:'階調反転カーブ',contrast:'コントラストを強調',linear:'キーフレーム補間：リニア',hold:'キーフレーム補間：ステップ',smooth:'キーフレーム補間：スムーズ'},
 en:{pixel:'Pixel',vector:'Vector',animation:'Animation',grayscale:'Grayscale',invert:'Invert curve',contrast:'Enhance contrast',linear:'Keyframe interpolation: Linear',hold:'Keyframe interpolation: Hold',smooth:'Keyframe interpolation: Smooth'},
 'zh-CN':{pixel:'像素',vector:'矢量',animation:'动画',grayscale:'灰度',invert:'反相曲线',contrast:'增强对比度',linear:'关键帧插值：线性',hold:'关键帧插值：保持',smooth:'关键帧插值：平滑'},
} satisfies Record<Locale,Record<FilterCategory|FilterPreset,string>>;
export const filterPresets:Record<FilterCategory,FilterPreset[]>={pixel:['grayscale','invert','contrast'],vector:['grayscale','invert','contrast'],animation:['hold','linear','smooth']};
export function filterAvailability(doc:DocumentSnapshot,editable:boolean):Record<FilterCategory,boolean>{
 const layer=doc.layers.find(l=>l.id===doc.layerId);
 const ready=editable&&!!layer&&!layer.locked&&doc.layerEditTarget==='content'&&doc.editingChannel===0;
 return {pixel:ready&&layer?.kind==='paint',vector:ready&&layer?.kind!=='paint',animation:ready&&layer?.kind!=='paint'};
}
