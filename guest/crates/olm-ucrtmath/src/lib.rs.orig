//! Safe float-only interface; mode is fixed on first use for the worker lifetime.
use std::sync::OnceLock;
fn mode() -> i32 {
    static MODE: OnceLock<i32> = OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("OLM_UCRT_FMA") {
        Err(std::env::VarError::NotPresent) => 1,
        Ok(s) if s == "1" => 1,
        Ok(s) if s == "0" => 0,
        _ => panic!("OLM_UCRT_FMA must be 0 or 1"),
    })
}
unsafe extern "C" {
    fn ucrt_sinf(x: f32, mode: i32) -> f32;
    fn ucrt_cosf(x: f32, mode: i32) -> f32;
    fn ucrt_log2f(x: f32, mode: i32) -> f32;

    fn ucrt_expf(x: f32, mode: i32) -> f32;
    fn ucrt_atan2f(y: f32, x: f32, mode: i32) -> f32;
    fn ucrt_powf(x: f32, y: f32, mode: i32) -> f32;
}
pub fn expf(x: f32) -> f32 { unsafe { ucrt_expf(x, mode()) } }
pub fn atan2f(y: f32, x: f32) -> f32 { unsafe { ucrt_atan2f(y, x, mode()) } }
pub fn powf(x: f32, y: f32) -> f32 { unsafe { ucrt_powf(x, y, mode()) } }

#[cfg(test)]
include!("tests_expf.rs");

#[cfg(test)]
include!("tests_atan2f.rs");

#[cfg(test)]
include!("tests_powf.rs");

#[cfg(test)]
include!("tests_sinf.rs");

#[cfg(test)]
include!("tests_cosf.rs");

#[test]
fn log2f_ieee_boundaries() {
    for mode in [0, 1] {
        for (input, expected) in [(0x3f800000,0), (0x40000000,0x3f800000), (0,0xff800000),
                                  (0xbf800000,0xffc00000), (0x7f812345,0x7fc12345)] {
            assert_eq!(unsafe { ucrt_log2f(f32::from_bits(input),mode) }.to_bits(), expected);
        }
    }
}

pub fn sinf(x: f32) -> f32 { unsafe { ucrt_sinf(x, mode()) } }
pub fn cosf(x: f32) -> f32 { unsafe { ucrt_cosf(x, mode()) } }
pub fn log2f(x: f32) -> f32 { unsafe { ucrt_log2f(x, mode()) } }
