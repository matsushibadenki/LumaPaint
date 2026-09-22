//! Validated, platform-independent structure for a future non-destructive document.
//! This does not replace the v1 document or its save format.
use crate::document::Document;
use crate::tiles::{TileCoord, TileInvalidation, TiledRasterDocument, TILE_SIZE};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

const MAX_NODES: usize = 256;
const MAX_INPUTS: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessingGraph {
    pub output: String,
    pub nodes: Vec<GraphNode>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    #[serde(flatten)]
    pub kind: NodeKind,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum NodeKind {
    RasterSource {
        source_id: String,
    },
    VectorSource {
        source_id: String,
    },
    Mask {
        input: String,
        mask_id: String,
        density: f32,
        inverted: bool,
    },
    Filter {
        input: String,
        filter: FilterKind,
    },
    Composite {
        inputs: Vec<CompositeInput>,
    },
    Output {
        input: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum FilterKind {
    Exposure { stops: f32 },
    GaussianBlur { radius_px: f32 },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompositeInput {
    pub node: String,
    pub opacity: f32,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeTarget {
    Source,
    Mask,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeInvalidation {
    pub node_id: String,
    pub coords: Vec<TileCoord>,
}

impl ProcessingGraph {
    pub fn affected_output_tiles(
        &self,
        change: &TileInvalidation,
        target: ChangeTarget,
        dimensions: (u32, u32),
    ) -> Result<Vec<TileCoord>, String> {
        let affected = self.affected_tiles(change, target, dimensions)?;
        if change.coords.is_empty() {
            return Ok(Vec::new());
        }
        if affected.is_empty() {
            return Ok(Vec::new());
        }
        affected
            .into_iter()
            .find(|item| item.node_id == self.output)
            .map(|item| item.coords)
            .ok_or_else(|| "Graph output invalidation was lost".into())
    }

    /// The graph for a retained raster preview. The underlying tile document
    /// remains authoritative; this only describes its composite dependencies.
    pub fn project_raster(document: &TiledRasterDocument) -> Result<Self, String> {
        let mut nodes = Vec::with_capacity(document.layers().len() * 2 + 2);
        let mut inputs = Vec::with_capacity(document.layers().len());
        for layer in document.layers() {
            let key = stable_layer_key(&layer.id);
            let source = format!("source-{key:016x}");
            nodes.push(GraphNode {
                id: source.clone(),
                kind: NodeKind::RasterSource {
                    source_id: layer.id.clone(),
                },
            });
            let input = if layer.mask_enabled {
                let masked = format!("mask-{key:016x}");
                nodes.push(GraphNode {
                    id: masked.clone(),
                    kind: NodeKind::Mask {
                        input: source,
                        mask_id: layer.id.clone(),
                        density: layer.mask_density,
                        inverted: layer.mask_inverted,
                    },
                });
                masked
            } else {
                source
            };
            inputs.push(CompositeInput {
                node: input,
                opacity: layer.opacity,
                visible: layer.visible,
            });
        }
        nodes.push(GraphNode {
            id: "composite".into(),
            kind: NodeKind::Composite { inputs },
        });
        nodes.push(GraphNode {
            id: "output".into(),
            kind: NodeKind::Output {
                input: "composite".into(),
            },
        });
        let graph = Self {
            output: "output".into(),
            nodes,
        };
        graph.validate()?;
        Ok(graph)
    }

    /// Translate a raster or mask edit into cache invalidations for every
    /// dependent node. `radius_px` is the finite sampling support of a blur.
    /// The v1 renderer does not consume these invalidations yet.
    pub fn affected_tiles(
        &self,
        change: &TileInvalidation,
        target: ChangeTarget,
        dimensions: (u32, u32),
    ) -> Result<Vec<NodeInvalidation>, String> {
        self.validate()?;
        let (width, height) = dimensions;
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err("Invalid graph canvas dimensions".into());
        }
        let columns = width.div_ceil(TILE_SIZE);
        let rows = height.div_ceil(TILE_SIZE);
        if change
            .coords
            .iter()
            .any(|coord| coord.x >= columns || coord.y >= rows)
        {
            return Err("Changed tile outside graph canvas".into());
        }
        let mut downstream = vec![Vec::new(); self.nodes.len()];
        let indices: HashMap<_, _> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.id.as_str(), i))
            .collect();
        for (consumer, node) in self.nodes.iter().enumerate() {
            let inputs: Vec<&str> = match &node.kind {
                NodeKind::RasterSource { .. } | NodeKind::VectorSource { .. } => Vec::new(),
                NodeKind::Mask { input, .. }
                | NodeKind::Filter { input, .. }
                | NodeKind::Output { input } => vec![input],
                NodeKind::Composite { inputs } => {
                    inputs.iter().map(|input| input.node.as_str()).collect()
                }
            };
            for input in inputs {
                downstream[indices[input]].push(consumer);
            }
        }
        let changed: BTreeSet<_> = change.coords.iter().copied().collect();
        let mut affected = vec![BTreeSet::new(); self.nodes.len()];
        let mut pending = VecDeque::new();
        let mut found = false;
        for (index, node) in self.nodes.iter().enumerate() {
            let matches = match (&node.kind, target) {
                (
                    NodeKind::RasterSource { source_id } | NodeKind::VectorSource { source_id },
                    ChangeTarget::Source,
                ) => source_id == &change.layer_id,
                (NodeKind::Mask { mask_id, .. }, ChangeTarget::Mask) => mask_id == &change.layer_id,
                _ => false,
            };
            if matches {
                found = true;
                pending.push_back((index, changed.clone()));
            }
        }
        if !found {
            if target == ChangeTarget::Mask {
                // A disabled mask is intentionally absent from the graph. Its
                // stored pixels may still be edited, but they cannot affect the
                // current output until the mask is enabled.
                return Ok(Vec::new());
            }
            return Err("Changed layer is not in the processing graph".into());
        }
        while let Some((index, incoming)) = pending.pop_front() {
            let new: BTreeSet<_> = incoming.difference(&affected[index]).copied().collect();
            if new.is_empty() {
                continue;
            }
            affected[index].extend(&new);
            for &consumer in &downstream[index] {
                let next = match &self.nodes[consumer].kind {
                    NodeKind::Filter {
                        filter: FilterKind::GaussianBlur { radius_px },
                        ..
                    } => expand_tiles(
                        &new,
                        (radius_px.ceil() as u32).div_ceil(TILE_SIZE),
                        columns,
                        rows,
                    ),
                    _ => new.clone(),
                };
                pending.push_back((consumer, next));
            }
        }
        Ok(self
            .nodes
            .iter()
            .zip(affected)
            .filter_map(|(node, coords)| {
                (!coords.is_empty()).then(|| NodeInvalidation {
                    node_id: node.id.clone(),
                    coords: coords.into_iter().collect(),
                })
            })
            .collect())
    }

    /// A structural, read-only view of v1 layers. Source pixels and vector
    /// objects remain owned by the legacy document; this graph is not rendered
    /// or saved as the project's authoritative format yet.
    pub fn project_legacy(document: &Document) -> Result<Self, String> {
        let snapshot = document.snapshot();
        let mut nodes = Vec::with_capacity(snapshot.layers.len() * 2 + 2);
        let mut inputs = Vec::with_capacity(snapshot.layers.len());
        for layer in &snapshot.layers {
            let key = stable_layer_key(&layer.id);
            let source = format!("source-{key:016x}");
            let kind = match layer.kind {
                "paint" => NodeKind::RasterSource {
                    source_id: layer.id.clone(),
                },
                "svg" | "vector" => NodeKind::VectorSource {
                    source_id: layer.id.clone(),
                },
                _ => return Err("Unsupported legacy layer kind".into()),
            };
            nodes.push(GraphNode {
                id: source.clone(),
                kind,
            });
            let input = if layer.mask_enabled {
                let masked = format!("mask-{key:016x}");
                nodes.push(GraphNode {
                    id: masked.clone(),
                    kind: NodeKind::Mask {
                        input: source,
                        mask_id: layer.id.clone(),
                        density: layer.mask_density,
                        inverted: layer.mask_inverted,
                    },
                });
                masked
            } else {
                source
            };
            inputs.push(CompositeInput {
                node: input,
                opacity: layer.opacity,
                visible: layer.visible,
            });
        }
        nodes.push(GraphNode {
            id: "composite".into(),
            kind: NodeKind::Composite { inputs },
        });
        nodes.push(GraphNode {
            id: "output".into(),
            kind: NodeKind::Output {
                input: "composite".into(),
            },
        });
        let graph = Self {
            output: "output".into(),
            nodes,
        };
        graph.validate()?;
        Ok(graph)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.nodes.is_empty() || self.nodes.len() > MAX_NODES {
            return Err("Invalid graph node count".into());
        }
        let mut indices = HashMap::new();
        for (index, node) in self.nodes.iter().enumerate() {
            validate_id(&node.id)?;
            if indices.insert(node.id.as_str(), index).is_some() {
                return Err("Duplicate graph node ID".into());
            }
        }
        let Some(&output) = indices.get(self.output.as_str()) else {
            return Err("Missing graph output".into());
        };
        if !matches!(self.nodes[output].kind, NodeKind::Output { .. }) {
            return Err("Graph output ID is not an Output node".into());
        }
        if self
            .nodes
            .iter()
            .filter(|node| matches!(node.kind, NodeKind::Output { .. }))
            .count()
            != 1
        {
            return Err("Graph must have one Output node".into());
        }
        let mut edges = vec![Vec::new(); self.nodes.len()];
        for (index, node) in self.nodes.iter().enumerate() {
            let inputs: Vec<&str> = match &node.kind {
                NodeKind::RasterSource { source_id } | NodeKind::VectorSource { source_id } => {
                    validate_id(source_id)?;
                    Vec::new()
                }
                NodeKind::Mask {
                    input,
                    mask_id,
                    density,
                    ..
                } => {
                    validate_id(mask_id)?;
                    if !density.is_finite() || !(0.0..=1.0).contains(density) {
                        return Err("Invalid mask density".into());
                    }
                    vec![input]
                }
                NodeKind::Filter { input, filter } => {
                    match filter {
                        FilterKind::Exposure { stops }
                            if !stops.is_finite() || stops.abs() > 32.0 =>
                        {
                            return Err("Invalid exposure".into())
                        }
                        FilterKind::GaussianBlur { radius_px }
                            if !radius_px.is_finite() || !(0.0..=4096.0).contains(radius_px) =>
                        {
                            return Err("Invalid blur radius".into())
                        }
                        _ => {}
                    }
                    vec![input]
                }
                NodeKind::Composite { inputs } => {
                    if inputs.is_empty() || inputs.len() > MAX_INPUTS {
                        return Err("Invalid composite input count".into());
                    }
                    for input in inputs {
                        if !input.opacity.is_finite() || !(0.0..=1.0).contains(&input.opacity) {
                            return Err("Invalid composite opacity".into());
                        }
                    }
                    inputs.iter().map(|input| input.node.as_str()).collect()
                }
                NodeKind::Output { input } => vec![input],
            };
            for input in inputs {
                let Some(&source) = indices.get(input) else {
                    return Err("Missing graph input".into());
                };
                if matches!(self.nodes[source].kind, NodeKind::Output { .. }) {
                    return Err("Output node cannot be an input".into());
                }
                edges[index].push(source);
            }
        }
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        for start in 0..self.nodes.len() {
            let mut stack = vec![(start, false)];
            while let Some((index, leaving)) = stack.pop() {
                if leaving {
                    visiting.remove(&index);
                    visited.insert(index);
                } else if !visited.contains(&index) {
                    if !visiting.insert(index) {
                        return Err("Graph contains a cycle".into());
                    }
                    stack.push((index, true));
                    for &input in edges[index].iter().rev() {
                        if !visited.contains(&input) {
                            stack.push((input, false));
                        }
                    }
                }
            }
        }
        let mut reachable = HashSet::new();
        let mut pending = vec![output];
        while let Some(index) = pending.pop() {
            if reachable.insert(index) {
                pending.extend(edges[index].iter().copied());
            }
        }
        if reachable.len() != self.nodes.len() {
            return Err("Unreachable graph node".into());
        }
        Ok(())
    }
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 || id.trim() != id || id.chars().any(char::is_control) {
        return Err("Invalid graph ID".into());
    }
    Ok(())
}

fn expand_tiles(
    coords: &BTreeSet<TileCoord>,
    radius: u32,
    columns: u32,
    rows: u32,
) -> BTreeSet<TileCoord> {
    let mut expanded = BTreeSet::new();
    for coord in coords {
        for y in coord.y.saturating_sub(radius)..=coord.y.saturating_add(radius).min(rows - 1) {
            for x in
                coord.x.saturating_sub(radius)..=coord.x.saturating_add(radius).min(columns - 1)
            {
                expanded.insert(TileCoord { x, y });
            }
        }
    }
    expanded
}

fn stable_layer_key(id: &str) -> u64 {
    // FNV-1a keeps projected node IDs stable when a v1 layer is reordered.
    id.as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::LayerSettings;
    use crate::tiles::TiledRasterDocument;

    fn fixture() -> ProcessingGraph {
        ProcessingGraph {
            output: "out".into(),
            nodes: vec![
                GraphNode {
                    id: "paint".into(),
                    kind: NodeKind::RasterSource {
                        source_id: "layer-1".into(),
                    },
                },
                GraphNode {
                    id: "mask".into(),
                    kind: NodeKind::Mask {
                        input: "paint".into(),
                        mask_id: "mask-1".into(),
                        density: 1.0,
                        inverted: false,
                    },
                },
                GraphNode {
                    id: "out".into(),
                    kind: NodeKind::Output {
                        input: "mask".into(),
                    },
                },
            ],
        }
    }

    fn invalidation_fixture() -> ProcessingGraph {
        ProcessingGraph {
            output: "out".into(),
            nodes: vec![
                GraphNode {
                    id: "paint".into(),
                    kind: NodeKind::RasterSource {
                        source_id: "layer-1".into(),
                    },
                },
                GraphNode {
                    id: "mask".into(),
                    kind: NodeKind::Mask {
                        input: "paint".into(),
                        mask_id: "layer-1".into(),
                        density: 1.0,
                        inverted: false,
                    },
                },
                GraphNode {
                    id: "blur".into(),
                    kind: NodeKind::Filter {
                        input: "mask".into(),
                        filter: FilterKind::GaussianBlur { radius_px: 1.0 },
                    },
                },
                GraphNode {
                    id: "other".into(),
                    kind: NodeKind::VectorSource {
                        source_id: "vector-1".into(),
                    },
                },
                GraphNode {
                    id: "merge".into(),
                    kind: NodeKind::Composite {
                        inputs: vec![
                            CompositeInput {
                                node: "blur".into(),
                                opacity: 1.0,
                                visible: true,
                            },
                            CompositeInput {
                                node: "other".into(),
                                opacity: 1.0,
                                visible: true,
                            },
                        ],
                    },
                },
                GraphNode {
                    id: "out".into(),
                    kind: NodeKind::Output {
                        input: "merge".into(),
                    },
                },
            ],
        }
    }

    #[test]
    fn accepts_a_valid_non_destructive_chain() {
        let graph = fixture();
        graph.validate().unwrap();
        let decoded: ProcessingGraph =
            serde_json::from_slice(&serde_json::to_vec(&graph).unwrap()).unwrap();
        decoded.validate().unwrap();
    }

    #[test]
    fn rejects_missing_duplicate_cyclic_and_invalid_nodes() {
        let mut graph = fixture();
        graph.nodes[1].kind = NodeKind::Mask {
            input: "missing".into(),
            mask_id: "mask-1".into(),
            density: 1.0,
            inverted: false,
        };
        assert!(graph.validate().is_err());
        graph = fixture();
        graph.nodes[1].id = "paint".into();
        assert!(graph.validate().is_err());
        graph = fixture();
        graph.nodes[1].kind = NodeKind::Filter {
            input: "mask".into(),
            filter: FilterKind::Exposure { stops: 0.0 },
        };
        assert!(graph.validate().is_err());
        graph = fixture();
        graph.nodes[1].kind = NodeKind::Mask {
            input: "paint".into(),
            mask_id: "mask-1".into(),
            density: f32::NAN,
            inverted: false,
        };
        assert!(graph.validate().is_err());
    }

    #[test]
    fn rejects_unknown_node_fields_and_extra_outputs() {
        let mut value = serde_json::to_value(fixture()).unwrap();
        value["nodes"][0]["unexpected"] = true.into();
        assert!(serde_json::from_value::<ProcessingGraph>(value).is_err());
        let mut graph = fixture();
        graph.nodes.push(GraphNode {
            id: "other-output".into(),
            kind: NodeKind::Output {
                input: "paint".into(),
            },
        });
        assert!(graph.validate().is_err());
        let mut graph = fixture();
        graph.nodes.push(GraphNode {
            id: "unused".into(),
            kind: NodeKind::RasterSource {
                source_id: "layer-2".into(),
            },
        });
        assert!(graph.validate().is_err());
    }

    #[test]
    fn legacy_projection_preserves_layer_order_masks_and_source_identity() {
        let mut document = Document::default();
        let paint = document.add_paint_layer().unwrap();
        let vector = document.add_vector_layer().unwrap();
        document
            .import_svg(
                "Logo".into(),
                "<svg xmlns=\"http://www.w3.org/2000/svg\"/>".into(),
            )
            .unwrap();
        let svg = document.snapshot().layers.last().unwrap().id.clone();
        document
            .set_layer_settings(LayerSettings {
                id: paint.clone(),
                name: "Paint 2".into(),
                opacity: 0.4,
                locked: false,
                alpha_locked: false,
                mask_enabled: true,
                mask_inverted: true,
                mask_density: 0.7,
            })
            .unwrap();
        let before = serde_json::to_value(document.snapshot()).unwrap();
        let projected = ProcessingGraph::project_legacy(&document).unwrap();
        assert_eq!(serde_json::to_value(document.snapshot()).unwrap(), before);
        let composite = projected
            .nodes
            .iter()
            .find_map(|node| match &node.kind {
                NodeKind::Composite { inputs } => Some(inputs),
                _ => None,
            })
            .unwrap();
        assert_eq!(composite.len(), 4);
        assert_eq!(composite[1].opacity, 0.4);
        let mask = projected
            .nodes
            .iter()
            .find(|node| node.id == composite[1].node)
            .unwrap();
        assert!(
            matches!(&mask.kind, NodeKind::Mask { mask_id, density, inverted: true, .. } if mask_id == &paint && *density == 0.7)
        );
        let vector_source = projected
            .nodes
            .iter()
            .find(|node| node.id == composite[2].node)
            .unwrap();
        assert!(
            matches!(&vector_source.kind, NodeKind::VectorSource { source_id } if source_id == &vector)
        );
        let svg_source = projected
            .nodes
            .iter()
            .find(|node| node.id == composite[3].node)
            .unwrap();
        assert!(
            matches!(&svg_source.kind, NodeKind::VectorSource { source_id } if source_id == &svg)
        );

        document
            .reorder_layers(&[paint.clone(), svg, vector])
            .unwrap();
        let reordered = ProcessingGraph::project_legacy(&document).unwrap();
        let new_composite = reordered
            .nodes
            .iter()
            .find_map(|node| match &node.kind {
                NodeKind::Composite { inputs } => Some(inputs),
                _ => None,
            })
            .unwrap();
        assert_eq!(new_composite[3].node, composite[1].node);
    }

    #[test]
    fn tile_document_edits_invalidate_only_dependent_nodes_and_blur_neighbors() {
        let mut tiles = TiledRasterDocument::new(769, 513).unwrap();
        tiles.add_layer("layer-1".into(), "Paint".into()).unwrap();
        let changed = tiles
            .write_rect("layer-1", [256, 256, 1, 1], &[10, 20, 30, 255])
            .unwrap()
            .unwrap();
        let graph = invalidation_fixture();
        let affected = graph
            .affected_tiles(&changed, ChangeTarget::Source, (769, 513))
            .unwrap();
        assert_eq!(
            affected
                .iter()
                .map(|entry| entry.node_id.as_str())
                .collect::<Vec<_>>(),
            ["paint", "mask", "blur", "merge", "out"]
        );
        assert_eq!(affected[0].coords, vec![TileCoord { x: 1, y: 1 }]);
        assert_eq!(affected[2].coords.len(), 9);
        assert_eq!(affected[4].coords, affected[2].coords);

        let mask_change = tiles
            .write_mask_rect("layer-1", [768, 512, 1, 1], &[0])
            .unwrap()
            .unwrap();
        let affected = graph
            .affected_tiles(&mask_change, ChangeTarget::Mask, (769, 513))
            .unwrap();
        assert_eq!(
            affected
                .iter()
                .map(|entry| entry.node_id.as_str())
                .collect::<Vec<_>>(),
            ["mask", "blur", "merge", "out"]
        );
        assert_eq!(affected[0].coords, vec![TileCoord { x: 3, y: 2 }]);
        assert_eq!(affected[1].coords.len(), 4);
        assert!(graph
            .affected_tiles(&mask_change, ChangeTarget::Mask, (256, 256))
            .is_err());
    }

    #[test]
    fn raster_preview_projection_preserves_changed_upload_tiles() {
        let mut tiles = TiledRasterDocument::new(513, 257).unwrap();
        tiles.add_layer("paint".into(), "Paint".into()).unwrap();
        tiles.set_layer_mask("paint", true, false, 0.8).unwrap();
        let changed = tiles
            .write_rect(
                "paint",
                [255, 256, 3, 1],
                &[100, 0, 0, 255, 0, 100, 0, 255, 0, 0, 100, 255],
            )
            .unwrap()
            .unwrap();
        let graph = ProcessingGraph::project_raster(&tiles).unwrap();
        let output = graph
            .affected_tiles(&changed, ChangeTarget::Source, tiles.dimensions())
            .unwrap()
            .into_iter()
            .find(|affected| affected.node_id == graph.output)
            .unwrap();
        assert_eq!(output.coords, changed.coords);
        let projected = TileInvalidation {
            layer_id: changed.layer_id.clone(),
            coords: output.coords,
        };
        assert_eq!(
            tiles.prepare_uploads(&changed).unwrap(),
            tiles.prepare_uploads(&projected).unwrap()
        );
    }

    #[test]
    fn disabled_mask_edits_do_not_invalidate_graph_output() {
        let mut tiles = TiledRasterDocument::new(256, 256).unwrap();
        tiles.add_layer("paint".into(), "Paint".into()).unwrap();
        let changed = tiles
            .write_mask_rect("paint", [12, 14, 1, 1], &[0])
            .unwrap()
            .unwrap();
        let graph = ProcessingGraph::project_raster(&tiles).unwrap();

        assert!(graph
            .affected_output_tiles(&changed, ChangeTarget::Mask, tiles.dimensions())
            .unwrap()
            .is_empty());
        assert!(graph
            .affected_output_tiles(
                &TileInvalidation {
                    layer_id: "missing".into(),
                    coords: changed.coords,
                },
                ChangeTarget::Source,
                tiles.dimensions(),
            )
            .is_err());
    }
}
