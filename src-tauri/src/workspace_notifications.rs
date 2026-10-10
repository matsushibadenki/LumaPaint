//! Main-thread notification mirrors. Artwork remains in the Rust document.
use crate::canvas::DocumentWorkspaceSnapshot;
use lumapaint_core::document::LayerObjectSnapshot;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;

const MAX_CACHED_OBJECTS: usize = 4096;
#[derive(Default)]
struct Cache {
    sequence: u64,
    active_id: Option<u64>,
    objects: HashMap<String, Vec<LayerObjectSnapshot>>,
    ready: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ObjectPatch {
    layer_id: String,
    upsert: Vec<LayerObjectSnapshot>,
    // A changed order also identifies removals. No geometry is transported.
    order: Option<Vec<String>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Notification<'a> {
    sequence: u64,
    base_sequence: Option<u64>,
    workspace: &'a DocumentWorkspaceSnapshot,
    object_patches: Vec<ObjectPatch>,
}
impl Cache {
    fn publish(
        &mut self,
        mut workspace: DocumentWorkspaceSnapshot,
        force_full: bool,
        send: impl FnOnce(String) -> Result<(), String>,
    ) -> Result<(), String> {
        let count = workspace
            .active
            .as_ref()
            .map_or(0, |d| d.layers.iter().map(|l| l.objects.len()).sum());
        let cacheable = count <= MAX_CACHED_OBJECTS;
        let delta = !force_full && cacheable && self.ready && self.active_id == workspace.active_id;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Notification sequence exhausted")?;
        let mut incoming = HashMap::new();
        let mut patches = Vec::new();
        if delta {
            if let Some(document) = &mut workspace.active {
                for layer in &mut document.layers {
                    let objects = std::mem::take(&mut layer.objects);
                    let old = self.objects.get(&layer.id);
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
                        .cloned()
                        .collect();
                    let same_order = old.is_some_and(|old| {
                        old.iter().map(|o| &o.id).eq(objects.iter().map(|o| &o.id))
                    });
                    if !upsert.is_empty() || !same_order {
                        patches.push(ObjectPatch {
                            layer_id: layer.id.clone(),
                            upsert,
                            order: (!same_order)
                                .then(|| objects.iter().map(|o| o.id.clone()).collect()),
                        });
                    }
                    incoming.insert(layer.id.clone(), objects);
                }
            }
        }
        let json = serde_json::to_string(&Notification {
            sequence,
            base_sequence: delta.then_some(self.sequence),
            workspace: &workspace,
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
        // Failed delivery must not advance the base. Move the new summaries into
        // the mirror rather than cloning every retained object a second time.
        if !delta && cacheable {
            if let Some(document) = &mut workspace.active {
                for layer in &mut document.layers {
                    incoming.insert(layer.id.clone(), std::mem::take(&mut layer.objects));
                }
            }
        }
        self.sequence = sequence;
        self.active_id = workspace.active_id;
        self.ready = cacheable;
        self.objects = incoming;
        Ok(())
    }
}
thread_local! { static CACHES: RefCell<HashMap<String, Cache>> = RefCell::new(HashMap::new()); }
pub(crate) fn publish(
    label: &str,
    snapshot: DocumentWorkspaceSnapshot,
    send: impl FnOnce(String) -> Result<(), String>,
) -> Result<(), String> {
    static FULL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let full =
        *FULL.get_or_init(|| std::env::var_os("LUMAPAINT_WORKSPACE_FULL_NOTIFICATIONS").is_some());
    CACHES.with(|c| {
        c.borrow_mut()
            .entry(label.into())
            .or_default()
            .publish(snapshot, full, send)
    })
}
pub(crate) fn reset(label: &str) {
    CACHES.with(|c| {
        if let Some(cache) = c.borrow_mut().get_mut(label) {
            cache.ready = false;
            cache.objects.clear();
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
        assert!(cache.objects.is_empty());
        assert!(publish(&mut cache, snapshot)["baseSequence"].is_null());
    }
}
