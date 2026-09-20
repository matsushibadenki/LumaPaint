**漫画・イラスト・AI生成・アニメーションを同じ非破壊ドキュメントモデルで扱う制作環境LumaPaint**

特に重要なのは、AI生成結果を「画像を生成して貼り付ける機能」にしないことです。**AI生成そのものを非破壊レイヤー／生成ノードとして扱う**設計にすると、このソフト独自の強みになります。

なお2026年9月現在、OpenAIの `gpt-image-2.5-flare` と `gpt-image-2.5-sunburst` は実際にAPI提供されており、Sunburstは編集精度重視、Flareは速度・コストとのバランスを重視したモデルです。任意解像度、透明背景、画像編集にも対応しています。([OpenAI][1])
Google側も Nano Banana 系をAPI提供しており、現行では `gemini-3.1-flash-image` などが利用できます。画像＋テキストによる編集にも対応しています。([Google AI for Developers][2])
日本語・英語・簡体字中国語に対応してください。
ライトモード・ダークモードを実装してください。
---

# 1. ソフト全体のコンセプト

仮称を **LumaPaint** としておきます。

```text
LumaPaint
│
├── Painting
│   ├── Raster Paint
│   ├── Vector Paint
│   ├── Brush Engine
│   ├── Selection / Mask
│   └── Filters
│
├── Comic
│   ├── Page Manager
│   ├── Panel / Frame
│   ├── Speech Balloon
│   ├── Text
│   ├── Perspective Ruler
│   └── Manga Assets
│
├── AI Studio
│   ├── Character Generation
│   ├── Background Generation
│   ├── Inpainting
│   ├── Outpainting
│   ├── Sketch → Render
│   ├── Character Reference
│   └── AI Layer
│
├── Animation
│   ├── Timeline
│   ├── Keyframe
│   ├── Layer Animation
│   ├── Camera
│   ├── Effects
│   └── Audio
│
├── Color
│   ├── RGB
│   ├── CMYK
│   ├── ICC
│   └── Soft Proof
│
└── Render / Export
    ├── PNG
    ├── JPEG
    ├── TIFF
    ├── PSD
    ├── PDF
    ├── WebP
    └── Video
```

CLIP STUDIO PAINT + Photoshop + After Effects + AI画像生成を統合した構造です。

---

# 2. 基本アーキテクチャ

Tauri 2をUIシェルとして使用しますが、**描画エンジンをWebViewに依存させない**ことを強く推奨します。

```text
┌──────────────────────────────────────────┐
│               Tauri 2 UI                 │
│                                          │
│ React / JavaScript                       │
│ ┌────────┐ ┌────────┐ ┌───────────────┐ │
│ │ Tools  │ │ Layers │ │ Properties    │ │
│ └────────┘ └────────┘ └───────────────┘ │
│                                          │
│ ┌──────────────────────────────────────┐ │
│ │ Native Canvas Surface                │ │
│ └──────────────────────────────────────┘ │
│                                          │
│ ┌──────────────────────────────────────┐ │
│ │ Timeline                             │ │
│ └──────────────────────────────────────┘ │
└─────────────────┬────────────────────────┘
                  │ IPC
                  ▼
┌──────────────────────────────────────────┐
│               Rust Core                  │
│                                          │
│ Document Engine                          │
│ Layer Engine                             │
│ Brush Engine                             │
│ Compositor                               │
│ Color Management                         │
│ Timeline                                 │
│ AI Router                                │
│ Asset Manager                            │
│ Undo / Redo                              │
└─────────────────┬────────────────────────┘
                  │
            wgpu / Metal
                  │
                  ▼
             GPU Renderer
```

macOSでは最終的に

```text
wgpu
 ↓
Metal
 ↓
Apple GPU
```

Windowsでは

```text
wgpu
 ↓
Vulkan / DX12
```

Linuxでは

```text
wgpu
 ↓
Vulkan
```

とします。

これならApple Siliconをかなり活用できます。

---

# 3. 最大のポイント：すべてを「ノード」として保持する

一般的なペイントソフトは

```text
Document
 └─ Layer
     └─ Pixels
```

ですが、LumaPaintでは

```text
Document
 └─ Scene Graph
      └─ Layer Node
           └─ Processing Graph
```

とします。

例えば、

```text
Character
 ↓
AI Generate
 ↓
Color Correction
 ↓
Transform
 ↓
Mask
 ↓
Blur
 ↓
Composite
```

という処理履歴そのものを保存します。

つまりAfter Effectsに近い考え方です。

---

# 4. レイヤー構造

レイヤー型をかなり多くします。

```rust
enum LayerKind {
    Raster,
    Vector,
    Text,
    Group,

    Adjustment,
    Filter,
    Mask,

    ComicPanel,
    Balloon,

    AiGenerated,
    AiEdit,

    Video,
    Audio,

    Camera,
    Light,
}
```

例えば、

```text
PAGE 001

├─ Character
│   ├─ AI Character
│   ├─ Color Correction
│   └─ Mask
│
├─ Background
│   ├─ AI Background
│   └─ Perspective Transform
│
├─ Effects
│
├─ Balloon
│
└─ Panel Frames
```

とできます。

---

# 5. 非破壊処理

これはアプリの中核にします。

たとえばGaussian Blurをかけても、

```text
元画像

↓

FilterNode
{
    type: GaussianBlur
    radius: 12
}
```

と記録するだけです。

元画像を書き換えません。

同様に、

```text
Exposure
Hue/Saturation
Levels
Curves
Color Balance
LUT
Blur
Sharpen
Noise
Transform
Perspective
Liquify
```

もノード化します。

概念的には

```text
Source
  ↓
Transform
  ↓
Mask
  ↓
ColorCorrection
  ↓
Effect
  ↓
Composite
```

です。

この方式なら後から全部変更できます。

---

# 6. AIも非破壊ノードにする

ここが重要です。

AI生成レイヤーを

```rust
AiLayer {
    provider,
    model,
    prompt,
    negative_prompt,
    references,
    mask,
    seed,
    parameters,
    result,
}
```

として保持します。

例えばユーザーが

> 雨の新宿。夜。映画的な背景。

と入力した場合、

```text
AI Background Layer

Provider:
OpenAI

Model:
gpt-image-2.5-sunburst

Prompt:
"Rainy Shinjuku at night..."

Reference:
background-sketch.png

Result:
asset://ai/82736.webp
```

という状態になります。

つまり生成画像だけでなく、**どう生成したかも作品データ**になります。

---

# 7. AI Provider abstraction

モデル名をコードに直書きしない構造にします。

```text
AiProvider
│
├── OpenAIProvider
├── GeminiProvider
├── LocalProvider
├── ComfyUIProvider
└── CustomProvider
```

共通インターフェースは、

```rust
trait ImageProvider {
    async fn generate(
        &self,
        request: GenerateRequest
    ) -> Result<ImageResult>;

    async fn edit(
        &self,
        request: EditRequest
    ) -> Result<ImageResult>;
}
```

程度にします。

こうしておけば将来、

```text
OpenAI
Google
Adobe
Stability
Black Forest Labs
ComfyUI
Draw Things
ローカルモデル
```

などを簡単に追加できます。

---

# 8. AI設定画面

例えば、

```text
AI Providers

OpenAI
────────────────
API Key
••••••••••••••••

Default Model
GPT Image 2.5 Flare ▼


Google Gemini
────────────────
API Key
••••••••••••••••

Default Model
Nano Banana 2 ▼
```

とします。

APIキーはプロジェクトファイルには絶対保存せず、

```text
macOS → Keychain
Windows → Credential Manager
Linux → Secret Service
```

に保存します。

---

# 9. AI生成UI

右側にAIパネルを設けます。

```text
┌──────────────────────────┐
│ AI Studio                │
├──────────────────────────┤
│ Model                    │
│ GPT Image 2.5 Sunburst ▼ │
│                          │
│ Mode                     │
│ Background ▼             │
│                          │
│ Prompt                   │
│ ┌──────────────────────┐ │
│ │ 雨の東京、新宿...    │ │
│ └──────────────────────┘ │
│                          │
│ References               │
│ [Character] [Pose] [+]   │
│                          │
│ [ Generate ]             │
└──────────────────────────┘
```

---

# 10. AI生成モード

単なるGenerate以外に、

```text
Generate
Edit
Inpaint
Outpaint
Sketch → Image
Line Art → Color
Color → Line Art
Background
Character
Pose variation
Expression variation
Lighting variation
Style variation
```

を用意します。

OpenAI Images 2.5は画像入力による編集と、透明背景生成にも対応しているため、この構造と非常に相性がいいです。([OpenAI Developers][3])

---

# 11. キャラクター管理

漫画制作では特に重要です。

「Character Library」を作ります。

```text
Characters

HARUKA
├─ Reference Front
├─ Reference Side
├─ Reference Back
├─ Face
├─ Costume
├─ Color Palette
└─ AI Instructions
```

例えば、

```text
Character ID:
CHR_001

Name:
Haruka

Hair:
short black hair

Eyes:
brown

Height:
162cm

Costume:
school uniform

References:
front.png
side.png
face.png
```

AI生成するとき自動的に参照画像として渡します。

GPT Image 2.5でもキャラクター参照を使った一貫性維持が公式に想定されています。([OpenAI Developers][4])

Nano Banana 2も複数参照画像と一貫性を重視したモデルとして提供されています。([Google AI for Developers][2])

---

# 12. AI背景生成

背景では現在選択しているコマの情報もAIへ送ります。

例えば漫画ページが

```text
┌─────────────┐
│             │
│   Panel 1   │
│             │
├──────┬──────┤
│Panel2│Panel3│
└──────┴──────┘
```

ならPanel 2を選択して、

> 東京駅前。夕方。

と入力。

AIには

```text
Panel dimensions
Camera perspective
Character position
Existing sketch
Previous panel
Color palette
Prompt
```

を送ります。

つまりAIが**漫画の構図を理解して背景だけ生成**します。

---

# 13. 漫画コマ割り

Panelは単なる線ではなくオブジェクトです。

```rust
ComicPanel {
    polygon,
    padding,
    bleed,
    border,
    content_root,
    camera,
}
```

つまり

```text
Panel
 └─ mini scene graph
```

にします。

そのため各コマ内部に独立した

```text
Character
Background
Effects
Text
```

を持てます。

---

# 14. コマ割りツール

CLIP STUDIOよりさらに柔軟に、

```text
Split Horizontal
Split Vertical
Diagonal Split
Bezier Split
Free Polygon
Grid
Golden Ratio
Manga Templates
```

を用意します。

ドラッグすると

```text
┌─────────────┐
│             │
├─────────────┤
│      │      │
│      │      │
└──────┴──────┘
```

のように自動分割。

---

# 15. ページ管理

漫画の場合は1枚キャンバスでは足りません。

```text
Project

├── Cover
├── Page 001
├── Page 002
├── Page 003
...
└── Page 120
```

を扱います。

ページ一覧は左側に

```text
[001]
[002]
[003]
[004]
```

と表示します。

見開きも

```text
[ 12 ][ 13 ]
```

として扱います。

---

# 16. After Effects型Timeline

下部を完全なTimelineにします。

```text
Timeline

00:00     01:00     02:00     03:00

Character
────●────────●────────────

Background
━━━━━━━━━━━━━━━━━━━━━━━━━━

Camera
──────●──────────●────────

Effects
──────────●────────────────
```

レイヤーとTimelineは同じオブジェクトを参照します。

---

# 17. Animatable Property

ほぼすべての値をアニメーション可能にします。

```text
Transform
 Position
 Scale
 Rotation
 Anchor

Opacity

Filter
 Blur
 Exposure
 Hue

Mask
 Path
 Feather

AI
 Prompt
 Strength
```

内部的には、

```rust
Animated<T> {
    default: T,
    keyframes: Vec<Keyframe<T>>
}
```

という設計にできます。

---

# 18. AI Prompt自体もキーフレーム化

かなり面白い機能になります。

例えば

```text
0 sec
晴れた東京

↓

5 sec
曇り

↓

10 sec
激しい雨
```

を、

```text
AI Prompt Track
```

として保持します。

将来、動画生成モデルを追加すれば、

```text
AI Prompt Timeline
```

そのものが動画生成ディレクションになります。

---

# 19. Brush Engine

ブラシはRust + GPUで実装します。

```text
Input
 ↓
Stroke Sampling
 ↓
Smoothing
 ↓
Dynamics
 ↓
Brush Stamp
 ↓
Texture
 ↓
Blend
 ↓
GPU Composite
```

対応パラメータ：

```text
Size
Opacity
Flow
Spacing
Hardness
Rotation
Scatter
Texture

Pressure → Size
Pressure → Opacity
Tilt → Rotation
Velocity → Size
```

Apple Pencil / Wacomにも対応します。

---

# 20. ラスターを巨大1枚画像として持たない

高解像度漫画では重要です。

例えば

```text
8000 × 12000
```

を一枚のRGBAバッファにすると非常に重くなります。

そこで、

```text
Tile Engine

256×256
または
512×512
```

単位に分割します。

```text
Canvas

Tile Tile Tile
Tile Tile Tile
Tile Tile Tile
```

変更されたTileだけGPUへ送ります。

---

# 21. Sparse Tile

さらに、

```text
透明Tile
```

はメモリを持たせません。

例えば線画なら、

```text
10000 tiles

実データ
1800 tiles
```

程度になる可能性があります。

これで巨大キャンバスにも強くなります。

---

# 22. Undo/Redo

画像全体をコピーしてはいけません。

Command方式にします。

```text
Command

PaintStroke
TransformLayer
ChangeFilter
GenerateAI
EditPrompt
MovePanel
```

そして変更Tileだけ

```text
before
after
```

を保存。

AIの場合は

```text
Generation ID
Prompt
Result Asset
```

をUndo履歴にします。

---

# 23. RGB / CMYK

内部演算は基本的に

```text
Linear RGB
16-bit float
```

を推奨します。

つまり、

```text
RGBA16F
```

を内部標準にします。

理由はGPU演算との相性と非破壊処理です。

---

# 24. CMYK

CMYKを単なる

```text
RGB → CMYK
```

数式変換にしてはいけません。

ICC Profileを使います。

Rust側ではLittleCMSを利用する設計が現実的です。

```text
Working RGB
↓
ICC conversion
↓
CMYK profile
↓
Soft Proof
```

例えば

```text
Japan Color 2011 Coated
FOGRA39
GRACoL
```

など。

---

# 25. CMYKドキュメント

重要なのは、

```text
Document Color Space
```

と

```text
Display Color Space
```

を分離することです。

例えば

```text
Document:
CMYK / Japan Color

Processing:
Linear RGB float

Preview:
Display P3

Export:
CMYK TIFF
```

という処理も可能にします。


[1]: https://openai.com/index/introducing-chatgpt-images-2-5/?utm_source=chatgpt.com "Introducing ChatGPT Images 2.5 | OpenAI"
[2]: https://ai.google.dev/gemini-api/docs/image-generation?utm_source=chatgpt.com "Gemini API  |  Google AI for Developers"
[3]: https://developers.openai.com/api/reference/cli/resources/images/methods/generate?utm_source=chatgpt.com "Create image | OpenAI API Reference"
[4]: https://developers.openai.com/ja-JP/api/docs/guides/image-prompting?utm_source=chatgpt.com "画像のプロンプト | OpenAI API"
