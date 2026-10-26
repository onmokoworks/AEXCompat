//! Depth conversion shared by the native and emulated image transports.
use crate::render_pixel_format::RenderPixelFormat;

/// Encode packed little-endian RGBA, retaining the decoded source depth.
/// AE 16-bpc white is 32768; float samples are not clamped to display range.
pub fn decoded_rgba(decoded: image::DynamicImage, format: RenderPixelFormat) -> Vec<u8> {
    match format {
        RenderPixelFormat::Argb8 => decoded.into_rgba8().into_raw(),
        RenderPixelFormat::Argb16 => decoded
            .into_rgba16()
            .into_raw()
            .into_iter()
            .flat_map(|value| (((u32::from(value) * 32768 + 32767) / 65535) as u16).to_le_bytes())
            .collect(),
        RenderPixelFormat::Argb32f => decoded
            .into_rgba32f()
            .into_raw()
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_rgba_keeps_legacy_eight_bit_conversion() {
        for sample in 0..=255u8 {
            let source = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                1,
                1,
                image::Rgba([sample; 4]),
            ));
            assert_eq!(
                decoded_rgba(source.clone(), RenderPixelFormat::Argb8),
                [sample; 4]
            );
            let word = ((u32::from(sample) * 32768 + 127) / 255) as u16;
            assert_eq!(
                decoded_rgba(source.clone(), RenderPixelFormat::Argb16),
                word.to_le_bytes().repeat(4)
            );
            assert_eq!(
                decoded_rgba(source, RenderPixelFormat::Argb32f),
                (f32::from(sample) / 255.0).to_le_bytes().repeat(4)
            );
        }
    }

    #[test]
    fn decoded_rgba_preserves_fine_sixteen_bit_and_hdr_samples() {
        let source = image::DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
            1,
            1,
            image::Rgba([20001u16, 7, 40001, 32769]),
        ));
        let expected = [10001u16, 4, 20001, 16385]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            decoded_rgba(source.clone(), RenderPixelFormat::Argb16),
            expected
        );
        let expected = [20001.0f32, 7.0, 40001.0, 32769.0]
            .into_iter()
            .flat_map(|v| (v / 65535.0).to_le_bytes())
            .collect::<Vec<_>>();
        assert_eq!(decoded_rgba(source, RenderPixelFormat::Argb32f), expected);
        let samples = [1.25f32, -0.125, 0.1001, -0.0];
        let source = image::DynamicImage::ImageRgba32F(image::ImageBuffer::from_pixel(
            1,
            1,
            image::Rgba(samples),
        ));
        assert_eq!(
            decoded_rgba(source, RenderPixelFormat::Argb32f),
            samples
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>()
        );
    }
}
