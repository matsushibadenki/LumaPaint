import type { ColorMode, DisplayChannel } from '../bridge';
import type { Locale } from '../i18n';

const labels = {
  ja: { title: '表示チャンネル', composite: '合成', red: 'レッド', green: 'グリーン', blue: 'ブルー', cyan: 'シアン', magenta: 'マゼンタ', yellow: 'イエロー', black: 'ブラック', alpha: 'アルファ（透明度）', hint: '色成分をグレースケールで確認できます。表示のみの切り替えです。アルファは白が不透明、黒が透明です。', cmyk: 'CMYKはRGBからの簡易変換表示です。ICCプロファイルによる色分解ではありません。' },
  en: { title: 'Display channel', composite: 'Composite', red: 'Red', green: 'Green', blue: 'Blue', cyan: 'Cyan', magenta: 'Magenta', yellow: 'Yellow', black: 'Black', alpha: 'Alpha (opacity)', hint: 'Inspect components in grayscale. This changes the preview only. Alpha is white for opaque and black for transparent.', cmyk: 'CMYK is an approximate RGB conversion, not an ICC color separation.' },
  'zh-CN': { title: '显示通道', composite: '复合', red: '红', green: '绿', blue: '蓝', cyan: '青', magenta: '洋红', yellow: '黄', black: '黑', alpha: 'Alpha（不透明度）', hint: '以灰度查看颜色分量，仅影响预览。Alpha中白色表示不透明，黑色表示透明。', cmyk: 'CMYK由RGB近似转换，并非基于ICC配置文件的分色。' },
};
export function ChannelsPanel({ locale, mode, value, enabled, onChange }: {
  locale: Locale; mode: ColorMode; value: DisplayChannel; enabled: boolean; onChange: (value: DisplayChannel) => void;
}) {
  const t = labels[locale];
  const rows: { id: DisplayChannel; label: string; symbol: string }[] = [
    { id: 0, label: `${mode.toUpperCase()} · ${t.composite}`, symbol: mode.toUpperCase() },
    ...(mode === 'cmyk' ? [
      { id: 5 as const, label: t.cyan, symbol: 'C' }, { id: 6 as const, label: t.magenta, symbol: 'M' },
      { id: 7 as const, label: t.yellow, symbol: 'Y' }, { id: 8 as const, label: t.black, symbol: 'K' },
    ] : [
      { id: 1 as const, label: t.red, symbol: 'R' }, { id: 2 as const, label: t.green, symbol: 'G' }, { id: 3 as const, label: t.blue, symbol: 'B' },
    ]),
    { id: 4, label: t.alpha, symbol: 'α' },
  ];
  return <section className="channel-list" aria-label={t.title}>
    {rows.map(row => <button type="button" key={row.id} className={`channel-row${value === row.id ? ' active' : ''}${row.id === 4 ? ' alpha' : ''}`} aria-pressed={value === row.id} disabled={!enabled} onClick={() => onChange(row.id)}>
      <span aria-hidden="true">{value === row.id ? '●' : ''}</span><span className="channel-symbol" aria-hidden="true">{row.symbol}</span><span>{row.label}</span>
    </button>)}
    <p className="channel-hint">{t.hint}</p>
    {mode === 'cmyk' && <p className="channel-hint">{t.cmyk}</p>}
  </section>;
}
