# Motion reference coverage — 2026-10-12

Source: [Motion Reference](https://motion-reference.takayustudio.jp/). The rendered merged catalog contained 782 entries in 64 categories. Research inspected the public item data, transform/keyframe programs, SVG demonstrations and text recipes, including press/hold/rebound timing. This file records the functional mapping, not a claim of pixel-identical reproduction of the reference demonstrations.

All IDs have independent Rust timeline programs. The website’s implementation, descriptions, HTML and demonstration artwork are not bundled. Names and IDs identify the functional presets. Chinese names and interface text are localized. 185 programs retain numeric tracks for individual actors; the remaining programs compose the motion kernels and character layout operations.

## Category inventory

| Category | 日本語 | English | 简体中文 | Types |
|---|---|---|---|---:|
| `principles` | 基本原則 | Principles | 基本原理 | 17 |
| `easing` | イージング | Easing | 缓动 | 25 |
| `spring` | スプリング | Springs | 弹簧 | 16 |
| `timing` | タイミング・リズム | Timing & Rhythm | 时序与节奏 | 15 |
| `entrance` | 出現・退場 | Entrance & Exit | 进入与退出 | 11 |
| `move` | 移動 | Movement | 移动 | 11 |
| `scale` | 拡大・縮小 | Scale | 缩放 | 11 |
| `rotate` | 回転 | Rotation | 旋转 | 11 |
| `reaction` | 弾み・反動 | Bounce & Reaction | 弹跳与反馈 | 11 |
| `collection` | リスト・グリッド | List & Grid | 列表与网格 | 12 |
| `attention` | 注目・強調 | Attention & Emphasis | 强调 | 12 |
| `light` | 発光・ライト | Light & Glow | 光效与发光 | 12 |
| `effect` | エフェクト・切替 | Effects & Transitions | 效果与切换 | 12 |
| `transitions` | トランジション | Transitions | 转场 | 25 |
| `camera` | カメラ | Camera | 相机 | 18 |
| `web-scroll` | Webスクロール | Web Scroll | 网页滚动 | 12 |
| `web-interaction` | Web操作・反応 | Web Interaction | 网页交互 | 15 |
| `web-layout` | レイアウト・遷移 | Layout & Navigation | 布局与导航 | 19 |
| `web-feedback` | 読込・データ | Loading & Data | 加载与数据 | 12 |
| `form-state` | フォーム・状態 | Forms & State | 表单与状态 | 8 |
| `gesture` | ジェスチャー | Gesture & Mobile | 手势 | 8 |
| `svg-path` | SVG・パス | SVG & Path | SVG与路径 | 12 |
| `ui-button` | UIボタン | UI Buttons | 界面按钮 | 22 |
| `ui-control` | 選択・コントロール | Selection & Controls | 选择与控件 | 14 |
| `ui-navigation` | ナビ・オーバーレイ | Navigation & Overlays | 导航与浮层 | 6 |
| `ui-loading` | ローダー・進捗 | Loading & Progress | 加载与进度 | 10 |
| `ui-feedback` | 通知・リアクション | Feedback & Reactions | 通知与反馈 | 8 |
| `game-hud` | ゲームHUD | Game HUD | 游戏HUD | 12 |
| `game-combat` | 戦闘・ヒット表現 | Combat & Hit FX | 战斗与命中 | 8 |
| `game-reward` | 報酬・獲得演出 | Rewards & Loot | 奖励与获取 | 10 |
| `game-progress` | 成長・クエスト | Progression & Quests | 成长与任务 | 9 |
| `game-cinematic` | ゲーム演出・警告 | Game Cinematics | 游戏演出 | 8 |
| `ui-spatial` | 空間UI・奥行き | Spatial UI & Depth | 空间界面与景深 | 20 |
| `ui-material` | 質感・マテリアル変形 | Material & Surface Motion | 材质与形变 | 20 |
| `game-impact-fx` | ゲーム瞬間FX | Game Impact FX | 游戏瞬间特效 | 20 |
| `data-story` | データ・物語化 | Data Storytelling | 数据叙事 | 23 |
| `particles` | パーティクル | Particles | 粒子 | 8 |
| `loops` | ループ・アンビエント | Loops & Ambient | 循环与环境 | 7 |
| `character` | キャラクター | Character | 角色 | 7 |
| `look` | 質感・ルック | Texture & Look | 纹理与外观 | 10 |
| `depth3d` | 3D | 3D | 3D | 3 |
| `sound` | 音と同期 | Sound Sync | 声音同步 | 4 |
| `tx-reveal` | 出現 | Reveal | 文字进入 | 19 |
| `tx-exit` | 退場 | Exit | 文字退出 | 9 |
| `tx-emphasis` | 強調 | Emphasis | 文字强调 | 14 |
| `tx-swap` | 入れ替え | Swap | 文字替换 | 12 |
| `tx-typing` | タイピング・解読 | Typing & Decoding | 打字与解码 | 12 |
| `tx-physics` | 文字の物理 | Glyph Physics | 字符物理 | 12 |
| `tx-path` | パス・形 | Path & Shape | 文字路径与形状 | 9 |
| `tx-mask` | マスク・クリップ | Mask & Clip | 文字遮罩 | 12 |
| `tx-color-light` | 色・光 | Colour & Light | 文字颜色与光 | 12 |
| `tx-blur` | ぼかし・フォーカス | Blur & Focus | 文字模糊与对焦 | 9 |
| `tx-glitch` | 歪み・グリッチ | Distortion & Glitch | 文字扭曲与故障 | 8 |
| `tx-depth` | 3D・奥行き | 3D & Depth | 文字3D与景深 | 10 |
| `tx-variable` | 可変フォント | Variable Fonts | 可变字体 | 9 |
| `tx-scroll` | スクロール | Scroll | 文字滚动 | 13 |
| `tx-pointer` | ポインター | Pointer | 文字指针交互 | 9 |
| `tx-counter` | 数字・データ | Numbers & Data | 数字与数据 | 9 |
| `tx-caption` | 字幕・キャプション | Captions & Subtitles | 字幕与说明 | 13 |
| `tx-layout` | レイアウト・改行 | Layout & Reflow | 布局与换行 | 10 |
| `tx-loop` | ループ・装飾 | Loops & Decoration | 循环与装饰 | 8 |
| `tx-japanese` | 日本語 | Japanese | 日语文字 | 14 |
| `tx-title` | タイトル・映画 | Titles & Film | 标题与电影 | 13 |
| `tx-broadcast` | 放送・テロップ | Broadcast & Telop | 广播字幕 | 12 |

## Program inventory

Durations are initial active durations in milliseconds, rounded to document frames when added. Objects use the saved order; actor tracks cycle through that order. Compound UI/game examples need multiple artwork objects to show all parts of the reaction. Text programs require editable text. Curve parameters, source geometry and user overrides determine the result.

| ID | Name | Category | ms | Curve | Components | Actors |
|---|---|---|---:|---|---|---:|
| `fade-slide` | フェード＋スライド | `entrance` | 600 | `power` | fade, slideX | 0 |
| `mask-wipe` | マスクワイプ | `entrance` | 700 | `power` | wipe | 0 |
| `iris-open` | アイリスオープン | `entrance` | 760 | `power` | iris | 0 |
| `soft-rise` | ソフトライズ | `entrance` | 900 | `sine` | slideY, fade, blur | 0 |
| `snap-in` | スナップイン | `entrance` | 380 | `expo` | fade, slideY | 0 |
| `split-reveal` | スプリットリビール | `entrance` | 720 | `power` | split | 0 |
| `trail-exit` | 残像アウト | `entrance` | 520 | `power` | slideX, fade, repeat | 0 |
| `collapse-away` | 折りたたみ退場 | `entrance` | 560 | `back` | scaleY, fade | 0 |
| `linear-travel` | 一定速移動 | `move` | 800 | `linear` | travel | 0 |
| `smooth-arrival` | なめらかな到着 | `move` | 760 | `power` | travel | 0 |
| `arc-travel` | 弧を描く移動 | `move` | 800 | `sine` | arc | 0 |
| `anticipation-move` | 予備動作つき移動 | `move` | 820 | `back` | anticipate, travel | 0 |
| `heavy-stop` | 重い停止 | `move` | 680 | `power` | travel, squash, recoil | 0 |
| `follow-path` | パス追従 | `move` | 1000 | `sine` | arc, follow | 0 |
| `stagger-follow` | 段差追従 | `move` | 900 | `power` | follow | 0 |
| `magnetic-snap` | マグネット吸着 | `move` | 620 | `power` | travel, recoil | 0 |
| `pop-scale` | ポップスケール | `scale` | 520 | `back` | scale | 0 |
| `breathing` | 呼吸する拡縮 | `scale` | 1200 | `sine` | pulse | 0 |
| `elastic-scale` | エラスティックスケール | `scale` | 900 | `elastic` | scale | 0 |
| `axis-stretch` | 軸方向ストレッチ | `scale` | 700 | `back` | travel, jelly | 0 |
| `pulse-focus` | フォーカスパルス | `scale` | 680 | `power` | repeat | 0 |
| `depth-zoom` | 奥行きズーム | `scale` | 740 | `power` | scale, blur, fade | 0 |
| `soft-shrink` | ソフトシュリンク | `scale` | 600 | `power` | scale, fade | 0 |
| `impact-squash` | インパクトスクワッシュ | `scale` | 620 | `back` | hop, squash | 0 |
| `simple-turn` | シンプルターン | `rotate` | 700 | `power` | spin | 0 |
| `spin-stop` | スピン停止 | `rotate` | 850 | `power` | spin | 0 |
| `wobble` | 振り子の揺れ | `rotate` | 1000 | `elastic` | swing | 0 |
| `flip-card` | カードフリップ | `rotate` | 760 | `power` | flipY, textSwap | 0 |
| `orbit` | オービット | `rotate` | 1100 | `linear` | orbit | 0 |
| `tilt-reaction` | チルトリアクション | `rotate` | 560 | `back` | tilt | 0 |
| `tumble-in` | タンブルイン | `rotate` | 820 | `power` | spin, slideX | 0 |
| `counter-rotate` | カウンターローテーション | `rotate` | 1000 | `sine` | spin | 0 |
| `overshoot` | オーバーシュート | `reaction` | 600 | `back` | travel | 0 |
| `bounce-land` | バウンス着地 | `reaction` | 700 | `bounce` | hop | 0 |
| `spring-follow` | スプリング追従 | `reaction` | 1800 | `spring` | follow | 0 |
| `rubber-band` | ラバーバンド | `reaction` | 900 | `elastic` | jelly | 0 |
| `recoil` | リコイル | `reaction` | 520 | `power` | recoil | 0 |
| `double-bounce` | ダブルバウンス | `reaction` | 820 | `bounce` | hop | 0 |
| `snap-back` | スナップバック | `reaction` | 560 | `back` | recoil, jelly | 0 |
| `after-shake` | 余震 | `reaction` | 720 | `expo` | shake | 0 |
| `list-stagger` | リスト順次表示 | `collection` | 760 | `power` | fade, gather | 0 |
| `list-cascade` | カスケード表示 | `collection` | 820 | `power` | fade, gather | 0 |
| `list-alternate` | 左右交互イン | `collection` | 780 | `power` | fade, gather | 0 |
| `list-expand` | 行の展開 | `collection` | 720 | `back` | fade, gather | 0 |
| `grid-wave` | グリッドウェーブ | `collection` | 900 | `power` | wave | 0 |
| `grid-center` | 中央から展開 | `collection` | 860 | `back` | fade, gather | 0 |
| `grid-row-sweep` | 行スイープ | `collection` | 800 | `power` | sweep | 0 |
| `grid-shuffle` | シャッフル整列 | `collection` | 980 | `back` | reorder | 0 |
| `nudge` | ナッジ | `attention` | 520 | `back` | pulse | 0 |
| `attention-bounce` | アテンションバウンス | `attention` | 620 | `bounce` | hop | 0 |
| `badge-pop` | バッジポップ | `attention` | 560 | `back` | scale | 0 |
| `focus-halo` | フォーカスハロー | `attention` | 900 | `power` | repeat | 0 |
| `highlight-sweep` | ハイライトスイープ | `attention` | 820 | `power` | sweep | 0 |
| `border-chase` | ボーダーチェイス | `attention` | 1000 | `bezier` | strokeTrim, glow | 0 |
| `press-feedback` | プレスフィードバック | `attention` | 420 | `power` | squash | 0 |
| `jelly-emphasis` | ゼリー強調 | `attention` | 760 | `elastic` | jelly | 0 |
| `glow-pulse` | グローパルス | `light` | 1100 | `sine` | glow | 0 |
| `neon-flicker` | ネオン点滅 | `light` | 900 | `bezier` | glow | 0 |
| `light-sweep` | ライトスイープ | `light` | 950 | `power` | sweep, glow | 0 |
| `bloom-reveal` | ブルーム出現 | `light` | 780 | `power` | glow, scale | 0 |
| `rim-glow` | リムグロー | `light` | 1000 | `sine` | glow | 0 |
| `energy-charge` | エネルギーチャージ | `light` | 1200 | `power` | glow | 0 |
| `light-trail` | 光跡 | `light` | 900 | `power` | repeat, glow, travel | 0 |
| `scan-line` | スキャンライン | `light` | 1100 | `bezier` | glow | 0 |
| `blur-zoom` | ブラーズーム | `effect` | 720 | `power` | scale, blur | 0 |
| `chromatic-split` | 色収差分離 | `effect` | 900 | `power` | split, chromatic | 0 |
| `glitch-in` | グリッチイン | `effect` | 650 | `bezier` | chromatic, shake | 0 |
| `pixel-dissolve` | ピクセルディゾルブ | `effect` | 900 | `power` | blocks, fade | 0 |
| `particle-burst` | パーティクルバースト | `effect` | 820 | `power` | particles | 0 |
| `ripple-reveal` | リップル出現 | `effect` | 950 | `power` | displace, repeat | 0 |
| `slice-reveal` | スライスリビール | `effect` | 840 | `power` | blinds, slideX | 0 |
| `noise-reveal` | ノイズ解像 | `effect` | 1000 | `sine` | noise, fade | 0 |
| `scroll-reveal` | スクロールリビール | `web-scroll` | 900 | `power` | spin | 0 |
| `parallax-layers` | パララックスレイヤー | `web-scroll` | 1200 | `linear` | parallax | 0 |
| `sticky-pin` | スティッキーピン | `web-scroll` | 1100 | `linear` | slideY | 0 |
| `scroll-progress` | スクロール進捗 | `web-scroll` | 1000 | `linear` | spin | 0 |
| `horizontal-scrub` | 横スクロールスクラブ | `web-scroll` | 1200 | `linear` | slideY | 0 |
| `section-snap` | セクションスナップ | `web-scroll` | 850 | `power` | slideY | 0 |
| `text-scroll-scrub` | 文字スクロール解像 | `web-scroll` | 1000 | `linear` | spin | 0 |
| `scroll-depth-zoom` | スクロール奥行きズーム | `web-scroll` | 1200 | `power` | spin, scale | 0 |
| `hover-lift` | ホバーリフト | `web-interaction` | 1600 | `power` | tilt | 0 |
| `magnetic-button` | マグネティックボタン | `web-interaction` | 1800 | `power` | gather | 0 |
| `pointer-tilt` | ポインターチルト | `web-interaction` | 1800 | `power` | tilt | 0 |
| `cursor-follow` | カーソル追従 | `web-interaction` | 850 | `power` | follow | 0 |
| `drag-inertia` | ドラッグ慣性 | `web-interaction` | 1000 | `power` | travel | 0 |
| `click-ripple` | クリックリップル | `web-interaction` | 650 | `power` | displace, repeat | 0 |
| `toggle-morph` | トグルモーフ | `web-interaction` | 480 | `back` | morph | 0 |
| `accordion-open` | アコーディオン展開 | `web-interaction` | 1600 | `power` | tilt | 0 |
| `shared-element` | 共有要素トランジション | `web-layout` | 850 | `power` | scale, fade | 0 |
| `view-crossfade` | ビュークロスフェード | `web-layout` | 520 | `power` | fade | 0 |
| `flip-reorder` | FLIP並べ替え | `web-layout` | 760 | `power` | flipY, reorder | 0 |
| `modal-morph` | モーダルモーフ | `web-layout` | 700 | `power` | morph | 0 |
| `tab-indicator` | タブインジケーター | `web-layout` | 520 | `power` | scale, fade | 0 |
| `filter-reflow` | フィルター再配置 | `web-layout` | 820 | `power` | reorder | 0 |
| `route-slide` | ルートスライド | `web-layout` | 600 | `power` | slideX | 0 |
| `page-wipe` | ページワイプ | `web-layout` | 780 | `power` | wipe | 0 |
| `skeleton-shimmer` | スケルトンシマー | `web-feedback` | 1200 | `linear` | sweep | 0 |
| `spinner-arc` | スピナーアーク | `web-feedback` | 900 | `linear` | spin, strokeTrim | 0 |
| `progress-fill` | 進捗バー | `web-feedback` | 1300 | `power` | scaleX | 0 |
| `circular-progress` | 円形進捗 | `web-feedback` | 1400 | `power` | strokeDraw, counter | 0 |
| `loading-dots` | ローディングドット | `web-feedback` | 900 | `sine` | hop, pulse | 0 |
| `count-up` | カウントアップ | `web-feedback` | 1000 | `power` | counter | 0 |
| `chart-draw` | チャート描画 | `web-feedback` | 1200 | `power` | strokeDraw | 0 |
| `toast-stack` | トーストスタック | `web-feedback` | 900 | `power` | slideY, fade, reorder | 0 |
| `diagonal-wipe` | 斜めワイプ | `entrance` | 680 | `power` | wipe, diagonal | 0 |
| `fold-in` | 3D折り込み | `entrance` | 760 | `power` | flipX | 0 |
| `shrink-fade-exit` | 縮小フェード退場 | `entrance` | 520 | `power` | fade | 0 |
| `zigzag-travel` | ジグザグ移動 | `move` | 920 | `sine` | zigzag | 0 |
| `momentum-drift` | 慣性ドリフト | `move` | 880 | `power` | travel, wave | 0 |
| `waypoint-hop` | ウェイポイントホップ | `move` | 860 | `power` | hop | 0 |
| `center-punch` | センターパンチ | `scale` | 480 | `back` | scale | 0 |
| `width-reveal` | 横幅リビール | `scale` | 650 | `power` | scaleX | 0 |
| `depth-pullback` | 奥行きプルバック | `scale` | 760 | `power` | scale, blur | 0 |
| `coin-spin` | コインスピン | `rotate` | 900 | `power` | flipY | 0 |
| `gyroscope` | ジャイロ回転 | `rotate` | 1100 | `sine` | flipX, flipY | 0 |
| `rotate-reveal` | 回転リビール | `rotate` | 720 | `power` | flipY, fade | 0 |
| `settle-drop` | 沈み込み着地 | `reaction` | 760 | `power` | hop, squash | 0 |
| `drag-resistance` | ドラッグ抵抗 | `reaction` | 820 | `back` | travel, jelly | 0 |
| `follow-through` | フォロースルー | `reaction` | 860 | `power` | follow, swing | 0 |
| `line-by-line` | 行ごとのリビール | `tx-reveal` | 980 | `power` | lines, fade, slideY | 0 |
| `list-slide-stack` | リスト積み重ね | `collection` | 900 | `power` | slideX | 0 |
| `list-trim-collapse` | リスト削除詰め | `collection` | 780 | `power` | scaleY, strokeTrim | 0 |
| `grid-spiral` | グリッド螺旋展開 | `collection` | 1100 | `power` | orbit, gather | 0 |
| `grid-diagonal` | グリッド斜めウェーブ | `collection` | 940 | `power` | diagonal | 0 |
| `color-pulse` | カラーパルス | `attention` | 760 | `sine` | pulse, tint | 0 |
| `icon-nod` | アイコンうなずき | `attention` | 620 | `back` | pulse | 0 |
| `spotlight-focus` | スポットライトフォーカス | `attention` | 900 | `power` | blur, glow, iris | 0 |
| `error-shake` | エラーシェイク | `attention` | 540 | `expo` | shake | 0 |
| `aurora-glow` | オーロラグロー | `light` | 1500 | `sine` | glow | 0 |
| `radial-flash` | ラジアルフラッシュ | `light` | 560 | `expo` | glow | 0 |
| `lens-flare-sweep` | レンズフレアスイープ | `light` | 1050 | `power` | sweep, glow | 0 |
| `holographic-sheen` | ホログラムシーン | `light` | 1300 | `sine` | sweep | 0 |
| `liquid-morph` | リキッドモーフ | `effect` | 1100 | `power` | morph | 0 |
| `ink-reveal` | インクリビール | `effect` | 980 | `power` | dissolve | 0 |
| `mosaic-flip` | モザイクフリップ | `effect` | 1150 | `power` | blocks, flipY | 0 |
| `displacement-wave` | ディスプレイス波 | `effect` | 1000 | `sine` | wave, displace | 0 |
| `pinned-story-steps` | ピン留めストーリー | `web-scroll` | 1400 | `power` | slideY | 0 |
| `scroll-marquee` | スクロールマルキー | `web-scroll` | 1250 | `linear` | spin, marquee | 0 |
| `image-clip-scrub` | 画像クリップスクラブ | `web-scroll` | 1200 | `power` | slideY | 0 |
| `velocity-skew` | スクロール速度スキュー | `web-scroll` | 900 | `power` | skew | 0 |
| `button-fill-hover` | ボタン塗りホバー | `web-interaction` | 1400 | `power` | tilt | 0 |
| `icon-morph` | アイコンモーフ | `web-interaction` | 560 | `power` | morph | 0 |
| `card-spotlight` | カードスポットライト | `web-interaction` | 1800 | `sine` | glow, iris | 0 |
| `drag-snap` | ドラッグ吸着 | `web-interaction` | 820 | `back` | tilt | 0 |
| `sidebar-reveal` | サイドバー展開 | `web-layout` | 650 | `power` | scale, fade | 0 |
| `command-palette-open` | コマンドパレット展開 | `web-layout` | 520 | `power` | scale, fade | 0 |
| `dropdown-origin` | ドロップダウン起点展開 | `web-layout` | 480 | `power` | slideY | 0 |
| `detail-expand` | 一覧から詳細展開 | `web-layout` | 820 | `power` | tail | 0 |
| `success-check` | 成功チェック描画 | `web-feedback` | 760 | `power` | strokeDraw | 0 |
| `bar-chart-stagger` | 棒グラフ順次成長 | `web-feedback` | 1100 | `power` | scaleY | 0 |
| `donut-segments` | ドーナツ分割描画 | `web-feedback` | 1200 | `power` | strokeDraw | 0 |
| `optimistic-update` | 楽観的更新 | `web-feedback` | 900 | `power` | slideY | 0 |
| `floating-label` | フローティングラベル | `form-state` | 520 | `power` | scale | 0 |
| `checkbox-stroke` | チェック線描画 | `form-state` | 480 | `power` | strokeDraw | 0 |
| `switch-state-spring` | スイッチ状態スプリング | `form-state` | 560 | `back` | scale | 0 |
| `inline-validation` | インライン検証 | `form-state` | 620 | `power` | scale | 0 |
| `submit-success` | 送信成功モーフ | `form-state` | 820 | `power` | scale | 0 |
| `upload-progress-state` | アップロード状態遷移 | `form-state` | 1400 | `power` | slideY | 0 |
| `password-strength` | 強度メーター | `form-state` | 720 | `power` | scale | 0 |
| `skeleton-resolve` | スケルトン解決 | `form-state` | 860 | `power` | scale | 0 |
| `swipe-dismiss` | スワイプ退場 | `gesture` | 700 | `power` | wipe | 0 |
| `swipe-actions` | スワイプ操作露出 | `gesture` | 640 | `power` | wipe | 0 |
| `long-press-progress` | 長押し進行 | `gesture` | 1000 | `linear` | squash | 0 |
| `pull-to-refresh` | 引いて更新 | `gesture` | 920 | `back` | travel | 0 |
| `drag-pickup` | ドラッグ持ち上げ | `gesture` | 540 | `power` | slideY | 0 |
| `reorder-drag` | 並べ替えドラッグ | `gesture` | 900 | `power` | reorder | 0 |
| `bottom-sheet-drag` | ボトムシート追従 | `gesture` | 760 | `power` | travel | 0 |
| `pinch-zoom` | ピンチズーム | `gesture` | 820 | `power` | scale | 0 |
| `svg-stroke-draw` | SVG線描画 | `svg-path` | 1100 | `power` | strokeDraw | 0 |
| `svg-path-morph` | SVGパスモーフ | `svg-path` | 1000 | `power` | morph | 0 |
| `svg-trim-path` | 区間トリム | `svg-path` | 900 | `power` | strokeTrim | 0 |
| `svg-motion-path` | モーションパス追従 | `svg-path` | 1200 | `power` | arc | 0 |
| `svg-mask-reveal` | SVGマスクリビール | `svg-path` | 980 | `power` | clipWindow | 0 |
| `svg-gradient-flow` | グラデーション流動 | `svg-path` | 1300 | `linear` | hue, sweep | 0 |
| `svg-gooey-merge` | グーイーマージ | `svg-path` | 1050 | `power` | morph | 0 |
| `svg-signature` | 署名ストローク | `svg-path` | 1500 | `power` | strokeDraw | 0 |
| `popover-transition` | ポップオーバー出入り | `web-layout` | 520 | `power` | scale | 0 |
| `dialog-top-layer` | トップレイヤーダイアログ | `web-layout` | 620 | `power` | scale, fade | 0 |
| `view-transition-circle` | 円形ビュー遷移 | `web-layout` | 760 | `power` | scale, fade | 0 |
| `discrete-exit` | 離散プロパティ退場 | `web-layout` | 480 | `power` | scale, fade | 0 |
| `auto-height-expand` | 自動高さ展開 | `web-layout` | 650 | `power` | scaleY | 0 |
| `page-transition-stack` | ページスタック遷移 | `web-layout` | 720 | `power` | scale, fade | 0 |
| `catalog-ui-button-bounce-press` | バウンスプレス | `ui-button` | 740 | `back` | hop, squash | 1 |
| `catalog-ui-button-jelly-squash` | ジェリースカッシュ | `ui-button` | 787 | `spring` | squash, jelly | 1 |
| `catalog-ui-button-tap-ripple` | タップリップル | `ui-button` | 834 | `power` | displace, repeat | 1 |
| `catalog-ui-button-pulse-glow` | パルスグロー | `ui-button` | 1341 | `sine` | pulse, glow | 1 |
| `catalog-ui-button-error-shake` | エラーシェイク | `ui-button` | 668 | `linear` | shake | 1 |
| `catalog-ui-button-fill-sweep` | フィルスイープ | `ui-button` | 1315 | `power` | sweep | 1 |
| `catalog-ui-button-send-morph` | 送信モーフ | `ui-button` | 922 | `power` | morph | 4 |
| `catalog-ui-button-push-3d` | 3Dプッシュ | `ui-button` | 1269 | `spring` | squash | 1 |
| `catalog-ui-button-confetti-burst` | 紙吹雪バースト | `ui-button` | 1196 | `power` | confetti, particles | 11 |
| `catalog-ui-button-hold-confirm` | 長押し確定 | `ui-button` | 643 | `linear` | squash | 2 |
| `catalog-ui-button-border-draw` | ボーダードロー | `ui-button` | 950 | `power` | strokeDraw | 1 |
| `catalog-ui-button-shine-sweep` | シャインスイープ | `ui-button` | 1457 | `power` | sweep | 1 |
| `catalog-ui-button-loading-success` | 読込から成功 | `ui-button` | 1244 | `back` | squash | 2 |
| `catalog-ui-button-icon-swap` | アイコンスワップ | `ui-button` | 1171 | `back` | textSwap | 2 |
| `catalog-ui-button-heartbeat` | ハートビート | `ui-button` | 878 | `power` | pulse, beat | 1 |
| `catalog-ui-button-wobble-tilt` | ウォブルチルト | `ui-button` | 925 | `spring` | swing, tilt | 1 |
| `catalog-ui-button-elastic-drag` | エラスティックドラッグ | `ui-button` | 972 | `spring` | squash | 2 |
| `catalog-ui-button-inner-press` | インナープレス | `ui-button` | 1219 | `power` | squash | 1 |
| `catalog-ui-button-color-splash` | カラースプラッシュ | `ui-button` | 1146 | `power` | tint | 1 |
| `catalog-ui-button-breathing-gradient` | 呼吸グラデーション | `ui-button` | 1313 | `sine` | pulse, hue, sweep | 0 |
| `catalog-ui-control-spring-switch` | スプリングスイッチ | `ui-control` | 900 | `spring` | travel | 2 |
| `catalog-ui-control-day-night` | 昼夜トグル | `ui-control` | 1407 | `power` | travel | 4 |
| `catalog-ui-control-liquid-stretch` | リキッドストレッチ | `ui-control` | 994 | `power` | jelly, morph | 2 |
| `catalog-ui-control-check-draw` | チェック描画 | `ui-control` | 521 | `power` | strokeDraw | 2 |
| `catalog-ui-control-heart-like` | ハートいいね | `ui-control` | 828 | `back` | travel | 10 |
| `catalog-ui-control-lock-unlock` | ロック解除 | `ui-control` | 1035 | `spring` | travel | 1 |
| `catalog-ui-control-mute-pop` | ミュートポップ | `ui-control` | 1382 | `back` | scale | 3 |
| `catalog-ui-control-play-pause` | 再生停止モーフ | `ui-control` | 1309 | `power` | travel | 0 |
| `catalog-ui-control-sliding-underline` | スライド下線 | `ui-control` | 1096 | `power` | underline | 1 |
| `catalog-ui-control-pill-indicator` | ピルインジケーター | `ui-control` | 803 | `spring` | travel | 0 |
| `catalog-ui-control-bubble-expand` | バブル展開 | `ui-control` | 850 | `spring` | travel | 3 |
| `catalog-ui-control-jumping-dot` | ジャンプドット | `ui-control` | 1357 | `linear` | hop | 0 |
| `catalog-ui-control-liquid-blob` | リキッドブロブ | `ui-control` | 944 | `power` | morph | 1 |
| `catalog-ui-control-tint-shift` | ティントシフト | `ui-control` | 731 | `power` | travel | 2 |
| `catalog-ui-navigation-hero-expand` | ヒーロー展開 | `ui-navigation` | 518 | `power` | slideY, fade | 2 |
| `catalog-ui-navigation-spring-sheet` | スプリングシート | `ui-navigation` | 825 | `spring` | slideY, fade | 2 |
| `catalog-ui-navigation-card-flip` | カードフリップ | `ui-navigation` | 1032 | `power` | flipY | 1 |
| `catalog-ui-navigation-circular-reveal` | 円形リビール | `ui-navigation` | 1379 | `power` | iris | 1 |
| `catalog-ui-navigation-parallax-push` | パララックスプッシュ | `ui-navigation` | 1306 | `power` | parallax | 2 |
| `catalog-ui-navigation-staggered-reveal` | 段階リビール | `ui-navigation` | 493 | `power` | slideY, fade | 4 |
| `catalog-ui-loading-bouncing-dots` | バウンドドット | `ui-loading` | 960 | `power` | pulse | 3 |
| `catalog-ui-loading-gradient-arc` | グラデーションアーク | `ui-loading` | 1307 | `linear` | arc, hue, sweep | 1 |
| `catalog-ui-loading-progress-success` | 進捗から成功 | `ui-loading` | 1234 | `power` | pulse | 3 |
| `catalog-ui-loading-skeleton-shimmer` | スケルトンシマー | `ui-loading` | 681 | `linear` | pulse | 1 |
| `catalog-ui-loading-typing-indicator` | 入力インジケーター | `ui-loading` | 988 | `sine` | typewriter | 3 |
| `catalog-ui-loading-equalizer-bars` | イコライザーバー | `ui-loading` | 935 | `sine` | audioPulse | 5 |
| `catalog-ui-loading-orbit-trail` | オービットトレイル | `ui-loading` | 1162 | `linear` | orbit | 4 |
| `catalog-ui-feedback-heart-burst` | ハートバースト | `ui-feedback` | 1329 | `spring` | particles | 7 |
| `catalog-ui-feedback-star-rating` | 星評価 | `ui-feedback` | 1256 | `back` | pulse | 4 |
| `catalog-ui-feedback-clap-counter` | 拍手カウンター | `ui-feedback` | 703 | `power` | counter | 3 |
| `catalog-ui-feedback-emoji-bar` | 絵文字バー | `ui-feedback` | 750 | `spring` | pulse | 6 |
| `catalog-ui-feedback-number-ticker` | 数値ティッカー | `ui-feedback` | 957 | `power` | counter, marquee | 1 |
| `catalog-ui-feedback-confetti-rain` | 紙吹雪レイン | `ui-feedback` | 1304 | `linear` | confetti, snow | 16 |
| `catalog-game-hud-coin-collect` | コイン回収 | `game-hud` | 1351 | `power` | scale | 7 |
| `catalog-game-hud-hp-ghost-bar` | HPゴーストバー | `game-hud` | 1278 | `power` | scale | 4 |
| `catalog-game-hud-cooldown-ability` | クールダウン | `game-hud` | 725 | `linear` | slideY | 3 |
| `catalog-game-hud-score-rollup` | スコアロールアップ | `game-hud` | 1112 | `power` | slideY, spin, counter | 2 |
| `catalog-game-hud-stamina-pulse` | スタミナ脈動 | `game-hud` | 979 | `sine` | pulse | 3 |
| `catalog-game-hud-ammo-reload` | 弾薬リロード | `game-hud` | 1326 | `back` | scale | 10 |
| `catalog-game-hud-compass-lock` | コンパスロック | `game-hud` | 1253 | `power` | scale | 3 |
| `catalog-game-hud-minimap-ping` | ミニマップピン | `game-hud` | 700 | `power` | scale | 2 |
| `catalog-game-hud-objective-marker` | 目標マーカー | `game-hud` | 747 | `spring` | highlight | 2 |
| `catalog-game-hud-resource-cap` | 資源上限警告 | `game-hud` | 954 | `linear` | scale | 4 |
| `catalog-game-hud-party-status` | パーティ状態 | `game-hud` | 1301 | `back` | scale | 3 |
| `catalog-game-hud-buff-timer` | バフタイマー | `game-hud` | 1228 | `linear` | scale | 4 |
| `catalog-game-combat-damage-numbers` | ダメージ数値 | `game-combat` | 1095 | `power` | counter | 3 |
| `catalog-game-combat-combo-counter` | コンボカウンター | `game-combat` | 1142 | `power` | counter | 8 |
| `catalog-game-combat-critical-flash` | クリティカルフラッシュ | `game-combat` | 929 | `spring` | shake | 4 |
| `catalog-game-combat-hit-confirm` | ヒット確認 | `game-combat` | 976 | `spring` | shake | 2 |
| `catalog-game-combat-guard-break` | ガードブレイク | `game-combat` | 1023 | `power` | shake | 3 |
| `catalog-game-combat-perfect-dodge` | ジャスト回避 | `game-combat` | 910 | `expo` | shake | 4 |
| `catalog-game-combat-parry-window` | パリィ成功 | `game-combat` | 1117 | `linear` | shake | 4 |
| `catalog-game-combat-status-ailment` | 状態異常付与 | `game-combat` | 1204 | `back` | shake | 8 |
| `catalog-game-reward-level-up` | レベルアップ | `game-reward` | 791 | `back` | slideY | 15 |
| `catalog-game-reward-chest-open` | 宝箱オープン | `game-reward` | 998 | `back` | scale, glow | 3 |
| `catalog-game-reward-three-star` | 三つ星クリア | `game-reward` | 1345 | `spring` | scale, glow | 4 |
| `catalog-game-reward-gacha-pull` | ガチャ排出 | `game-reward` | 1392 | `back` | scale, glow | 4 |
| `catalog-game-reward-new-record` | 新記録スタンプ | `game-reward` | 719 | `power` | scale, glow | 2 |
| `catalog-game-reward-loot-rarity` | レア度確定 | `game-reward` | 1226 | `power` | scale, glow | 2 |
| `catalog-game-reward-achievement` | 実績解除 | `game-reward` | 973 | `spring` | scale, glow | 1 |
| `catalog-game-reward-daily-bonus` | デイリーボーナス | `game-reward` | 1320 | `spring` | scale, glow | 3 |
| `catalog-game-reward-currency-rain` | 通貨レイン | `game-reward` | 1247 | `power` | snow | 11 |
| `catalog-game-reward-reward-card` | 報酬カード公開 | `game-reward` | 694 | `power` | scale, glow | 2 |
| `catalog-game-progress-quest-banner` | クエストバナー | `game-progress` | 1201 | `expo` | slideX, fade | 2 |
| `catalog-game-progress-skill-unlock` | スキル解放 | `game-progress` | 1128 | `power` | slideX, fade | 4 |
| `catalog-game-progress-login-streak` | 連続ログイン | `game-progress` | 575 | `expo` | slideX, fade | 3 |
| `catalog-game-progress-energy-refill` | エネルギー回復 | `game-progress` | 882 | `power` | slideX, fade | 3 |
| `catalog-game-progress-xp-overflow` | XP繰り上がり | `game-progress` | 1269 | `power` | slideX, fade | 3 |
| `catalog-game-progress-battle-pass` | バトルパス進行 | `game-progress` | 1436 | `power` | slideX, fade | 4 |
| `catalog-game-progress-mission-step` | ミッション段階 | `game-progress` | 1103 | `back` | slideX, fade | 3 |
| `catalog-game-progress-rank-up` | ランク昇格 | `game-progress` | 550 | `back` | slideY | 11 |
| `catalog-game-progress-collection-complete` | 収集コンプリート | `game-progress` | 857 | `power` | slideX, fade | 2 |
| `catalog-game-cinematic-lucky-roulette` | ラッキールーレット | `game-cinematic` | 1244 | `power` | scale, fade | 2 |
| `catalog-game-cinematic-countdown-go` | カウントダウンGO | `game-cinematic` | 691 | `expo` | slideY, counter, scale | 2 |
| `catalog-game-cinematic-boss-warning` | ボス警告 | `game-cinematic` | 1158 | `expo` | scale, fade | 4 |
| `catalog-game-cinematic-victory-title` | 勝利タイトル | `game-cinematic` | 945 | `expo` | scale, fade | 13 |
| `catalog-game-cinematic-defeat-dissolve` | 敗北ディゾルブ | `game-cinematic` | 1292 | `power` | dissolve | 7 |
| `catalog-game-cinematic-chapter-intro` | チャプター導入 | `game-cinematic` | 1219 | `expo` | scale, fade | 3 |
| `catalog-game-cinematic-raid-start` | レイド開始 | `game-cinematic` | 666 | `spring` | scale, fade | 5 |
| `catalog-game-cinematic-letterbox-focus` | シネマ帯フォーカス | `game-cinematic` | 973 | `power` | blur | 4 |
| `catalog-ui-spatial-cursor-depth-pull` | カーソル奥行きプル | `ui-spatial` | 920 | `spring` | parallax | 1 |
| `catalog-ui-spatial-focus-portal` | フォーカスポータル | `ui-spatial` | 1267 | `power` | iris, scale, blur | 2 |
| `catalog-ui-spatial-radial-command-bloom` | 放射コマンドブルーム | `ui-spatial` | 1194 | `spring` | glow, scale | 6 |
| `catalog-ui-spatial-spatial-breadcrumb` | 空間パンくず | `ui-spatial` | 641 | `spring` | parallax | 3 |
| `catalog-ui-spatial-perspective-dock` | 遠近ドック | `ui-spatial` | 948 | `power` | parallax | 5 |
| `catalog-ui-spatial-layer-peel` | レイヤーピール | `ui-spatial` | 1155 | `power` | flipY | 1 |
| `catalog-ui-spatial-tooltip-orbit` | ツールチップ軌道 | `ui-spatial` | 1122 | `spring` | orbit | 1 |
| `catalog-ui-spatial-depth-lens` | 奥行きレンズ | `ui-spatial` | 1169 | `power` | parallax | 1 |
| `catalog-ui-spatial-magnetic-constellation` | 磁気コンステレーション | `ui-spatial` | 616 | `spring` | gather | 6 |
| `catalog-ui-spatial-infinite-canvas-jump` | 無限キャンバスジャンプ | `ui-spatial` | 923 | `power` | hop | 1 |
| `catalog-ui-spatial-z-notification` | Z軸通知 | `ui-spatial` | 1130 | `spring` | parallax | 1 |
| `catalog-ui-spatial-camera-dolly-ui` | カメラドリーUI | `ui-spatial` | 1097 | `power` | camera | 2 |
| `catalog-ui-spatial-volumetric-menu` | ボリュームメニュー | `ui-spatial` | 1144 | `spring` | parallax | 3 |
| `catalog-ui-spatial-parallax-inspector` | パララックス検査面 | `ui-spatial` | 1191 | `power` | parallax | 3 |
| `catalog-ui-spatial-spatial-search` | 空間検索リビール | `ui-spatial` | 898 | `spring` | arc | 3 |
| `catalog-ui-spatial-orbit-context` | 軌道コンテキストメニュー | `ui-spatial` | 1285 | `spring` | orbit | 3 |
| `catalog-ui-spatial-depth-stack-reorder` | 奥行きスタック並べ替え | `ui-spatial` | 1332 | `power` | reorder | 3 |
| `catalog-ui-spatial-calendar-fold` | 遠近カレンダーフォールド | `ui-spatial` | 1119 | `power` | flipX | 1 |
| `catalog-ui-spatial-cursor-raycast` | カーソルレイキャスト選択 | `ui-spatial` | 566 | `power` | parallax | 2 |
| `catalog-ui-spatial-spatial-audio-locator` | 空間オーディオロケーター | `ui-spatial` | 873 | `linear` | audioPulse | 3 |
| `catalog-ui-material-liquid-tab-bridge` | 液体タブブリッジ | `ui-material` | 920 | `power` | morph | 2 |
| `catalog-ui-material-gel-button-split` | ジェルボタン分裂 | `ui-material` | 967 | `power` | split | 3 |
| `catalog-ui-material-glass-refraction` | ガラス屈折ホバー | `ui-material` | 1214 | `power` | displace | 2 |
| `catalog-ui-material-ink-adhesion` | インク吸着 | `ui-material` | 801 | `power` | morph | 4 |
| `catalog-ui-material-metallic-foil` | メタリック箔チルト | `ui-material` | 1308 | `power` | sweep | 1 |
| `catalog-ui-material-soft-body-toggle` | ソフトボディトグル | `ui-material` | 895 | `power` | morph | 1 |
| `catalog-ui-material-viscous-progress` | 粘性プログレス | `ui-material` | 1282 | `spring` | morph | 2 |
| `catalog-ui-material-chromatic-glass-edge` | 色分散ガラスエッジ | `ui-material` | 1449 | `power` | chromatic, displace | 2 |
| `catalog-ui-material-paper-fold` | ペーパーフォールド | `ui-material` | 776 | `power` | flipX | 2 |
| `catalog-ui-material-cloth-wave` | クロスウェーブパネル | `ui-material` | 823 | `sine` | wave | 12 |
| `catalog-ui-material-bubble-merge` | バブル結合メニュー | `ui-material` | 870 | `spring` | morph | 2 |
| `catalog-ui-material-frost-melt` | フロスト融解アンロック | `ui-material` | 1377 | `power` | blur, dissolve, displace | 1 |
| `catalog-ui-material-membrane-press` | メンブレンプレス | `ui-material` | 964 | `spring` | squash | 1 |
| `catalog-ui-material-sand-dissolve` | 砂状ステート切替 | `ui-material` | 751 | `power` | dissolve | 31 |
| `catalog-ui-material-oil-slick` | オイルスリック選択 | `ui-material` | 958 | `spring` | morph | 0 |
| `catalog-ui-material-magnetic-fluid` | 磁性流体バッジ | `ui-material` | 1305 | `spring` | gather | 12 |
| `catalog-ui-material-glass-crack-recover` | ガラス亀裂リカバー | `ui-material` | 1352 | `power` | displace, blur | 1 |
| `catalog-ui-material-neon-tube-draw` | ネオン管ドロー | `ui-material` | 1399 | `linear` | glow, strokeDraw | 1 |
| `catalog-ui-material-chrome-ripple` | クローム面リップル | `ui-material` | 986 | `power` | displace, repeat | 2 |
| `catalog-ui-material-clay-icon-morph` | クレイアイコンモーフ | `ui-material` | 933 | `power` | morph | 1 |
| `catalog-kinetic-type-glyph-slot-roll` | グリフスロットロール | `tx-swap` | 1160 | `back` | spin | 4 |
| `catalog-kinetic-type-variable-font-pulse` | 可変フォントパルス | `tx-variable` | 867 | `power` | pulse | 0 |
| `catalog-kinetic-type-character-magnetize` | 文字マグネット整列 | `tx-physics` | 1088 | `spring` | gather | 5 |
| `catalog-kinetic-type-line-break-reflow` | 改行リフロー | `tx-layout` | 955 | `power` | reorder | 2 |
| `catalog-kinetic-type-terminal-decode` | ターミナルデコード | `tx-typing` | 842 | `linear` | scramble | 7 |
| `catalog-kinetic-type-counterweight-headline` | カウンターウェイト見出し | `tx-layout` | 1229 | `spring` | scale, reorder | 1 |
| `catalog-kinetic-type-echo-caption` | エコートレイル字幕 | `tx-caption` | 1396 | `expo` | caption | 3 |
| `catalog-kinetic-type-letter-collision` | 文字衝突バウンス | `tx-physics` | 1024 | `power` | hop | 2 |
| `catalog-kinetic-type-type-tunnel` | 文字遠近トンネル | `tx-depth` | 1251 | `linear` | flipX | 4 |
| `catalog-kinetic-type-chapter-index-scrub` | 章番号スクラブ | `tx-scroll` | 1298 | `linear` | highlight | 2 |
| `catalog-kinetic-type-focus-word-explosion` | 注目語エクスプロージョン | `tx-emphasis` | 792 | `expo` | blur | 5 |
| `catalog-kinetic-type-mono-data-shuffle` | 等幅データシャッフル | `tx-counter` | 999 | `power` | reorder | 6 |
| `catalog-kinetic-type-headline-ribbon` | 見出しリボンラップ | `tx-title` | 1273 | `expo` | fade, tracking | 3 |
| `catalog-game-impact-fx-hit-stop` | ヒットストップフレーム | `game-impact-fx` | 1140 | `expo` | hold, shake | 2 |
| `catalog-game-impact-fx-shockwave-type` | 衝撃波タイポ | `game-impact-fx` | 927 | `power` | wave | 3 |
| `catalog-game-impact-fx-slash-reveal` | 方向スラッシュリビール | `game-impact-fx` | 554 | `power` | shake, glow | 1 |
| `catalog-game-impact-fx-freeze-color-drain` | 静止色抜き | `game-impact-fx` | 1021 | `power` | tint, hold, desaturate, snow | 1 |
| `catalog-game-impact-fx-combo-heat` | コンボ熱量上昇 | `game-impact-fx` | 1068 | `back` | shake, glow | 4 |
| `catalog-game-impact-fx-edge-damage` | 画面端ダメージ脈動 | `game-impact-fx` | 1115 | `expo` | shake, glow | 1 |
| `catalog-game-impact-fx-armor-crack` | アーマー亀裂網 | `game-impact-fx` | 902 | `expo` | blur | 3 |
| `catalog-game-impact-fx-critical-compress` | クリティカルレンズ圧縮 | `game-impact-fx` | 949 | `power` | squash | 2 |
| `catalog-game-impact-fx-phase-shatter` | ボスフェーズ破砕 | `game-impact-fx` | 836 | `power` | shake, glow | 7 |
| `catalog-game-impact-fx-speedline-tunnel` | スピードライントンネル | `game-impact-fx` | 1223 | `power` | shake, glow | 13 |
| `catalog-game-impact-fx-parry-time-slice` | パリィ時間スライス | `game-impact-fx` | 1390 | `expo` | blinds | 3 |
| `catalog-game-impact-fx-element-burst` | 属性アフィニティバースト | `game-impact-fx` | 1317 | `power` | particles | 9 |
| `catalog-game-impact-fx-camera-kick` | カメラキックHUD | `game-impact-fx` | 924 | `spring` | camera | 1 |
| `catalog-game-impact-fx-afterimage-dash` | 残像ダッシュ | `game-impact-fx` | 1151 | `expo` | shake, glow | 4 |
| `catalog-game-impact-fx-victory-hold` | 勝利インパクトホールド | `game-impact-fx` | 1018 | `power` | shake, glow | 2 |
| `catalog-game-impact-fx-loot-beam` | ルートビーム柱 | `game-impact-fx` | 1365 | `expo` | shake, glow | 2 |
| `catalog-game-impact-fx-warning-stripe` | 警告ストライプスラム | `game-impact-fx` | 1112 | `spring` | shake, glow | 3 |
| `catalog-game-impact-fx-shield-recharge` | シールド再充填スイープ | `game-impact-fx` | 739 | `linear` | shake, glow | 2 |
| `catalog-game-impact-fx-mission-stamp` | ミッションスタンプパンチ | `game-impact-fx` | 946 | `power` | shake, glow | 1 |
| `catalog-game-impact-fx-finisher-cut` | フィニッシャータイトルカット | `game-impact-fx` | 993 | `expo` | cut | 3 |
| `catalog-data-story-sankey-flow` | サンキー流量リビール | `data-story` | 1220 | `linear` | strokeDraw | 2 |
| `catalog-data-story-odometer-cascade` | 指標オドメーター | `data-story` | 1267 | `power` | counter, slideY | 3 |
| `catalog-data-story-threshold-wave` | 閾値アラーム波 | `data-story` | 1314 | `linear` | wave | 2 |
| `catalog-data-story-confidence-fog` | 予測信頼度フォグ | `data-story` | 921 | `power` | blur, fade | 2 |
| `catalog-data-story-signal-pulse-map` | ライブ信号パルスマップ | `data-story` | 1268 | `power` | pulse | 3 |
| `catalog-data-story-delta-split` | 比較デルタ分割 | `data-story` | 1195 | `expo` | split | 2 |
| `catalog-data-story-timeline-trail` | タイムライン軌跡 | `data-story` | 1242 | `linear` | travel, repeat | 2 |
| `catalog-data-story-topology-bloom` | ネットワークトポロジー展開 | `data-story` | 949 | `power` | glow, scale | 5 |
| `catalog-data-story-milestone-firefly` | 進捗マイルストーン蛍 | `data-story` | 1456 | `power` | glow, orbit | 3 |
| `catalog-data-story-heatmap-ripple` | ヒートマップ注目波 | `data-story` | 783 | `power` | displace, repeat | 35 |
| `catalog-data-story-stream-breathe` | ストリームグラフ呼吸 | `data-story` | 1170 | `power` | pulse | 0 |
| `catalog-data-story-rank-ladder` | 順位ラダー上昇 | `data-story` | 617 | `spring` | slideY, reorder | 3 |
| `catalog-data-story-cluster-merge` | データクラスタ結合 | `data-story` | 924 | `power` | gather | 12 |
| `catalog-data-story-event-domino` | イベント列ドミノ | `data-story` | 1131 | `back` | flipX, wave | 4 |
| `catalog-data-story-kpi-relay` | KPIスポットライトリレー | `data-story` | 1218 | `spring` | highlight, pulse | 3 |
| `catalog-data-story-budget-bloom` | 放射予算ブルーム | `data-story` | 1145 | `power` | glow, scale | 0 |
| `catalog-data-story-calendar-density` | カレンダー密度スイープ | `data-story` | 592 | `power` | sweep, tint | 28 |
| `catalog-data-story-queue-compress` | ライブキュー圧縮 | `data-story` | 899 | `spring` | squash | 4 |
| `catalog-data-story-sentiment-spectrum` | 感情スペクトラム遷移 | `data-story` | 1406 | `spring` | hue | 1 |
| `catalog-data-story-goal-projection` | 目標予測アーク | `data-story` | 1453 | `power` | arc, glow | 1 |
| `squash` | スクワッシュ＆ストレッチ | `principles` | 550 | `power` | squash | 0 |
| `anticipation` | 予備動作（アンティシペーション） | `principles` | 400 | `back` | anticipate, travel | 0 |
| `staging` | ステージング | `principles` | 200 | `power` | travel | 0 |
| `pose` | ポーズ・トゥ・ポーズ / ストレートアヘッド | `principles` | 1000 | `power` | travel | 0 |
| `follow` | フォロースルー／オーバーラップ | `principles` | 600 | `power` | follow | 0 |
| `slowinout` | スローイン／スローアウト | `principles` | 1200 | `power` | travel | 0 |
| `arcs` | アーク（弧の軌道） | `principles` | 900 | `sine` | arc | 0 |
| `secondary` | セカンダリーアクション | `principles` | 500 | `power` | travel | 0 |
| `timing-weight` | タイミング（重さ） | `principles` | 500 | `power` | weight | 0 |
| `exaggeration` | 誇張 | `principles` | 450 | `back` | travel | 0 |
| `solid` | ソリッドドローイング（立体感） | `principles` | 800 | `power` | travel | 0 |
| `appeal` | アピール（気持ちよさ） | `principles` | 180 | `power` | travel | 0 |
| `cause` | 原因と結果の連鎖 | `principles` | 400 | `power` | travel | 0 |
| `jitter` | アンチ・ロボット（ゆらぎ） | `principles` | 400 | `power` | shake | 0 |
| `layers3` | 主役・脇役・背景の 3 層 | `principles` | 600 | `power` | travel | 0 |
| `ambient-hold` | 止まって見えるコマを作らない | `principles` | 4000 | `sine` | travel | 0 |
| `counter` | カウンターモーション | `principles` | 800 | `power` | counter | 0 |
| `e-linear` | linear（等速） | `easing` | 1200 | `linear` | travel | 0 |
| `e-outexpo` | ease-out expo（入り） | `easing` | 400 | `expo` | travel | 0 |
| `e-inexpo` | ease-in expo（退場） | `easing` | 250 | `expo` | travel | 0 |
| `e-inout` | ease-in-out cubic（移動） | `easing` | 350 | `power` | travel | 0 |
| `e-house` | シンピーク（ハウスカーブ） | `easing` | 600 | `power` | travel | 0 |
| `e-whip` | WHIP（ムチ） | `easing` | 500 | `bezier` | travel | 0 |
| `e-glide` | GLIDE（グライド） | `easing` | 300 | `bezier` | travel | 0 |
| `e-quint` | ease-out quint | `easing` | 700 | `power` | travel | 0 |
| `e-md3` | Material 3 標準 | `easing` | 300 | `bezier` | travel | 0 |
| `e-md3e` | Material 3 強調（減速） | `easing` | 500 | `bezier` | travel | 0 |
| `e-md3a` | Material 3 加速（退場） | `easing` | 200 | `bezier` | travel | 0 |
| `e-apple` | Apple 標準 | `easing` | 350 | `bezier` | travel | 0 |
| `e-outback` | back out（オーバーシュート） | `easing` | 500 | `back` | travel | 0 |
| `e-inback` | back in（引いてから去る） | `easing` | 400 | `back` | travel | 0 |
| `e-inoutback` | back in-out | `easing` | 600 | `back` | travel | 0 |
| `e-sine` | sine in-out（呼吸） | `easing` | 2000 | `sine` | travel | 0 |
| `e-circ` | circ out | `easing` | 400 | `circular` | travel | 0 |
| `e-elastic` | elastic out（ゴム） | `easing` | 1000 | `elastic` | travel | 0 |
| `e-bounce` | bounce out（弾む） | `easing` | 800 | `bounce` | travel | 0 |
| `e-steps` | steps（コマ打ち） | `easing` | 500 | `steps` | travel | 0 |
| `e-slowmo` | SlowMo（速い→遅い→速い） | `easing` | 1200 | `slowMo` | travel | 0 |
| `e-rough` | RoughEase（ガタつき） | `easing` | 1200 | `rough` | travel | 0 |
| `e-expo-scale` | ExpoScale（等速に見えるズーム） | `easing` | 1800 | `expoScale` | travel | 0 |
| `e-wiggle` | CustomWiggle（揺れ） | `easing` | 600 | `wiggle` | travel | 0 |
| `e-custombounce` | CustomBounce＋スクワッシュ | `easing` | 1200 | `bounce` | travel | 0 |
| `s-snap` | snap（指ではじく） | `spring` | 370 | `spring` | travel | 0 |
| `s-bounce` | bounce（ボール） | `spring` | 820 | `spring` | travel | 0 |
| `s-smooth` | smooth（ダンパー付きの扉） | `spring` | 830 | `spring` | travel | 0 |
| `s-heavy` | heavy（重い物） | `spring` | 930 | `spring` | travel | 0 |
| `s-rsdefault` | react-spring default | `spring` | 510 | `spring` | travel | 0 |
| `s-wobbly` | react-spring wobbly | `spring` | 650 | `spring` | travel | 0 |
| `s-molasses` | react-spring molasses | `spring` | 1940 | `spring` | travel | 0 |
| `s-framerbouncy` | Framer bouncy | `spring` | 550 | `spring` | travel | 0 |
| `s-modal` | Apple モーダル | `spring` | 260 | `spring` | travel | 0 |
| `s-critical` | 臨界減衰（揺れない最速） | `spring` | 380 | `spring` | travel | 0 |
| `s-track` | 目標が変わるバネ（足し合わせ） | `spring` | 600 | `spring` | travel | 0 |
| `s-split` | 伸びるタブ（前後で別のバネ） | `spring` | 400 | `spring` | follow | 0 |
| `s-vs` | スプリング vs トゥイーン | `spring` | 500 | `spring` | travel | 0 |
| `s-budget` | オーバーシュートの予算 | `spring` | 500 | `back` | travel | 0 |
| `s-inertia` | 慣性（投げて止まる） | `spring` | 700 | `inertia` | travel | 0 |
| `s-projectile` | 放物運動（Physics2D） | `spring` | 900 | `gravity` | travel, hop | 0 |
| `t-size` | 大きさで尺を変える | `timing` | 350 | `power` | travel | 0 |
| `t-distance` | 距離で尺を変える（√ の法則） | `timing` | 600 | `power` | travel | 0 |
| `t-exit` | 退場は登場より短く | `timing` | 210 | `power` | travel | 0 |
| `t-stagger-seq` | ステッガー：順番に | `timing` | 700 | `power` | travel | 0 |
| `t-stagger-center` | ステッガー：中央から | `timing` | 700 | `power` | travel | 0 |
| `t-stagger-edges` | ステッガー：両端から | `timing` | 700 | `power` | travel | 0 |
| `t-stagger-random` | ステッガー：ランダム | `timing` | 600 | `power` | travel | 0 |
| `t-stagger-grid` | ステッガー：グリッド斜め | `timing` | 700 | `power` | travel | 0 |
| `t-twos` | 1 コマ打ち・2 コマ打ち・3 コマ打ち | `timing` | 1000 | `steps` | hold, travel | 0 |
| `t-snaphold` | パッと出て、止めて見せる | `timing` | 100 | `expo` | travel, hold | 0 |
| `t-beat` | 拍のグリッド（120BPM） | `timing` | 500 | `expo` | beat | 0 |
| `t-ramp` | スピードランプ | `timing` | 1200 | `power` | travel | 0 |
| `t-overlap` | 重なり（オーバーラップ）の量 | `timing` | 400 | `power` | travel | 0 |
| `t-read` | 読める時間を確保する | `timing` | 1000 | `steps` | travel | 0 |
| `t-energy` | エネルギーの弧 | `timing` | 4000 | `power` | travel | 0 |
| `tr-cut` | ハードカット（拍で切る） | `transitions` | 100 | `steps` | cut | 0 |
| `tr-dissolve` | クロスフェード（ディゾルブ） | `transitions` | 500 | `linear` | dissolve | 0 |
| `tr-blur` | ブラー・クロスフェード | `transitions` | 600 | `sine` | blur | 0 |
| `tr-push` | プッシュ（押し出し） | `transitions` | 400 | `power` | wipe | 0 |
| `tr-whip` | ウィップパン | `transitions` | 250 | `bezier` | wipe | 0 |
| `tr-wipe` | ワイプ | `transitions` | 500 | `power` | wipe | 0 |
| `tr-inset` | インセット・クリップ | `transitions` | 500 | `power` | wipe | 0 |
| `tr-clock` | クロックワイプ | `transitions` | 700 | `sine` | wipe | 0 |
| `tr-iris` | アイリス（丸く開く） | `transitions` | 600 | `power` | iris | 0 |
| `tr-shapeiris` | 形のアイリス（ハート・鍵穴） | `transitions` | 700 | `power` | iris | 0 |
| `tr-flood` | カラーフラッド | `transitions` | 800 | `power` | wipe | 0 |
| `tr-zoomthrough` | ズームスルー | `transitions` | 600 | `power` | scale | 0 |
| `tr-blinds` | スラット（ブラインド） | `transitions` | 700 | `power` | blinds | 0 |
| `tr-blocks` | ブロックで覆う | `transitions` | 800 | `power` | blocks | 0 |
| `tr-pixel` | ピクセルディゾルブ | `transitions` | 600 | `steps` | blocks | 0 |
| `tr-morph` | シェイプモーフで橋渡し | `transitions` | 700 | `power` | morph | 0 |
| `tr-match` | マッチカット | `transitions` | 100 | `steps` | wipe | 0 |
| `tr-glitch` | グリッチ（RGB ずれ） | `transitions` | 200 | `steps` | chromatic, shake | 0 |
| `tr-leak` | ライトリーク | `transitions` | 800 | `sine` | glow, sweep | 0 |
| `tr-flip` | 3D フリップ | `transitions` | 600 | `power` | flipY | 0 |
| `tr-brush` | ブラシワイプ | `transitions` | 600 | `power` | blinds | 0 |
| `tr-letter` | 文字の中を抜けて次へ | `transitions` | 900 | `expo` | wipe | 0 |
| `tr-drop` | グラビティドロップ | `transitions` | 700 | `power` | wipe | 0 |
| `tr-streak` | ライトストリーク・ウィップ | `transitions` | 350 | `power` | wipe | 0 |
| `tr-jcut` | J カット／L カット | `transitions` | 400 | `power` | fade | 0 |
| `cam-pushin` | プッシュイン（ドリーイン） | `camera` | 1600 | `power` | camera | 0 |
| `cam-pullout` | プルアウト | `camera` | 1600 | `power` | camera | 0 |
| `cam-truck` | トラック（横移動） | `camera` | 2000 | `sine` | slideX | 0 |
| `cam-pan` | パン（首振り） | `camera` | 2000 | `sine` | slideX | 0 |
| `cam-pedestal` | ペデスタル（上下移動） | `camera` | 2000 | `sine` | slideY | 0 |
| `cam-roll` | ロール（傾き） | `camera` | 300 | `power` | spin | 0 |
| `cam-zoomvsdolly` | ズーム（平板） | `camera` | 1600 | `power` | scale | 0 |
| `cam-vertigo` | ドリーズーム（めまい） | `camera` | 2000 | `power` | camera, scale | 0 |
| `cam-crane` | クレーン（ジブ） | `camera` | 2000 | `power` | crane | 0 |
| `cam-whip` | ウィップ（急な振り） | `camera` | 450 | `bezier` | camera | 0 |
| `cam-orbit` | オービット（周回） | `camera` | 4000 | `linear` | orbit, camera | 0 |
| `cam-rack` | ラックフォーカス | `camera` | 650 | `power` | blur | 0 |
| `cam-handheld` | 手持ち風のゆれ | `camera` | 3000 | `sine` | camera | 0 |
| `cam-shake` | 衝撃のシェイク | `camera` | 450 | `inertia` | shake | 0 |
| `cam-punch` | 拍でパンチイン | `camera` | 300 | `inertia` | camera | 0 |
| `cam-kenburns` | ケン・バーンズ | `camera` | 4000 | `linear` | camera, slideX | 0 |
| `cam-parallax` | 視差（パララックス）3 層 | `camera` | 4000 | `linear` | parallax | 0 |
| `cam-follow` | フォロー（先読みで追う） | `camera` | 600 | `spring` | follow | 0 |
| `ty-slam` | 強調のスラム | `tx-emphasis` | 700 | `spring` | pulse | 0 |
| `ty-flip` | 文字のフリップ | `tx-swap` | 400 | `power` | flipY | 0 |
| `ty-elastic` | 伸び縮みする文字 | `tx-emphasis` | 800 | `spring` | pulse | 0 |
| `ty-ghost` | 移動量が減っていく語の出現 | `tx-reveal` | 330 | `expo` | fade, slideY | 0 |
| `ty-kinetic` | 動詞で行が立ち上がる | `tx-reveal` | 300 | `power` | fade, slideY | 0 |
| `ty-inertia` | 飛び込んで揺れて止まる文字 | `tx-physics` | 900 | `sine` | travel | 0 |
| `ui-press` | ボタンを押す | `ui-button` | 100 | `spring` | squash | 0 |
| `ui-morph` | 1 つの形が変わり続ける（切らない） | `ui-button` | 500 | `spring` | morph | 0 |
| `ui-modal` | モーダルを開く・閉じる | `web-layout` | 400 | `spring` | scale, fade | 0 |
| `ui-toast` | トースト通知 | `ui-feedback` | 400 | `spring` | pulse | 0 |
| `ui-fan` | カードを広げる（スタック） | `web-interaction` | 600 | `spring` | tilt | 0 |
| `ui-island` | ダイナミックアイランド | `ui-feedback` | 500 | `spring` | pulse | 0 |
| `ui-tooltip` | ツールチップ | `web-interaction` | 200 | `power` | tilt | 0 |
| `ui-cursorarc` | カーソルは弧を描いて動く | `web-interaction` | 900 | `power` | arc | 0 |
| `ld-bar` | 不定のバー | `ui-loading` | 1800 | `power` | pulse | 0 |
| `ld-pulse` | 脈打つ輪 | `ui-loading` | 1600 | `power` | pulse | 0 |
| `ld-squares` | 四角が入れかわる | `ui-loading` | 600 | `power` | pulse | 0 |
| `sh-repeater` | リピーター（放射状の複製） | `svg-path` | 1500 | `sine` | repeat, spin | 0 |
| `sh-blob` | ゆらぐブロブ | `svg-path` | 4000 | `rough` | morph | 0 |
| `sh-burst` | 集中線・放射の線 | `svg-path` | 400 | `power` | particles | 0 |
| `sh-march` | 破線が流れる（マーチングアント） | `svg-path` | 1000 | `linear` | arc, strokeTrim | 0 |
| `pt-confetti` | 紙吹雪 | `particles` | 2000 | `gravity` | confetti | 0 |
| `pt-twinkle` | きらめき | `particles` | 600 | `sine` | particles | 0 |
| `pt-embers` | 立ち昇る火の粉 | `particles` | 3000 | `rough` | embers | 0 |
| `pt-snow` | 雪 | `particles` | 4000 | `sine` | snow | 0 |
| `pt-fireworks` | 花火（尾を引く） | `particles` | 1500 | `gravity` | fireworks | 0 |
| `pt-swarm` | 群れ（漂う光） | `particles` | 6000 | `rough` | particles | 0 |
| `pt-flow` | フローフィールド | `particles` | 4000 | `linear` | particles | 0 |
| `pt-coalesce` | 粒が集まって形になる | `particles` | 1200 | `spring` | coalesce | 0 |
| `lp-seamless` | 継ぎ目のないループ | `loops` | 2000 | `linear` | wave | 0 |
| `lp-float` | 浮遊 | `loops` | 3000 | `sine` | wave | 0 |
| `lp-marquee` | マーキー（流れる帯） | `loops` | 3000 | `linear` | marquee | 0 |
| `lp-conic` | 回る光の輪 | `loops` | 2000 | `linear` | wave | 0 |
| `lp-wobble` | ゆらゆら（ノイズ） | `loops` | 4000 | `rough` | swing | 0 |
| `lp-boil` | ラインボイル（線の震え） | `loops` | 167 | `steps` | wave | 0 |
| `lp-holdcycle` | ホールドとサイクル | `loops` | 1000 | `sine` | wave | 0 |
| `dv-stat` | 大きな数字のカード | `data-story` | 900 | `expo` | counter, scaleX | 0 |
| `dv-pins` | 地図にピンが落ちる | `data-story` | 400 | `bounce` | slideY, hop | 0 |
| `dv-scatter` | 散布図が現れる | `data-story` | 600 | `power` | scatter | 0 |
| `ch-walk` | 歩き（ウォークサイクル） | `character` | 1000 | `sine` | walk | 0 |
| `ch-jump` | ジャンプ（溜め→伸び→着地） | `character` | 1000 | `power` | hop, squash | 0 |
| `ch-blink` | まばたき | `character` | 250 | `power` | blink | 0 |
| `ch-take` | テイク（驚き） | `character` | 600 | `back` | walk | 0 |
| `ch-turn` | 振り向き | `character` | 600 | `power` | flipY | 0 |
| `ch-idle` | 待機（呼吸） | `character` | 3000 | `sine` | pulse | 0 |
| `ch-tail` | しっぽ・髪のフォロースルー | `character` | 800 | `spring` | tail | 0 |
| `lk-grain` | フィルムグレイン | `look` | 100 | `rough` | noise, snow | 0 |
| `lk-mblur` | モーションブラー（サブフレーム） | `look` | 400 | `power` | blur | 0 |
| `lk-riso` | 版ずれ（リソグラフ） | `look` | 125 | `steps` | noise | 0 |
| `lk-halftone` | ハーフトーン | `look` | 1500 | `sine` | noise | 0 |
| `lk-watercolor` | 水彩のにじみ | `look` | 2000 | `power` | tint | 0 |
| `lk-handwrite` | 筆で書く（ライトオン） | `look` | 1500 | `sine` | strokeDraw | 0 |
| `lk-papercut` | 切り絵・ストップモーション | `look` | 400 | `back` | shadow, wave | 0 |
| `lk-pixel` | ピクセル化（粗くする） | `look` | 600 | `steps` | blocks | 0 |
| `lk-vignette` | ビネット（呼吸する周辺減光） | `look` | 3000 | `sine` | iris | 0 |
| `lk-crt` | CRT（走査線） | `look` | 3000 | `linear` | noise, blinds | 0 |
| `3d-cube` | 立方体の回転 | `depth3d` | 4000 | `linear` | flipY, shadow | 0 |
| `3d-coverflow` | カバーフロー | `depth3d` | 500 | `spring` | flipY, reorder | 0 |
| `3d-depth` | 奥行きのある重なり | `depth3d` | 4000 | `linear` | parallax, scale, fade | 0 |
| `sd-cues` | SFX を拍に置く | `sound` | 120 | `expo` | audioPulse | 0 |
| `sd-bars` | オーディオビジュアライザー | `sound` | 100 | `power` | audioPulse | 0 |
| `sd-duck` | サイドチェイン・ダック | `sound` | 100 | `power` | audioDuck | 0 |
| `sd-lyric` | 歌詞・字幕を語に合わせる | `sound` | 250 | `back` | audioPulse | 0 |
| `tx-fade-up-chars` | 文字フェードアップ | `tx-reveal` | 900 | `power` | fade, slideY | 0 |
| `tx-fade-up-words` | 単語フェードアップ | `tx-reveal` | 500 | `power` | fade, slideY | 0 |
| `tx-slide-down-chars` | 文字スライドダウン | `tx-reveal` | 550 | `power` | slideX, slideY, fade | 0 |
| `tx-alternating-line-direction` | 行ごと交互方向リビール | `tx-reveal` | 650 | `power` | fade, slideY | 0 |
| `tx-stretch-rise` | 伸縮せり上がり | `tx-reveal` | 900 | `back` | slideY, jelly, fade | 0 |
| `tx-scale-y-unfold` | 縦方向展開 | `tx-reveal` | 600 | `back` | scale, scaleY, flipX, fade | 0 |
| `tx-converge-from-spread` | 中央へ収束 | `tx-reveal` | 1000 | `power` | gather, fade | 0 |
| `tx-center-out-reveal` | 中央から外へ展開 | `tx-reveal` | 450 | `power` | fade, slideY | 0 |
| `tx-scatter-assemble` | 散乱から集合 | `tx-reveal` | 1000 | `power` | scatter, gather, fade | 0 |
| `tx-random-tilt-scale-in` | ランダム傾きズームイン | `tx-reveal` | 900 | `power` | tilt, scale, fade | 0 |
| `tx-smear-stretch-in` | スミア伸長イン | `tx-reveal` | 800 | `power` | jelly, fade | 0 |
| `tx-scale-pop-in` | 弾性スケールポップ | `tx-reveal` | 600 | `spring` | scale, fade | 0 |
| `tx-range-soft-wave-reveal` | レンジセレクター・ソフトウェーブ | `tx-reveal` | 1800 | `linear` | wave, fade | 0 |
| `tx-random-order-fade` | ランダム順フェード | `tx-reveal` | 300 | `power` | fade | 0 |
| `tx-tracking-in-expand` | トラッキング・エクスパンド | `tx-reveal` | 700 | `bezier` | tracking, blur, fade | 0 |
| `tx-emoji-grapheme-pop` | 絵文字混じりポップ | `tx-reveal` | 450 | `spring` | scale, fade | 0 |
| `tx-char-exit-up` | 文字上抜けアウト | `tx-exit` | 500 | `power` | slideY, fade | 0 |
| `tx-fade-down-out` | 文字フェードダウン退場 | `tx-exit` | 600 | `power` | fade, slideY | 0 |
| `tx-blur-evaporate-out` | ブラー蒸発アウト | `tx-exit` | 600 | `power` | blur, fade | 0 |
| `tx-explode-scatter-out` | 爆散アウト | `tx-exit` | 800 | `power` | scatter, fade | 0 |
| `tx-gravity-drop-out` | 重力落下アウト | `tx-exit` | 1000 | `power` | hop, fade | 0 |
| `tx-scale-down-vanish` | 縮小消滅 | `tx-exit` | 250 | `power` | slideY, scale, fade | 0 |
| `tx-reverse-erase` | 逆順消去 | `tx-exit` | 500 | `power` | fade, slideY | 0 |
| `tx-collapse-to-center` | 中央へ吸い込み | `tx-exit` | 500 | `power` | scaleY, fade | 0 |
| `tx-noise-dissolve-out` | ノイズ溶解 | `tx-exit` | 1200 | `linear` | dissolve, noise, fade | 0 |
| `tx-bump-wave` | バンプウェーブ | `tx-emphasis` | 1900 | `linear` | wave | 0 |
| `tx-jelly-rubber-band` | ゼリー・ラバーバンド | `tx-emphasis` | 900 | `power` | jelly | 0 |
| `tx-keyword-punch` | キーワードの打ち込み | `tx-emphasis` | 200 | `power` | pulse | 0 |
| `tx-wiggly-jitter` | ウィグリー揺らぎ | `tx-emphasis` | 125 | `rough` | shake | 0 |
| `tx-marker-highlight` | マーカー強調 | `tx-emphasis` | 1200 | `power` | glow, highlight | 0 |
| `tx-underline-draw` | 下線ドロー | `tx-emphasis` | 400 | `bezier` | strokeDraw, underline | 0 |
| `tx-scribble-circle` | 手描き丸囲み | `tx-emphasis` | 800 | `power` | pulse | 0 |
| `tx-strike-correct` | 取り消し線で訂正 | `tx-emphasis` | 500 | `back` | pulse | 0 |
| `tx-attention-bounce` | 注目バウンス | `tx-emphasis` | 800 | `bounce` | hop | 0 |
| `tx-pointer-highlight-box` | ポインタ付き強調枠 | `tx-emphasis` | 1000 | `power` | glow, highlight | 0 |
| `tx-comic-pop` | コミック・ポップ | `tx-emphasis` | 600 | `back` | scale | 0 |
| `tx-split-flap` | スプリットフラップ(パタパタ) | `tx-swap` | 100 | `power` | flipX, textSwap, split | 0 |
| `tx-rotating-letters-spring` | 回転テキスト(文字スプリング) | `tx-swap` | 450 | `spring` | textSwap | 0 |
| `tx-vertical-word-roll` | 縦ロールで単語切替 | `tx-swap` | 500 | `power` | spin, textSwap, slideY | 0 |
| `tx-flip-words-blur` | フリップワード(ブラー) | `tx-swap` | 200 | `spring` | flipY, blur | 0 |
| `tx-gooey-morph` | グーイー・モーフ | `tx-swap` | 1500 | `linear` | morph | 0 |
| `tx-blur-crossfade` | ブラー・クロスフェード | `tx-swap` | 600 | `power` | fade, blur | 0 |
| `tx-cube-rotate-swap` | キューブ回転で切替 | `tx-swap` | 600 | `power` | spin, textSwap | 0 |
| `tx-skew-swipe` | スキュースワイプ | `tx-swap` | 400 | `expo` | skew, wipe | 0 |
| `tx-letter-cascade-swap` | 文字カスケード入替 | `tx-swap` | 400 | `power` | textSwap | 0 |
| `tx-diff-retype` | 差分だけ打ち替え | `tx-swap` | 2000 | `linear` | typewriter, textSwap | 0 |
| `tx-typewriter` | タイプライター | `tx-typing` | 100 | `steps` | typewriter | 0 |
| `tx-caret-typewriter` | カーソル点滅タイプ | `tx-typing` | 100 | `steps` | typewriter | 0 |
| `tx-typewriter-loop` | タイプ&バックスペース | `tx-typing` | 100 | `steps` | typewriter | 0 |
| `tx-human-typewriter` | 人間らしいタイプ | `tx-typing` | 100 | `steps` | typewriter | 0 |
| `tx-terminal-typing` | ターミナル風 | `tx-typing` | 140 | `power` | typewriter | 0 |
| `tx-scramble-decode` | スクランブル解読 | `tx-typing` | 1000 | `linear` | scramble | 0 |
| `tx-decrypt-center` | 中央から復号 | `tx-typing` | 100 | `steps` | scramble | 0 |
| `tx-char-flicker-in` | 記号フリッカー出現 | `tx-typing` | 100 | `linear` | typewriter | 0 |
| `tx-slot-shuffle` | スロット・シャッフル | `tx-typing` | 800 | `power` | reorder | 0 |
| `tx-char-code-cycle` | 文字コード送り | `tx-typing` | 1000 | `power` | typewriter | 0 |
| `tx-placeholder-replace` | 伏せ字から置換 | `tx-typing` | 100 | `steps` | typewriter | 0 |
| `tx-drop-bounce-in` | 落下バウンド | `tx-physics` | 900 | `bounce` | hop | 0 |
| `tx-raining-characters` | 文字の雨 | `tx-physics` | 500 | `power` | snow | 0 |
| `tx-falling-pile` | 落下して積もる | `tx-physics` | 600 | `gravity` | hop | 0 |
| `tx-pendulum-swing` | 振り子スイング | `tx-physics` | 2400 | `spring` | swing | 0 |
| `tx-squash-landing` | 着地スクワッシュ | `tx-physics` | 1100 | `elastic` | squash | 0 |
| `tx-spring-chain` | バネ連鎖追従 | `tx-physics` | 1900 | `spring` | hop | 0 |
| `tx-zero-gravity-drift` | 無重力ドリフト | `tx-physics` | 4500 | `sine` | hop, wave | 0 |
| `tx-domino-topple` | ドミノ倒し | `tx-physics` | 120 | `power` | hop | 0 |
| `tx-particle-text` | パーティクル文字 | `tx-physics` | 900 | `spring` | particles | 0 |
| `tx-circular-text-spin` | 円形テキスト回転 | `tx-path` | 10000 | `linear` | spin | 0 |
| `tx-curved-loop-marquee` | 曲線ループマーキー | `tx-path` | 4000 | `linear` | marquee | 0 |
| `tx-text-along-path` | パスに沿って走る | `tx-path` | 1200 | `power` | arc | 0 |
| `tx-path-inertia` | パス+慣性トラッキング | `tx-path` | 900 | `inertia` | travel, arc | 0 |
| `tx-outline-draw` | アウトライン線描 | `tx-path` | 2000 | `power` | strokeDraw | 0 |
| `tx-handwriting` | 手書き線描 | `tx-path` | 2200 | `power` | strokeDraw | 0 |
| `tx-spiral-text` | 渦巻きテキスト | `tx-path` | 8000 | `linear` | orbit, gather | 0 |
| `tx-morphing-path-ride` | 変形するパスに乗る | `tx-path` | 1500 | `power` | morph, arc | 0 |
| `tx-shape-contour-text` | 図形の輪郭を巡る | `tx-path` | 10000 | `linear` | arc | 0 |
| `tx-line-mask-rise` | 行マスク・せり上がり | `tx-mask` | 1000 | `expo` | slideY, lines, wipe | 0 |
| `tx-char-mask-rise` | 文字マスク・せり上がり | `tx-mask` | 500 | `power` | slideY | 0 |
| `tx-slide-x-mask` | 横スライドイン(マスク) | `tx-mask` | 800 | `expo` | slideX | 0 |
| `tx-split-halves` | 上下分割スライド | `tx-mask` | 700 | `expo` | split | 0 |
| `tx-soft-mask-wipe` | ソフトマスク・ワイプ | `tx-mask` | 1200 | `power` | wipe | 0 |
| `tx-bar-wipe-reveal` | バーワイプ出現 | `tx-mask` | 800 | `power` | wipe | 0 |
| `tx-iris-reveal` | アイリス円形リビール | `tx-mask` | 800 | `bezier` | iris | 0 |
| `tx-shape-mask-cut` | 形状マスク切替 | `tx-mask` | 600 | `power` | cut | 0 |
| `tx-blinds-reveal` | ブラインド・リビール | `tx-mask` | 600 | `power` | blinds | 0 |
| `tx-diagonal-cut-reveal` | 斜めカット・リビール | `tx-mask` | 700 | `expo` | diagonal, cut | 0 |
| `tx-outline-fill-sweep` | 輪郭→塗りスイープ | `tx-mask` | 1000 | `power` | sweep, strokeDraw | 0 |
| `tx-texture-fill` | テクスチャ/動画の流し込み | `tx-mask` | 10000 | `linear` | wipe | 0 |
| `tx-shiny-sweep` | シャイン・スイープ | `tx-color-light` | 2000 | `bezier` | sweep | 0 |
| `tx-gradient-flow` | グラデーション流動 | `tx-color-light` | 8000 | `linear` | hue, sweep | 0 |
| `tx-aurora-text` | オーロラテキスト | `tx-color-light` | 6000 | `power` | hue | 0 |
| `tx-conic-spin` | コニック回転 | `tx-color-light` | 4000 | `linear` | spin | 0 |
| `tx-color-band-sweep` | カラーバンド・スイープ | `tx-color-light` | 1500 | `power` | sweep, tint | 0 |
| `tx-hue-cycle` | 色相サイクル | `tx-color-light` | 4000 | `linear` | hue | 0 |
| `tx-random-color-pop` | カラフル文字サイクル | `tx-color-light` | 500 | `power` | scale, tint | 0 |
| `tx-neon-flicker` | ネオン点灯 | `tx-color-light` | 900 | `steps` | glow | 0 |
| `tx-glow-pulse` | グロー脈動 | `tx-color-light` | 667 | `power` | pulse, glow | 0 |
| `tx-line-shadow` | ラインシャドウ | `tx-color-light` | 10000 | `linear` | shadow | 0 |
| `tx-long-shadow-grow` | ロングシャドウ伸長 | `tx-color-light` | 800 | `power` | shadow | 0 |
| `tx-blend-overlap` | 文字重なり合成 | `tx-color-light` | 900 | `power` | hue | 0 |
| `tx-text-generate` | テキスト生成エフェクト | `tx-blur` | 800 | `power` | blur, fade | 0 |
| `tx-blur-in-3step` | 3段ブラーイン | `tx-blur` | 700 | `power` | blur | 0 |
| `tx-blur-in-chars` | 文字ブラーイン | `tx-blur` | 700 | `power` | blur | 0 |
| `tx-blur-random-in` | ランダムブラーイン | `tx-blur` | 600 | `power` | blur | 0 |
| `tx-tracking-blur-build` | ブラー&トラッキング出現 | `tx-blur` | 2000 | `power` | blur, tracking | 0 |
| `tx-directional-blur-slide` | 方向ブラースライド | `tx-blur` | 550 | `expo` | slideX, blur | 0 |
| `tx-scale-blur-zoom` | スケールブラー・ズームイン | `tx-blur` | 700 | `bezier` | scale, blur | 0 |
| `tx-rack-focus` | ラックフォーカス | `tx-blur` | 1200 | `power` | blur | 0 |
| `tx-true-focus` | トゥルーフォーカス枠 | `tx-blur` | 500 | `bezier` | blur | 0 |
| `tx-rgb-split-glitch` | RGB分離グリッチ | `tx-glitch` | 160 | `steps` | split, chromatic, shake | 0 |
| `tx-slice-glitch` | スライス変位グリッチ | `tx-glitch` | 100 | `steps` | blinds, chromatic, shake | 0 |
| `tx-fuzzy-text` | ファジーテキスト | `tx-glitch` | 100 | `rough` | noise | 0 |
| `tx-turbulence-wobble` | タービュランス揺らぎ | `tx-glitch` | 100 | `steps` | swing, displace | 0 |
| `tx-ascii-render` | ASCII テキスト | `tx-glitch` | 800 | `linear` | chromatic, shake | 0 |
| `tx-vhs-scanline` | VHS 走査線ノイズ | `tx-glitch` | 3000 | `linear` | chromatic, shake | 0 |
| `tx-pixel-mosaic` | モザイク解像 | `tx-glitch` | 720 | `steps` | blocks | 0 |
| `tx-melt-drip` | 溶けて垂れる | `tx-glitch` | 2000 | `power` | displace | 0 |
| `tx-flip-up-3d` | 3D フリップアップ | `tx-depth` | 800 | `expo` | slideY, flipY | 0 |
| `tx-swing-door` | スイングドア | `tx-depth` | 1000 | `expo` | swing | 0 |
| `tx-z-fly-in` | 奥から飛来 | `tx-depth` | 900 | `back` | flipX | 0 |
| `tx-tumble-in` | 回転落下イン | `tx-depth` | 1300 | `power` | spin, slideX | 0 |
| `tx-roll-flip-hover` | 3D ロールフリップ | `tx-depth` | 400 | `power` | flipY, spin | 0 |
| `tx-cylinder-text` | 3D 円筒テキスト | `tx-depth` | 6000 | `linear` | flipX | 0 |
| `tx-layered-depth` | レイヤー奥行き文字 | `tx-depth` | 600 | `spring` | flipX | 0 |
| `tx-paper-fold` | 折りたたみ | `tx-depth` | 700 | `power` | flipX, blinds | 0 |
| `tx-motext-falloff` | エフェクター掃引(MoText) | `tx-depth` | 1700 | `spring` | flipX | 0 |
| `tx-weight-wave` | ウェイトウェーブ | `tx-variable` | 1600 | `power` | weight, wave | 0 |
| `tx-width-stretch` | 可変幅ストレッチ | `tx-variable` | 600 | `power` | jelly, scaleX | 0 |
| `tx-weight-breathe` | ウェイト呼吸 | `tx-variable` | 3000 | `power` | pulse, weight | 0 |
| `tx-thin-to-bold-in` | 細→太で出現 | `tx-variable` | 900 | `power` | weight | 0 |
| `tx-slant-swing` | 傾き(slnt)スイング | `tx-variable` | 2000 | `power` | swing, skew | 0 |
| `tx-width-slant-scroll` | 速度で幅・傾き変化 | `tx-variable` | 800 | `spring` | scaleX, skew | 0 |
| `tx-text-pressure` | テキストプレッシャー | `tx-variable` | 300 | `linear` | squash | 0 |
| `tx-variable-proximity` | 可変プロキシミティ | `tx-variable` | 300 | `power` | weight | 0 |
| `tx-scroll-word-highlight` | スクロールで単語明転 | `tx-scroll` | 3800 | `linear` | spin, glow, highlight | 0 |
| `tx-view-timeline-words` | CSS view() 単語リビール | `tx-scroll` | 3800 | `linear` | highlight | 0 |
| `tx-scroll-blur-tilt` | 傾き+ブラー解像 | `tx-scroll` | 3800 | `linear` | spin, tilt, blur | 0 |
| `tx-scroll-clip-write` | スクロール書き出し | `tx-scroll` | 3600 | `linear` | spin | 0 |
| `tx-scroll-velocity-marquee` | スクロール速度マーキー | `tx-scroll` | 5000 | `spring` | spin, marquee | 0 |
| `tx-scroll-skew` | スクロール速度スキュー | `tx-scroll` | 800 | `power` | spin, skew | 0 |
| `tx-horizontal-letter-drop` | 横スクロール文字落下 | `tx-scroll` | 4000 | `back` | highlight | 0 |
| `tx-dual-wave-columns` | デュアルウェーブ列 | `tx-scroll` | 4000 | `linear` | wave | 0 |
| `tx-repetition-echo` | テキスト反復エコー | `tx-scroll` | 1000 | `power` | highlight | 0 |
| `tx-text-window-zoom` | 文字窓ズーム | `tx-scroll` | 4200 | `linear` | scale | 0 |
| `tx-sticky-shrink-headline` | 追従縮小見出し | `tx-scroll` | 3800 | `linear` | highlight | 0 |
| `tx-parallax-lines` | 行ごとのパララックス | `tx-scroll` | 4000 | `linear` | parallax | 0 |
| `tx-magnetic-letters` | マグネット文字 | `tx-pointer` | 400 | `power` | gather | 0 |
| `tx-proximity-repel` | 近接反発 | `tx-pointer` | 300 | `inertia` | scatter | 0 |
| `tx-proximity-scramble` | 近接スクランブル | `tx-pointer` | 1200 | `linear` | scramble | 0 |
| `tx-clone-roll-hover` | 複製ロールホバー | `tx-pointer` | 400 | `power` | spin | 0 |
| `tx-hover-word-lift` | ホバー単語リフト | `tx-pointer` | 250 | `spring` | gather | 0 |
| `tx-cursor-text-trail` | カーソル軌跡テキスト | `tx-pointer` | 500 | `back` | gather | 0 |
| `tx-spotlight-outline` | スポットライト輪郭 | `tx-pointer` | 4000 | `power` | glow, iris, strokeDraw | 0 |
| `tx-pointer-reveal-card` | ポインタでめくる | `tx-pointer` | 400 | `power` | gather | 0 |
| `tx-lens-bulge` | カーソル膨張レンズ | `tx-pointer` | 350 | `power` | gather | 0 |
| `tx-count-up` | カウントアップ | `tx-counter` | 2000 | `expo` | slideY, counter | 0 |
| `tx-odometer-roll` | オドメーター桁ロール | `tx-counter` | 750 | `power` | spin, counter, slideY | 0 |
| `tx-ticker-flash` | 価格ティッカー・フラッシュ | `tx-counter` | 600 | `power` | marquee | 0 |
| `tx-score-update` | スコア更新 | `tx-counter` | 350 | `power` | slideY, counter | 0 |
| `tx-number-scramble` | 数字シャッフル確定 | `tx-counter` | 120 | `steps` | scramble, counter | 0 |
| `tx-progress-percent` | パーセント+リング | `tx-counter` | 1600 | `power` | counter, strokeDraw | 0 |
| `tx-countdown-zoom` | カウントダウン | `tx-counter` | 1000 | `back` | slideY, scale, counter | 0 |
| `tx-seven-segment` | 7セグメント表示 | `tx-counter` | 100 | `linear` | counter | 0 |
| `tx-pop-on-subtitle` | ポップオン字幕 | `tx-caption` | 4200 | `steps` | scale, caption | 0 |
| `tx-paint-on-caption` | ペイントオン字幕 | `tx-caption` | 120 | `steps` | caption | 0 |
| `tx-roll-up-caption` | ロールアップ字幕 | `tx-caption` | 270 | `power` | slideY, spin, caption | 0 |
| `tx-live-caption-block` | 生字幕(2行ブロック) | `tx-caption` | 5400 | `steps` | caption | 0 |
| `tx-word-highlight-caption` | 現在語ハイライト字幕 | `tx-caption` | 100 | `power` | glow, highlight, caption | 0 |
| `tx-highlight-pill` | 背景ピル移動 | `tx-caption` | 100 | `power` | glow, highlight | 0 |
| `tx-hormozi-caption` | ショート動画ポップ字幕 | `tx-caption` | 150 | `back` | caption | 0 |
| `tx-caption-spring-pop` | 字幕スプリングポップ | `tx-caption` | 800 | `spring` | scale, caption | 0 |
| `tx-karaoke-fill` | カラオケ塗り | `tx-caption` | 4800 | `linear` | highlight | 0 |
| `tx-forced-narrative-fade` | 強制字幕のフェード同期 | `tx-caption` | 300 | `linear` | fade | 0 |
| `tx-rsvp-flash` | RSVP 一語提示 | `tx-caption` | 200 | `steps` | caption | 0 |
| `tx-phrase-line-break` | 文節改行字幕 | `tx-caption` | 550 | `expo` | caption | 0 |
| `tx-fit-text-title` | 幅ぴったり見出し | `tx-layout` | 800 | `power` | lines | 0 |
| `tx-stacked-justified` | 均等幅スタック | `tx-layout` | 300 | `power` | lines | 0 |
| `tx-layout-word-insert` | レイアウト追従ワード入替 | `tx-layout` | 400 | `spring` | lines | 0 |
| `tx-anagram-flip` | アナグラム並び替え | `tx-layout` | 600 | `bezier` | flipY | 0 |
| `tx-balanced-reflow` | バランス改行と再分割 | `tx-layout` | 600 | `expo` | reorder | 0 |
| `tx-line-spacing-collapse` | 行間の収束 | `tx-layout` | 1000 | `power` | scaleY | 0 |
| `tx-kinetic-camera` | カメラ追従キネティック | `tx-layout` | 400 | `bezier` | camera, slideX | 0 |
| `tx-expand-truncated` | 省略から全文展開 | `tx-layout` | 450 | `bezier` | lines | 0 |
| `tx-wave-loop` | ウェーブループ | `tx-loop` | 1200 | `power` | wave | 0 |
| `tx-infinite-marquee` | 無限マーキー | `tx-loop` | 10000 | `linear` | marquee | 0 |
| `tx-scale-breathe` | 拡縮の呼吸 | `tx-loop` | 1600 | `sine` | pulse, scale | 0 |
| `tx-sparkles-text` | スパークル文字 | `tx-loop` | 800 | `power` | particles | 0 |
| `tx-jump-sequence` | 順番ジャンプ | `tx-loop` | 400 | `power` | hop | 0 |
| `tx-y-spin-stagger` | 文字スピン連鎖 | `tx-loop` | 800 | `power` | spin | 0 |
| `tx-loading-dots` | 読み込み中… | `tx-loop` | 1200 | `power` | wave | 0 |
| `tx-hanging-sway` | 吊り看板の揺れ | `tx-loop` | 2400 | `power` | wave | 0 |
| `tx-bunsetsu-reveal` | 文節単位リビール | `tx-japanese` | 600 | `expo` | typewriter | 0 |
| `tx-yakumono-pause` | 約物ポーズ | `tx-japanese` | 100 | `steps` | typewriter | 0 |
| `tx-tategaki-reveal` | 縦書きリビール | `tx-japanese` | 500 | `expo` | typewriter | 0 |
| `tx-ruby-sync` | ルビ同期リビール | `tx-japanese` | 600 | `power` | typewriter | 0 |
| `tx-ime-conversion` | かな漢字変換 | `tx-japanese` | 100 | `steps` | scramble, textSwap | 0 |
| `tx-kanji-stroke-order` | 漢字書き順 | `tx-japanese` | 300 | `power` | strokeDraw | 0 |
| `tx-brush-write-on` | 筆文字ライトオン | `tx-japanese` | 350 | `power` | strokeDraw | 0 |
| `tx-onomatopoeia-impact` | 擬音インパクト | `tx-japanese` | 167 | `sine` | typewriter | 0 |
| `tx-lyric-tame-tsume` | リリックモーション(タメツメ) | `tx-japanese` | 300 | `bezier` | typewriter | 0 |
| `tx-danmaku-comments` | 弾幕コメント | `tx-japanese` | 3500 | `linear` | marquee | 0 |
| `tx-eva-mincho-card` | 極太明朝タイトルカード | `tx-japanese` | 333 | `steps` | typewriter | 0 |
| `tx-jp-subtitle-timing` | 日本語字幕(1秒4文字) | `tx-japanese` | 2000 | `steps` | caption | 0 |
| `tx-vertical-subtitle` | 縦字幕 | `tx-japanese` | 4200 | `steps` | caption | 0 |
| `tx-cinema-handwritten-subtitle` | 手書き風映画字幕 | `tx-japanese` | 4400 | `steps` | caption | 0 |
| `tx-tracking-in-contract` | トラッキング収束 | `tx-title` | 2800 | `power` | tracking, blur | 0 |
| `tx-slow-tracking-expand` | スロー字間拡散 | `tx-title` | 6000 | `linear` | tracking, blur | 0 |
| `tx-fade-through-black` | フェードスルーブラック | `tx-title` | 500 | `sine` | fade | 0 |
| `tx-slam-in-title` | スラムイン | `tx-title` | 200 | `bezier` | fade, tracking | 0 |
| `tx-flash-frame-cut` | フラッシュフレームカット | `tx-title` | 125 | `steps` | cut | 0 |
| `tx-opening-crawl` | オープニングクロール | `tx-title` | 10000 | `linear` | marquee, flipX | 0 |
| `tx-racing-credits` | グラフィック線のクレジット | `tx-title` | 270 | `linear` | marquee | 0 |
| `tx-split-bars-title` | 横線分断タイトル | `tx-title` | 700 | `expo` | split | 0 |
| `tx-film-scratch-jitter` | フィルム傷とジッター | `tx-title` | 100 | `steps` | shake | 0 |
| `tx-stamp-cutout` | スタンプ切り絵ポップ | `tx-title` | 167 | `steps` | cut | 0 |
| `tx-light-leak-burn` | ライトリーク焼き込み | `tx-title` | 900 | `power` | glow, sweep | 0 |
| `tx-letterbox-reveal` | レターボックス開き | `tx-title` | 800 | `power` | fade, tracking | 0 |
| `tx-lower-third` | ロワーサード | `tx-broadcast` | 400 | `expo` | slideX, caption | 0 |
| `tx-news-ticker` | ニュースティッカー | `tx-broadcast` | 4000 | `linear` | marquee | 0 |
| `tx-breaking-banner` | 速報バナー | `tx-broadcast` | 267 | `power` | slideX, caption | 0 |
| `tx-credits-roll` | エンドクレジット | `tx-broadcast` | 10000 | `linear` | spin, marquee | 0 |
| `tx-led-dot-matrix` | LED 電光掲示板 | `tx-broadcast` | 100 | `steps` | slideX, caption | 0 |
| `tx-telop-pop-outline` | 二重フチのポン出しテロップ | `tx-broadcast` | 200 | `back` | scale, strokeDraw, caption | 0 |
| `tx-telop-shake` | 揺れテロップ | `tx-broadcast` | 100 | `rough` | shake, caption | 0 |
| `tx-telop-zabuton` | 座布団ワイプテロップ | `tx-broadcast` | 233 | `expo` | highlight, caption | 0 |
| `tx-telop-tsukkomi` | ツッコミテロップ | `tx-broadcast` | 100 | `power` | caption | 0 |
| `tx-telop-kirakira` | キラキラ出現テロップ | `tx-broadcast` | 233 | `back` | caption | 0 |
| `tx-telop-comment-follow` | コメントフォロー(話者色) | `tx-broadcast` | 167 | `back` | follow, caption | 0 |
| `tx-telop-side-super` | サイドスーパー | `tx-broadcast` | 333 | `power` | slideY, caption | 0 |
