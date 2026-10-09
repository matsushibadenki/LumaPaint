import type { Locale } from '../i18n';
export const maskLinkLabels = {
  ja: { link: '本体とマスクをリンク', unlink: '本体とマスクのリンクを解除' },
  en: { link: 'Link layer and mask', unlink: 'Unlink layer and mask' },
  'zh-CN': { link: '链接图层与蒙版', unlink: '取消图层与蒙版链接' },
};
export function MaskLinkButton({locale,linked,disabled,onToggle}:{locale:Locale;linked:boolean;disabled:boolean;onToggle:()=>void}) {
  const label=maskLinkLabels[locale][linked?'unlink':'link'];
  return <button type="button" className="mask-link-button" title={label} aria-label={label} aria-pressed={linked} disabled={disabled}
    onPointerDown={e=>e.stopPropagation()} onClick={e=>{e.stopPropagation();onToggle();}}>
    <svg width="12" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden="true">
      {linked?<><path d="M9 14a5 5 0 0 0 7 0l4-4a5 5 0 0 0-7-7l-2 2"/><path d="M15 10a5 5 0 0 0-7 0l-4 4a5 5 0 0 0 7 7l2-2"/></>:<><path d="m14 7 2-2a3 3 0 0 1 4 4l-2 2M10 17l-2 2a3 3 0 0 1-4-4l2-2M3 3l18 18"/></>}
    </svg>
  </button>;
}
