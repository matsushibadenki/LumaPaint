import type { Locale } from './i18n';
import type { MotionPreset } from './motion-types';
const groups:[string[],Record<Locale,string>][]=[
 [['fade','cut'],{ja:'不透明度を変化させて出現・退場します。',en:'Opacity controls the entrance or exit.', 'zh-CN':'改变不透明度以出现或退场。'}],
 [['slideX','slideY','travel','arc','orbit','zigzag','hop','follow','wave','marquee'],{ja:'位置を時間変化させます。移動量と遅延幅で動きを調整します。',en:'Position changes over time; distance and stagger control the motion.', 'zh-CN':'位置随时间变化，可调节位移和延迟范围。'}],
 [['scale','scaleX','scaleY','pulse','squash','jelly','weight'],{ja:'拡縮や変形で、弾み・呼吸・柔らかさを表現します。',en:'Scale and deformation express rebound, breathing or softness.', 'zh-CN':'通过缩放和形变表现弹跳、呼吸或柔软质感。'}],
 [['spin','swing','flipX','flipY','tilt','skew'],{ja:'支点を中心に回転・傾斜します。フリップは平面上への投影です。',en:'Rotate or tilt around the pivot. Flips project onto the artwork plane.', 'zh-CN':'围绕锚点旋转或倾斜，翻转投影在画面平面上。'}],
 [['anticipate','recoil','shake'],{ja:'予備動作・反動・減衰する揺れを加えます。',en:'Add anticipation, recoil or a decaying shake.', 'zh-CN':'加入预备动作、反冲或逐渐衰减的震动。'}],
 [['parallax','reorder','gather','scatter','camera','crane'],{ja:'オブジェクトの順序と支点を使い、集合・分散・カメラ移動を表現します。',en:'Object order and pivots control gathering, scattering or camera movement.', 'zh-CN':'利用对象顺序和锚点表现聚集、散开或镜头移动。'}],
 [['wipe','diagonal','iris','split','blinds','blocks','dissolve','clipWindow'],{ja:'マスクの範囲を変化させて作品を見せます。',en:'An animated mask reveals the artwork.', 'zh-CN':'通过动态遮罩逐渐显示画面。'}],
 [['blur','glow','sweep','hue','tint','desaturate','chromatic','noise','displace','shadow'],{ja:'ぼかし・色・光・歪みを時間変化させます。',en:'Blur, color, light or distortion changes over time.', 'zh-CN':'模糊、色彩、光线或扭曲随时间变化。'}],
 [['strokeDraw','strokeTrim','morph'],{ja:'パスの描画範囲や形状を変えます。モーフ先のパスを指定すると形状間を補間します。',en:'Animate path reveal or shape. A morph target interpolates between shapes.', 'zh-CN':'改变路径显示范围或形状，指定目标路径可在形状间插值。'}],
 [['repeat','particles','confetti','snow','embers','fireworks','coalesce'],{ja:'複製・残像・粒子を生成します。シードを固定すると同じ動きを再現できます。',en:'Generate copies, trails or particles; a fixed seed reproduces the motion.', 'zh-CN':'生成副本、残影或粒子，固定种子可重现相同运动。'}],
 [['typewriter','scramble','textSwap','counter','tracking','underline','highlight','caption','lines'],{ja:'文字内容・文字単位の遅延・組版を時間変化させます。',en:'Animate text content, character timing or layout.', 'zh-CN':'改变文字内容、逐字时序或排版。'}],
 [['audioPulse','audioDuck','beat','walk','blink','tail','hold'],{ja:'設定した周期に沿ってリズムや反復動作を再生します。',en:'Play rhythmic or repeated movement at the configured frequency.', 'zh-CN':'按照设定周期播放节奏或重复动作。'}],
 [['meshPlane','cube','depthStack','coverflow'],{ja:'立体メッシュを透視投影します。奥行き・カメラ距離・3軸の回転を調整します。',en:'Project a 3D mesh with perspective. Adjust depth, camera distance and rotation on three axes.','zh-CN':'以透视方式投影立体网格，可调节景深、镜头距离和三轴旋转。'}],
 [['fontAxes'],{ja:'可変フォントの実軸を動かします。フォントごとの対応軸と範囲を使います。',en:'Animate real variable-font axes within the selected font’s supported ranges.','zh-CN':'在所选字体支持的范围内调整真实可变字体轴。'}],
 [['kanjiStroke'],{ja:'KanjiVGの正しい画順と方向で、長さに応じて漢字を書きます。',en:'Draw kanji using canonical KanjiVG stroke order and direction, with timing based on stroke length.','zh-CN':'按KanjiVG标准笔顺和方向书写汉字，时长依据笔画长度。'}],
];
export function motionBehavior(preset:MotionPreset,locale:Locale):string {
 if(['tx-layered-depth','tx-motext-falloff'].includes(preset.id))return ({ja:'文字の輪郭に前面・背面・内側を含む側面を作ります。「立体押し出し」をオフにすると層の重なりに戻ります。輪郭化できない文字は層で描画します。',en:'Build front, back and inner sidewalls from glyph outlines. Turn off solid extrusion for stacked layers. Glyphs without supported outlines retain layers.','zh-CN':'根据字形轮廓生成前面、背面和内侧面。关闭立体挤出可使用堆叠层；无法转换轮廓的字符保留堆叠效果。'})[locale];
 const sentences=groups.filter(([operations])=>preset.operations.some(op=>operations.includes(op))).map(([,text])=>text[locale]);
 if(preset.actors?.length)sentences.push(({ja:'複数要素の反応は、登録順に押下・保持・反動のタイミングを適用します。',en:'Multiple elements follow authored press, hold and rebound timings in object order.','zh-CN':'多个元素按对象顺序应用按下、保持和反弹的时序。'})[locale]);
 return sentences.join(' ');
}
