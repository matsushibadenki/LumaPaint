//! Thread-confined Metal cache rasterization. CPU fallback remains in vector.rs.
use metal::foreign_types::ForeignType;
use skia_safe::{gpu, ImageInfo, Surface};
use std::cell::RefCell;

enum State {
    Uninitialized,
    Ready(gpu::DirectContext),
    Unavailable,
}

thread_local! {
    // Ganesh contexts may not be shared concurrently across rendering workers.
    static CONTEXT: RefCell<State> = const { RefCell::new(State::Uninitialized) };
}

fn context() -> Option<gpu::DirectContext> {
    let device = metal::Device::all()
        .into_iter()
        .find(|device| !device.is_low_power())
        .or_else(metal::Device::system_default)?;
    let queue = device.new_command_queue();
    // BackendContext retains both handles; Ganesh takes its own references.
    let backend =
        unsafe { gpu::mtl::BackendContext::new(device.as_ptr().cast(), queue.as_ptr().cast()) };
    let mut context = gpu::direct_contexts::make_metal(&backend, None)?;
    context.set_resource_cache_limit(64 * 1024 * 1024);
    Some(context)
}

pub(super) fn rasterize(info: &ImageInfo, draw: impl FnOnce(&mut Surface)) -> Option<Vec<u8>> {
    CONTEXT.with(|slot| {
        let mut state = slot.try_borrow_mut().ok()?;
        if matches!(*state, State::Uninitialized) {
            *state = context().map_or(State::Unavailable, State::Ready);
        }
        let State::Ready(context) = &mut *state else {
            return None;
        };
        if context.abandoned() {
            *state = State::Unavailable;
            return None;
        }
        let mut surface = gpu::surfaces::render_target(
            context,
            gpu::Budgeted::Yes,
            info,
            Some(0),
            gpu::SurfaceOrigin::TopLeft,
            None,
            false,
            false,
        )?;
        draw(&mut surface);
        context.flush_and_submit();
        // This stays inside Rust. The existing wgpu upload contract consumes RGBA.
        // A failed readback/allocation must never discard the CPU-renderable edit.
        let mut pixels = vec![0; info.width() as usize * info.height() as usize * 4];
        if surface.read_pixels(info, &mut pixels, info.width() as usize * 4, (0, 0)) {
            Some(pixels)
        } else {
            *state = State::Unavailable;
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_gpu_returns_control_to_cpu() {
        CONTEXT.with(|state| *state.borrow_mut() = State::Unavailable);
        let info = ImageInfo::new_n32_premul((16, 16), None);
        assert!(rasterize(&info, |_| panic!("unavailable GPU must not draw")).is_none());
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="red"/></svg>"#;
        let result = super::super::rasterize_svg(source, 16, 16).unwrap();
        assert_eq!(result.backend, super::super::SvgBackend::Skia);
        assert_eq!(
            &result.pixels[(8 * 16 + 8) * 4..(8 * 16 + 8) * 4 + 4],
            &[255, 0, 0, 255]
        );
        CONTEXT.with(|state| *state.borrow_mut() = State::Uninitialized);
    }

    #[test]
    #[ignore = "requires a real Metal device"]
    fn metal_cache_renders_paths_and_text_with_premultiplied_pixels() {
        CONTEXT.with(|state| *state.borrow_mut() = State::Uninitialized);
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="red" fill-opacity="0.5"/></svg>"#;
        let gpu = super::super::rasterize_svg(source, 64, 64).unwrap();
        assert_eq!(gpu.backend, super::super::SvgBackend::SkiaGpu);
        CONTEXT.with(|state| *state.borrow_mut() = State::Unavailable);
        let cpu = super::super::rasterize_svg(source, 64, 64).unwrap();
        for (actual, expected) in gpu.pixels.iter().zip(&cpu.pixels) {
            assert!(actual.abs_diff(*expected) <= 1);
        }
        CONTEXT.with(|state| *state.borrow_mut() = State::Uninitialized);
        let text = r#"<svg xmlns="http://www.w3.org/2000/svg" width="180" height="80"><text x="5" y="40" font-size="24" font-family="sans-serif">GPU 日本語</text></svg>"#;
        let result = super::super::rasterize_svg(text, 180, 80).unwrap();
        assert_eq!(result.backend, super::super::SvgBackend::SkiaGpu);
        assert!(result
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0));
    }
}
