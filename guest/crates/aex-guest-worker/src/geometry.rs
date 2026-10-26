//! Session geometry. Raw files contain packed, already-downsampled pixels.
use aex_abi::x86_64_windows as abi;
use std::sync::OnceLock;
#[derive(Clone, Debug)]
pub struct Geometry {
    pub full: [u32; 2],
    pub scale: [[i32; 2]; 2],
    pub aspect: [i32; 2],
    pub checkout: Option<[i32; 4]>,
    pub crop: bool,
    pub negotiate: bool,
    pub pad: u32,
    pub extent: Option<[i32; 4]>,
    pub request: Option<[i32; 4]>,
}
pub static SESSION: OnceLock<Geometry> = OnceLock::new();
impl Default for Geometry {
    fn default() -> Self {
        Self {
            full: [0, 0],
            scale: [[1, 1]; 2],
            aspect: [1, 1],
            checkout: None,
            crop: false,
            negotiate: false,
            pad: 0,
            extent: None,
            request: None,
        }
    }
}
pub fn parse(option: &str, value: &str, g: &mut Geometry) -> Result<(), String> {
    if option == "--checkout-rect" && value == "auto" {
        g.crop = true;
        return Ok(());
    }
    let v = value
        .split(|c| c == '/' || c == ',')
        .map(str::parse::<i32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    match option {
        "--negotiate-output" => {
            if v.len() != 1 || !(0..=1).contains(&v[0]) {
                return Err("expected 0 or 1".into());
            }
            g.negotiate = v[0] == 1;
        }
        "--downsample-x" | "--downsample-y" | "--pixel-aspect" => {
            if v.len() != 2 || v[0] <= 0 || v[1] <= 0 {
                return Err("expected positive num/den".into());
            }
            let r = [v[0], v[1]];
            if option == "--pixel-aspect" {
                g.aspect = r;
            } else {
                if r[0] > r[1] {
                    return Err("downsample must be <=1".into());
                }
                g.scale[usize::from(option == "--downsample-y")] = r;
            }
        }
        "--rowbytes-pad" => {
            if v.len() != 1 || !(0..=256).contains(&v[0]) {
                return Err("padding must be 0..256".into());
            }
            g.pad = v[0] as u32;
        }
        "--extent-hint" | "--request-rect" | "--checkout-rect" => {
            if v.len() != 4 {
                return Err("expected left,top,right,bottom".into());
            }
            let r = [v[0], v[1], v[2], v[3]];
            if option == "--checkout-rect" {
                g.checkout = Some(r);
                g.crop = true;
            } else if option == "--extent-hint" {
                g.extent = Some(r);
            } else {
                g.request = Some(r);
            }
        }
        _ => return Err("unknown geometry option".into()),
    }
    Ok(())
}
impl Geometry {
    pub fn sampled(&self) -> Result<[u32; 2], String> {
        if self.crop && !self.negotiate {
            return Err("checkout rectangles require --negotiate-output 1".into());
        }
        let mut out = [0; 2];
        for i in 0..2 {
            if self.full[i] == 0 || self.full[i] > i32::MAX as u32 {
                return Err("invalid full dimension".into());
            }
            out[i] = ((u64::from(self.full[i]) * self.scale[i][0] as u64 + self.scale[i][1] as u64
                - 1)
                / self.scale[i][1] as u64) as u32;
        }
        for r in [
            self.checkout,
            self.extent,
            if self.negotiate { None } else { self.request },
        ]
        .into_iter()
        .flatten()
        {
            if r[0] < 0
                || r[1] < 0
                || r[2] <= r[0]
                || r[3] <= r[1]
                || r[2] > out[0] as i32
                || r[3] > out[1] as i32
            {
                return Err("rectangle outside sampled world".into());
            }
        }
        Ok(out)
    }
    pub fn world(&self) -> Result<[u32; 2], String> {
        let full = self.sampled()?;
        Ok(self
            .checkout
            .map(|r| [(r[2] - r[0]) as u32, (r[3] - r[1]) as u32])
            .unwrap_or(full))
    }
}
fn put(b: &mut [u8], at: usize, v: i32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
pub fn input(b: &mut [u8]) {
    if let Some(g) = SESSION.get() {
        put(b, abi::IN_WIDTH_OFFSET, g.full[0] as i32);
        put(b, abi::IN_HEIGHT_OFFSET, g.full[1] as i32);
        for (at, r) in [
            (abi::IN_DOWNSAMPLE_X_OFFSET, g.scale[0]),
            (abi::IN_DOWNSAMPLE_Y_OFFSET, g.scale[1]),
            (abi::IN_PIXEL_ASPECT_RATIO_OFFSET, g.aspect),
        ] {
            put(b, at, r[0]);
            put(b, at + 4, r[1]);
        }
        if let Some(r) = g.extent {
            for (i, v) in r.into_iter().enumerate() {
                put(b, abi::IN_EXTENT_HINT_OFFSET + i * 4, v);
            }
        }
    }
}
pub fn world(b: &mut [u8]) {
    if let Some(g) = SESSION.get() {
        put(b, abi::LAYER_PIX_ASPECT_RATIO_OFFSET, g.aspect[0]);
        put(b, abi::LAYER_PIX_ASPECT_RATIO_OFFSET + 4, g.aspect[1]);
    }
}

// Last primary checkout origin, for PF_InData output_origin (input in output coords).
pub static CHECKOUT_ORIGIN: std::sync::Mutex<[i32; 2]> = std::sync::Mutex::new([0, 0]);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sampled_and_checkout_dimensions_are_independent() {
        let mut g = Geometry {
            full: [41, 31],
            ..Geometry::default()
        };
        parse("--downsample-x", "1/2", &mut g).unwrap();
        parse("--downsample-y", "1/3", &mut g).unwrap();
        assert_eq!(g.world().unwrap(), [21, 11]);
        parse("--negotiate-output", "1", &mut g).unwrap();
        parse("--checkout-rect", "3,4,20,10", &mut g).unwrap();
        assert_eq!(g.sampled().unwrap(), [21, 11]);
        assert_eq!(g.world().unwrap(), [17, 6]);
        assert_eq!(g.full, [41, 31]);
    }
    #[test]
    fn rejects_invalid_geometry_and_requires_negotiation_for_cropping() {
        let mut g = Geometry {
            full: [40, 30],
            ..Geometry::default()
        };
        for (flag, value) in [
            ("--downsample-x", "0/1"),
            ("--downsample-y", "2/1"),
            ("--pixel-aspect", "1/0"),
            ("--rowbytes-pad", "257"),
            ("--rowbytes-pad", "-1"),
            ("--checkout-rect", "0,0,1"),
        ] {
            assert!(parse(flag, value, &mut g).is_err());
        }
        parse("--checkout-rect", "auto", &mut g).unwrap();
        assert!(g.world().is_err());
        parse("--negotiate-output", "1", &mut g).unwrap();
        assert_eq!(g.world().unwrap(), [40, 30]);
        parse("--checkout-rect", "0,0,41,30", &mut g).unwrap();
        assert!(g.world().is_err());
    }
    #[test]
    fn full_hd_and_uhd_dimensions_preserve_pixel_counts() {
        for full in [[1920, 1080], [3840, 2160]] {
            let mut g = Geometry {
                full,
                ..Geometry::default()
            };
            assert_eq!(g.world().unwrap(), full);
            parse("--rowbytes-pad", "3", &mut g).unwrap();
            parse("--pixel-aspect", "10/11", &mut g).unwrap();
            assert_eq!(g.world().unwrap(), full);
        }
    }
}
