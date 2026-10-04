//! Editable pathfinder recipes; the resolved path stays portable to all renderers.
use super::*;
use crate::vector::PathfinderOperation;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompoundShape {
    pub id: String,
    pub operation: PathfinderOperation,
    pub operands: Vec<VectorObject>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompoundShapeSnapshot {
    pub id: String,
    pub operation: PathfinderOperation,
    pub operands: Vec<CompoundOperandSnapshot>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompoundOperandSnapshot {
    pub id: String,
    pub name: String,
    pub transform: [f32; 6],
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompoundShapeEdit {
    pub action: String,
    pub operation: Option<PathfinderOperation>,
    pub operand: Option<String>,
    pub translation: Option<[f32; 2]>,
}
fn shape_mode(operation: PathfinderOperation) -> bool {
    matches!(
        operation,
        PathfinderOperation::Unite
            | PathfinderOperation::MinusFront
            | PathfinderOperation::Intersect
            | PathfinderOperation::Exclude
            | PathfinderOperation::MinusBack
    )
}
pub(super) fn validate_compound_shapes(
    shapes: &[CompoundShape],
    layers: &[SvgLayer],
) -> Result<(), String> {
    if shapes.len() > 4096 {
        return Err("Too many compound shapes".into());
    }
    let mut ids = std::collections::HashSet::new();
    let mut bytes = 0;
    let mut owned_operands = std::collections::HashSet::new();
    let roots: std::collections::HashSet<_> = layers
        .iter()
        .flat_map(|l| &l.vector_objects)
        .map(|o| o.id.as_str())
        .collect();
    let operands: std::collections::HashSet<_> = shapes
        .iter()
        .flat_map(|s| &s.operands)
        .map(|o| o.id.as_str())
        .collect();
    for shape in shapes {
        if !ids.insert(&shape.id)
            || !(2..=64).contains(&shape.operands.len())
            || !shape_mode(shape.operation)
            || !(roots.contains(shape.id.as_str()) || operands.contains(shape.id.as_str()))
        {
            return Err("Invalid compound shape".into());
        }
        let mut child_ids = std::collections::HashSet::new();
        for object in &shape.operands {
            if object.id == shape.id
                || roots.contains(object.id.as_str())
                || !child_ids.insert(&object.id)
                || !owned_operands.insert(&object.id)
            {
                return Err("Invalid compound operand identity".into());
            }
            object.validate()?;
            bytes += serde_json::to_vec(object).map_err(|e| e.to_string())?.len();
            if bytes > MAX_SVG_TOTAL_BYTES {
                return Err("Compound operands exceed the document budget".into());
            }
        }
        // Bounded, acyclic recipe dependencies, including latent nested shapes.
        let mut stack = vec![(shape.id.as_str(), 0usize)];
        let mut visited = std::collections::HashSet::new();
        while let Some((id, depth)) = stack.pop() {
            if depth > 16 || !visited.insert(id) {
                return Err("Cyclic or excessive compound shapes".into());
            }
            if let Some(recipe) = shapes.iter().find(|s| s.id == id) {
                stack.extend(
                    recipe
                        .operands
                        .iter()
                        .filter(|o| shapes.iter().any(|s| s.id == o.id))
                        .map(|o| (o.id.as_str(), depth + 1)),
                );
            }
        }
    }
    Ok(())
}
impl Document {
    pub(super) fn reserved_compound_ids(&self) -> std::collections::HashSet<String> {
        self.compound_shapes
            .iter()
            .flat_map(|s| {
                std::iter::once(s.id.clone()).chain(s.operands.iter().flat_map(|o| {
                    std::iter::once(o.id.clone())
                        .chain(o.group_path.iter().cloned())
                        .chain(o.clipping_group.iter().cloned())
                }))
            })
            .collect()
    }
    pub(super) fn compound_shape_snapshot(&self) -> Vec<CompoundShapeSnapshot> {
        self.compound_shapes
            .iter()
            .map(|s| CompoundShapeSnapshot {
                id: s.id.clone(),
                operation: s.operation,
                operands: s
                    .operands
                    .iter()
                    .map(|o| CompoundOperandSnapshot {
                        id: o.id.clone(),
                        name: o.name.clone(),
                        transform: o.transform,
                    })
                    .collect(),
            })
            .collect()
    }
    pub fn make_compound_shape(
        &mut self,
        operation: PathfinderOperation,
        compute: impl FnOnce(&[VectorObject], PathfinderOperation) -> Result<Vec<VectorObject>, String>,
    ) -> Result<(), String> {
        if !shape_mode(operation) {
            return Err("Choose a shape mode".into());
        }
        let operands: Vec<_> = self
            .svg_layers
            .iter()
            .flat_map(|l| &l.vector_objects)
            .filter(|o| self.selected_vector_objects.contains(&o.id))
            .cloned()
            .collect();
        if !(2..=64).contains(&operands.len()) {
            return Err("Select 2 to 64 shapes".into());
        }
        let stored_bytes = self
            .compound_shapes
            .iter()
            .flat_map(|s| &s.operands)
            .chain(&operands)
            .try_fold(0usize, |n, o| {
                serde_json::to_vec(o)
                    .map(|b| n + b.len())
                    .map_err(|e| e.to_string())
            })?;
        if stored_bytes > MAX_SVG_TOTAL_BYTES {
            return Err("Compound operands exceed the document budget".into());
        }
        let previous_recipes = self.compound_shapes.clone();
        if previous_recipes.len() >= 4096 {
            return Err("Too many compound shapes".into());
        }
        for operand in &operands {
            let mut stack = vec![(operand.id.as_str(), 1usize)];
            while let Some((id, depth)) = stack.pop() {
                if depth > 16 {
                    return Err("Excessive compound nesting".into());
                }
                if let Some(recipe) = previous_recipes.iter().find(|s| s.id == id) {
                    stack.extend(recipe.operands.iter().map(|o| (o.id.as_str(), depth + 1)));
                }
            }
        }
        self.pathfinder_selected_vectors(operation, |objects, op| {
            let result = compute(objects, op)?;
            Ok(vec![resolved_shape(objects, result)?])
        })?;
        self.compound_shapes = previous_recipes;
        self.compound_shapes.push(CompoundShape {
            id: self.selected_vector_objects[0].clone(),
            operation,
            operands,
        });
        self.prune_compound_shapes();
        Ok(())
    }
    pub fn edit_compound_shape(
        &mut self,
        edit: CompoundShapeEdit,
        compute: impl FnOnce(&[VectorObject], PathfinderOperation) -> Result<Vec<VectorObject>, String>,
    ) -> Result<(), String> {
        if self.selected_vector_objects.len() != 1 {
            return Err("Select one compound shape".into());
        }
        let id = &self.selected_vector_objects[0];
        let si = self
            .compound_shapes
            .iter()
            .position(|s| &s.id == id)
            .ok_or("Select a live compound shape")?;
        let (li, oi) = self
            .svg_layers
            .iter()
            .enumerate()
            .find_map(|(li, l)| {
                l.vector_objects
                    .iter()
                    .position(|o| &o.id == id)
                    .map(|oi| (li, oi))
            })
            .ok_or("Compound shape not found")?;
        let root = self.svg_layers[li].vector_objects[oi].clone();
        if self.svg_layers[li].locked || !self.svg_layers[li].visible || self.object_is_locked(id) {
            return Err("Compound shape is locked or hidden".into());
        }
        let mut shape = self.compound_shapes[si].clone();
        let mut layer = self.svg_layers[li].clone();
        let selection;
        match edit.action.as_str() {
            "expand" => {
                selection = vec![root.id.clone()];
            }
            "release" => {
                for o in &mut shape.operands {
                    if self
                        .svg_layers
                        .iter()
                        .flat_map(|l| &l.vector_objects)
                        .any(|live| live.id == o.id)
                    {
                        return Err("Operand identity conflicts with an existing object".into());
                    }
                    o.transform = compose(root.transform, o.transform);
                    o.validate()?;
                }
                selection = shape.operands.iter().map(|o| o.id.clone()).collect();
                layer.vector_objects.splice(oi..=oi, shape.operands.clone());
            }
            "update" => {
                if let Some(op) = edit.operation {
                    if !shape_mode(op) {
                        return Err("Choose a shape mode".into());
                    }
                    shape.operation = op;
                }
                if let Some(translation) = edit.translation {
                    if translation
                        .iter()
                        .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
                    {
                        return Err("Invalid operand position".into());
                    }
                    let operand = edit.operand.as_ref().ok_or("Choose an operand")?;
                    let o = shape
                        .operands
                        .iter_mut()
                        .find(|o| &o.id == operand)
                        .ok_or("Operand not found")?;
                    o.transform[4] = translation[0];
                    o.transform[5] = translation[1];
                    o.validate()?;
                }
                let result =
                    resolved_shape(&shape.operands, compute(&shape.operands, shape.operation)?)?;
                let updated = &mut layer.vector_objects[oi];
                updated.path = result.path;
                updated.control_points = result.control_points;
                selection = vec![root.id.clone()];
            }
            _ => return Err("Unknown compound shape edit".into()),
        }
        layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
        validate_svg_layer(&layer)?;
        if layer.source.len()
            + self
                .svg_layers
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != li)
                .map(|(_, l)| l.source.len())
                .sum::<usize>()
            > MAX_SVG_TOTAL_BYTES
            || layer.vector_objects.len() > 4096
        {
            return Err("Compound result exceeds the document budget".into());
        }
        let mut recipes = self.compound_shapes.clone();
        if edit.action == "update" {
            recipes[si] = shape.clone();
        } else {
            recipes.remove(si);
        }
        let mut candidate_layers = self.svg_layers.clone();
        candidate_layers[li] = layer.clone();
        // Recipes no longer reachable after expansion are removed below; validate updates before mutation.
        if edit.action == "update" {
            validate_compound_shapes(&recipes, &candidate_layers)?;
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers[li] = layer;
        self.selected_vector_objects = selection;
        if edit.action == "update" {
            self.compound_shapes[si] = shape;
        } else {
            self.compound_shapes.remove(si);
        }
        self.prune_compound_shapes();
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
    pub(super) fn duplicate_compound_recipes(
        &self,
        mapping: &std::collections::HashMap<String, String>,
        mut fresh: impl FnMut() -> String,
    ) -> Vec<CompoundShape> {
        let mut mapping = mapping.clone();
        for _ in 0..17 {
            let old = mapping.len();
            for recipe in &self.compound_shapes {
                if mapping.contains_key(&recipe.id) {
                    for operand in &recipe.operands {
                        mapping.entry(operand.id.clone()).or_insert_with(&mut fresh);
                        for group in &operand.group_path {
                            mapping.entry(group.clone()).or_insert_with(&mut fresh);
                        }
                        if let Some(group) = &operand.clipping_group {
                            mapping.entry(group.clone()).or_insert_with(&mut fresh);
                        }
                    }
                }
            }
            if old == mapping.len() {
                break;
            }
        }
        self.compound_shapes
            .iter()
            .filter_map(|recipe| {
                let id = mapping.get(&recipe.id)?.clone();
                let mut copy = recipe.clone();
                copy.id = id;
                for operand in &mut copy.operands {
                    operand.id = mapping[&operand.id].clone();
                    for group in &mut operand.group_path {
                        *group = mapping[group].clone();
                    }
                    if let Some(group) = &mut operand.clipping_group {
                        *group = mapping[group].clone();
                    }
                }
                Some(copy)
            })
            .collect()
    }
    pub(super) fn prune_compound_shapes(&mut self) {
        let mut retained: std::collections::HashSet<String> = self
            .svg_layers
            .iter()
            .flat_map(|l| &l.vector_objects)
            .map(|o| o.id.clone())
            .collect();
        for _ in 0..17 {
            let count = retained.len();
            for shape in &self.compound_shapes {
                if retained.contains(&shape.id) {
                    retained.extend(shape.operands.iter().map(|o| o.id.clone()));
                }
            }
            if count == retained.len() {
                break;
            }
        }
        self.compound_shapes.retain(|s| retained.contains(&s.id));
    }
}
fn resolved_shape(
    objects: &[VectorObject],
    mut result: Vec<VectorObject>,
) -> Result<VectorObject, String> {
    if result.len() > 1 {
        return Err("Shape mode returned multiple results".into());
    }
    let mut object = if let Some(result) = result.pop() {
        result
    } else {
        let mut empty = objects.first().ok_or("No operands")?.clone();
        empty.path.data = "M0 0".into();
        empty.control_points = vec![[0., 0.]];
        empty
    };
    object.transform = [1., 0., 0., 1., 0., 0.];
    object.kind = VectorObjectKind::Compound;
    object.text = None;
    object.image_frame = None;
    object.group_path.clear();
    object.clipping_group = None;
    object.live_corners = None;
    object.rectangle_radii = None;
    object.validate()?;
    Ok(object)
}
fn compose(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}
