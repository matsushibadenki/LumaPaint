//! Canvas placement, transform gestures and deferred history for EDFX layers.
use super::*;
use lumapaint_core::tiles::EdfxTransform;

pub(super) fn queue_history(action: DocumentAction) -> bool {
    let redo = match action {
        DocumentAction::Undo => false,
        DocumentAction::Redo => true,
        _ => return false,
    };
    EDFX_TILED_HISTORY.with(|pending| {
        let mut pending = pending.borrow_mut();
        let Some(actions) = pending.get_mut(&ACTIVE_DOCUMENT_ID.with(|id| id.get())) else {
            return false;
        };
        actions.push(redo);
        true
    })
}

pub(crate) fn edfx_canvas_target() -> Result<(u64, Option<crate::edfx_editor::TileTarget>), String>
{
    let target = ACTIVE_TILED_DOCUMENT.with(|doc| {
        let doc = doc.borrow();
        doc.as_ref().map(|session| {
            let resolution = session.document.resolution();
            crate::edfx_editor::TileTarget {
                dimensions: session.document.dimensions(),
                ppi: resolution.map_or([72., 72.], |r| [r.x_ppi, r.y_ppi]),
            }
        })
    });
    if target.is_some() {
        ensure_tiled_open()?;
    } else {
        ensure_document_open()?;
    }
    Ok((ACTIVE_DOCUMENT_ID.with(|id| id.get()), target))
}

pub(crate) fn selected_edfx() -> Result<crate::edfx_editor::Placed, String> {
    let (document_id, tiled) = edfx_canvas_target()?;
    let layer_id = if tiled.is_some() {
        ACTIVE_TILED_DOCUMENT.with(|doc| {
            doc.borrow()
                .as_ref()
                .map(|s| s.selected_layer_id().to_owned())
                .unwrap_or_default()
        })
    } else {
        DOCUMENT.with(|doc| doc.borrow().selected_layer_id().to_owned())
    };
    edfx_layer(document_id, layer_id)
}

fn edfx_layer(document_id: u64, layer_id: String) -> Result<crate::edfx_editor::Placed, String> {
    let (active_id, tiled) = edfx_canvas_target()?;
    if document_id != active_id {
        return Err("The target document has changed".into());
    }
    let (data, transform) = if tiled.is_some() {
        ACTIVE_TILED_DOCUMENT.with(|doc| {
            let doc = doc.borrow();
            let layer = doc
                .as_ref()
                .and_then(|session| {
                    session
                        .document
                        .layers()
                        .iter()
                        .find(|layer| layer.id == layer_id)
                })
                .ok_or("Select an EDFX layer")?;
            let data = layer.edfx.clone().ok_or("Select an EDFX layer")?;
            Ok::<_, String>((data, layer.edfx_transform.unwrap_or_default()))
        })?
    } else {
        DOCUMENT.with(|doc| {
            let doc = doc.borrow();
            let data = doc
                .svg_layers()
                .find(|layer| layer.id == layer_id)
                .and_then(|layer| layer.edfx.clone())
                .ok_or("Select an EDFX layer")?;
            Ok::<_, String>((data, doc.placed_edfx_transform(&layer_id)?))
        })?
    };
    Ok(crate::edfx_editor::Placed {
        document_id,
        layer_id,
        data,
        transform,
    })
}

pub(crate) fn apply_edfx(
    document_id: u64,
    placed: Option<crate::edfx_editor::Placed>,
    paths: String,
    tiles: Option<lumapaint_core::tiles::SparseTiles>,
    data: std::sync::Arc<str>,
) -> Result<crate::edfx_editor::Placed, String> {
    let (active_id, target) = edfx_canvas_target()?;
    if active_id != document_id || target.is_some() != tiles.is_some() {
        return Err("The target document has changed".into());
    }

    let transform = placed
        .as_ref()
        .map_or_else(EdfxTransform::default, |layer| layer.transform);
    if let Some(layer) = &placed {
        if edfx_layer(document_id, layer.layer_id.clone())?.transform != transform {
            return Err("The EDFX transform changed".into());
        }
    }
    let layer_id = if let Some(tiles) = tiles {
        ACTIVE_TILED_DOCUMENT.with(|doc| -> Result<String, String> {
            let mut doc = doc.borrow_mut();
            let session = doc.as_mut().ok_or("Missing tiled document")?;
            let id = if let Some(placed) = placed {
                session.document.update_edfx(
                    &placed.layer_id,
                    &placed.data,
                    tiles,
                    data.clone(),
                )?;
                placed.layer_id
            } else {
                session.document.place_edfx(tiles, data.clone())?
            };
            session.selected_layer = Some(id.clone());
            Ok(id)
        })?
    } else {
        text_editor::finish(true)?;
        DOCUMENT.with(|doc| -> Result<String, String> {
            let mut doc = doc.borrow_mut();
            if let Some(placed) = placed {
                doc.update_placed_edfx(&placed.layer_id, &placed.data, &paths, data.clone())?;
                Ok(placed.layer_id)
            } else {
                doc.place_edfx(&paths, data.clone())
            }
        })?
    };
    if let Err(error) = redraw() {
        emit_error(error);
    }
    emit_document();
    Ok(crate::edfx_editor::Placed {
        document_id,
        layer_id,
        data,
        transform,
    })
}

pub(crate) fn finish_edfx_transform(
    placed: crate::edfx_editor::Placed,
    tiles: Result<Option<lumapaint_core::tiles::SparseTiles>, String>,
    transform: EdfxTransform,
) -> Result<(), String> {
    let actions =
        EDFX_TILED_HISTORY.with(|pending| pending.borrow_mut().remove(&placed.document_id));
    let is_current = ACTIVE_DOCUMENT_ID.with(|id| id.get()) == placed.document_id;
    let result = tiles.and_then(|tiles| {
        commit_edfx_transform(placed, tiles, transform, actions.unwrap_or_default())
    });
    if result.is_ok() {
        if let Err(error) = redraw() {
            emit_error(error);
        }
        emit_document();
    } else if is_current {
        // Restore the true history availability even when the worker failed.
        emit_workspace_snapshot(workspace_snapshot());
    }
    result
}

fn commit_edfx_transform(
    placed: crate::edfx_editor::Placed,
    tiles: Option<lumapaint_core::tiles::SparseTiles>,
    transform: EdfxTransform,
    actions: Vec<bool>,
) -> Result<(), String> {
    let current = edfx_layer(placed.document_id, placed.layer_id.clone())?;
    if current.transform != placed.transform || !std::sync::Arc::ptr_eq(&current.data, &placed.data)
    {
        return Err("The EDFX layer changed".into());
    }
    if let Some(tiles) = tiles {
        ACTIVE_TILED_DOCUMENT.with(|doc| {
            let mut doc = doc.borrow_mut();
            let document = &mut doc.as_mut().ok_or("Missing tiled document")?.document;
            document.transform_edfx(&placed.layer_id, &placed.data, tiles, transform)?;
            for redo in actions {
                if redo {
                    document.redo()?;
                } else {
                    document.undo()?;
                }
            }
            Ok::<_, String>(())
        })?;
    } else {
        DOCUMENT.with(|doc| {
            doc.borrow_mut()
                .transform_placed_edfx(&placed.layer_id, transform)
        })?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum EdfxGesture {
    Move,
    Scale,
}

struct EdfxDrag {
    owner: String,
    mode: EdfxGesture,
    start: Point,
    placed: crate::edfx_editor::Placed,
    target: Option<crate::edfx_editor::TileTarget>,
}

thread_local! {
    static EDFX_MODE: RefCell<std::collections::HashMap<String, (u64, String, EdfxGesture)>> = RefCell::new(Default::default());
    // While a transform is rendering, keep history commands in input order.
    static EDFX_TILED_HISTORY: RefCell<std::collections::HashMap<u64, Vec<bool>>> = RefCell::new(Default::default());
    static EDFX_DRAG: RefCell<Option<EdfxDrag>> = const { RefCell::new(None) };
}

pub(crate) fn edfx_transform_mode(mode: Option<String>) -> Result<(), String> {
    let label = current_label();
    let next = match mode.as_deref() {
        Some("move") => Some(EdfxGesture::Move),
        Some("scale") => Some(EdfxGesture::Scale),
        Some(_) => return Err("Unknown EDFX transform mode".into()),
        None => None,
    };
    let selected = next
        .map(|mode| selected_edfx().map(|layer| (layer.document_id, layer.layer_id, mode)))
        .transpose()?;
    EDFX_MODE.with(|modes| {
        let mut modes = modes.borrow_mut();
        if let Some(selected) = selected {
            modes.insert(label, selected);
        } else {
            modes.remove(&label);
        }
    });
    EDFX_DRAG.with(|drag| {
        drag.borrow_mut().take();
    });
    Ok(())
}

pub(super) fn edfx_mode_active() -> bool {
    EDFX_MODE.with(|modes| {
        modes
            .borrow()
            .get(&current_label())
            .is_some_and(|(id, layer, _)| {
                *id == ACTIVE_DOCUMENT_ID.with(|id| id.get())
                    && ACTIVE_TILED_DOCUMENT.with(|doc| {
                        if let Some(session) = doc.borrow().as_ref() {
                            session.selected_layer_id() == layer
                                && session
                                    .document
                                    .layers()
                                    .iter()
                                    .any(|l| l.id == *layer && !l.locked)
                        } else {
                            DOCUMENT.with(|doc| {
                                let doc = doc.borrow();
                                doc.selected_layer_id() == layer
                                    && doc.svg_layers().any(|l| l.id == *layer && !l.locked)
                            })
                        }
                    })
            })
    })
}

pub(super) fn edfx_tiled_transform_pending() -> bool {
    EDFX_TILED_HISTORY.with(|pending| {
        pending
            .borrow()
            .contains_key(&ACTIVE_DOCUMENT_ID.with(|id| id.get()))
    })
}

pub(super) fn edfx_pointer(point: Point, phase: u8) -> bool {
    let label = current_label();
    if phase == 0 && edfx_mode_active() {
        if edfx_tiled_transform_pending() {
            return true;
        }
        let mode = EDFX_MODE.with(|modes| modes.borrow()[&label].2);
        match selected_edfx()
            .and_then(|placed| edfx_canvas_target().map(|(_, target)| (placed, target)))
        {
            Ok((placed, target)) => EDFX_DRAG.with(|drag| {
                *drag.borrow_mut() = Some(EdfxDrag {
                    owner: label,
                    mode,
                    start: point,
                    placed,
                    target,
                });
            }),
            Err(error) => emit_error(error),
        }
        return true;
    }
    if phase == 0 || phase == 3 {
        return false;
    }
    let tracking = EDFX_DRAG.with(|drag| {
        drag.borrow()
            .as_ref()
            .is_some_and(|drag| drag.owner == label)
    });
    if tracking && phase == 2 {
        let Some(EdfxDrag {
            mode,
            start,
            placed,
            target,
            ..
        }) = EDFX_DRAG.with(|drag| drag.borrow_mut().take())
        else {
            return true;
        };
        let dx = f64::from(point.x - start.x);
        let dy = f64::from(point.y - start.y);
        let [x, y, scale] = placed.transform.0;
        let values = match mode {
            EdfxGesture::Move => [x + dx, y + dy, scale],
            EdfxGesture::Scale => {
                let screen_zoom = CANVAS.with(|canvas| {
                    canvas
                        .borrow()
                        .as_ref()
                        .map_or(1., |c| c.viewport.screen_zoom())
                });
                [
                    x,
                    y,
                    (scale * 2f64.powf((dx - dy) * f64::from(screen_zoom) / 160.))
                        .clamp(0.01, 100.),
                ]
            }
        };
        let transform = EdfxTransform(values);
        if transform != placed.transform {
            if let Some(window) = APP.get().and_then(|app| app.get_webview_window(&label)) {
                if target.is_some() {
                    EDFX_TILED_HISTORY.with(|pending| {
                        pending.borrow_mut().insert(placed.document_id, Vec::new());
                    });
                    emit_workspace_snapshot(workspace_snapshot());
                }
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = crate::edfx_editor::transform_placement(
                        window.clone(),
                        placed,
                        target,
                        transform,
                    )
                    .await
                    {
                        let _ = window.emit("canvas-error", error);
                    }
                });
            }
        }
    }
    tracking
}
