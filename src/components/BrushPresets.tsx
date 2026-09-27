import type { Brush } from '../bridge';
import type { Locale } from '../i18n';
import { BrushEnvelopePanel } from './BrushEnvelopePanel';

const presets: { id: string; size: number; hardness: number; simulation?: Brush['simulation']; names: readonly string[] }[] = [
  { id: 'ink-pen', size: 14, hardness: 1, simulation: 'ink', names: ['インクペン', 'Ink pen', '墨水笔'] },
  { id: 'graphite', size: 8, hardness: .75, simulation: 'pencil', names: ['鉛筆・紙目', 'Graphite', '铅笔纸纹'] },
  { id: 'dry-brush', size: 48, hardness: .85, simulation: 'dryBrush', names: ['ドライブラシ', 'Dry brush', '干刷'] },
  { id: 'detail', size: 2, hardness: 1, names: ['細線', 'Fine liner', '细线'] },
  { id: 'ink', size: 6, hardness: 1, names: ['線画', 'Inking', '勾线'] },
  { id: 'round', size: 16, hardness: 1, names: ['ハード円', 'Hard round', '硬圆'] },
  { id: 'bold', size: 40, hardness: 1, names: ['太線', 'Bold round', '粗线'] },
  { id: 'paint', size: 64, hardness: .8, names: ['塗り', 'Painting', '铺色'] },
  { id: 'soft', size: 32, hardness: .5, names: ['ソフト円', 'Soft round', '柔圆'] },
  { id: 'shade', size: 80, hardness: .2, names: ['柔らかい陰影', 'Soft shading', '柔和阴影'] },
  { id: 'air', size: 160, hardness: 0, names: ['広いぼかし', 'Broad soft', '大幅柔化'] },
];

const labels = {
  ja: { title: 'ブラシプリセット', custom: 'カスタム', size: 'サイズ', hardness: '硬さ', hint: '線の見本をクリックして選択。サイズ・硬さは下で調整できます。' },
  en: { title: 'Brush presets', custom: 'Custom', size: 'Size', hardness: 'Hardness', hint: 'Choose a stroke sample. Adjust size and hardness below.' },
  'zh-CN': { title: '画笔预设', custom: '自定义', size: '大小', hardness: '硬度', hint: '点击笔触示例选择，下方可调整大小和硬度。' },
};

// Small vector previews only. Document strokes remain in the Rust/GPU renderer.
function StrokeSample({ size, hardness, simulation = 'round' }: { size: number; hardness: number; simulation?: Brush['simulation'] }) {
  const width = Math.max(1, Math.min(27, size * .35));
  return <svg viewBox="0 0 160 42" aria-hidden="true" focusable="false">
    {simulation === 'ink' ? <path d="M16 28 C43 28 43 10 73 11 S110 31 144 14 C110 37 98 20 73 18 S43 30 16 28Z" fill="currentColor" /> : <>
    {Array.from({ length: 20 }, (_, index) => {
      const radius = 1 - index / 20;
      const t = Math.max(0, Math.min(1, (radius - hardness) / Math.max(.001, 1 - hardness)));
      const coverage = 1 - t * t * (3 - 2 * t);
      return <path key={index} d="M 16 28 C 43 28 43 12 73 14 S 110 32 144 14" fill="none" stroke="currentColor" strokeWidth={width * radius} strokeLinecap="round" strokeDasharray={simulation === 'dryBrush' ? '2 3' : simulation === 'pencil' ? '1 1.5' : undefined} opacity={hardness === 1 ? 1 : coverage * .28} />;
    })}</>}
  </svg>;
}

export function BrushPresets({ locale, brush, enabled, onChange }: {
  locale: Locale; brush: Brush; enabled: boolean; onChange: (brush: Brush) => void;
}) {
  const t = labels[locale];
  const language = locale === 'ja' ? 0 : locale === 'en' ? 1 : 2;
  const selected = presets.find(preset => preset.size === brush.size && Math.abs(preset.hardness - brush.hardness) < .001 && (preset.simulation ?? 'round') === (brush.simulation ?? 'round'));
  return <div className="brush-presets">
    <div className="brush-presets-heading"><strong>{t.title}</strong><span>{selected?.names[language] ?? t.custom}</span></div>
    <p className="muted small">{t.hint}</p>
    <label className="brush-simulation-choice">
      <span>{locale === 'ja' ? '描画方式' : locale === 'en' ? 'Stroke simulation' : '笔触模拟'}</span>
      <select disabled={!enabled} value={brush.simulation ?? 'round'} onChange={event => onChange({ ...brush, simulation: event.target.value as Brush['simulation'] })}>
        <option value="round">{locale === 'ja' ? '通常' : locale === 'en' ? 'Round' : '普通'}</option>
        {presets.filter(preset => preset.simulation).map(preset => <option key={preset.id} value={preset.simulation}>{preset.names[language]}</option>)}
      </select>
    </label>
    <div className="brush-preset-grid" role="group" aria-label={t.title}>
      {presets.map(preset => <button type="button" key={preset.id} className="brush-preset" disabled={!enabled}
        aria-pressed={selected?.id === preset.id}
        aria-label={`${preset.names[language]}, ${t.size} ${preset.size}px, ${t.hardness} ${preset.hardness * 100}%`}
        onClick={() => onChange({ ...brush, size: preset.size, hardness: preset.hardness, simulation: preset.simulation ?? 'round' })}>
        <StrokeSample size={preset.size} hardness={preset.hardness} simulation={preset.simulation} />
        <span className="brush-preset-name">{preset.names[language]}</span>
        <small>{preset.size}px · {preset.hardness * 100}%</small>
      </button>)}
    </div>
    <BrushEnvelopePanel locale={locale} brush={brush} enabled={enabled} onChange={onChange} />
  </div>;
}
