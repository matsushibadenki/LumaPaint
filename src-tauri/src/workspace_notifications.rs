//! Main-thread notification mirrors. Artwork remains in the Rust document.
use crate::canvas::DocumentWorkspaceSnapshot;
use lumapaint_core::document::DocumentSnapshot;
use lumapaint_core::document::LayerObjectSnapshot;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
#[path = "workspace_notification_view.rs"]
mod wire;

const MAX_CACHED_OBJECTS: usize = 4096;
#[derive(Default)]
struct Cache {
    sequence: u64,
    active_id: Option<u64>,
    previous: Option<Rc<DocumentSnapshot>>,
    ready: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ObjectPatch<'a> {
    layer_id: &'a str,
    upsert: Vec<&'a LayerObjectSnapshot>,
    // A changed order also identifies removals. No geometry is transported.
    order: Option<Vec<&'a str>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Notification<'a> {
    sequence: u64,
    base_sequence: Option<u64>,
    workspace: WorkspaceView<'a>,
    object_patches: Vec<ObjectPatch<'a>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceView<'a> {
    active_id: Option<u64>,
    active: Option<wire::DocumentView<'a>>,
    documents: &'a [crate::canvas::DocumentTabSnapshot],
}
impl Cache {
    #[cfg(test)]
    fn publish(
        &mut self,
        workspace: DocumentWorkspaceSnapshot,
        force_full: bool,
        send: impl FnOnce(String) -> Result<(), String>,
    ) -> Result<(), String> {
        self.publish_shared(
            workspace.active_id,
            workspace.active.map(Rc::new),
            &workspace.documents,
            force_full,
            send,
        )
    }
    fn publish_shared(
        &mut self,
        active_id: Option<u64>,
        active: Option<Rc<DocumentSnapshot>>,
        documents: &[crate::canvas::DocumentTabSnapshot],
        force_full: bool,
        send: impl FnOnce(String) -> Result<(), String>,
    ) -> Result<(), String> {
        let count = active
            .as_ref()
            .map_or(0, |d| d.layers.iter().map(|l| l.objects.len()).sum());
        let cacheable = count <= MAX_CACHED_OBJECTS;
        let delta = !force_full && cacheable && self.ready && self.active_id == active_id;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Notification sequence exhausted")?;
        let same_summary = self
            .previous
            .as_ref()
            .zip(active.as_ref())
            .is_some_and(|(old, new)| Rc::ptr_eq(old, new));
        let mut patches = Vec::new();
        if delta && !same_summary {
            let old_layers: HashMap<_, _> = self
                .previous
                .iter()
                .flat_map(|d| &d.layers)
                .map(|layer| (layer.id.as_str(), &layer.objects))
                .collect();
            if let Some(document) = &active {
                for layer in &document.layers {
                    let objects = &layer.objects;
                    let old = old_layers.get(layer.id.as_str()).copied();
                    let index: HashMap<_, _> = old
                        .into_iter()
                        .flatten()
                        .map(|o| (o.id.as_str(), o))
                        .collect();
                    let upsert: Vec<_> = objects
                        .iter()
                        .filter(|o| {
                            index
                                .get(o.id.as_str())
                                .is_none_or(|previous| **previous != **o)
                        })
                        .collect();
                    let same_order = old.is_some_and(|old| {
                        old.iter().map(|o| &o.id).eq(objects.iter().map(|o| &o.id))
                    });
                    if !upsert.is_empty() || !same_order {
                        patches.push(ObjectPatch {
                            layer_id: &layer.id,
                            upsert,
                            order: (!same_order)
                                .then(|| objects.iter().map(|o| o.id.as_str()).collect()),
                        });
                    }
                }
            }
        }
        let json = serde_json::to_string(&Notification {
            sequence,
            base_sequence: delta.then_some(self.sequence),
            workspace: WorkspaceView {
                active_id,
                active: active.as_deref().map(|d| wire::DocumentView(d, delta)),
                documents,
            },
            object_patches: patches,
        })
        .map_err(|e| e.to_string())?;
        let bytes = json.len() as u64;
        send(json)?;
        lumapaint_core::performance::count("host.workspace_notification_bytes", bytes);
        lumapaint_core::performance::count(
            if delta {
                "host.workspace_delta_notifications"
            } else {
                "host.workspace_full_notifications"
            },
            1,
        );
        // Only successful delivery advances this window's base. Retain the
        // immutable summary itself instead of copying objects for every view.
        self.sequence = sequence;
        self.active_id = active_id;
        self.ready = cacheable;
        self.previous = if cacheable { active } else { None };
        Ok(())
    }
}
thread_local! { static CACHES: RefCell<HashMap<String, Cache>> = RefCell::new(HashMap::new()); }
pub(crate) fn publish(
    label: &str,
    snapshot: DocumentWorkspaceSnapshot,
    send: impl FnOnce(String) -> Result<(), String>,
) -> Result<(), String> {
    publish_shared(
        label,
        snapshot.active_id,
        snapshot.active.map(Rc::new),
        &snapshot.documents,
        send,
    )
}
pub(crate) fn publish_shared(
    label: &str,
    active_id: Option<u64>,
    active: Option<Rc<DocumentSnapshot>>,
    documents: &[crate::canvas::DocumentTabSnapshot],
    send: impl FnOnce(String) -> Result<(), String>,
) -> Result<(), String> {
    static FULL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let full =
        *FULL.get_or_init(|| std::env::var_os("LUMAPAINT_WORKSPACE_FULL_NOTIFICATIONS").is_some());
    CACHES.with(|c| {
        c.borrow_mut()
            .entry(label.into())
            .or_default()
            .publish_shared(active_id, active, documents, full, send)
    })
}
pub(crate) fn reset(label: &str) {
    CACHES.with(|c| {
        if let Some(cache) = c.borrow_mut().get_mut(label) {
            cache.ready = false;
            cache.previous = None;
        }
    });
}
pub(crate) fn forget(label: &str) {
    CACHES.with(|c| {
        c.borrow_mut().remove(label);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> DocumentWorkspaceSnapshot {
        let mut document = lumapaint_core::document::Document::default();
        document.add_vector_layer().unwrap();
        let mut active = document.snapshot();
        let object = LayerObjectSnapshot {
            image_frame: None,
            locked: false,
            opacity: 1.,
            blend_mode: "normal".into(),
            fill_gradient: None,
            stroke_gradient: None,
            fill_color: Some([0, 0, 0, 255]),
            stroke_color: None,
            stroke_width: 0.,
            stroke_style: None,
            stroke_contours: vec![],
            id: "a".into(),
            name: "English 日本語 简体中文".into(),
            group_path: vec![],
            clipping_mask: false,
            kind: lumapaint_core::vector::VectorObjectKind::Rectangle,
            visible: true,
        };
        active.layers[0].objects = vec![
            object.clone(),
            LayerObjectSnapshot {
                id: "b".into(),
                ..object
            },
        ];
        DocumentWorkspaceSnapshot {
            active_id: Some(1),
            active: Some(active),
            documents: vec![],
        }
    }
    fn publish(cache: &mut Cache, snapshot: DocumentWorkspaceSnapshot) -> serde_json::Value {
        let mut packet = None;
        cache
            .publish(snapshot, false, |json| {
                packet = Some(serde_json::from_str(&json).unwrap());
                Ok(())
            })
            .unwrap();
        packet.unwrap()
    }
    #[test]
    fn change_reorder_delete_and_switch() {
        let mut cache = Cache::default();
        let mut snapshot = fixture();
        assert!(publish(&mut cache, snapshot.clone())["baseSequence"].is_null());
        let unchanged = publish(&mut cache, snapshot.clone());
        assert_eq!(unchanged["objectPatches"], serde_json::json!([]));
        snapshot.active.as_mut().unwrap().layers[0].objects[0].name = "Changed".into();
        let changed = publish(&mut cache, snapshot.clone());
        assert_eq!(
            changed["objectPatches"][0]["upsert"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(changed["objectPatches"][0]["order"].is_null());
        snapshot.active.as_mut().unwrap().layers[0]
            .objects
            .reverse();
        let reordered = publish(&mut cache, snapshot.clone());
        assert_eq!(
            reordered["objectPatches"][0]["order"],
            serde_json::json!(["b", "a"])
        );
        snapshot.active.as_mut().unwrap().layers[0].objects.clear();
        assert_eq!(
            publish(&mut cache, snapshot.clone())["objectPatches"][0]["order"],
            serde_json::json!([])
        );
        snapshot.active_id = Some(2);
        assert!(publish(&mut cache, snapshot)["baseSequence"].is_null());
    }
    #[test]
    fn changed_summary_survives_failed_delivery_without_advancing_mirror() {
        let mut cache = Cache::default();
        let mut snapshot = fixture();
        publish(&mut cache, snapshot.clone());
        let objects = &mut snapshot.active.as_mut().unwrap().layers[0].objects;
        objects[0].name = "Updated 日本語 简体中文".repeat(32);
        objects[0].group_path = vec!["group-a".into(), "group-b".into()];
        objects[0].stroke_contours = vec![true, false, true];
        objects[0].opacity = 0.375;
        let expected = serde_json::to_value(&objects[0]).unwrap();
        let previous = Rc::clone(cache.previous.as_ref().unwrap());
        assert!(cache
            .publish(snapshot.clone(), false, |_| Err("delivery failed".into()))
            .is_err());
        assert_eq!(cache.sequence, 1);
        assert!(Rc::ptr_eq(cache.previous.as_ref().unwrap(), &previous));
        let packet = publish(&mut cache, snapshot.clone());
        assert_eq!(packet["baseSequence"], 1);
        assert_eq!(
            packet["objectPatches"][0]["upsert"],
            serde_json::json!([expected])
        );
        assert!(packet["workspace"]["active"]["layers"][0]["objects"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(publish(&mut cache, snapshot)["objectPatches"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn borrowed_wire_matches_owned_format_for_full_and_delta() {
        let mut snapshot = fixture();
        snapshot.active.as_mut().unwrap().layers[0].raster_blend_mode =
            Some(lumapaint_core::tiles::RasterBlendMode::Multiply);
        snapshot.active.as_mut().unwrap().file_name = Some("文書.lumapaint".into());
        snapshot.active.as_mut().unwrap().raster_resolution =
            Some(lumapaint_core::tiles::RasterResolution {
                x_ppi: 300.,
                y_ppi: 144.,
                x_display: lumapaint_core::tiles::ResolutionDisplayUnit::PixelsPerInch,
                y_display: lumapaint_core::tiles::ResolutionDisplayUnit::PixelsPerCentimeter,
                width_display: lumapaint_core::tiles::PrintDimensionUnit::Inches,
                height_display: lumapaint_core::tiles::PrintDimensionUnit::Centimeters,
            });
        // Compare the actual wire round-trip: to_value widens f32 values,
        // whereas JSON text writes their shortest f32 representation.
        let expected: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
        let mut cache = Cache::default();
        let full = publish(&mut cache, snapshot.clone());
        assert_eq!(full["workspace"], expected);
        let delta = publish(&mut cache, snapshot.clone());
        let mut expected_delta = expected;
        for layer in expected_delta["active"]["layers"].as_array_mut().unwrap() {
            layer["objects"] = serde_json::json!([]);
        }
        assert_eq!(delta["workspace"], expected_delta);
        let mut no_optional = snapshot.active.as_ref().unwrap().clone();
        no_optional.raster_resolution = None;
        for layer in &mut no_optional.layers {
            layer.effects = None;
            layer.raster_blend_mode = None;
        }
        assert_eq!(
            serde_json::to_value(wire::DocumentView(&no_optional, false)).unwrap(),
            serde_json::to_value(&no_optional).unwrap()
        );
        let empty = DocumentWorkspaceSnapshot {
            active_id: None,
            active: None,
            documents: vec![],
        };
        assert_eq!(
            publish(&mut cache, empty.clone())["workspace"],
            serde_json::to_value(empty).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap()["active"]["layers"][0]["objects"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn shared_cache_keeps_delivery_bases_independent_without_copying_summary() {
        let snapshot = fixture();
        let source = Rc::new(snapshot.active.unwrap());
        let mut a = Cache::default();
        let mut b = Cache::default();
        a.publish_shared(Some(1), Some(Rc::clone(&source)), &[], false, |_| Ok(()))
            .unwrap();
        b.publish_shared(Some(1), Some(Rc::clone(&source)), &[], false, |_| Ok(()))
            .unwrap();
        assert!(Rc::ptr_eq(a.previous.as_ref().unwrap(), &source));
        assert!(Rc::ptr_eq(b.previous.as_ref().unwrap(), &source));
        let mut changed = (*source).clone();
        changed.layers[0].objects[0].name = "Updated 日本語 简体中文".into();
        let next = Rc::new(changed);
        assert!(a
            .publish_shared(Some(1), Some(Rc::clone(&next)), &[], false, |_| Err(
                "failed".into()
            ))
            .is_err());
        b.publish_shared(Some(1), Some(Rc::clone(&next)), &[], false, |_| Ok(()))
            .unwrap();
        assert_eq!(a.sequence, 1);
        assert_eq!(b.sequence, 2);
        assert!(Rc::ptr_eq(a.previous.as_ref().unwrap(), &source));
        assert!(Rc::ptr_eq(b.previous.as_ref().unwrap(), &next));
        a.publish_shared(Some(1), Some(Rc::clone(&next)), &[], false, |json| {
            let packet: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(packet["baseSequence"], 1);
            assert_eq!(
                packet["objectPatches"][0]["upsert"][0]["name"],
                "Updated 日本語 简体中文"
            );
            Ok(())
        })
        .unwrap();
        assert!(Rc::ptr_eq(a.previous.as_ref().unwrap(), &next));
        assert_eq!(source.layers[0].objects[0].name, "English 日本語 简体中文");
        b.publish_shared(Some(1), Some(Rc::clone(&next)), &[], false, |json| {
            let packet: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(packet["baseSequence"], 2);
            assert_eq!(packet["objectPatches"], serde_json::json!([]));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn reset_and_forget_release_shared_summary() {
        let label = "shared-cache-release-test";
        forget(label);
        let source = Rc::new(fixture().active.unwrap());
        let weak = Rc::downgrade(&source);
        super::publish_shared(label, Some(1), Some(source), &[], |_| Ok(())).unwrap();
        assert!(weak.upgrade().is_some());
        reset(label);
        assert!(weak.upgrade().is_none());
        let source = Rc::new(fixture().active.unwrap());
        let weak = Rc::downgrade(&source);
        super::publish_shared(label, Some(1), Some(source), &[], |_| Ok(())).unwrap();
        forget(label);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn thousand_unchanged_objects_leave_a_small_packet() {
        let mut cache = Cache::default();
        let mut snapshot = fixture();
        let objects = &mut snapshot.active.as_mut().unwrap().layers[0].objects;
        let prototype = objects[0].clone();
        *objects = (0..1000)
            .map(|i| LayerObjectSnapshot {
                id: format!("object-{i}"),
                ..prototype.clone()
            })
            .collect();
        let full = publish(&mut cache, snapshot.clone()).to_string().len();
        let delta = publish(&mut cache, snapshot).to_string().len();
        assert!(delta * 10 < full, "full={full}, delta={delta}");
    }
    #[test]
    fn subscriber_reset_keeps_sequence_and_window_mirrors_independent() {
        let label = "notification-test";
        let sibling = "notification-sibling";
        forget(label);
        forget(sibling);
        publish_for_test(label);
        publish_for_test(sibling);
        reset(label);
        CACHES.with(|c| {
            let c = c.borrow();
            assert_eq!(c[label].sequence, 1);
            assert!(!c[label].ready);
            assert!(c[sibling].ready);
        });
        let mut packet = None;
        super::publish(label, fixture(), |json| {
            packet = Some(serde_json::from_str::<serde_json::Value>(&json).unwrap());
            Ok(())
        })
        .unwrap();
        assert_eq!(packet.as_ref().unwrap()["sequence"], 2);
        assert!(packet.unwrap()["baseSequence"].is_null());
        forget(label);
        forget(sibling);
    }
    fn publish_for_test(label: &str) {
        super::publish(label, fixture(), |_| Ok(())).unwrap();
    }
    #[test]
    fn failure_and_cache_limit_require_full_recovery() {
        let mut cache = Cache::default();
        let snapshot = fixture();
        assert!(cache
            .publish(snapshot.clone(), false, |_| Err("delivery failed".into()))
            .is_err());
        assert_eq!(cache.sequence, 0);
        assert!(publish(&mut cache, snapshot.clone())["baseSequence"].is_null());
        assert!(cache
            .publish(snapshot.clone(), false, |_| Err("delivery failed".into()))
            .is_err());
        assert_eq!(publish(&mut cache, snapshot.clone())["baseSequence"], 1);
        let mut large = snapshot.clone();
        let objects = &mut large.active.as_mut().unwrap().layers[0].objects;
        objects.resize(MAX_CACHED_OBJECTS + 1, objects[0].clone());
        assert!(publish(&mut cache, large)["baseSequence"].is_null());
        assert!(cache.previous.is_none());
        assert!(publish(&mut cache, snapshot)["baseSequence"].is_null());
    }
}
