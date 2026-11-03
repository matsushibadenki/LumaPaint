//! Atomic conversion of selected artwork; renderers return ordinary document objects.
use super::*;
impl Document {
    pub fn convert_selected_objects(
        &mut self,
        combine: bool,
        mut convert: impl FnMut(&str, &[VectorObject], u32) -> Result<Vec<VectorObject>, String>,
    ) -> Result<(), String> {
        if self.has_active_saved_path() || self.selected_vector_objects.is_empty() {
            return Err("Select artwork / オブジェクトを選択してください / 请选择对象".into());
        }
        let selected = &self.selected_vector_objects;
        let mut layers = self.svg_layers.clone();
        let mut occupied: std::collections::HashSet<String> = layers
            .iter()
            .flat_map(|l| {
                l.vector_objects
                    .iter()
                    .flat_map(|o| std::iter::once(o.id.clone()).chain(o.group_path.iter().cloned()))
            })
            .collect();
        occupied.extend(self.reserved_compound_ids());
        let mut serial = 0;
        let mut fresh = || loop {
            serial += 1;
            let id = format!("converted-{}-{serial}", self.revision);
            if occupied.insert(id.clone()) {
                break id;
            }
        };
        let mut result_ids = Vec::new();
        let affected: Vec<_> = layers
            .iter()
            .filter(|l| l.vector_objects.iter().any(|o| selected.contains(&o.id)))
            .collect();
        if affected.is_empty() || (combine && affected.len() != 1) {
            return Err("Select objects in one vector layer / 同じベクターレイヤーのオブジェクトを選択してください / 请选择同一矢量图层中的对象".into());
        }
        for layer in &mut layers {
            let indices: Vec<_> = layer
                .vector_objects
                .iter()
                .enumerate()
                .filter(|(_, o)| selected.contains(&o.id))
                .map(|(i, _)| i)
                .collect();
            if indices.is_empty() {
                continue;
            }
            if !layer.vector_layer || layer.locked || !layer.visible {
                return Err("Unlock and show the vector layer / ベクターレイヤーを表示しロックを解除してください / 请显示并解锁矢量图层".into());
            }
            for &i in &indices {
                let object = &layer.vector_objects[i];
                if object.blend_mode != "normal" {
                    return Err("Use Normal blending before conversion / 描画モードを通常にしてから変換してください / 请将混合模式设为正常后再转换".into());
                }
                if !object.visible
                    || self.object_is_locked(&object.id)
                    || self.is_publication_text_frame(&object.id)
                    || object.clipping_group.is_some()
                    || layer.vector_objects.iter().any(|o| {
                        o.clipping_group
                            .as_ref()
                            .is_some_and(|g| object.group_path.contains(g))
                    })
                {
                    return Err("Release clipping and unlock selected objects first / クリッピングとロックを解除してから変換してください / 请先释放剪切并解锁所选对象".into());
                }
            }
            if combine
                && (indices.windows(2).any(|p| p[1] != p[0] + 1)
                    || indices.iter().any(|&i| {
                        layer.vector_objects[i].group_path
                            != layer.vector_objects[indices[0]].group_path
                    }))
            {
                return Err("Select consecutive objects in the same group / 同じグループ内で重ね順が連続したオブジェクトを選択してください / 请选择同一组中堆叠顺序连续的对象".into());
            }
            let batches: Vec<Vec<usize>> = if combine {
                vec![indices]
            } else {
                indices.into_iter().map(|i| vec![i]).collect()
            };
            // Replace from the end so original stacking positions stay stable.
            for batch in batches.into_iter().rev() {
                let originals: Vec<_> = batch
                    .iter()
                    .map(|&i| layer.vector_objects[i].clone())
                    .collect();
                let sources: Vec<_> = originals
                    .iter()
                    .cloned()
                    .map(|mut o| {
                        o.group_path.clear();
                        o
                    })
                    .collect();
                let svg = vector_svg(self.width, self.height, &sources);
                let mut replacements = convert(&svg, &originals, self.resolution)?;
                if replacements.is_empty() {
                    return Err("Conversion produced no objects / 変換可能な内容がありません / 无可转换内容".into());
                }
                let group = fresh();
                for replacement in &mut replacements {
                    replacement.id = fresh();
                    replacement.group_path = originals[0].group_path.clone();
                    replacement.group_path.push(group.clone());
                    replacement.clipping_group = None;
                    replacement.validate()?;
                    result_ids.push(replacement.id.clone());
                }
                layer
                    .vector_objects
                    .splice(batch[0]..=batch[batch.len() - 1], replacements);
            }
            if layer.vector_objects.len() > 4096 {
                return Err("Too many converted objects / 変換後のオブジェクト数が上限を超えます / 转换后的对象数量超出限制".into());
            }
            layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
            validate_svg_layer(layer)?;
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Converted artwork exceeds project limits / 変換データが上限を超えます / 转换数据超出项目限制".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.selected_vector_objects = result_ids;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
}
