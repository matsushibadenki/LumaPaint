import type { Locale } from './i18n';

export const brushBlendGroups = [
  ['normal','dissolve','behind','clear'],
  ['darken','multiply','colorBurn','linearBurn','darkerColor'],
  ['lighten','screen','colorDodge','linearDodge','lighterColor'],
  ['overlay','softLight','hardLight','vividLight','linearLight','pinLight','hardMix'],
  ['difference','exclusion','subtract','divide'],
  ['hue','saturation','color','luminosity'],
] as const;
export type BrushBlendMode = typeof brushBlendGroups[number][number];
const names: Record<BrushBlendMode, [string,string,string]> = {
  normal:['通常','Normal','正常'], dissolve:['ディザ合成','Dissolve','溶解'], behind:['背景','Behind','背后'], clear:['消去','Clear','清除'],
  darken:['比較（暗）','Darken','变暗'], multiply:['乗算','Multiply','正片叠底'], colorBurn:['焼き込みカラー','Color Burn','颜色加深'], linearBurn:['焼き込み（リニア）','Linear Burn','线性加深'], darkerColor:['カラー比較（暗）','Darker Color','深色'],
  lighten:['比較（明）','Lighten','变亮'], screen:['スクリーン','Screen','滤色'], colorDodge:['覆い焼きカラー','Color Dodge','颜色减淡'], linearDodge:['覆い焼き（リニア）- 加算','Linear Dodge (Add)','线性减淡（添加）'], lighterColor:['カラー比較（明）','Lighter Color','浅色'],
  overlay:['オーバーレイ','Overlay','叠加'], softLight:['ソフトライト','Soft Light','柔光'], hardLight:['ハードライト','Hard Light','强光'], vividLight:['ビビッドライト','Vivid Light','亮光'], linearLight:['リニアライト','Linear Light','线性光'], pinLight:['ピンライト','Pin Light','点光'], hardMix:['ハードミックス','Hard Mix','实色混合'],
  difference:['差の絶対値','Difference','差值'], exclusion:['除外','Exclusion','排除'], subtract:['減算','Subtract','减去'], divide:['除算','Divide','划分'], hue:['色相','Hue','色相'], saturation:['彩度','Saturation','饱和度'], color:['カラー','Color','颜色'], luminosity:['輝度','Luminosity','明度'],
};
export const brushSettingsLabels = {
  ja:{opacity:'不透明度',flow:'流量',smoothing:'滑らかさ',alpha:'アルファ',blendMode:'合成モード',hint:'不透明度はストロークの上限、流量は色の乗る量、アルファはブラシ色の透明度です。',groups:['基本','暗くする','明るくする','コントラスト','比較・演算','色の成分']},
  en:{opacity:'Opacity',flow:'Flow',smoothing:'Smoothing',alpha:'Alpha',blendMode:'Blend mode',hint:'Opacity caps each stroke, flow controls buildup, and alpha sets the brush color transparency.',groups:['Basic','Darken','Lighten','Contrast','Comparison','Color components']},
  'zh-CN':{opacity:'不透明度',flow:'流量',smoothing:'平滑',alpha:'Alpha',blendMode:'混合模式',hint:'不透明度限制单次笔画浓度，流量控制颜色累积，Alpha 设置画笔颜色透明度。',groups:['基本','变暗','变亮','对比度','比较与运算','颜色分量']},
};
export function brushBlendName(mode:BrushBlendMode,locale:Locale) {return names[mode][locale==='ja'?0:locale==='en'?1:2];}
