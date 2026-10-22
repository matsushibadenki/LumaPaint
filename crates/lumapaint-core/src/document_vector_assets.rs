//! Page-local styles, static symbols and threaded text. Artwork stays in Rust.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub type TextLayout = fn(&mut VectorText, [u8; 3]) -> Result<(), String>;
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VectorAssets {
    #[serde(default)]
    next_id: u64,
    #[serde(default)]
    pub styles: Vec<VectorAsset>,
    #[serde(default)]
    pub symbols: Vec<VectorAsset>,
    #[serde(default)]
    pub threads: Vec<TextThread>,
    #[serde(default)]
    pub style_links: BTreeMap<String, String>,
    #[serde(default)]
    pub symbol_links: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VectorAsset {
    pub id: String,
    pub name: String,
    pub source: std::sync::Arc<VectorObject>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextThread {
    pub id: String,
    pub name: String,
    pub frames: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorAssetSummary {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub count: usize,
    pub selected: bool,
    pub overset: bool,
}
fn valid_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name != name.trim()
        || name.chars().count() > 100
        || name.chars().any(char::is_control)
    {
        return Err("Invalid asset name / 名前が不正です / 名称无效".into());
    }
    Ok(())
}
fn objects(layers: &[SvgLayer]) -> impl Iterator<Item = &VectorObject> {
    layers
        .iter()
        .filter(|l| l.vector_layer)
        .flat_map(|l| &l.vector_objects)
}
fn appearance_equal(a: &VectorObject, b: &VectorObject) -> bool {
    a.fill == b.fill
        && a.stroke == b.stroke
        && a.fill_gradient == b.fill_gradient
        && a.stroke_gradient == b.stroke_gradient
        && a.stroke_width == b.stroke_width
        && a.stroke_style == b.stroke_style
        && a.opacity == b.opacity
        && a.blend_mode == b.blend_mode
}
fn symbol_equal(a: &VectorObject, b: &VectorObject) -> bool {
    appearance_equal(a, b)
        && a.path == b.path
        && a.kind == b.kind
        && a.live_corners == b.live_corners
        && a.rectangle_radii == b.rectangle_radii
        && a.text.is_none()
        && b.text.is_none()
        && a.image_frame.is_none()
        && b.image_frame.is_none()
}
fn copy_appearance(to: &mut VectorObject, from: &VectorObject) {
    to.fill = from.fill;
    to.stroke = from.stroke;
    to.fill_gradient = from.fill_gradient.clone();
    to.stroke_gradient = from.stroke_gradient.clone();
    to.stroke_width = from.stroke_width;
    to.stroke_style = from.stroke_style.clone();
    to.opacity = from.opacity;
    to.blend_mode = from.blend_mode.clone();
    // Explicit per-character colors take precedence in text. Applying a style
    // changes those paint attributes without replacing font or text content.
    if let Some(text) = &mut to.text {
        text.no_color = from.fill.is_none() && from.fill_gradient.is_none();
        for run in &mut text.runs {
            run.style.no_color = text.no_color;
            if let Some(fill) = from.fill {
                run.style.color = [fill.color[0], fill.color[1], fill.color[2]];
            }
        }
    }
}
impl VectorAssets {
    pub(super) fn history_bytes(&self) -> usize {
        let links = self
            .style_links
            .iter()
            .chain(&self.symbol_links)
            .map(|(id, asset)| 96 + id.len() + asset.len())
            .sum::<usize>();
        let definitions = self
            .styles
            .iter()
            .chain(&self.symbols)
            .map(|a| {
                let o = &a.source;
                512 + a.id.len()
                    + a.name.len()
                    + o.path.data.len()
                    + o.control_points.len() * 8
                    + o.stroke_style.dash_array.len() * 4
                    + o.stroke_style.width_curve.len() * 12
                    + o.stroke_style.contour_alignments.len() * 4
                    + [&o.fill_gradient, &o.stroke_gradient]
                        .into_iter()
                        .flatten()
                        .map(|g| g.stops.len() * 32)
                        .sum::<usize>()
            })
            .sum::<usize>();
        links
            + definitions
            + self
                .threads
                .iter()
                .map(|t| {
                    128 + t.name.len() + t.frames.iter().map(|id| 24 + id.len()).sum::<usize>()
                })
                .sum::<usize>()
    }
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty() && self.symbols.is_empty() && self.threads.is_empty()
    }
    pub(super) fn validate(&self, layers: &[SvgLayer]) -> Result<(), String> {
        if self.styles.len() + self.symbols.len() + self.threads.len() > 128
            || self.style_links.len() + self.symbol_links.len() > 65536
            || serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > 2 * 1024 * 1024
        {
            return Err("Too many vector assets / 素材の上限を超えています / 矢量资源过多".into());
        }
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for asset in self.styles.iter().chain(&self.symbols) {
            valid_name(&asset.name)?;
            if !names.insert(asset.name.as_str()) {
                return Err("Asset name already exists / 同じ名前が存在します / 名称已存在".into());
            }
            asset.source.validate()?;
            if asset.id.is_empty() || asset.id.len() > 128 || !ids.insert(asset.id.as_str()) {
                return Err("Invalid asset identity".into());
            }
        }
        if self.symbols.iter().any(|s| {
            s.source.text.is_some()
                || s.source.image_frame.is_some()
                || s.source.clipping_group.is_some()
        }) {
            return Err("Symbols require a vector outline / シンボルにはベクター図形を指定してください / 符号需要矢量图形".into());
        }
        let mut frames = BTreeSet::new();
        let all: BTreeMap<_, _> = objects(layers).map(|o| (o.id.as_str(), o)).collect();
        for thread in &self.threads {
            valid_name(&thread.name)?;
            if !names.insert(thread.name.as_str()) {
                return Err("Asset name already exists / 同じ名前が存在します / 名称已存在".into());
            }
            if thread.id.is_empty()
                || thread.id.len() > 128
                || !ids.insert(thread.id.as_str())
                || thread.frames.len() < 2
                || thread.frames.len() > 64
            {
                return Err("Invalid text thread".into());
            }
            for id in &thread.frames {
                if !frames.insert(id.as_str())
                    || !all.get(id.as_str()).is_some_and(|o| {
                        o.text.as_ref().is_some_and(|t| {
                            !t.point_text
                                && t.box_height.is_some()
                                && t.writing_mode == crate::vector::WritingMode::Horizontal
                        })
                    })
                {
                    return Err("Text threads require distinct horizontal area frames / 横組みのエリア内文字フレームを指定してください / 请指定独立的横排区域文本框".into());
                }
            }
        }
        for (links, defs) in [
            (&self.style_links, &self.styles),
            (&self.symbol_links, &self.symbols),
        ] {
            if links.iter().any(|(id, asset)| {
                id.is_empty()
                    || id.len() > 200
                    || !all.contains_key(id.as_str())
                    || !defs.iter().any(|d| &d.id == asset)
            }) {
                return Err("Invalid vector asset link".into());
            }
        }
        Ok(())
    }
    pub(super) fn reconcile(&mut self, layers: &[SvgLayer]) {
        let all: BTreeMap<_, _> = objects(layers).map(|o| (o.id.as_str(), o)).collect();
        self.style_links.retain(|id, asset| {
            all.get(id.as_str()).is_some_and(|o| {
                self.styles
                    .iter()
                    .any(|s| &s.id == asset && appearance_equal(o, &s.source))
            })
        });
        self.symbol_links.retain(|id, asset| {
            all.get(id.as_str()).is_some_and(|o| {
                self.symbols
                    .iter()
                    .any(|s| &s.id == asset && symbol_equal(o, &s.source))
            })
        });
        for thread in &mut self.threads {
            thread.frames.retain(|id| {
                all.get(id.as_str()).is_some_and(|o| {
                    o.text.as_ref().is_some_and(|t| {
                        !t.point_text
                            && t.box_height.is_some()
                            && t.writing_mode == crate::vector::WritingMode::Horizontal
                    })
                })
            });
        }
        self.threads.retain(|t| t.frames.len() >= 2);
    }
    pub(super) fn matching_ids(&self, kind: &str, references: &[&str]) -> Option<BTreeSet<&str>> {
        let refs: BTreeSet<_> = references.iter().copied().collect();
        match kind {
            "graphicStyle" | "symbolInstance" => {
                let links = if kind == "graphicStyle" {
                    &self.style_links
                } else {
                    &self.symbol_links
                };
                let assets: BTreeSet<_> = refs.iter().filter_map(|id| links.get(*id)).collect();
                Some(
                    links
                        .iter()
                        .filter(|(_, id)| assets.contains(id))
                        .map(|(id, _)| id.as_str())
                        .collect(),
                )
            }
            "linkedText" => Some(
                self.threads
                    .iter()
                    .filter(|t| t.frames.iter().any(|id| refs.contains(id.as_str())))
                    .flat_map(|t| t.frames.iter().map(String::as_str))
                    .collect(),
            ),
            _ => None,
        }
    }
    pub(super) fn same(&self, kind: &str, a: &str, b: &str) -> bool {
        match kind {
            "graphicStyle" => self
                .style_links
                .get(a)
                .is_some_and(|id| self.style_links.get(b) == Some(id)),
            "symbolInstance" => self
                .symbol_links
                .get(a)
                .is_some_and(|id| self.symbol_links.get(b) == Some(id)),
            "linkedText" => self
                .threads
                .iter()
                .any(|t| t.frames.iter().any(|id| id == a) && t.frames.iter().any(|id| id == b)),
            _ => false,
        }
    }
}
impl Document {
    pub fn set_text_layout_backend(&mut self, layout: Option<TextLayout>) {
        self.text_layout_backend = layout;
    }
    pub(super) fn vector_assets_snapshot(&self) -> Vec<VectorAssetSummary> {
        let all: BTreeMap<_, _> = objects(&self.svg_layers)
            .map(|o| (o.id.as_str(), o))
            .collect();
        let mut result = Vec::new();
        for (kind, defs, links) in [
            (
                "graphicStyle",
                &self.vector_assets.styles,
                &self.vector_assets.style_links,
            ),
            (
                "symbolInstance",
                &self.vector_assets.symbols,
                &self.vector_assets.symbol_links,
            ),
        ] {
            for asset in defs {
                let members: Vec<_> = links
                    .iter()
                    .filter(|(id, a)| *a == &asset.id && all.contains_key(id.as_str()))
                    .map(|(id, _)| id)
                    .collect();
                result.push(VectorAssetSummary {
                    id: asset.id.clone(),
                    name: asset.name.clone(),
                    kind,
                    count: members.len(),
                    selected: members
                        .iter()
                        .any(|id| self.selected_vector_objects.contains(id)),
                    overset: false,
                });
            }
        }
        for thread in &self.vector_assets.threads {
            let last = thread
                .frames
                .last()
                .and_then(|id| all.get(id.as_str()))
                .and_then(|o| o.text.as_ref());
            result.push(VectorAssetSummary {
                id: thread.id.clone(),
                name: thread.name.clone(),
                kind: "linkedText",
                count: thread.frames.len(),
                selected: thread
                    .frames
                    .iter()
                    .any(|id| self.selected_vector_objects.contains(id)),
                overset: last.is_some_and(text_overset),
            });
        }
        result
    }
    fn asset_editable(&self, ids: &[String]) -> Result<(), String> {
        if ids.iter().any(|id| {
            !self.svg_layers.iter().any(|l| {
                l.vector_layer
                    && l.visible
                    && !l.locked
                    && self.can_edit_path_layer(l)
                    && l.vector_objects
                        .iter()
                        .any(|o| &o.id == id && o.visible && !self.object_is_locked(id))
            })
        }) {
            return Err("Asset members are locked, hidden or unavailable / 対象にロック・非表示・未対応のオブジェクトが含まれます / 对象被锁定、隐藏或不可用".into());
        }
        Ok(())
    }
    fn commit_assets(
        &mut self,
        mut layers: Vec<SvgLayer>,
        mut assets: VectorAssets,
    ) -> Result<(), String> {
        assets.reconcile(&layers);
        assets.validate(&layers)?;
        let mut bytes = 0;
        for (layer, old) in layers.iter_mut().zip(&self.svg_layers) {
            if layer.vector_objects != old.vector_objects {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
            bytes += layer.source.len();
        }
        if bytes > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        if layers
            .iter()
            .zip(&self.svg_layers)
            .all(|(a, b)| a.vector_objects == b.vector_objects && a.source == b.source)
            && assets == *self.vector_assets
        {
            return Ok(());
        }
        if layers
            .iter()
            .zip(&self.svg_layers)
            .all(|(a, b)| a.vector_objects == b.vector_objects && a.source == b.source)
        {
            let previous = std::mem::replace(&mut self.vector_assets, std::sync::Arc::new(assets));
            self.vector_undo
                .push(VectorHistoryEntry::VectorAssets(previous));
            self.undo_order.push(HistoryKind::Vector);
            self.vector_redo.clear();
            self.redo_order.clear();
            self.redo.clear();
            self.revision += 1;
            return Ok(());
        }
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.vector_assets = std::sync::Arc::new(assets);
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
    pub fn vector_asset_action(&mut self, request: VectorSelectionRequest) -> Result<(), String> {
        if self.path_editing.is_some() {
            return Err(
                "Finish path editing first / パスの編集を終了してください / 请先结束路径编辑"
                    .into(),
            );
        }
        let kind = request.criterion.as_deref().unwrap_or("");
        if !["graphicStyle", "symbolInstance", "linkedText"].contains(&kind) {
            return Err("Invalid asset kind".into());
        }
        if request.action == "assetSelect" {
            let id = request.name.as_deref().unwrap_or("");
            let exists = match kind {
                "graphicStyle" => self.vector_assets.styles.iter().any(|a| a.id == id),
                "symbolInstance" => self.vector_assets.symbols.iter().any(|a| a.id == id),
                _ => self.vector_assets.threads.iter().any(|a| a.id == id),
            };
            if !exists {
                return Err("Asset not found".into());
            }
            let ids = self
                .svg_layers
                .iter()
                .filter(|l| l.vector_layer && l.visible && !l.locked && self.can_edit_path_layer(l))
                .flat_map(|l| &l.vector_objects)
                .filter(|o| o.visible && !self.object_is_locked(&o.id))
                .filter(|o| match kind {
                    "graphicStyle" => self
                        .vector_assets
                        .style_links
                        .get(&o.id)
                        .is_some_and(|a| a == id),
                    "symbolInstance" => self
                        .vector_assets
                        .symbol_links
                        .get(&o.id)
                        .is_some_and(|a| a == id),
                    _ => self
                        .vector_assets
                        .threads
                        .iter()
                        .any(|t| t.id == id && t.frames.contains(&o.id)),
                })
                .map(|o| o.id.clone())
                .collect();
            return self.apply_vector_selection(ids);
        }
        let mut assets = self.vector_assets.as_ref().clone();
        let mut layers = self.svg_layers.clone();
        let selected = self.selected_vector_objects.clone();
        if ["assetCreate", "assetApply", "assetRedefine", "assetDetach"]
            .contains(&request.action.as_str())
        {
            self.asset_editable(&selected)?;
        }
        let refs: Vec<_> = objects(&layers)
            .filter(|o| selected.contains(&o.id))
            .cloned()
            .collect();
        let name = request.name.as_deref().unwrap_or("");
        let mut inserted = None;
        if request.action == "assetCreate" {
            valid_name(name)?;
            if (kind == "linkedText" && refs.len() < 2) || (kind != "linkedText" && refs.len() != 1)
            {
                return Err("Select one source object, or at least two text frames / 元のオブジェクト1個、または文字フレーム2個以上を選択してください / 请选择一个源对象或至少两个文本框".into());
            }
            if assets
                .styles
                .iter()
                .chain(&assets.symbols)
                .any(|a| a.name == name)
                || assets.threads.iter().any(|a| a.name == name)
            {
                return Err("Asset name already exists / 同じ名前が存在します / 名称已存在".into());
            }
            assets.next_id = assets
                .next_id
                .checked_add(1)
                .ok_or("Asset identity overflow")?;
            let id = format!("asset-{}", assets.next_id);
            if kind == "linkedText" {
                let frames: Vec<_> = refs.iter().map(|o| o.id.clone()).collect();
                if assets
                    .threads
                    .iter()
                    .any(|t| t.frames.iter().any(|id| frames.contains(id)))
                {
                    return Err("Unlink existing text threads first / 既存の文字の連結を解除してください / 请先取消现有文本链接".into());
                }
                assets.threads.push(TextThread {
                    id,
                    name: name.into(),
                    frames,
                });
                assets.validate(&layers)?;
                reflow_thread(
                    &mut layers,
                    assets.threads.last().unwrap(),
                    self.text_layout_backend
                        .ok_or("Text layout backend unavailable")?,
                )?;
            } else {
                let mut source = refs[0].clone();
                if kind == "graphicStyle" {
                    source.text = None;
                    source.image_frame = None;
                    source.group_path.clear();
                    source.clipping_group = None;
                    source.control_points.clear();
                    source.live_corners = None;
                    source.rectangle_radii = None;
                    source.kind = VectorObjectKind::Path;
                }
                let asset = VectorAsset {
                    id: id.clone(),
                    name: name.into(),
                    source: std::sync::Arc::new(source),
                };
                if kind == "graphicStyle" {
                    assets.style_links.insert(refs[0].id.clone(), id);
                    assets.styles.push(asset);
                } else {
                    assets.symbol_links.insert(refs[0].id.clone(), id);
                    assets.symbols.push(asset);
                }
            }
        } else if kind == "linkedText" {
            let index = assets
                .threads
                .iter()
                .position(|t| t.id == name)
                .ok_or("Text thread not found")?;
            let frames = assets.threads[index].frames.clone();
            if ["assetReverse", "assetApply"].contains(&request.action.as_str()) {
                self.asset_editable(&frames)?;
            }
            match request.action.as_str() {
                "assetRename" => {
                    let value = request.new_name.as_deref().unwrap_or("");
                    valid_name(value)?;
                    assets.threads[index].name = value.into();
                }
                "assetDelete" | "assetDetach" => {
                    assets.threads.remove(index);
                }
                "assetReverse" => {
                    assets.threads[index].frames.reverse();
                    reflow_thread_in_order(
                        &mut layers,
                        &assets.threads[index],
                        &frames,
                        self.text_layout_backend
                            .ok_or("Text layout backend unavailable")?,
                    )?;
                }
                "assetApply" => {
                    for o in refs {
                        if !frames.contains(&o.id) {
                            if assets.threads.iter().any(|t| t.frames.contains(&o.id)) {
                                return Err("Frame is already linked / 文字フレームは既に連結されています / 文本框已链接".into());
                            }
                            assets.threads[index].frames.push(o.id);
                        }
                    }
                    assets.validate(&layers)?;
                    reflow_thread(
                        &mut layers,
                        &assets.threads[index],
                        self.text_layout_backend
                            .ok_or("Text layout backend unavailable")?,
                    )?;
                }
                _ => return Err("Invalid text thread operation".into()),
            }
        } else {
            let (defs, links) = if kind == "graphicStyle" {
                (&mut assets.styles, &mut assets.style_links)
            } else {
                (&mut assets.symbols, &mut assets.symbol_links)
            };
            let index = defs
                .iter()
                .position(|a| a.id == name)
                .ok_or("Asset not found")?;
            match request.action.as_str() {
                "assetRename" => {
                    let value = request.new_name.as_deref().unwrap_or("");
                    valid_name(value)?;
                    defs[index].name = value.into();
                }
                "assetDelete" => {
                    defs.remove(index);
                    links.retain(|_, id| id != name);
                }
                "assetDetach" => links.retain(|id, asset| asset != name || !selected.contains(id)),
                "assetInsert" if kind == "symbolInstance" => {
                    let target_id = self
                        .selected_layer
                        .clone()
                        .filter(|id| {
                            layers
                                .iter()
                                .any(|l| &l.id == id && l.vector_layer && l.visible && !l.locked)
                        })
                        .or_else(|| self.editable_vector_layer_id())
                        .ok_or("Select an editable vector layer / 編集可能なベクターレイヤーを選択してください / 请选择可编辑矢量图层")?;
                    let target = layers.iter_mut().find(|l| l.id == target_id).unwrap();
                    if target.vector_objects.len() >= 4096 {
                        return Err("Vector layer is full / ベクターレイヤーの上限を超えています / 矢量图层已满".into());
                    }
                    let mut source = defs[index].source.as_ref().clone();
                    let mut serial = 1;
                    while self
                        .svg_layers
                        .iter()
                        .flat_map(|l| &l.vector_objects)
                        .any(|o| o.id == format!("symbol-{serial}"))
                    {
                        serial += 1;
                    }
                    source.id = format!("symbol-{serial}");
                    source.name = defs[index].name.clone();
                    source.group_path.clear();
                    source.clipping_group = None;
                    source.transform[4] += 24.;
                    source.transform[5] += 24.;
                    source.visible = true;
                    links.insert(source.id.clone(), name.into());
                    inserted = Some(source.id.clone());
                    target.vector_objects.push(source);
                }
                "assetApply" | "assetRedefine" => {
                    if request.action == "assetRedefine" {
                        if refs.len() != 1 {
                            return Err("Select one source object / 元のオブジェクトを1個選択してください / 请选择一个源对象".into());
                        }
                        defs[index].source = std::sync::Arc::new(refs[0].clone());
                    }
                    let ids = if request.action == "assetRedefine" {
                        links
                            .iter()
                            .filter(|(_, id)| id.as_str() == name)
                            .map(|(id, _)| id.clone())
                            .collect::<Vec<_>>()
                    } else {
                        if selected.is_empty() {
                            return Err("Select target objects / 対象のオブジェクトを選択してください / 请选择目标对象".into());
                        }
                        selected.clone()
                    };
                    self.asset_editable(&ids)?;
                    for o in layers
                        .iter_mut()
                        .flat_map(|l| &mut l.vector_objects)
                        .filter(|o| ids.contains(&o.id))
                    {
                        if kind == "graphicStyle" {
                            copy_appearance(o, &defs[index].source);
                        } else {
                            if o.text.is_some()
                                || o.image_frame.is_some()
                                || o.clipping_group.is_some()
                            {
                                return Err("Symbols require vector outlines / シンボルにはベクター図形を指定してください / 符号需要矢量图形".into());
                            }
                            let mut instance = defs[index].source.as_ref().clone();
                            instance.id = o.id.clone();
                            instance.name = o.name.clone();
                            instance.transform = o.transform;
                            instance.visible = o.visible;
                            instance.group_path = o.group_path.clone();
                            *o = instance;
                        }
                        links.insert(o.id.clone(), name.into());
                    }
                }
                _ => return Err("Invalid asset operation".into()),
            }
        }
        self.commit_assets(layers, assets)?;
        if let Some(id) = inserted {
            self.apply_vector_selection(vec![id])?;
        }
        Ok(())
    }
    pub(super) fn update_threaded_object(
        &mut self,
        layer_id: &str,
        object: VectorObject,
    ) -> Result<(), String> {
        let thread = self
            .vector_assets
            .threads
            .iter()
            .find(|t| t.frames.contains(&object.id))
            .ok_or("Text thread missing")?
            .clone();
        self.asset_editable(&thread.frames)?;
        let mut layers = self.svg_layers.clone();
        let slot = layers
            .iter_mut()
            .find(|l| l.id == layer_id)
            .and_then(|l| l.vector_objects.iter_mut().find(|o| o.id == object.id))
            .ok_or("Text frame missing")?;
        *slot = object;
        self.vector_assets.validate(&layers)?;
        reflow_thread(
            &mut layers,
            &thread,
            self.text_layout_backend
                .ok_or("Text layout backend unavailable")?,
        )?;
        self.commit_assets(layers, self.vector_assets.as_ref().clone())
    }
}
fn text_overset(t: &VectorText) -> bool {
    if t.content.is_empty() {
        return false;
    }
    let last = t.visual_lines().len().saturating_sub(1);
    let bottom = t
        .line_baselines
        .get(last)
        .copied()
        .unwrap_or((last + 1) as f32 * t.font_size * t.line_height)
        + t.font_size * 0.25;
    t.box_height.is_some_and(|h| bottom > h + 0.01)
}
fn slice_text(content: &str, start: usize, end: usize) -> Result<String, String> {
    let units: Vec<_> = content.encode_utf16().collect();
    String::from_utf16(units.get(start..end).ok_or("Invalid story range")?)
        .map_err(|_| "Invalid story boundary".into())
}
fn slice_runs(
    runs: &[crate::vector::TextRun],
    start: usize,
    end: usize,
) -> Vec<crate::vector::TextRun> {
    runs.iter()
        .filter_map(|r| {
            let a = r.start.max(start);
            let b = r.end.min(end);
            (a < b).then(|| crate::vector::TextRun {
                start: a - start,
                end: b - start,
                style: r.style.clone(),
            })
        })
        .collect()
}
fn reflow_thread(
    layers: &mut [SvgLayer],
    thread: &TextThread,
    layout: TextLayout,
) -> Result<(), String> {
    reflow_thread_in_order(layers, thread, &thread.frames, layout)
}
fn reflow_thread_in_order(
    layers: &mut [SvgLayer],
    thread: &TextThread,
    source_frames: &[String],
    layout: TextLayout,
) -> Result<(), String> {
    let mut story = String::new();
    let mut runs = Vec::new();
    let mut at = 0;
    for id in source_frames {
        let o = objects(layers)
            .find(|o| &o.id == id)
            .ok_or("Text frame missing")?;
        let t = o.text.as_ref().ok_or("Text frame missing")?;
        let color = o
            .fill
            .map_or([0, 0, 0], |p| [p.color[0], p.color[1], p.color[2]]);
        let mut starts = vec![0];
        for r in &t.runs {
            starts.extend([r.start, r.end]);
        }
        starts.push(t.content.encode_utf16().count());
        starts.sort_unstable();
        starts.dedup();
        for pair in starts.windows(2) {
            runs.push(crate::vector::TextRun {
                start: at + pair[0],
                end: at + pair[1],
                style: t.style_at(pair[0], color),
            });
        }
        at += t.content.encode_utf16().count();
        story.push_str(&t.content);
    }
    if at > 65536 {
        return Err(
            "Linked story is too long / 連結された文字数が上限を超えています / 链接文本过长".into(),
        );
    }
    let mut start = 0;
    for (index, id) in thread.frames.iter().enumerate() {
        let o = layers
            .iter_mut()
            .flat_map(|l| &mut l.vector_objects)
            .find(|o| &o.id == id)
            .ok_or("Text frame missing")?;
        let color = o
            .fill
            .map_or([0, 0, 0], |p| [p.color[0], p.color[1], p.color[2]]);
        let t = o.text.as_mut().ok_or("Text frame missing")?;
        let mut candidate = t.clone();
        candidate.content = slice_text(&story, start, at)?;
        candidate.runs = slice_runs(&runs, start, at);
        candidate.clear_measured_layout();
        layout(&mut candidate, color)?;
        let mut end = at;
        if index + 1 < thread.frames.len() {
            for (line, (offset, _, _)) in candidate.visual_lines().iter().enumerate() {
                let size = candidate.style_at(*offset, color).font_size;
                let bottom = candidate
                    .line_baselines
                    .get(line)
                    .copied()
                    .unwrap_or((line + 1) as f32 * candidate.font_size * candidate.line_height)
                    + size * 0.25;
                if bottom > candidate.box_height.unwrap_or(f32::INFINITY) + 0.01 {
                    end = start + offset;
                    break;
                }
            }
        }
        candidate.content = slice_text(&story, start, end)?;
        candidate.runs = slice_runs(&runs, start, end);
        candidate.clear_measured_layout();
        layout(&mut candidate, color)?;
        candidate.validate()?;
        *t = candidate;
        o.control_points = t.control_points();
        start = end;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn object(id: &str, color: [u8; 4]) -> VectorObject {
        serde_json::from_value(serde_json::json!({"id":id,"name":id,"visible":true,"path":{"data":"M0 0H20V20H0Z","fillRule":"nonZero"},"transform":[1.,0.,0.,1.,0.,0.],"fill":{"color":color},"stroke":null,"strokeWidth":0.,"kind":"rectangle"})).unwrap()
    }
    fn document() -> Document {
        let mut d = Document::default();
        let layer = d.add_vector_layer().unwrap();
        for id in ["a", "b", "c"] {
            d.upsert_vector_object(&layer, object(id, [255, 0, 0, 255]))
                .unwrap();
        }
        d.selected_vector_objects = vec!["a".into()];
        d
    }
    fn request(action: &str, kind: &str, name: &str) -> VectorSelectionRequest {
        VectorSelectionRequest {
            action: action.into(),
            criterion: Some(kind.into()),
            name: Some(name.into()),
            new_name: None,
            bulk: None,
        }
    }
    fn query(d: &mut Document, kind: &str) {
        d.vector_selection_action(request("same", kind, ""))
            .unwrap();
    }
    #[test]
    fn graphic_styles_use_identity_apply_detach_and_restore_history() {
        let mut d = document();
        d.vector_asset_action(request("assetCreate", "graphicStyle", "Red"))
            .unwrap();
        let id = d.vector_assets.styles[0].id.clone();
        d.selected_vector_objects = vec!["b".into()];
        d.vector_asset_action(request("assetApply", "graphicStyle", &id))
            .unwrap();
        let rev = d.revision();
        query(&mut d, "graphicStyle");
        assert_eq!(d.selected_vector_ids(), ["a", "b"]);
        assert_eq!(d.revision(), rev);
        let layer = d.svg_layers[0].id.clone();
        let mut b = d.svg_layers[0].vector_objects[1].clone();
        b.opacity = 0.5;
        d.upsert_vector_object(&layer, b).unwrap();
        d.selected_vector_objects = vec!["a".into()];
        query(&mut d, "graphicStyle");
        assert_eq!(d.selected_vector_ids(), ["a"]);
        d.undo();
        d.selected_vector_objects = vec!["a".into()];
        query(&mut d, "graphicStyle");
        assert_eq!(d.selected_vector_ids(), ["a", "b"]);
        d.redo();
        assert!(!d.vector_assets.style_links.contains_key("b"));
        let state =
            serde_json::from_slice(&serde_json::to_vec(&d.document_state()).unwrap()).unwrap();
        let loaded = Document::from_document_state(state).unwrap();
        assert_eq!(loaded.vector_assets, d.vector_assets);
        d.vector_asset_action(request("assetDelete", "graphicStyle", &id))
            .unwrap();
        assert!(d.vector_assets.styles.is_empty());
        assert_eq!(objects(&d.svg_layers).count(), 3);
    }
    #[test]
    fn symbols_insert_redefine_preserve_transforms_and_reject_locked_members() {
        let mut d = document();
        d.vector_asset_action(request("assetCreate", "symbolInstance", "Box"))
            .unwrap();
        let id = d.vector_assets.symbols[0].id.clone();
        d.vector_asset_action(request("assetInsert", "symbolInstance", &id))
            .unwrap();
        query(&mut d, "symbolInstance");
        assert_eq!(d.selected_vector_ids(), ["a", "symbol-1"]);
        let instance = d.svg_layers[0].vector_objects.last().unwrap().transform;
        d.svg_layers[0].vector_objects[2].path.data = "M0 0L40 0L0 30Z".into();
        d.selected_vector_objects = vec!["c".into()];
        d.vector_asset_action(request("assetRedefine", "symbolInstance", &id))
            .unwrap();
        assert_eq!(
            d.svg_layers[0].vector_objects[0].path.data,
            "M0 0L40 0L0 30Z"
        );
        assert_eq!(
            d.svg_layers[0].vector_objects.last().unwrap().transform,
            instance
        );
        d.locked_objects.insert("symbol-1".into());
        let before = serde_json::to_vec(&d.document_state()).unwrap();
        let rev = d.revision();
        assert!(d
            .vector_asset_action(request("assetRedefine", "symbolInstance", &id))
            .is_err());
        assert_eq!(d.revision(), rev);
        assert_eq!(serde_json::to_vec(&d.document_state()).unwrap(), before);
        d.selected_vector_objects = vec!["a".into()];
        query(&mut d, "symbolInstance");
        assert_eq!(d.selected_vector_ids(), ["a"]);
        d.vector_asset_action(request("assetDelete", "symbolInstance", &id))
            .unwrap();
        assert_eq!(objects(&d.svg_layers).count(), 4);
        assert!(d.vector_assets.symbol_links.is_empty());
    }
    fn layout(t: &mut VectorText, _color: [u8; 3]) -> Result<(), String> {
        t.reflow_soft_breaks_with(|s, _| Ok(s.chars().count() as f32 * 10.))
    }
    fn text_document() -> Document {
        let mut d = document();
        d.set_text_layout_backend(Some(layout));
        for (index, o) in d.svg_layers[0].vector_objects.iter_mut().enumerate() {
            o.kind = VectorObjectKind::Text;
            o.text = Some(VectorText {
                content: ["AB😀CDEF", "後続", ""][index].into(),
                point_text: false,
                font_size: 10.,
                line_height: 1.,
                box_width: 20.,
                box_height: Some(25.),
                ..Default::default()
            });
        }
        d.selected_vector_objects = vec!["a".into(), "b".into(), "c".into()];
        d
    }
    fn story(d: &Document) -> String {
        d.vector_assets.threads[0]
            .frames
            .iter()
            .map(|id| {
                objects(&d.svg_layers)
                    .find(|o| &o.id == id)
                    .unwrap()
                    .text
                    .as_ref()
                    .unwrap()
                    .content
                    .as_str()
            })
            .collect()
    }
    #[test]
    fn threads_flow_utf16_preserve_story_on_reorder_edit_unlink_and_reload() {
        let mut d = text_document();
        d.vector_asset_action(request("assetCreate", "linkedText", "Story"))
            .unwrap();
        let id = d.vector_assets.threads[0].id.clone();
        assert_eq!(story(&d), "AB😀CDEF後続");
        assert_eq!(
            d.svg_layers[0].vector_objects[0]
                .text
                .as_ref()
                .unwrap()
                .content,
            "AB😀"
        );
        d.selected_vector_objects = vec!["b".into()];
        query(&mut d, "linkedText");
        assert_eq!(d.selected_vector_ids(), ["a", "b", "c"]);
        d.vector_asset_action(request("assetReverse", "linkedText", &id))
            .unwrap();
        assert_eq!(story(&d), "AB😀CDEF後続");
        assert_eq!(d.vector_assets.threads[0].frames, ["c", "b", "a"]);
        let layer = d.svg_layers[0].id.clone();
        let mut o = d.svg_layers[0].vector_objects[2].clone();
        let t = o.text.as_mut().unwrap();
        t.content = "NEW😀".into();
        t.runs.clear();
        d.upsert_vector_object(&layer, o).unwrap();
        assert!(story(&d).starts_with("NEW😀"));
        let before = d.vector_assets.clone();
        let state =
            serde_json::from_slice(&serde_json::to_vec(&d.document_state()).unwrap()).unwrap();
        let mut loaded = Document::from_document_state(state).unwrap();
        assert_eq!(loaded.vector_assets, before);
        loaded.set_text_layout_backend(Some(layout));
        let pieces: Vec<_> = objects(&loaded.svg_layers)
            .map(|o| o.text.as_ref().unwrap().content.clone())
            .collect();
        loaded
            .vector_asset_action(request("assetDetach", "linkedText", &id))
            .unwrap();
        assert!(loaded.vector_assets.threads.is_empty());
        assert_eq!(
            objects(&loaded.svg_layers)
                .map(|o| o.text.as_ref().unwrap().content.clone())
                .collect::<Vec<_>>(),
            pieces
        );
        loaded.undo();
        assert_eq!(loaded.vector_assets, before);
    }
    #[test]
    fn vector_asset_styles_preserve_contour_specific_strokes() {
        let mut d = document();
        let layer = d.svg_layers[0].id.clone();
        let mut source = d.svg_layers[0].vector_objects[0].clone();
        source.kind = VectorObjectKind::Path;
        source.path.data = "M0 0H20V20H0Z M30 0H50V20H30Z".into();
        source.stroke = Some(VectorPaint {
            registration: false,
            color: [0, 0, 0, 255],
        });
        source.stroke_width = 2.;
        source.stroke_style.contour_alignments = vec![
            crate::stroke::StrokeAlignment::Center,
            crate::stroke::StrokeAlignment::Inside,
        ];
        d.upsert_vector_object(&layer, source).unwrap();
        d.vector_asset_action(request("assetCreate", "graphicStyle", "Contours"))
            .unwrap();
        let state = d.document_state();
        assert_eq!(
            state.vector_assets.styles[0]
                .source
                .stroke_style
                .contour_alignments
                .len(),
            2
        );
        Document::from_document_state(state).unwrap();
    }
    #[test]
    fn vector_asset_metadata_history_can_be_offloaded_and_restored() {
        struct Blob(Vec<u8>);
        impl crate::history_storage::HistoryBlob for Blob {
            fn read(&self) -> Result<Vec<u8>, String> {
                Ok(self.0.clone())
            }
        }
        struct Store;
        impl crate::history_storage::HistoryStore for Store {
            fn write(&self, bytes: &[u8]) -> Result<crate::history_storage::StoredHistory, String> {
                Ok(crate::history_storage::StoredHistory(std::sync::Arc::new(
                    Blob(bytes.to_vec()),
                )))
            }
        }
        let mut d = document();
        d.vector_asset_action(request("assetCreate", "graphicStyle", "Original"))
            .unwrap();
        let id = d.vector_assets.styles[0].id.clone();
        for name in ["One", "Two", "Three", "Four"] {
            let mut edit = request("assetRename", "graphicStyle", &id);
            edit.new_name = Some(name.into());
            d.vector_asset_action(edit).unwrap();
        }
        assert!(matches!(
            d.vector_undo.last(),
            Some(VectorHistoryEntry::VectorAssets(_))
        ));
        d.manage_history(&Store, 0, 100).unwrap();
        for _ in 0..4 {
            d.undo();
        }
        assert_eq!(d.vector_assets.styles[0].name, "Original");
        for _ in 0..4 {
            d.redo();
        }
        assert_eq!(d.vector_assets.styles[0].name, "Four");
    }
    #[test]
    fn invalid_assets_and_failed_layout_do_not_mutate_document() {
        let mut d = text_document();
        let before = serde_json::to_vec(&d.document_state()).unwrap();
        d.set_text_layout_backend(Some(|_, _| Err("layout failed".into())));
        assert!(d
            .vector_asset_action(request("assetCreate", "linkedText", "Story"))
            .is_err());
        assert_eq!(serde_json::to_vec(&d.document_state()).unwrap(), before);
        d.selected_vector_objects = vec!["a".into()];
        assert!(d
            .vector_asset_action(request("assetCreate", "symbolInstance", "Text symbol"))
            .is_err());
        let mut value = serde_json::to_value(d.document_state()).unwrap();
        value.as_object_mut().unwrap().remove("vectorAssets");
        let legacy: DocumentState = serde_json::from_value(value).unwrap();
        assert!(Document::from_document_state(legacy)
            .unwrap()
            .vector_assets
            .is_empty());
    }
}
