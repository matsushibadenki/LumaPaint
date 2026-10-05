import type { RasterBlendMode } from '../bridge';
export const rasterBlendModes: RasterBlendMode[] = ['normal','multiply','screen','darken','lighten','difference','exclusion'];
export const rasterBlendLabels = {
  ja: { normal:'通常', multiply:'乗算', screen:'スクリーン', darken:'比較（暗）', lighten:'比較（明）', difference:'差の絶対値', exclusion:'除外' },
  en: { normal:'Normal', multiply:'Multiply', screen:'Screen', darken:'Darken', lighten:'Lighten', difference:'Difference', exclusion:'Exclusion' },
  'zh-CN': { normal:'正常', multiply:'正片叠底', screen:'滤色', darken:'变暗', lighten:'变亮', difference:'差值', exclusion:'排除' },
};
