import { useMeasurementUnit, pixelsPerMeasurement, unitSymbols } from '../measurement-units';
import { useRef, useState } from 'react';
import type { Brush, DocumentSnapshot, TransformAction, PathOperation } from '../bridge';
import type { Locale } from '../i18n';
import { transformLabels } from './TransformDialog';
import { ColorPickerPopover } from './ColorPickerPopover';

const labels = {
  ja: { selection: '選択内容', count: '個選択', fill: '塗り', stroke: '線', width: '線幅', none: 'なし', mixed: '混在', swap: '塗りと線を交換', path: 'パス', rectangle: '長方形', ellipse: '楕円', text: 'テキスト', compound: '複合パス', position: '位置・サイズ', invalid: '有効な数値を入力してください。' },
  en: { selection: 'Selection', count: 'selected', fill: 'Fill', stroke: 'Stroke', width: 'Weight', none: 'None', mixed: 'Mixed', swap: 'Swap fill and stroke', path: 'Path', rectangle: 'Rectangle', ellipse: 'Ellipse', text: 'Text', compound: 'Compound path', position: 'Position and size', invalid: 'Enter a valid number.' },
  'zh-CN': { selection: '选择信息', count: '项已选择', fill: '填充', stroke: '描边', width: '线宽', none: '无', mixed: '混合', swap: '交换填充和描边', path: '路径', rectangle: '矩形', ellipse: '椭圆', text: '文字', compound: '复合路径', position: '位置和大小', invalid: '请输入有效数字。' },
};
const blendModes = ['normal','darken','multiply','color-burn','lighten','screen','color-dodge','overlay','soft-light','hard-light','difference','exclusion','hue','saturation','color','luminosity'];
const appearanceLabels = {
  ja: { opacity: '不透明度', blend: '描画モード', modes: ['通常','比較（暗）','乗算','焼き込みカラー','比較（明）','スクリーン','覆い焼きカラー','オーバーレイ','ソフトライト','ハードライト','差の絶対値','除外','色相','彩度','カラー','輝度'] },
  en: { opacity: 'Opacity', blend: 'Blend mode', modes: ['Normal','Darken','Multiply','Color Burn','Lighten','Screen','Color Dodge','Overlay','Soft Light','Hard Light','Difference','Exclusion','Hue','Saturation','Color','Luminosity'] },
  'zh-CN': { opacity: '不透明度', blend: '混合模式', modes: ['正常','变暗','正片叠底','颜色加深','变亮','滤色','颜色减淡','叠加','柔光','强光','差值','排除','色相','饱和度','颜色','明度'] },
};
function NumberField({ label, value, placeholder, disabled, onCommit }: { label: string; value: number | null; placeholder?: string; disabled: boolean; onCommit: (value: number) => void }) {
  const formatted = value === null ? '' : String(Number(value.toFixed(3)));
  const [draft, setDraft] = useState<string | null>(null);
  return <label>{label}<input type="number" step="any" disabled={disabled} value={draft ?? formatted} placeholder={placeholder}
    onChange={e => setDraft(e.target.value)} onBlur={() => { const text = draft; setDraft(null); if (text !== null && text.trim() !== '' && Number.isFinite(Number(text)) && Number(text) !== value) onCommit(Number(text)); }}
    onKeyDown={e => { if (e.key === 'Enter') e.currentTarget.blur(); if (e.key === 'Escape') { e.preventDefault(); setDraft(null); } }} /></label>;
}
export function SelectionOptions({ document, locale, enabled, onAppearance, onPaint, onWidth, onTransform, onTransformMenu, onCombine, onError }: {
  document: DocumentSnapshot; locale: Locale; enabled: boolean;
  onAppearance: (opacity: number | null, blendMode: string | null) => Promise<void>;
  onPaint: (target: 'fill' | 'stroke' | 'swap', color: Brush['color'] | null) => Promise<void>;
  onWidth: (width: number) => Promise<void>;
  onTransform: (action: TransformAction, values: number[]) => Promise<void>;
  onCombine: (operation: PathOperation) => Promise<void>;
  onTransformMenu: (action: TransformAction) => void; onError: (message: string) => void;
}) {
  const measurementUnit=useMeasurementUnit(), factor=pixelsPerMeasurement(measurementUnit,document.resolution), symbol=unitSymbols[measurementUnit];
  const appearance = appearanceLabels[locale];
  const t = labels[locale], transforms = transformLabels[locale];
  const [busy, setBusy] = useState(false);
  const pendingPaint = useRef<{ target: 'fill' | 'stroke'; color: Brush['color'] } | null>(null);
  const painting = useRef(false);
  const objects = document.layers.flatMap(layer => layer.objects.filter(o => document.selectedVectorObjects.includes(o.id)).map(object => ({ object, layer })));
  const first = objects[0]?.object;
  const editable = enabled && !busy;
  const paintable = enabled && !document.activeSavedPath && objects.length > 0 && objects.every(({object, layer}) => object.kind !== 'text' && object.visible && layer.visible && !layer.locked);
  const run = async (operation: () => Promise<void>) => { if (!editable) return; setBusy(true); try { await operation(); } catch (error) { onError(String(error)); } finally { setBusy(false); } };
  const paintLive = (target: 'fill' | 'stroke', color: Brush['color']) => {
    if (!paintable) return;
    pendingPaint.current = { target, color };
    if (painting.current) return;
    painting.current = true;
    void (async () => {
      try {
        while (pendingPaint.current) {
          const next = pendingPaint.current;
          pendingPaint.current = null;
          await onPaint(next.target, next.color);
        }
      } catch (error) {
        pendingPaint.current = null;
        onError(String(error));
      } finally {
        painting.current = false;
      }
    })();
  };
  const kind = first?.kind;
  const name = objects.some(({object}) => object.kind !== kind) ? t.mixed : kind === 'rectangle' ? t.rectangle : kind === 'ellipse' ? t.ellipse : kind === 'text' ? t.text : kind === 'compound' ? t.compound : t.path;
  const mixedWidth = objects.some(({object}) => object.strokeWidth !== first?.strokeWidth);
  const bounds = document.selectedBounds;
  const selectionKey = document.selectedVectorObjects.join('|')+symbol;
  return <div className="selection-options" role="group" aria-label={t.selection}>
    <span className="selection-kind" title={objects.map(({object}) => object.name).join(', ')}>{name}<small>{document.selectedVectorObjects.length} {t.count}</small></span>
    {!document.activeSavedPath && <>
      <label>{appearance.blend}<select aria-label={appearance.blend} disabled={!paintable || busy} value={objects.some(({object}) => object.blendMode !== first?.blendMode) ? '' : first?.blendMode ?? 'normal'} onChange={event => { const mode = event.target.value; void run(() => onAppearance(null, mode)); }}>
        <option value="" disabled>{t.mixed}</option>
        {blendModes.map((mode, index) => <option key={mode} value={mode}>{appearance.modes[index]}</option>)}
      </select></label>
      <NumberField key={`opacity-${selectionKey}`} label={`${appearance.opacity} (%)`} value={objects.some(({object}) => object.opacity !== first?.opacity) ? null : (first?.opacity ?? 1) * 100} placeholder={t.mixed} disabled={!paintable || busy} onCommit={value => {
        if (value < 0 || value > 100) { onError(t.invalid); return; }
        void run(() => onAppearance(value / 100, null));
      }} />
      {(['fill', 'stroke'] as const).map(target => {
        const key = target === 'fill' ? 'fillColor' : 'strokeColor';
        const color = first?.[key];
        const rgb = (color?.slice(0, 3) ?? [0, 0, 0]) as Brush['color'];
        const mixed = objects.some(({object}) => JSON.stringify(object[key]) !== JSON.stringify(color));
        return <div className="selection-paint" key={target}><span>{t[target]}</span><ColorPickerPopover locale={locale} color={rgb} disabled={!paintable} label={t[target]} onChange={color => paintLive(target, color)} />
          <button disabled={!paintable} title={`${t[target]}: ${t.none}`} onClick={() => void run(() => onPaint(target,null))}>∅</button>
          {(mixed || !color) && <small>{mixed ? t.mixed : t.none}</small>}</div>;
      })}
      <button disabled={!paintable} title={t.swap} aria-label={t.swap} onClick={() => void run(() => onPaint('swap',null))}>⇄</button>
      <NumberField key={`width-${selectionKey}`} label={`${t.width} (${symbol})`} value={mixedWidth || !first ? null : first.strokeWidth / factor} placeholder={t.mixed} disabled={!paintable}
        onCommit={value => { const width = value * factor; if (width < 0 || width > 4096) { onError(t.invalid); return; } void run(() => onWidth(width)); }} />
    </>}
    {bounds && <div className="selection-geometry" role="group" aria-label={t.position}>
      {(['X','Y','W','H'] as const).map((label,index) => {
        const value = index < 2 ? bounds[index] : bounds[index] - bounds[index - 2];
        return <NumberField key={`${selectionKey}-${label}`} label={`${label} (${symbol})`} value={value/factor} disabled={!editable || (index >= 2 && value <= 0)} onCommit={displayNext => {
          const next=displayNext*factor;
          if (index < 2) void run(() => onTransform('move',[index === 0 ? next-value : 0,index === 1 ? next-value : 0,0,0]));
          else if (next > 0 && next/value >= 0.01 && next/value <= 100) void run(() => onTransform('scale',[index === 2 ? next/value : 1,index === 3 ? next/value : 1,0,0]));
          else onError(t.invalid);
        }} />;
      })}
    </div>}
    {document.selectedVectorObjects.length === 2 && !document.activeSavedPath && <select value="" disabled={!paintable} aria-label={locale === 'ja' ? '図形を合成' : locale === 'en' ? 'Combine shapes' : '合并形状'} onChange={e => { const operation = e.target.value as PathOperation; void run(() => onCombine(operation)); }}>
      <option value="">{locale === 'ja' ? '図形を合成' : locale === 'en' ? 'Combine shapes' : '合并形状'}</option>
      {(['union','difference','intersection','xor'] as const).map((operation,index) => <option key={operation} value={operation}>{({ja:['合体','前面を型抜き','交差','中マド'],en:['Union','Difference','Intersection','Exclude'],'zh-CN':['联合','减去','交集','排除']}[locale])[index]}</option>)}
    </select>}
    <select value="" disabled={!editable} aria-label={transforms.title} onChange={e => onTransformMenu(e.target.value as TransformAction)}><option value="">{transforms.title}</option>{(['move','rotate','reflect','scale','shear','individual','reset'] as const).map(action => <option key={action} value={action}>{transforms[action]}</option>)}</select>
  </div>;
}
