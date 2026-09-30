use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderPixelFormat {
    #[default]
    Argb8,
    Argb16,
    Argb32f,
}

impl RenderPixelFormat {
    pub fn report_name(self) -> &'static str {
        match self {
            Self::Argb8 => "argb8",
            Self::Argb16 => "argb16",
            Self::Argb32f => "argb32f",
        }
    }

    pub(crate) fn bytes_per_pixel(self) -> u64 {
        match self {
            Self::Argb8 => 4,
            Self::Argb16 => 8,
            Self::Argb32f => 16,
        }
    }

    pub(crate) fn raw_extension(self) -> Option<&'static str> {
        match self {
            Self::Argb8 => None,
            Self::Argb16 => Some("rgba16le"),
            Self::Argb32f => Some("rgba32f-le"),
        }
    }
}
