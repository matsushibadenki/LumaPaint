export const messages = {
  en: {
    language: 'Language', theme: 'Appearance', system: 'System', light: 'Light', dark: 'Dark',
    title: ['A foundation for', 'your next creation.'],
    description: 'Illustration, comics, AI, and animation in one non-destructive workspace.',
    stage: 'Development foundation',
    canvas: 'Canvas',
    canvasNote: 'Native painting canvas. Drag to draw with the selected brush.',
    zoom: 'Zoom relative to fit', zoomIn: 'Zoom in', zoomOut: 'Zoom out', hand: 'Hand', fit: 'Fit', errorDetails: 'Error details',
    canvasStatus: { loading: 'Initializing GPU…', ready: 'Ready', browser: 'Open the macOS app to preview native GPU rendering.', unsupported: 'Native canvas support for this platform is planned.', failed: 'Canvas could not be displayed. Check the error details and retry.', hidden: 'GPU preview paused' },
    connection: 'Core connection', connecting: 'Connecting…', connected: 'Rust core connected',
    browser: 'Browser preview · launch the desktop app to connect to Rust', failed: 'Connection failed',
    dismiss: 'Dismiss', retry: 'Retry', platform: 'Platform', version: 'Version', renderer: 'Native renderer',
    pending: 'Not implemented yet',
  },
  ja: {
    language: '言語', theme: '外観', system: 'システム', light: 'ライト', dark: 'ダーク',
    title: ['次の作品を、', 'ここから。'],
    description: 'イラスト、漫画、AI、アニメーションを、ひとつの非破壊ワークスペースに。',
    stage: '開発基盤', canvas: 'キャンバス',
    canvasNote: 'ネイティブ描画キャンバス。ドラッグすると選択したブラシで描けます。',
    zoom: '全体表示を基準とする倍率', zoomIn: '拡大', zoomOut: '縮小', hand: 'ハンド', fit: '全体表示', errorDetails: 'エラーの詳細',
    canvasStatus: { loading: 'GPUを準備中…', ready: '準備完了', browser: 'ネイティブGPU描画はmacOSアプリで確認できます。', unsupported: 'このOSのネイティブキャンバスは今後対応します。', failed: 'キャンバスを表示できません。以下のエラー詳細を確認し、再試行してください。', hidden: 'GPU描画を一時停止中' },
    connection: 'コアとの接続', connecting: '接続中…', connected: 'Rustコアに接続済み',
    browser: 'ブラウザプレビュー · Rustとの接続にはデスクトップアプリを起動してください',
    failed: '接続に失敗しました', dismiss: '閉じる', retry: '再試行', platform: 'プラットフォーム',
    version: 'バージョン', renderer: 'ネイティブ描画', pending: '未実装',
  },
  'zh-CN': {
    language: '语言', theme: '外观', system: '跟随系统', light: '浅色', dark: '深色',
    title: ['为下一部作品', '打好基础。'],
    description: '在统一的无损工作空间中创作插画、漫画、AI图像和动画。',
    stage: '开发基础', canvas: '画布',
    canvasNote: '原生绘画画布。拖动即可使用所选画笔绘画。',
    zoom: '相对于适应画布的缩放比例', zoomIn: '放大', zoomOut: '缩小', hand: '抓手', fit: '适应画布', errorDetails: '错误详情',
    canvasStatus: { loading: '正在初始化GPU…', ready: '就绪', browser: '请打开macOS应用以预览原生GPU渲染。', unsupported: '此平台的原生画布将在后续支持。', failed: '无法显示画布。请查看以下错误详情并重试。', hidden: 'GPU预览已暂停' },
    connection: '核心连接', connecting: '正在连接…', connected: '已连接Rust核心',
    browser: '浏览器预览 · 请启动桌面应用以连接Rust核心', failed: '连接失败',
    dismiss: '关闭', retry: '重试', platform: '平台', version: '版本', renderer: '原生渲染', pending: '尚未实现',
  },
} as const;

export type Locale = keyof typeof messages;
export type Theme = 'system' | 'light' | 'dark';

export function readPreference(key: string): string | null {
  try { return localStorage.getItem(`lumapaint.${key}`); } catch { return null; }
}

export function savePreference(key: string, value: string) {
  try { localStorage.setItem(`lumapaint.${key}`, value); } catch { /* Preferences remain usable in memory. */ }
}

export function initialLocale(): Locale {
  const stored = readPreference('locale');
  if (stored === 'en' || stored === 'ja' || stored === 'zh-CN') return stored;
  const language = navigator.language.toLowerCase();
  return language.startsWith('ja') ? 'ja' : language.startsWith('zh') ? 'zh-CN' : 'en';
}

export function initialTheme(): Theme {
  const stored = readPreference('theme');
  return stored === 'light' || stored === 'dark' ? stored : 'system';
}
