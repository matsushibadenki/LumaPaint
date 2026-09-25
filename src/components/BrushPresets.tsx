import type { Brush } from '../bridge';
import type { Locale } from '../i18n';

const presets = [
  { id: 'detail', size: 2, hardness: 1, names: ['細線', 'Fine liner', '细线'] },
  { id: 'ink', size: 6, hardness: 1, names: ['線画', 'Inking', '勾线'] },
  { id: 'round', size: 16, hardness: 1, names: ['ハード円', 'Hard round', '硬圆'] },
  { id: 'bold', size: 40, hardness: 1, names: ['太線', 'Bold round', '粗线'] },
  { id: 'paint', size: 64, hardness: .8, names: ['塗り', 'Painting', '铺色'] },
  { id: 'soft', size: 32, hardness: .5, names: ['ソフト円', 'Soft round', '柔圆'] },
  { id: 'shade', size: 80, hardness: .2, names: ['柔らかい陰影', 'Soft shading', '柔和阴影'] },
  { id: 'air', size: 160, hardness: 0, names: ['広いぼかし', 'Broad soft', '大幅柔化'] },
] as const;

const labels = {
  ja: { title: 'ブラシプリセット', custom: 'カスタム', size: 'サイズ', hardness: '硬さ', hint: '線の見本をクリックして選択。サイズ・硬さは下で調整できます。' },
  en: { title: 'Brush presets', custom: 'Custom', size: 'Size', hardness: 'Hardness', hint: 'Choose a stroke sample. Adjust size and hardness below.' },
  'zh-CN': { title: '画笔预设', custom: '自定义', size: '大小', hardness: '硬度', hint: '点击笔触示例选择，下方可调整大小和硬度。' },
};

// Small vector previews only. Document strokes remain in the Rust/GPU renderer.
function StrokeSample({ size, hardness }: { size: number; hardness: number }) {
  const width = Math.max(1, Math.min(27, size * .35));
  return <svg viewBox="0 0 160 42" aria-hidden="true" focusable="false">
    {Array.from({ length: 20 }, (_, index) => {
      const radius = 1 - index / 20;
      const t = Math.max(0, Math.min(1, (radius - hardness) / Math.max(.001, 1 - hardness)));
      const coverage = 1 - t * t * (3 - 2 * t);
      return <path key={index} d="M 16 28 C 43 28 43 12 73 14 S 110 32 144 14" fill="none" stroke="currentColor" strokeWidth={width * radius} strokeLinecap="round" opacity={hardness === 1 ? 1 : coverage * .28} />;
    })}
  </svg>;
}

export function BrushPresets({ locale, brush, enabled, onChange }: {
  locale: Locale; brush: Brush; enabled: boolean; onChange: (brush: Brush) => void;
}) {
  const t = labels[locale];
  const language = locale === 'ja' ? 0 : locale === 'en' ? 1 : 2;
  const selected = presets.find(preset => preset.size === brush.size && Math.abs(preset.hardness - brush.hardness) < .001);
  return <div className="brush-presets">
    <div className="brush-presets-heading"><strong>{t.title}</strong><span>{selected?.names[language] ?? t.custom}</span></div>
    <p className="muted small">{t.hint}</p>
    <div className="brush-preset-grid" role="group" aria-label={t.title}>
      {presets.map(preset => <button type="button" key={preset.id} className="brush-preset" disabled={!enabled}
        aria-pressed={selected?.id === preset.id}
        aria-label={`${preset.names[language]}, ${t.size} ${preset.size}px, ${t.hardness} ${preset.hardness * 100}%`}
        onClick={() => onChange({ ...brush, size: preset.size, hardness: preset.hardness })}>
        <StrokeSample size={preset.size} hardness={preset.hardness} />
        <span className="brush-preset-name">{preset.names[language]}</span>
        <small>{preset.size}px · {preset.hardness * 100}%</small>
      </button>)}
    </div>
  </div>;
}
