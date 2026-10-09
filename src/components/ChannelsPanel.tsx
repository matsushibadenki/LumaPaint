import {modeLabels} from '../document-color-modes';
import type { ColorMode, DisplayChannel, LayerEditTarget } from '../bridge';
import type { Locale } from '../i18n';

const labels = {
  ja: { mask: 'マスク（アルファ）', readOnly: 'このレイヤーの色成分は表示のみです。透明度はレイヤーマスクで編集できます。', none: 'レイヤーまたはマスクのサムネイルを選択してください。', title: 'チャンネル', composite: '合成', red: 'レッド', green: 'グリーン', blue: 'ブルー', cyan: 'シアン', magenta: 'マゼンタ', yellow: 'イエロー', black: 'ブラック', alpha: 'アルファ（透明度）', hint: '選択中のレイヤー・マスクの成分を表示します。ブラシ・消しゴムで選択チャンネルだけを編集できます。アルファは白が不透明、黒が透明です。', cmyk: 'CMYKの色成分は表示専用の簡易変換です。ICCプロファイルによる色分解ではありません。' },
  en: { mask: 'Mask (alpha)', readOnly: 'Color components of this layer are read-only. Use a layer mask to edit opacity.', none: 'Select a layer or mask thumbnail to edit.', title: 'Channels', composite: 'Composite', red: 'Red', green: 'Green', blue: 'Blue', cyan: 'Cyan', magenta: 'Magenta', yellow: 'Yellow', black: 'Black', alpha: 'Alpha (opacity)', hint: 'View components of the selected layer or mask. Brush and Eraser edit only the selected channel. Alpha is white for opaque and black for transparent.', cmyk: 'CMYK color components are read-only approximate RGB conversions, not an ICC color separation.' },
  'zh-CN': { mask: '蒙版（Alpha）', readOnly: '此图层的颜色分量仅供查看。可用图层蒙版编辑不透明度。', none: '请选择图层或蒙版缩略图进行编辑。', title: '通道', composite: '复合', red: '红', green: '绿', blue: '蓝', cyan: '青', magenta: '洋红', yellow: '黄', black: '黑', alpha: 'Alpha（不透明度）', hint: '显示所选图层或蒙版的分量。画笔和橡皮擦仅编辑所选通道。Alpha中白色表示不透明，黑色表示透明。', cmyk: 'CMYK颜色分量仅供查看，由RGB近似转换，并非基于ICC配置文件的分色。' },
};
export function ChannelsPanel({ target = 'content', layerName = '', editable = true, thumbnails = [], thumbnailError, locale, mode, value, enabled, onChange }: {
  target?: LayerEditTarget; layerName?: string; editable?: boolean;
  thumbnails?: string[]; thumbnailError?: string;
  locale: Locale; mode: ColorMode; value: DisplayChannel; enabled: boolean; onChange: (value: DisplayChannel) => void;
}) {
  const t = labels[locale];
  const rows: { id: DisplayChannel; label: string; symbol: string }[] = [
    { id: 0, label: `${modeLabels[locale][mode]} · ${t.composite}`, symbol: mode.toUpperCase() },
    ...((target === 'mask' || mode === 'grayscale' || mode === 'lab') ? [] : mode === 'cmyk' ? [
      { id: 5 as const, label: t.cyan, symbol: 'C' }, { id: 6 as const, label: t.magenta, symbol: 'M' },
      { id: 7 as const, label: t.yellow, symbol: 'Y' }, { id: 8 as const, label: t.black, symbol: 'K' },
    ] : [
      { id: 1 as const, label: t.red, symbol: 'R' }, { id: 2 as const, label: t.green, symbol: 'G' }, { id: 3 as const, label: t.blue, symbol: 'B' },
    ]),
    { id: 4, label: target === 'mask' ? t.mask : t.alpha, symbol: 'α' },
  ];
  return <section className="channel-list" aria-label={t.title}>
    {layerName && <p className="channel-hint">{layerName}</p>}
    {rows.map(row => <button type="button" key={row.id} className={`channel-row${value === row.id ? ' active' : ''}${row.id === 4 ? ' alpha' : ''}`} aria-pressed={value === row.id} disabled={!enabled} onClick={() => onChange(row.id)}>
      <span aria-hidden="true">{value === row.id ? '●' : ''}</span><span className="channel-thumbnail" aria-hidden="true" title={thumbnailError}>{thumbnails[row.id] ? <img src={thumbnails[row.id]} alt="" draggable={false} /> : row.symbol}</span><span>{row.label}</span>
    </button>)}
    <p className="channel-hint">{target === 'none' ? t.none : !editable && target !== 'mask' ? t.readOnly : t.hint}</p>
    {mode === 'cmyk' && target !== 'mask' && <p className="channel-hint">{t.cmyk}</p>}
  </section>;
}
