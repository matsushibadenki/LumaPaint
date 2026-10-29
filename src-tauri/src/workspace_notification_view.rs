//! Borrowed wire views. Exhaustive destructuring makes new snapshot fields a compile error.
use lumapaint_core::document::{DocumentSnapshot, LayerSnapshot};
use serde::{
    ser::{SerializeMap, SerializeSeq},
    Serialize, Serializer,
};

pub(super) struct DocumentView<'a>(pub &'a DocumentSnapshot, pub bool);
struct LayerView<'a>(&'a LayerSnapshot, bool);
struct LayersView<'a>(&'a [LayerSnapshot], bool);
impl Serialize for LayersView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for layer in self.0 {
            sequence.serialize_element(&LayerView(layer, self.1))?;
        }
        sequence.end()
    }
}
impl Serialize for DocumentView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let DocumentSnapshot {
            quick_mask,
            saved_pixel_selections,
            can_reselect_pixels,
            alignment,
            saved_vector_selections,
            vector_assets,
            can_reselect_vectors,
            compound_shapes,
            layer_groups,
            guides,
            pages,
            has_hidden_objects,
            has_locked_objects,
            selected_bounds,
            transform_panel,
            active_saved_path,
            saved_paths,
            selection,
            name,
            width,
            height,
            unit,
            raster_resolution,
            resolution,
            artboards,
            canvas_color,
            paper,
            pixel_aspect_ratio,
            layer_id,
            editing_channel,
            layer_edit_target,
            layer_visible,
            color_mode,
            color_profile,
            bit_depth,
            stroke_count,
            layers,
            selected_vector_objects,
            text_objects,
            can_undo,
            can_redo,
            revision,
            dirty,
            file_name,
        } = self.0;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("quickMask", quick_mask)?;
        map.serialize_entry("savedPixelSelections", saved_pixel_selections)?;
        map.serialize_entry("canReselectPixels", can_reselect_pixels)?;
        map.serialize_entry("alignment", alignment)?;
        map.serialize_entry("savedVectorSelections", saved_vector_selections)?;
        map.serialize_entry("vectorAssets", vector_assets)?;
        map.serialize_entry("canReselectVectors", can_reselect_vectors)?;
        map.serialize_entry("compoundShapes", compound_shapes)?;
        map.serialize_entry("layerGroups", layer_groups)?;
        map.serialize_entry("guides", guides)?;
        map.serialize_entry("pages", pages)?;
        map.serialize_entry("hasHiddenObjects", has_hidden_objects)?;
        map.serialize_entry("hasLockedObjects", has_locked_objects)?;
        map.serialize_entry("selectedBounds", selected_bounds)?;
        map.serialize_entry("transformPanel", transform_panel)?;
        map.serialize_entry("activeSavedPath", active_saved_path)?;
        map.serialize_entry("savedPaths", saved_paths)?;
        map.serialize_entry("selection", selection)?;
        map.serialize_entry("name", name)?;
        map.serialize_entry("width", width)?;
        map.serialize_entry("height", height)?;
        map.serialize_entry("unit", unit)?;
        if raster_resolution.is_some() {
            map.serialize_entry("rasterResolution", raster_resolution)?;
        }
        map.serialize_entry("resolution", resolution)?;
        map.serialize_entry("artboards", artboards)?;
        map.serialize_entry("canvasColor", canvas_color)?;
        map.serialize_entry("paper", paper)?;
        map.serialize_entry("pixelAspectRatio", pixel_aspect_ratio)?;
        map.serialize_entry("layerId", layer_id)?;
        map.serialize_entry("editingChannel", editing_channel)?;
        map.serialize_entry("layerEditTarget", layer_edit_target)?;
        map.serialize_entry("layerVisible", layer_visible)?;
        map.serialize_entry("colorMode", color_mode)?;
        map.serialize_entry("colorProfile", color_profile)?;
        map.serialize_entry("bitDepth", bit_depth)?;
        map.serialize_entry("strokeCount", stroke_count)?;
        map.serialize_entry("layers", &LayersView(layers, self.1))?;
        map.serialize_entry("selectedVectorObjects", selected_vector_objects)?;
        map.serialize_entry("textObjects", text_objects)?;
        map.serialize_entry("canUndo", can_undo)?;
        map.serialize_entry("canRedo", can_redo)?;
        map.serialize_entry("revision", revision)?;
        map.serialize_entry("dirty", dirty)?;
        map.serialize_entry("fileName", file_name)?;
        map.end()
    }
}
impl Serialize for LayerView<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let LayerSnapshot {
            effects,
            raster_blend_mode,
            guide_color,
            objects,
            id,
            name,
            kind,
            visible,
            opacity,
            locked,
            alpha_locked,
            sparse_mask_present,
            sparse_mask_linked,
            mask_enabled,
            mask_inverted,
            mask_density,
            deletable,
            stroke_count,
        } = self.0;
        let mut map = serializer.serialize_map(None)?;
        if effects.is_some() {
            map.serialize_entry("effects", effects)?;
        }
        if raster_blend_mode.is_some() {
            map.serialize_entry("rasterBlendMode", raster_blend_mode)?;
        }
        map.serialize_entry("guideColor", guide_color)?;
        map.serialize_entry(
            "objects",
            &if self.1 { &[][..] } else { objects.as_slice() },
        )?;
        map.serialize_entry("id", id)?;
        map.serialize_entry("name", name)?;
        map.serialize_entry("kind", kind)?;
        map.serialize_entry("visible", visible)?;
        map.serialize_entry("opacity", opacity)?;
        map.serialize_entry("locked", locked)?;
        map.serialize_entry("alphaLocked", alpha_locked)?;
        map.serialize_entry("sparseMaskPresent", sparse_mask_present)?;
        map.serialize_entry("sparseMaskLinked", sparse_mask_linked)?;
        map.serialize_entry("maskEnabled", mask_enabled)?;
        map.serialize_entry("maskInverted", mask_inverted)?;
        map.serialize_entry("maskDensity", mask_density)?;
        map.serialize_entry("deletable", deletable)?;
        map.serialize_entry("strokeCount", stroke_count)?;
        map.end()
    }
}
