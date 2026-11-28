use super::app::HarnessApp;
use eframe::egui;
use eframe::egui::{Color32, RichText};
use std::path::{Path, PathBuf};

pub(super) fn load_preview(
    ctx: &egui::Context,
    texture_name: &str,
    path: &Path,
) -> Result<egui::TextureHandle, String> {
    let image = decode_preview_image(path)?;
    Ok(ctx.load_texture(texture_name, image, PREVIEW_TEXTURE_OPTIONS))
}

/// Previews scaled above 100% keep each source pixel square so 1px results
/// (dither, halftone, anti-aliasing) stay readable; downscaled previews stay
/// smooth.
pub(super) const PREVIEW_TEXTURE_OPTIONS: egui::TextureOptions = egui::TextureOptions {
    magnification: egui::TextureFilter::Nearest,
    ..egui::TextureOptions::LINEAR
};

pub(super) fn decode_preview_image(path: &Path) -> Result<egui::ColorImage, String> {
    let image = image::open(path).map_err(|error| format!("image decode failed: {error}"))?;
    let rgba = image.into_rgba8();
    if rgba.width() == 0 || rgba.height() == 0 {
        return Err("decoded image has zero width or height".into());
    }
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        rgba.as_raw(),
    ))
}

pub(super) fn is_supported_input_image(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff" | "webp"
                )
            })
}

pub(super) fn canonical_deverbatim(path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("path could not be resolved: {error}"))?;
    let text = canonical.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        Ok(PathBuf::from(format!(r"\\{rest}")))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        Ok(PathBuf::from(rest))
    } else {
        Ok(canonical)
    }
}

pub(super) fn show_preview(ui: &mut egui::Ui, label: &str, texture: Option<&egui::TextureHandle>) {
    ui.label(RichText::new(label).strong());
    let Some(texture) = texture else {
        ui.label("Not available");
        return;
    };
    let available = ui.available_width().max(1.0);
    let scale = (available / texture.size()[0] as f32).min(1.0);
    ui.image((
        texture.id(),
        egui::vec2(
            texture.size()[0] as f32 * scale,
            texture.size()[1] as f32 * scale,
        ),
    ));
}

pub(super) fn show_viewer_texture(
    ui: &mut egui::Ui,
    label: &str,
    texture: Option<&egui::TextureHandle>,
    zoom: &mut f32,
    pan: &mut egui::Vec2,
) {
    let Some(texture) = texture else {
        ui.centered_and_justified(|ui| {
            ui.label(format!("{label} is not available"));
        });
        return;
    };
    let source = egui::vec2(texture.size()[0] as f32, texture.size()[1] as f32);
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong());
        ui.monospace(format!("{} x {}", texture.size()[0], texture.size()[1]));
        ui.weak("Wheel to zoom / drag to pan");
    });
    let available = ui.available_size().max(egui::vec2(1.0, 1.0));
    let (viewport, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());
    if response.hovered() {
        let scroll = ui.input(|input| input.raw_scroll_delta.y);
        if scroll != 0.0 {
            let previous_zoom = *zoom;
            *zoom = (*zoom * (scroll * 0.0025).exp()).clamp(0.25, 8.0);
            if let Some(pointer) = response.hover_pos() {
                let pointer_from_center = pointer - viewport.center();
                *pan = pointer_from_center - (pointer_from_center - *pan) * (*zoom / previous_zoom);
            }
        }
    }
    if response.dragged_by(egui::PointerButton::Primary)
        || response.dragged_by(egui::PointerButton::Middle)
    {
        *pan += response.drag_delta();
    }
    let fit_scale = (available.x / source.x).min(available.y / source.y);
    let display = source * fit_scale * *zoom;
    let image_rect = egui::Rect::from_center_size(viewport.center() + *pan, display);
    ui.painter().with_clip_rect(viewport).image(
        texture.id(),
        image_rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

pub(super) fn run_gui(repository: PathBuf) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AEXCompat Image Harness",
        options,
        Box::new(move |_cc| Ok(Box::new(HarnessApp::new(repository)))),
    )
}
