import type { Locale } from './i18n';

/** Stable English prompt values; menu labels follow the workspace language. */
export type Localized = Record<Locale, string>;
export type PromptChoice = { value: string; label: Localized };
export type ChoiceGroup = { label: Localized; choices: PromptChoice[] };
const text = (en: string, ja: string, zh: string): Localized => ({ en, ja, 'zh-CN': zh });
const group = (en: string, ja: string, zh: string, rows: string): ChoiceGroup => ({
  label: text(en, ja, zh),
  choices: rows.trim().split('\n').map(row => {
    const [value, japanese = value, chinese = value] = row.split('|');
    return { value, label: text(value, japanese, chinese) };
  }),
});
const numeric = (values: string[]) => group('Exposure values', '露出値', '曝光值', values.join('\n'));

export const promptOptions = {
  style: [
    group('Film & photography', '映像・写真', '影视与摄影', `Cinematic still|映画のワンシーン|电影画面
Independent film|インディペンデント映画|独立电影
Film noir|フィルム・ノワール|黑色电影
Period drama|時代劇・歴史映画|古装历史剧
Science fiction cinema|SF映画|科幻电影
Television drama|テレビドラマ|电视剧
Documentary|ドキュメンタリー|纪录片
Music video|ミュージックビデオ|音乐视频
Commercial photography|広告写真|商业摄影
Editorial portrait|エディトリアル・ポートレート|编辑人像摄影
Fashion photography|ファッション写真|时尚摄影
Street photography|ストリート写真|街头摄影
Nature photography|自然・風景写真|自然风光摄影
Product photography|商品写真|产品摄影
Architectural photography|建築写真|建筑摄影`),
    group('Illustration & comics', 'イラスト・漫画', '插画与漫画', `Editorial illustration|エディトリアルイラスト|编辑插画
Children's book illustration|絵本イラスト|绘本插画
Concept art|コンセプトアート|概念艺术
Fashion illustration|ファッションイラスト|时装插画
Monochrome manga with screentones|モノクロ漫画・スクリーントーン|黑白漫画与网点
Color manga|カラー漫画|彩色漫画
American comic book|アメリカンコミック|美式漫画
European graphic novel|バンド・デシネ|欧式漫画
Webtoon|ウェブトゥーン|条漫
Anime key visual|アニメのキービジュアル|动画主视觉
Cel animation|セルアニメ|赛璐璐动画
Storyboarding|ストーリーボード|分镜稿
Ink line art|インクの線画|墨线稿
Technical illustration|テクニカルイラスト|技术插图`),
    group('Art & design', '絵画・デザイン', '绘画与设计', `Watercolor painting|水彩画|水彩画
Oil painting|油絵|油画
Gouache painting|ガッシュ画|水粉画
Japanese woodblock print|木版画|木版画
Ink wash painting|水墨画|水墨画
Pencil sketch|鉛筆スケッチ|铅笔素描
Pastel drawing|パステル画|粉彩画
Charcoal drawing|木炭画|炭笔画
Flat vector illustration|フラットなベクターイラスト|扁平矢量插画
Pixel art|ピクセルアート|像素画
Paper cutout collage|切り絵・コラージュ|剪纸拼贴
Clay stop-motion|クレイアニメ|黏土定格动画
Isometric 3D illustration|アイソメトリック3D|等距3D插画
Photorealistic 3D render|写実的な3Dレンダー|写实3D渲染`),
  ],
  camera: [
    group('Digital & cinema', 'デジタル・シネマ', '数码与电影摄影机', `Full-frame DSLR|フルサイズ一眼レフ|全画幅单反
Full-frame mirrorless|フルサイズミラーレス|全画幅微单
APS-C camera|APS-Cカメラ|APS-C相机
Micro Four Thirds camera|マイクロフォーサーズ|微型四分之三相机
Digital medium format|デジタル中判|数码中画幅
Digital cinema camera|デジタルシネマカメラ|数字电影摄影机
Large-format cinema camera|ラージフォーマット・シネマ|大画幅电影摄影机
Compact digital camera|コンパクトデジタル|便携数码相机
Smartphone camera|スマートフォン|智能手机相机
Action camera|アクションカメラ|运动相机
Drone camera|ドローンカメラ|无人机相机
Vintage camcorder|レトロなビデオカメラ|复古摄像机`),
    group('Film & specialty', 'フィルム・特殊カメラ', '胶片与特殊相机', `35mm film SLR|35mmフィルム一眼レフ|35mm胶片单反
35mm rangefinder|35mmレンジファインダー|35mm旁轴相机
Half-frame film camera|ハーフサイズカメラ|半格胶片相机
6x6 medium-format film|6×6中判フィルム|6×6中画幅胶片
6x7 medium-format film|6×7中判フィルム|6×7中画幅胶片
4x5 large-format camera|4×5大判カメラ|4×5大画幅相机
8x10 large-format camera|8×10大判カメラ|8×10大画幅相机
Twin-lens reflex|二眼レフ|双镜头反光相机
Instant film camera|インスタントカメラ|即时成像相机
Pinhole camera|ピンホールカメラ|针孔相机
Super 8 film camera|スーパー8フィルム|超8毫米电影摄影机
16mm film camera|16mm映画カメラ|16毫米电影摄影机
35mm motion-picture camera|35mm映画カメラ|35毫米电影摄影机
Disposable film camera|レンズ付きフィルム|一次性胶片相机`),
  ],
  film: [
    group('Color film looks', 'カラーフィルムの表現', '彩色胶片风格', `Kodak Portra 160
Kodak Portra 400
Kodak Portra 800
Kodak Ektar 100
Kodak Gold 200
Kodak ColorPlus 200
Kodak Ultramax 400
Fujifilm Superia 400
Fujifilm Pro 400H
Fujifilm Velvia 50
Fujifilm Velvia 100
Fujifilm Provia 100F
Kodak Ektachrome E100
Kodachrome-inspired color|コダクローム調|柯达克罗姆风格
CineStill 800T
CineStill 50D`),
    group('Monochrome & motion picture', 'モノクロ・映画用', '黑白与电影胶片', `Kodak Tri-X 400
Kodak T-Max 100
Kodak T-Max 400
Ilford HP5 Plus
Ilford FP4 Plus
Ilford Delta 100
Ilford Delta 3200
Fujifilm Acros II
Kodak Vision3 250D
Kodak Vision3 500T
Kodak Double-X
Vintage faded film|退色したヴィンテージフィルム|褪色复古胶片
Cross-processed film|クロスプロセス|交叉冲洗胶片
Infrared film|赤外線フィルム|红外胶片
Instant film look|インスタントフィルム調|即时成像胶片风格
Clean digital, no film grain|クリーンなデジタル・粒子なし|纯净数码，无颗粒`),
  ],
  lens: [
    group('Focal length', '焦点距離', '焦距', `8mm fisheye|8mm 魚眼|8mm 鱼眼
14mm ultra-wide|14mm 超広角|14mm 超广角
18mm ultra-wide|18mm 超広角|18mm 超广角
24mm wide-angle|24mm 広角|24mm 广角
28mm wide-angle|28mm 広角|28mm 广角
35mm documentary lens|35mm スナップ|35mm 纪实
40mm normal lens|40mm 標準|40mm 标准
50mm prime|50mm 単焦点|50mm 定焦
58mm portrait lens|58mm ポートレート|58mm 人像
85mm portrait lens|85mm ポートレート|85mm 人像
100mm macro|100mm マクロ|100mm 微距
105mm portrait lens|105mm ポートレート|105mm 人像
135mm telephoto|135mm 望遠|135mm 长焦
200mm telephoto|200mm 望遠|200mm 长焦
300mm super-telephoto|300mm 超望遠|300mm 超长焦
600mm super-telephoto|600mm 超望遠|600mm 超长焦`),
    group('Optical character', '光学特性', '光学特性', `Anamorphic cinema lens|アナモルフィック|变形宽银幕镜头
Vintage uncoated lens|オールドレンズ・無コーティング|复古无镀膜镜头
Soft-focus portrait lens|ソフトフォーカス|柔焦人像镜头
Tilt-shift lens|ティルトシフト|移轴镜头
Probe macro lens|プローブマクロ|探针微距镜头
Petzval swirly-bokeh lens|ペッツバール・渦巻きぼけ|佩兹伐旋转散景镜头
Pancake lens|パンケーキレンズ|饼干镜头
24–70mm zoom|24–70mm 標準ズーム|24–70mm 标准变焦
70–200mm zoom|70–200mm 望遠ズーム|70–200mm 长焦变焦
Split diopter|スプリット・ディオプター|分裂式近摄镜
Diffusion-filter lens|拡散フィルター付き|带柔光滤镜镜头`),
  ],
  lighting: [
    group('Natural light', '自然光', '自然光', `Soft window light|柔らかい窓明かり|柔和窗光
Overcast daylight|曇天の拡散光|阴天漫射光
Direct midday sunlight|真昼の直射日光|正午直射阳光
Golden hour|ゴールデンアワー|黄金时刻
Blue hour|ブルーアワー|蓝调时刻
Dappled forest light|木漏れ日|林间斑驳光
Moonlight|月明かり|月光
Sunrise mist|朝霧に差す光|晨雾光线
Sunset glow|夕焼けの光|落日余晖
Storm light|嵐の合間の光|暴风雨间隙光线`),
    group('Studio & cinematic', 'スタジオ・映画照明', '影棚与电影灯光', `Large softbox|大型ソフトボックス|大型柔光箱
Beauty dish|ビューティーディッシュ|雷达柔光罩
Ring light|リングライト|环形灯
Three-point lighting|三点照明|三点布光
Rembrandt lighting|レンブラントライティング|伦勃朗光
Butterfly lighting|バタフライライティング|蝴蝶光
Split lighting|スプリットライティング|分割光
High-key studio|ハイキー・スタジオ|高调影棚光
Low-key studio|ローキー・スタジオ|低调影棚光
Hard spotlight|硬いスポットライト|硬质聚光灯
Bounce flash|バウンスフラッシュ|反射闪光
Direct on-camera flash|オンカメラ直射フラッシュ|机顶直射闪光
Neon practical lights|ネオン照明|霓虹实景灯
Candlelight|キャンドルライト|烛光
Tungsten practical lights|タングステン照明|钨丝灯光
Volumetric light beams|ボリューメトリック・光芒|体积光束
Colored gels|カラーフィルター照明|色片灯光
Screen glow|モニターからの光|屏幕光
Silhouette lighting|シルエット照明|剪影照明`),
  ],
  lightDirection: [group('Position & contrast', '光の位置・コントラスト', '光位与对比', `Front light|正面光|正面光
45-degree side light|斜め45度からの光|45度侧光
Side light|真横からの光|侧光
Back light|逆光|逆光
Rim light|輪郭を縁取る光|轮廓光
Top light|真上からの光|顶光
Underlighting|下からの光|底光
Cross lighting|クロスライティング|交叉光
Soft wraparound light|包み込む柔らかい光|包围式柔光
Hard directional light|方向性の強い硬い光|强方向硬光
Balanced key and fill|主光と補助光を均等に|主光与补光均衡
Strong key, minimal fill|強い主光・弱い補助光|强主光，弱补光
Negative fill, deep shadows|ネガティブフィル・深い影|负补光，深阴影
Even diffuse light|均一な拡散光|均匀漫射光`)],
  aperture:[numeric(['f/0.95','f/1.2','f/1.4','f/1.8','f/2','f/2.8','f/3.5','f/4','f/5.6','f/8','f/11','f/16','f/22','f/32','f/45','f/64'])],
  shutter:[numeric(['30 s','15 s','8 s','4 s','2 s','1 s','1/2 s','1/4 s','1/8 s','1/15 s','1/24 s','1/30 s','1/48 s','1/60 s','1/100 s','1/125 s','1/250 s','1/500 s','1/1000 s','1/2000 s','1/4000 s','1/8000 s'])],
  iso:[numeric(['25','50','64','80','100','125','160','200','250','320','400','500','640','800','1000','1250','1600','2000','2500','3200','6400','12800','25600'])],
  bokeh:[group('Depth & shape', '深度・ぼけの形', '景深与散景形状', `Deep focus, everything sharp|パンフォーカス|全景深，整体清晰
Moderate depth of field|中程度の被写界深度|中等景深
Shallow depth of field|浅い被写界深度|浅景深
Extremely shallow depth of field|極浅の被写界深度|极浅景深
Creamy background bokeh|クリーミーな背景ぼけ|奶油般背景散景
Soft circular bokeh|柔らかい円形ぼけ|柔和圆形散景
Polygonal aperture bokeh|多角形の絞りぼけ|多边形光圈散景
Anamorphic oval bokeh|アナモルフィックの楕円ぼけ|变形镜头椭圆散景
Swirly bokeh|ぐるぐるぼけ|旋转散景
Cat-eye bokeh|口径食・猫目ぼけ|猫眼散景
Donut bokeh|リングぼけ|甜甜圈散景
Foreground blur|前ぼけ|前景虚化
Background blur only|背景のみぼかす|仅背景虚化
Miniature tilt-shift blur|ミニチュア風ぼけ|微缩移轴虚化
Motion blur|モーションブラー|运动模糊
Panning blur|流し撮りのぼけ|追随拍摄模糊
Soft diffusion glow|柔らかい拡散光のにじみ|柔和漫射光晕`)],
  composition:[group('Arrangement', '画面構成', '画面构图', `Rule of thirds|三分割構図|三分法构图
Centered composition|中央配置|居中构图
Symmetrical composition|左右対称|对称构图
Golden ratio composition|黄金比構図|黄金比例构图
Golden spiral|黄金螺旋|黄金螺旋
Diagonal composition|対角線構図|对角线构图
Leading lines|リーディングライン|引导线
Frame within a frame|額縁構図|框架式构图
Triangular composition|三角構図|三角形构图
S-curve composition|S字構図|S形构图
Strong negative space|余白を大きく取る|大面积留白
Layered foreground and background|前景・中景・背景を重ねる|前中后景层次
Asymmetrical balance|非対称のバランス|非对称平衡
Pattern and repetition|パターンと反復|图案与重复
Minimalist composition|ミニマル構図|极简构图
Dynamic off-center subject|被写体を大胆にずらす|主体大胆偏离中心
Copy space on the left|左側に文字用の余白|左侧文字留白
Copy space on the right|右側に文字用の余白|右侧文字留白`)],
  viewpoint:[group('Camera angle', 'カメラアングル', '拍摄角度', `Eye-level view|目線の高さ|平视
Low-angle view|ローアングル|低角度
High-angle view|ハイアングル|高角度
Bird's-eye view|俯瞰・鳥瞰|鸟瞰
Top-down flat lay|真俯瞰・フラットレイ|垂直俯拍
Worm's-eye view|地面すれすれから見上げる|贴地仰视
Dutch angle|ダッチアングル・傾き|荷兰角倾斜
Over-the-shoulder view|肩越し|过肩视角
First-person perspective|一人称視点|第一人称视角
Three-quarter view|斜め45度|四分之三视角
Profile view|真横から|侧面
Rear view|背後から|背面
Aerial oblique view|空撮・斜め俯瞰|斜向航拍
Isometric view|アイソメトリック|等距视角
Orthographic front view|正投影・正面図|正交正视图
Underwater viewpoint|水中からの視点|水下视角`)],
  framing:[group('Shot size', 'ショットサイズ', '景别', `Extreme wide shot|超ロングショット|大远景
Wide establishing shot|情景を説明するロングショット|全景定场镜头
Full body shot|全身|全身镜头
Knee-up shot|膝上|膝上镜头
Medium shot, waist up|腰上・ミディアム|中景，腰部以上
Medium close-up|胸上|中近景
Close-up portrait|顔のクローズアップ|面部特写
Extreme close-up|極端なクローズアップ|大特写
Macro detail|マクロのディテール|微距细节
Two-person shot|ツーショット|双人镜头
Group shot|集合ショット|群像镜头
Environmental portrait|環境を含むポートレート|环境人像
Still-life arrangement|静物の配置|静物布置
Product hero shot|商品のヒーローショット|产品主视觉
Panoramic scene|パノラマの情景|全景场景`)],
  colorTone:[group('Palette & grading', '配色・カラーグレーディング', '配色与调色', `Natural neutral colors|自然でニュートラル|自然中性色
Warm amber tones|暖かい琥珀色|温暖琥珀色
Cool blue tones|クールな青系|冷蓝色调
Teal and orange|ティール＆オレンジ|青橙色调
Soft pastel palette|柔らかいパステル|柔和粉彩
Muted earth tones|落ち着いたアースカラー|低饱和大地色
Vivid primary colors|鮮やかな原色|鲜艳原色
Low-saturation colors|低彩度|低饱和色
High-saturation colors|高彩度|高饱和色
Black and white|モノクロ|黑白
Sepia tone|セピア|棕褐色调
Duotone|デュオトーン|双色调
Monochromatic blue|ブルーの単色調|蓝色单色调
Complementary colors|補色配色|互补色
Analogous colors|類似色配色|邻近色
Vintage faded colors|退色したヴィンテージカラー|褪色复古色
Bleach bypass|銀残し・ブリーチバイパス|漂白旁路
Soft matte blacks|マットな黒|柔和哑光黑
High-contrast color grading|高コントラストの色調|高对比调色
Neon cyberpunk palette|ネオン・サイバーパンク|霓虹赛博朋克
Warm highlights, cool shadows|暖色のハイライト・寒色の影|暖高光冷阴影`)],
  mood:[group('Atmosphere', '雰囲気', '氛围', `Serene and peaceful|静謐・穏やか|宁静平和
Dreamlike and ethereal|夢幻的・幻想的|梦幻空灵
Nostalgic|ノスタルジック|怀旧
Joyful and playful|明るく遊び心がある|欢快俏皮
Romantic and intimate|ロマンチック・親密|浪漫亲密
Melancholic|メランコリック|忧郁
Mysterious|ミステリアス|神秘
Tense and suspenseful|緊張感・サスペンス|紧张悬疑
Epic and majestic|壮大・荘厳|恢宏壮丽
Gritty and raw|荒々しく生々しい|粗粝真实
Elegant and refined|優雅・洗練|优雅精致
Cozy and inviting|温かく居心地がよい|温馨舒适
Lonely and contemplative|孤独・内省的|孤独沉思
Energetic and dynamic|エネルギッシュ・躍動的|活力动感
Surreal and uncanny|シュール・不思議|超现实奇异
Dark and ominous|暗く不穏|阴暗不祥
Hopeful and uplifting|希望に満ちた|充满希望
Whimsical and fantastical|空想的・ファンタジー|奇趣幻想`)],
  texture:[group('Surface & finish', '表面・仕上げ', '表面与质感', `Clean smooth finish|クリーンで滑らか|干净平滑
Fine film grain|細かなフィルム粒子|细腻胶片颗粒
Coarse film grain|粗いフィルム粒子|粗颗粒胶片
Matte paper texture|マットな紙の質感|哑光纸张质感
Watercolor paper texture|水彩紙の質感|水彩纸纹理
Canvas weave|キャンバスの織り目|画布织纹
Thick impasto paint|厚塗りの絵の具|厚涂油彩
Visible brush strokes|筆跡を残す|可见笔触
Ink bleeding|インクのにじみ|墨水渗化
Pencil hatching|鉛筆のハッチング|铅笔排线
Halftone dots|網点・ハーフトーン|半调网点
Risograph print texture|リソグラフ印刷|孔版印刷质感
Screen print texture|シルクスクリーン|丝网印刷质感
Glossy polished surface|光沢のある仕上げ|光亮抛光表面
Rough weathered surface|風化した粗い質感|粗糙风化表面
Analog video noise|アナログ映像のノイズ|模拟视频噪点
Embossed paper|エンボス紙|压纹纸
Handmade paper fibers|手すき紙の繊維|手工纸纤维`)],
  environment:[group('Setting', '場所・背景', '场景与背景', `Seamless studio backdrop|シームレスなスタジオ背景|无缝影棚背景
Minimal white background|ミニマルな白背景|极简白色背景
Dark studio background|暗いスタジオ背景|暗色影棚背景
Urban street|都市の通り|城市街道
Neon-lit city at night|ネオン街の夜|霓虹城市夜景
Quiet residential neighborhood|静かな住宅街|安静住宅区
Traditional Japanese street|日本の古い街並み|日本传统街道
Modern interior|モダンな室内|现代室内
Rustic interior|素朴な室内|质朴室内
Industrial warehouse|工場・倉庫|工业仓库
Lush forest|緑豊かな森|葱郁森林
Mountain landscape|山岳風景|山地风景
Coastline and ocean|海岸・海|海岸与海洋
Desert landscape|砂漠|沙漠
Snow-covered landscape|雪景色|雪景
Flower garden|花園|花园
Rainy city|雨の街|雨中城市
Foggy landscape|霧の風景|雾中风景
Fantasy world|ファンタジー世界|幻想世界
Futuristic city|未来都市|未来城市`)],
} satisfies Record<string, ChoiceGroup[]>;
export type PromptField = keyof typeof promptOptions;
export const promptSections: { title: Localized; fields: PromptField[] }[] = [
  { title:text('Look & atmosphere','画風・色・質感','风格、色彩与质感'),fields:['style','colorTone','mood','texture'] },
  { title:text('Composition & setting','構図・視点・背景','构图、视角与背景'),fields:['composition','viewpoint','framing','environment'] },
  { title:text('Camera & optics','カメラ・フィルム・レンズ','相机、胶片与镜头'),fields:['camera','film','lens','bokeh'] },
  { title:text('Lighting & exposure','照明・露出','灯光与曝光'),fields:['lighting','lightDirection','aperture','shutter','iso'] },
];
export const promptFieldLabels: Record<PromptField, Localized> = {
  style:text('Generation style','生成タイプ','生成类型'), colorTone:text('Color palette','色調・配色','色调与配色'), mood:text('Mood','雰囲気','氛围'), texture:text('Texture & finish','質感・仕上げ','质感与表面'),
  composition:text('Composition','構図','构图'),viewpoint:text('Viewpoint','視点・アングル','视角'),framing:text('Framing','画角・ショットサイズ','景别'),environment:text('Setting','背景・場所','背景与场景'),
  camera:text('Camera type','カメラタイプ','相机类型'),film:text('Film look','フィルムタイプ','胶片类型'),lens:text('Lens type','レンズタイプ','镜头类型'),bokeh:text('Depth & bokeh','被写界深度・ぼけ','景深与散景'),
  lighting:text('Lighting','照明','照明'),lightDirection:text('Light direction','光の方向・強さ','光的方向与强度'),aperture:text('Aperture','絞り','光圈'),shutter:text('Shutter speed','シャッタースピード','快门速度'),iso:text('ISO','感度（ISO）','感光度（ISO）'),
};
export const promptFields = Object.keys(promptOptions) as PromptField[];
export const emptyConditions = Object.fromEntries(promptFields.map(field=>[field,''])) as Record<PromptField,string>;
export type AiPrompt = Record<PromptField,string> & { name:string; text:string };
export const emptyPrompt:AiPrompt = { ...emptyConditions, name:'', text:'' };
