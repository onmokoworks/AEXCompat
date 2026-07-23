//! Native half of the YMM4 bridge.
//!
//! YMM4 owns the host/UI thread, while `RenderSession` owns process handles and
//! mapped transport state that must stay on one Rust thread. The C ABI therefore
//! exposes a small opaque handle and sends every frame through that dedicated
//! thread, matching the AviUtl2 bridge's crash-containment boundary.

use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use aexcompat_broker::image_render::{
    InteractiveParameter, RenderGpuBackend, RenderPixelFormat,
    inspect_experimental_with_diagnostics,
};
use aexcompat_broker::render_session::{FrameStatus, RenderSession, SessionOpenRequest};
use serde_json;
use sha2::{Digest, Sha256};

const FRAME_DEADLINE_MS: u64 = 30_000;

struct RenderRequest {
    frame_index: u32,
    current_time: i32,
    rgba: Vec<u8>,
    parameters: Option<Vec<InteractiveParameter>>,
    reply: Sender<RenderReply>,
}

enum RenderReply {
    Rendered {
        pixels: Vec<u8>,
        width: u32,
        height: u32,
    },
    Error(String),
}

pub struct AexYmm4Session {
    tx: Option<Sender<RenderRequest>>,
    join: Option<JoinHandle<()>>,
    last_error: Mutex<String>,
}

static LAST_OPEN_ERROR: OnceLock<Mutex<String>> = OnceLock::new();

fn open_error_slot() -> &'static Mutex<String> {
    LAST_OPEN_ERROR.get_or_init(|| Mutex::new(String::new()))
}

fn set_open_error(error: impl Into<String>) {
    if let Ok(mut slot) = open_error_slot().lock() {
        *slot = error.into();
    }
}

fn utf16_path(pointer: *const u16) -> Result<PathBuf, String> {
    if pointer.is_null() {
        return Err("null UTF-16 path".to_string());
    }
    // The managed caller passes a NUL-terminated UTF-16 string. Reading until
    // NUL is safe for that ABI contract and avoids a platform-specific wchar_t
    // size assumption on the Rust side.
    let mut length = 0usize;
    unsafe {
        while *pointer.add(length) != 0 {
            length = length
                .checked_add(1)
                .ok_or_else(|| "UTF-16 path is too long".to_string())?;
        }
        String::from_utf16(slice::from_raw_parts(pointer, length))
            .map(PathBuf::from)
            .map_err(|error| format!("invalid UTF-16 path: {error}"))
    }
}

fn plugin_sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("read AEX {}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn discover_parameters(
    repository: &Path,
    plugin: &Path,
    plugin_sha256: &str,
) -> Result<Vec<InteractiveParameter>, String> {
    inspect_experimental_with_diagnostics(repository, plugin, plugin_sha256)
        .map(|(parameters, _)| exposed_parameters(&parameters))
        .map_err(|error| format!("AEX parameter discovery failed: {error}"))
}

/// Keep the YMM4 surface aligned with the InteractiveParameter subset that can
/// be safely sent through the existing v:2 parameter payload. Unsupported
/// kinds stay at the AEX default instead of becoming a malformed render.
fn exposed_parameters(parameters: &[InteractiveParameter]) -> Vec<InteractiveParameter> {
    parameters
        .iter()
        .filter(|parameter| parameter.visible)
        .filter_map(|parameter| {
            let mut parameter = parameter.clone();
            match parameter.kind.as_str() {
                "float" if bounded_range(&parameter) => Some(parameter),
                "integer" if !parameter.choices.is_empty() => {
                    let count = parameter.choices.len() as f64;
                    parameter.minimum = 1.0;
                    parameter.maximum = count;
                    parameter.value = parameter.value.clamp(1.0, count);
                    Some(parameter)
                }
                "integer" if bounded_range(&parameter) => Some(parameter),
                "color" => Some(parameter),
                _ => None,
            }
        })
        .collect()
}

fn bounded_range(parameter: &InteractiveParameter) -> bool {
    parameter.minimum.is_finite()
        && parameter.maximum.is_finite()
        && parameter.minimum < parameter.maximum
}

fn open_session(
    repository: PathBuf,
    plugin: PathBuf,
    plugin_sha256: String,
    width: u32,
    height: u32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
    smart: bool,
    parameters: Vec<InteractiveParameter>,
    rx: Receiver<RenderRequest>,
) -> Result<(), String> {
    let baseline = (!parameters.is_empty()).then_some(parameters.as_slice());
    let mut session = RenderSession::open(SessionOpenRequest {
        repository: &repository,
        plugin_path: &plugin,
        plugin_sha256: &plugin_sha256,
        parameters: baseline,
        parameter_animation: None,
        aux_manifest: None,
        world_dump_dir: None,
        output_checksum_detail: false,
        mask_trailer: None,
        spatial_trailer: None,
        render_environment_trailer: None,
        audio_trailer: None,
        alpha_as_coverage_params: &[],
        conformance_render_settings: None,
        layers: &[],
        dependencies: Vec::new(),
        width,
        height,
        pixel_format: RenderPixelFormat::Argb8,
        time_step,
        total_time,
        time_scale,
        frame_deadline: Duration::from_millis(FRAME_DEADLINE_MS),
        smart,
        gpu_backend: RenderGpuBackend::Auto,
        gpu_runtime_policy: None,
        payload_override: None,
    })
    .map_err(|error| format!("RenderSession::open failed: {error}"))?;

    for request in rx {
        let reply = match session.render_frame_with_parameters(
            request.frame_index,
            request.current_time,
            &request.rgba,
            request.parameters.as_deref(),
        ) {
            Ok(outcome) => match outcome.status {
                FrameStatus::Rendered {
                    pixels,
                    width,
                    height,
                    ..
                } => RenderReply::Rendered {
                    pixels,
                    width,
                    height,
                },
                FrameStatus::FrameError {
                    render_error,
                    missing_dependency,
                } => RenderReply::Error(format!(
                    "AEX frame error {render_error}{}",
                    missing_dependency
                        .as_deref()
                        .map(|value| format!("; missing dependency: {value}"))
                        .unwrap_or_default()
                )),
            },
            Err(error) => {
                RenderReply::Error(format!("RenderSession::render_frame failed: {error}"))
            }
        };
        if request.reply.send(reply).is_err() {
            break;
        }
    }

    let _ = session.close();
    Ok(())
}

impl Drop for AexYmm4Session {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aexcompat_ymm4_open(
    repository: *const u16,
    plugin: *const u16,
    width: u32,
    height: u32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
    smart: u8,
) -> *mut AexYmm4Session {
    let result = (|| {
        let repository = utf16_path(repository)?;
        let plugin = utf16_path(plugin)?;
        if width == 0 || height == 0 || time_step <= 0 || total_time <= 0 || time_scale == 0 {
            return Err("invalid YMM4 session geometry or timing".to_string());
        }
        let plugin_sha256 = plugin_sha256(&plugin)?;
        let parameters =
            discover_parameters(&repository, &plugin, &plugin_sha256).unwrap_or_default();
        let (tx, rx) = channel::<RenderRequest>();
        let (open_tx, open_rx) = channel::<Result<(), String>>();
        let join = thread::Builder::new()
            .name("aex-ymm4-session".to_string())
            .spawn(move || {
                let result = open_session(
                    repository,
                    plugin,
                    plugin_sha256,
                    width,
                    height,
                    time_step,
                    total_time,
                    time_scale,
                    smart != 0,
                    parameters,
                    rx,
                );
                if let Err(error) = &result {
                    let _ = open_tx.send(Err(error.clone()));
                } else {
                    let _ = open_tx.send(Ok(()));
                }
            })
            .map_err(|error| format!("spawn session thread failed: {error}"))?;
        match open_rx.recv() {
            Ok(Ok(())) => Ok(AexYmm4Session {
                tx: Some(tx),
                join: Some(join),
                last_error: Mutex::new(String::new()),
            }),
            Ok(Err(error)) => {
                let _ = join.join();
                Err(error)
            }
            Err(error) => {
                let _ = join.join();
                Err(format!("session open handshake failed: {error}"))
            }
        }
    })();

    match result {
        Ok(session) => Box::into_raw(Box::new(session)),
        Err(error) => {
            set_open_error(error);
            ptr::null_mut()
        }
    }
}

/// Discover the bounded parameter set for the managed YMM4 property editor.
/// The caller owns the UTF-8 output buffer; failures are available through the
/// existing last-error ABI so the managed bridge can remain pass-through.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aexcompat_ymm4_discover(
    repository: *const u16,
    plugin: *const u16,
    output: *mut u8,
    output_len: usize,
) -> i32 {
    let result = (|| {
        let repository = utf16_path(repository)?;
        let plugin = utf16_path(plugin)?;
        let plugin_sha256 = plugin_sha256(&plugin)?;
        let parameters = discover_parameters(&repository, &plugin, &plugin_sha256)?;
        let bytes = serde_json::to_vec(&parameters)
            .map_err(|error| format!("serialize YMM4 parameter metadata failed: {error}"))?;
        if output.is_null() || output_len < bytes.len() {
            return Err(format!(
                "YMM4 parameter metadata buffer is too small (need {} bytes)",
                bytes.len()
            ));
        }
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
        i32::try_from(bytes.len()).map_err(|_| "YMM4 parameter metadata is too large".to_string())
    })();

    match result {
        Ok(length) => length,
        Err(error) => {
            set_open_error(error);
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aexcompat_ymm4_render(
    session: *mut AexYmm4Session,
    frame_index: u32,
    current_time: i32,
    rgba: *const u8,
    rgba_len: usize,
    output: *mut u8,
    output_len: usize,
    output_width: *mut u32,
    output_height: *mut u32,
    parameters: *const u8,
    parameters_len: usize,
) -> i32 {
    let result = (|| {
        if session.is_null() || rgba.is_null() || output.is_null() {
            return Err("null YMM4 render argument".to_string());
        }
        if output_width.is_null() || output_height.is_null() {
            return Err("null YMM4 output dimension argument".to_string());
        }
        let parameters = if parameters_len == 0 {
            None
        } else {
            if parameters.is_null() {
                return Err("null YMM4 parameter payload".to_string());
            }
            let bytes = unsafe { slice::from_raw_parts(parameters, parameters_len) };
            let decoded = serde_json::from_slice::<Vec<InteractiveParameter>>(bytes)
                .map_err(|error| format!("invalid YMM4 parameter payload: {error}"))?;
            let exposed = exposed_parameters(&decoded);
            (!exposed.is_empty()).then_some(exposed)
        };
        let session = unsafe { &mut *session };
        let rgba = unsafe { slice::from_raw_parts(rgba, rgba_len) }.to_vec();
        let (reply_tx, reply_rx) = channel();
        session
            .tx
            .as_ref()
            .ok_or_else(|| "YMM4 session is closed".to_string())?
            .send(RenderRequest {
                frame_index,
                current_time,
                rgba,
                parameters,
                reply: reply_tx,
            })
            .map_err(|_| "YMM4 session thread stopped".to_string())?;
        match reply_rx
            .recv()
            .map_err(|_| "YMM4 render reply lost".to_string())?
        {
            RenderReply::Rendered {
                pixels,
                width,
                height,
            } => {
                if pixels.len() > output_len {
                    return Err(format!(
                        "AEX output {} bytes exceeds YMM4 buffer {output_len}",
                        pixels.len()
                    ));
                }
                unsafe {
                    ptr::copy_nonoverlapping(pixels.as_ptr(), output, pixels.len());
                    *output_width = width;
                    *output_height = height;
                }
                Ok(())
            }
            RenderReply::Error(error) => Err(error),
        }
    })();

    match result {
        Ok(()) => 0,
        Err(error) => {
            if !session.is_null() {
                if let Ok(mut slot) = unsafe { (&*session).last_error.lock() } {
                    *slot = error;
                }
            }
            -1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aexcompat_ymm4_last_error(
    session: *const AexYmm4Session,
    output: *mut u8,
    output_len: usize,
) -> usize {
    let text = if session.is_null() {
        open_error_slot()
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| "unknown YMM4 open error".to_string())
    } else {
        unsafe { (&*session).last_error.lock() }
            .map(|value| value.clone())
            .unwrap_or_else(|_| "unknown YMM4 session error".to_string())
    };
    if output.is_null() || output_len == 0 {
        return text.len();
    }
    let bytes = text.as_bytes();
    let copied = bytes.len().min(output_len.saturating_sub(1));
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), output, copied);
        *output.add(copied) = 0;
    }
    bytes.len()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aexcompat_ymm4_close(session: *mut AexYmm4Session) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

// Keep the opaque handle clearly non-constructible from managed code and make
// accidental use in Rust tests impossible without going through `open`.
const _: Option<*mut c_void> = None;

#[cfg(test)]
mod tests {
    use super::*;

    fn parameter(kind: &str, minimum: f64, maximum: f64) -> InteractiveParameter {
        InteractiveParameter {
            slot: 1,
            name: "fixture".to_string(),
            kind: kind.to_string(),
            minimum,
            maximum,
            value: minimum,
            choices: Vec::new(),
            color: [255, 1, 2, 3],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }
    }

    #[test]
    fn utf16_path_rejects_null() {
        assert!(utf16_path(ptr::null()).is_err());
    }

    #[test]
    fn sha256_is_lowercase_hex() {
        let path = std::env::temp_dir().join(format!("aexcompat-ymm4-{}.aex", std::process::id()));
        fs::write(&path, b"fixture").expect("write fixture");
        let hash = plugin_sha256(&path).expect("hash fixture");
        assert_eq!(hash.len(), 64);
        assert!(
            hash.chars()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn exposed_parameters_are_bounded_and_json_roundtrip_safe() {
        let mut popup = parameter("integer", 0.0, 0.0);
        popup.choices = vec!["A".to_string(), "B".to_string()];
        popup.value = 99.0;
        let hidden = InteractiveParameter {
            visible: false,
            ..parameter("float", 0.0, 1.0)
        };
        let unsupported = parameter("point", 0.0, 1.0);
        let exposed = exposed_parameters(&[
            parameter("float", 0.0, 1.0),
            parameter("integer", 0.0, 1.0),
            popup,
            parameter("color", 0.0, 0.0),
            hidden,
            unsupported,
        ]);

        assert_eq!(exposed.len(), 4);
        assert_eq!(exposed[2].minimum, 1.0);
        assert_eq!(exposed[2].maximum, 2.0);
        assert_eq!(exposed[2].value, 2.0);

        let encoded = serde_json::to_vec(&exposed).expect("encode exposed parameters");
        let decoded: Vec<InteractiveParameter> =
            serde_json::from_slice(&encoded).expect("decode exposed parameters");
        assert_eq!(decoded.len(), exposed.len());
        assert_eq!(decoded[2].choices, vec!["A", "B"]);
    }
}
