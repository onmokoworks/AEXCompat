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
