use thiserror::Error;

pub const PF_PIXEL_FORMAT_ARGB32: i32 = 0x6267_7261;
pub const PF_PIXEL_FORMAT_ARGB64: i32 = 0x3631_6561;
pub const PF_PIXEL_FORMAT_ARGB128: i32 = 0x3233_6561;

const OUT_FLAG_DEEP_COLOR_AWARE: u32 = 1 << 25;
const OUT_FLAG2_FLOAT_COLOR_AWARE: u32 = 1 << 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FramePixelFormat {
    Argb8,
    Argb16,
    Argb32f,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PixelError {
    #[error("pixel format must be argb8, argb16, or argb32f")]
    UnsupportedFormat,
    #[error("pixel dimensions or byte count overflow")]
    SizeOverflow,
    #[error("{format} payload byte count {actual} is not a multiple of {stride}")]
    MisalignedPayload {
        format: &'static str,
        stride: usize,
        actual: usize,
    },
    #[error("expected {expected} {format} bytes, got {actual}")]
    ByteCount {
        format: &'static str,
        expected: usize,
        actual: usize,
    },
}

impl FramePixelFormat {
    pub fn parse(value: &str) -> Result<Self, PixelError> {
        match value {
            "argb8" => Ok(Self::Argb8),
            "argb16" => Ok(Self::Argb16),
            "argb32f" => Ok(Self::Argb32f),
            _ => Err(PixelError::UnsupportedFormat),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Argb8 => "argb8",
            Self::Argb16 => "argb16",
            Self::Argb32f => "argb32f",
        }
    }

    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Argb8 => 4,
            Self::Argb16 => 8,
            Self::Argb32f => 16,
        }
    }

    pub const fn bit_depth(self) -> i16 {
        match self {
            Self::Argb8 => 8,
            Self::Argb16 => 16,
            Self::Argb32f => 32,
        }
    }

    pub const fn world_flags(self) -> i32 {
        match self {
            Self::Argb8 => 0,
            Self::Argb16 | Self::Argb32f => 1,
        }
    }

    pub const fn pf_pixel_format(self) -> i32 {
        match self {
            Self::Argb8 => PF_PIXEL_FORMAT_ARGB32,
            Self::Argb16 => PF_PIXEL_FORMAT_ARGB64,
            Self::Argb32f => PF_PIXEL_FORMAT_ARGB128,
        }
    }

    pub const fn advertised_by(self, out_flags: u32, out_flags2: u32) -> bool {
        match self {
            Self::Argb8 => true,
            Self::Argb16 => out_flags & OUT_FLAG_DEEP_COLOR_AWARE != 0,
            Self::Argb32f => out_flags2 & OUT_FLAG2_FLOAT_COLOR_AWARE != 0,
        }
    }

    pub fn rowbytes(self, width: u32) -> Result<u32, PixelError> {
        width
            .checked_mul(self.bytes_per_pixel() as u32)
            .ok_or(PixelError::SizeOverflow)
    }

    pub fn byte_count(self, width: u32, height: u32) -> Result<usize, PixelError> {
        usize::try_from(self.rowbytes(width)?)
            .ok()
            .and_then(|rowbytes| rowbytes.checked_mul(height as usize))
            .ok_or(PixelError::SizeOverflow)
    }

    pub fn validate_bytes(self, width: u32, height: u32, bytes: &[u8]) -> Result<(), PixelError> {
        let expected = self.byte_count(width, height)?;
        if bytes.len() != expected {
            return Err(PixelError::ByteCount {
                format: self.name(),
                expected,
                actual: bytes.len(),
            });
        }
        Ok(())
    }

    pub fn promote_rgba8(self, rgba: &[u8]) -> Result<Vec<u8>, PixelError> {
        if !rgba.len().is_multiple_of(4) {
            return Err(PixelError::MisalignedPayload {
                format: "rgba8",
                stride: 4,
                actual: rgba.len(),
            });
        }
        let capacity = rgba
            .len()
            .checked_div(4)
            .and_then(|pixels| pixels.checked_mul(self.bytes_per_pixel()))
            .ok_or(PixelError::SizeOverflow)?;
        let mut output = Vec::with_capacity(capacity);
        for pixel in rgba.chunks_exact(4) {
            let channels = [pixel[3], pixel[0], pixel[1], pixel[2]];
            match self {
                Self::Argb8 => output.extend_from_slice(&channels),
                Self::Argb16 => {
                    for channel in channels {
                        let value = (u32::from(channel) * 32_768 + 127) / 255;
                        output.extend_from_slice(&(value as u16).to_le_bytes());
                    }
                }
                Self::Argb32f => {
                    for channel in channels {
                        output.extend_from_slice(&(f32::from(channel) / 255.0).to_le_bytes());
                    }
                }
            }
        }
        Ok(output)
    }

    pub fn to_argb8_preview(self, pixels: &[u8]) -> Result<Vec<u8>, PixelError> {
        if !pixels.len().is_multiple_of(self.bytes_per_pixel()) {
            return Err(PixelError::MisalignedPayload {
                format: self.name(),
                stride: self.bytes_per_pixel(),
                actual: pixels.len(),
            });
        }
        let mut output = Vec::with_capacity(pixels.len() / self.bytes_per_pixel() * 4);
        for pixel in pixels.chunks_exact(self.bytes_per_pixel()) {
            match self {
                Self::Argb8 => output.extend_from_slice(pixel),
                Self::Argb16 => {
                    for channel in pixel.chunks_exact(2) {
                        let value =
                            u32::from(u16::from_le_bytes([channel[0], channel[1]])).min(32_768);
                        output.push(((value * 255 + 16_384) / 32_768) as u8);
                    }
                }
                Self::Argb32f => {
                    for channel in pixel.chunks_exact(4) {
                        let mut value =
                            f32::from_le_bytes([channel[0], channel[1], channel[2], channel[3]]);
                        if !value.is_finite() {
                            value = 0.0;
                        }
                        output.push((value.clamp(0.0, 1.0) * 255.0).round() as u8);
                    }
                }
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_contract_matches_windows_fourcc_layout_and_capability_bits() {
        assert_eq!(FramePixelFormat::Argb8.pf_pixel_format(), 1_650_946_657);
        assert_eq!(FramePixelFormat::Argb16.pf_pixel_format(), 909_206_881);
        assert_eq!(FramePixelFormat::Argb32f.pf_pixel_format(), 842_229_089);
        assert_eq!(FramePixelFormat::Argb8.world_flags(), 0);
        assert_eq!(FramePixelFormat::Argb16.world_flags(), 1);
        assert_eq!(FramePixelFormat::Argb32f.world_flags(), 1);
        assert!(FramePixelFormat::Argb8.advertised_by(0, 0));
        assert!(!FramePixelFormat::Argb16.advertised_by(0, 0));
        assert!(FramePixelFormat::Argb16.advertised_by(1 << 25, 0));
        assert!(!FramePixelFormat::Argb32f.advertised_by(u32::MAX, 0));
        assert!(FramePixelFormat::Argb32f.advertised_by(0, 1 << 12));
    }

    #[test]
    fn rgba8_promotes_and_previews_with_windows_rounding() {
        let rgba = [255, 128, 0, 64];
        assert_eq!(
            FramePixelFormat::Argb8.promote_rgba8(&rgba).unwrap(),
            [64, 255, 128, 0]
        );
        let argb16 = FramePixelFormat::Argb16.promote_rgba8(&rgba).unwrap();
        assert_eq!(
            argb16
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>(),
            [8_224, 32_768, 16_448, 0]
        );
        assert_eq!(
            FramePixelFormat::Argb16.to_argb8_preview(&argb16).unwrap(),
            [64, 255, 128, 0]
        );
        let argb32f = FramePixelFormat::Argb32f.promote_rgba8(&rgba).unwrap();
        assert_eq!(
            FramePixelFormat::Argb32f
                .to_argb8_preview(&argb32f)
                .unwrap(),
            [64, 255, 128, 0]
        );
    }

    #[test]
    fn sizing_and_malformed_payloads_fail_closed() {
        assert_eq!(FramePixelFormat::Argb16.rowbytes(7).unwrap(), 56);
        assert_eq!(FramePixelFormat::Argb32f.byte_count(7, 5).unwrap(), 560);
        assert!(FramePixelFormat::parse("rgba8").is_err());
        assert!(
            FramePixelFormat::Argb16
                .validate_bytes(2, 2, &[0; 31])
                .is_err()
        );
        assert!(
            FramePixelFormat::Argb32f
                .to_argb8_preview(&[0; 15])
                .is_err()
        );
    }
}
