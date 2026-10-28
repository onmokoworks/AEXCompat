/// Packed little-endian RGBA at the requested AE depth (16-bit white = 32768).
/// The dimensions and format travel with the bytes; PNG previews are never inputs.
/// Float input preserves all IEEE-754 words. Callers reading rendered output
/// must enforce their output-finiteness contract separately.
pub struct NativeImageInput {
    pub width: u32,
    pub height: u32,
    pub format: RenderPixelFormat,
    pub rgba: Vec<u8>,
}

impl NativeImageInput {
    pub fn validate(&self) -> io::Result<()> {
        let bpp = self.format.bytes_per_pixel();
        crate::render_request::validate_image_buffer_layout(
            self.width.into(),
            self.height.into(),
            u64::from(self.width) * bpp,
            bpp,
            Some(self.rgba.len() as u64),
            MAX_DIMENSION.into(),
            MAX_PIXELS,
            MAX_PIXELS * 16,
        )?;
        if self.width == 0
            || self.height == 0
            || self.rgba.len() as u64 != u64::from(self.width) * u64::from(self.height) * bpp
        {
            return Err(invalid(
                "native input byte count does not match dimensions/depth",
            ));
        }
        // All IEEE-754 input words are transport data, including NaN payloads.
        // Layout/size limits above and worker output validation remain intact.
        Ok(())
    }

    pub fn decode(path: &Path, format: RenderPixelFormat) -> io::Result<Self> {
        let decoded = decode_bounded_image_for_format(path, "native input", format)?;
        Self::from_decoded(decoded, format)
    }

    fn from_decoded(decoded: image::DynamicImage, format: RenderPixelFormat) -> io::Result<Self> {
        let (width, height) = (decoded.width(), decoded.height());
        let rgba = crate::decoded_image::decoded_rgba(decoded, format);
        let image = Self {
            width,
            height,
            format,
            rgba,
        };
        image.validate()?;
        Ok(image)
    }

    pub fn read_raw(
        path: &Path,
        width: u32,
        height: u32,
        format: RenderPixelFormat,
    ) -> io::Result<Self> {
        let bpp = format.bytes_per_pixel();
        crate::render_request::validate_image_buffer_layout(
            width.into(),
            height.into(),
            u64::from(width) * bpp,
            bpp,
            None,
            MAX_DIMENSION.into(),
            MAX_PIXELS,
            MAX_PIXELS * 16,
        )?;
        let expected = u64::from(width) * u64::from(height) * bpp;
        let file = fs::File::open(path)?;
        if file.metadata()?.len() != expected {
            return Err(invalid(
                "native output byte count does not match dimensions/depth",
            ));
        }
        let mut rgba = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(file, expected + 1), &mut rgba)?;
        let image = Self {
            width,
            height,
            format,
            rgba,
        };
        image.validate()?;
        Ok(image)
    }
}

#[cfg(test)]
mod native_input_tests {
    use super::*;

    #[test]
    fn native_decode_admits_4k_deep_images_beyond_legacy_allocation_limit() {
        let root =
            std::env::temp_dir().join(format!("aexcompat-deep-decode-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let png = root.join("deep16.png");
        image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
            4096,
            2160,
            image::Rgba([20001, 7, 40001, 32769]),
        )
        .save(&png)
        .unwrap();
        assert!(decode_bounded_image(&png, "legacy").is_err());
        let decoded =
            decode_bounded_image_for_format(&png, "chain stage", RenderPixelFormat::Argb16)
                .unwrap();
        assert_eq!(
            (decoded.width(), decoded.height(), decoded.color()),
            (4096, 2160, image::ColorType::Rgba16)
        );
        drop(decoded);
        let tiff = root.join("float.tiff");
        image::ImageBuffer::<image::Rgba<f32>, Vec<f32>>::from_pixel(
            3840,
            2160,
            image::Rgba([1.25, -0.125, 0.1001, 0.5]),
        )
        .save(&tiff)
        .unwrap();
        assert!(decode_bounded_image(&tiff, "legacy").is_err());
        let native = NativeImageInput::decode(&tiff, RenderPixelFormat::Argb32f).unwrap();
        assert_eq!((native.width, native.height), (3840, 2160));
        assert_eq!(
            f32::from_le_bytes(native.rgba[..4].try_into().unwrap()),
            1.25
        );
        assert_eq!(
            f32::from_le_bytes(native.rgba[4..8].try_into().unwrap()),
            -0.125
        );
        fs::remove_file(png).unwrap();
        fs::remove_file(tiff).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn native_input_preserves_precision_and_rejects_malformed_layout() {
        let mut input = NativeImageInput {
            width: 1,
            height: 1,
            format: RenderPixelFormat::Argb32f,
            rgba: [1.25f32, -0.125, 0.1001, 0.5]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect(),
        };
        assert!(input.validate().is_ok());
        input.format = RenderPixelFormat::Argb16;
        assert!(input.validate().is_err());
        input.format = RenderPixelFormat::Argb32f;
        let words = [0x7fc12345u32, 0x7f800001, 0x7f800000, 0xff800000];
        input.rgba = words.into_iter().flat_map(u32::to_le_bytes).collect();
        assert!(input.validate().is_ok());
        let root = std::env::temp_dir().join(format!("native-nonfinite-{}", std::process::id()));
        fs::write(&root, &input.rgba).unwrap();
        let loaded = NativeImageInput::read_raw(&root, 1, 1, RenderPixelFormat::Argb32f).unwrap();
        assert_eq!(loaded.rgba, input.rgba);
        fs::remove_file(root).unwrap();
        input.rgba.pop();
        assert!(input.validate().is_err());
        input.width = 0;
        assert!(input.validate().is_err());
        let encoded = image::DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
            1,
            1,
            image::Rgba([20001u16, 7, 40001, 32769]),
        ));
        let decoded = NativeImageInput::from_decoded(encoded, RenderPixelFormat::Argb16).unwrap();
        let samples: Vec<_> = decoded
            .rgba
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(samples, [10001, 4, 20001, 16385]);
    }
}

/// Runs one chain stage using the existing isolated session and provenance path.
pub fn render_native_image_with_dependencies(
    repository: &Path,
    plugin_path: &Path,
    inspected_sha256: &str,
    input: &NativeImageInput,
    output_path: &Path,
    parameters: &[InteractiveParameter],
    timing: RenderTiming,
    smart: bool,
    host_context: Option<&crate::render_request::HostContext>,
    custom_ui_action: Option<RenderUiAction>,
    gpu_backend: RenderGpuBackend,
    dependencies: Vec<ApprovedImageArtifact>,
    search_dirs: Vec<PathBuf>,
) -> io::Result<Value> {
    input.validate()?;
    let actual = observe_selected_plugin_bytes(&fs::read(plugin_path)?, inspected_sha256)?;
    render_with_artifact(
        repository,
        "experimental",
        plugin_path,
        &actual,
        interactive_frame_deadline_ms()?,
        Path::new(""),
        output_path,
        Some(encode_interactive_payload(parameters)?),
        Some(parameters),
        host_context,
        timing,
        smart,
        input.format,
        gpu_backend,
        custom_ui_action,
        None,
        None,
        None,
        dependencies,
        search_dirs,
        None,
        input.format == RenderPixelFormat::Argb16,
        None,
        Some(input),
    )
}
