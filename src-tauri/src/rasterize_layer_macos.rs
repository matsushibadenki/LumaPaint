//! Worker rasterization with document identity/revision guards; no frame IPC.
use super::*;
pub(crate) struct Job {
    document_id: u64,
    revision: u64,
    target: String,
    selection: String,
    objects: Vec<String>,
    width: u32,
    height: u32,
    source: String,
}
pub(crate) struct Completed {
    job: Job,
    source: String,
}
pub(crate) fn prepare(id: String) -> Result<Job, String> {
    ensure_document_open()?;
    finish_open_pen()?;
    text_editor::finish(true)?;
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        return Err("Select a standard vector document / 通常のベクター文書で実行してください / 请在标准矢量文档中执行".into());
    }
    DOCUMENT.with(|slot|{let mut d=slot.borrow_mut();d.finish();let (width,height)=d.dimensions();
 if u64::from(width)*u64::from(height)>16_777_216{return Err("Rasterization exceeds 16 megapixels / ラスタライズは1600万画素までです / 栅格化上限为1600万像素".into())}
 let layer=d.svg_layers().find(|l|l.id==id).ok_or("Layer not found / レイヤーが見つかりません / 找不到图层")?;
 if !layer.vector_layer||layer.locked{return Err("Unlock the vector layer / ベクターレイヤーのロックを解除してください / 请解锁矢量图层".into())}
 let snapshot=d.snapshot();if snapshot.active_saved_path.is_some(){return Err("Finish path editing / パスの編集を終了してください / 请先结束路径编辑".into())}
 Ok(Job{document_id:ACTIVE_DOCUMENT_ID.with(|v|v.get()),revision:d.revision(),target:id,selection:d.selected_layer_id().into(),objects:snapshot.selected_vector_objects,width,height,source:layer.source.clone()})})
}
pub(crate) fn evaluate(job: Job) -> Result<Completed, String> {
    let raster = lumapaint_renderer::vector::rasterize_svg(&job.source, job.width, job.height)?;
    let png = lumapaint_renderer::vector::document_png(job.width, job.height, raster.pixels)?;
    let source = clipboard::image_svg(job.width, job.height, &png);
    Ok(Completed { job, source })
}
pub(crate) fn commit(result: Completed) -> Result<DocumentSnapshot, String> {
    let job = result.job;
    if ACTIVE_DOCUMENT_ID.with(|v| v.get()) != job.document_id {
        return Err("Document changed / 文書が変更されました / 文档已更改".into());
    }
    DOCUMENT.with(|slot|{let mut d=slot.borrow_mut();if d.has_active_stroke()||d.svg_layers().find(|l|l.id==job.target).is_none_or(|l|l.source!=job.source)||d.revision()!=job.revision||d.selected_layer_id()!=job.selection||d.snapshot().selected_vector_objects!=job.objects{return Err("Document changed during rasterization / ラスタライズ中に文書が変更されました / 栅格化期间文档已更改".into())}d.rasterize_vector_layer(&job.target,result.source)})?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> String {
        let mut state = Document::default().document_state();
        state.width = 32;
        state.height = 16;
        let mut d = Document::from_document_state(state).unwrap();
        let id = d.add_vector_layer().unwrap();
        let object=serde_json::from_value(serde_json::json!({"id":"shape","name":"Shape","path":{"data":"M4 4H20V12H4Z","fillRule":"nonZero"},"transform":[1,0,0,1,0,0],"fill":{"color":[200,80,20,128]},"stroke":null,"strokeWidth":0,"visible":true})).unwrap();
        d.upsert_vector_object(&id, object).unwrap();
        DOCUMENT.with(|slot| *slot.borrow_mut() = d);
        DOCUMENT_OPEN.with(|v| v.set(true));
        ACTIVE_DOCUMENT_ID.with(|v| v.set(910));
        id
    }
    #[test]
    fn rasterize_vector_layer_worker_preserves_alpha_and_restores_vectors() {
        let id = setup();
        let job = prepare(id.clone()).unwrap();
        let before = lumapaint_renderer::vector::rasterize_svg(&job.source, 32, 16)
            .unwrap()
            .pixels;
        let done = evaluate(job).unwrap();
        let image = lumapaint_renderer::vector::rasterize_svg(&done.source, 32, 16)
            .unwrap()
            .pixels;
        for (a, b) in before.iter().zip(&image) {
            assert!(a.abs_diff(*b) <= 2)
        }
        assert_eq!(image[3], 0);
        assert!((126..=130).contains(&image[((8 * 32 + 10) * 4 + 3) as usize]));
        commit(done).unwrap();
        DOCUMENT.with(|slot| {
            let mut d = slot.borrow_mut();
            let layer = d.svg_layers().find(|l| l.id == id).unwrap();
            assert!(layer.paint_layer && !layer.vector_layer);
            assert!(layer.vector_objects.is_empty());
            d.undo();
            let layer = d.svg_layers().find(|l| l.id == id).unwrap();
            assert!(layer.vector_layer);
            assert_eq!(layer.vector_objects.len(), 1);
            d.redo();
            assert!(d.svg_layers().find(|l| l.id == id).unwrap().paint_layer);
        });
    }
    #[test]
    fn rasterize_vector_layer_worker_rejects_stale_results() {
        let id = setup();
        let job = prepare(id.clone()).unwrap();
        DOCUMENT.with(|d| d.borrow_mut().set_bit_depth(16).unwrap());
        assert!(commit(Completed {
            job,
            source: String::new()
        })
        .is_err());
        let job = prepare(id.clone()).unwrap();
        ACTIVE_DOCUMENT_ID.with(|v| v.set(911));
        assert!(commit(Completed {
            job,
            source: String::new()
        })
        .is_err());
        assert!(DOCUMENT.with(|d| d
            .borrow()
            .svg_layers()
            .find(|l| l.id == id)
            .unwrap()
            .vector_layer));
    }
}
