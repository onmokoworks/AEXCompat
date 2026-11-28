use super::types_and_transport::RenderTiming;
use super::{InteractiveParameter, RenderPixelFormat};
use crate::secure_image_dispatch::ApprovedImageArtifact;
use serde_json::Value;
use std::io;
use std::path::Path;

// macOS などネイティブ worker が無い OS 向けの代わり。GUI から呼ばれる関数と同じ形で、
// 呼ぶと Unsupported を返す。Mac での実処理は harness 側の guest バックエンドが受け持つ。

#[cfg(not(windows))]
pub fn initialize_experimental_aegp(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported("initialize_experimental_aegp"))
}

#[cfg(not(windows))]
pub fn inspect_experimental_external_dependencies(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
    _missing_only: bool,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "inspect_experimental_external_dependencies",
    ))
}

#[cfg(not(windows))]
pub fn inspect_experimental_with_approved_dependencies_and_diagnostics(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
    _dependencies: Vec<ApprovedImageArtifact>,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    Err(native_worker_unsupported(
        "inspect_experimental_with_approved_dependencies_and_diagnostics",
    ))
}

#[cfg(not(windows))]
pub fn probe_experimental_automatic_options_dialog(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "probe_experimental_automatic_options_dialog",
    ))
}

#[cfg(not(windows))]
pub fn probe_experimental_copied_flattened_sequence(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "probe_experimental_copied_flattened_sequence",
    ))
}

#[cfg(not(windows))]
pub fn probe_experimental_expand_buffer(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "probe_experimental_expand_buffer",
    ))
}

#[cfg(not(windows))]
pub fn probe_experimental_flattened_sequence(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "probe_experimental_flattened_sequence",
    ))
}

#[cfg(not(windows))]
pub fn probe_experimental_input_buffer_write(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "probe_experimental_input_buffer_write",
    ))
}

#[cfg(not(windows))]
pub fn probe_experimental_nop_render(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported("probe_experimental_nop_render"))
}

#[cfg(not(windows))]
pub fn probe_experimental_options_dialog(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "probe_experimental_options_dialog",
    ))
}

#[cfg(not(windows))]
pub fn probe_experimental_persistent_sequence(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "probe_experimental_persistent_sequence",
    ))
}

#[cfg(not(windows))]
pub fn probe_experimental_shrink_buffer(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "probe_experimental_shrink_buffer",
    ))
}

#[cfg(not(windows))]
pub fn probe_experimental_smart_input_buffer_write(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "probe_experimental_smart_input_buffer_write",
    ))
}

#[cfg(not(windows))]
pub fn probe_experimental_smart_nop_render(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "probe_experimental_smart_nop_render",
    ))
}

#[cfg(not(windows))]
pub fn render_experimental_audio(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
    _input_path: &Path,
    _output_path: &Path,
    _parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    Err(native_worker_unsupported("render_experimental_audio"))
}

#[cfg(not(windows))]
pub fn render_experimental_image_at_time_with_format_and_context(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
    _input_path: &Path,
    _output_path: &Path,
    _parameters: &[InteractiveParameter],
    _timing: RenderTiming,
    _smart: bool,
    _pixel_format: RenderPixelFormat,
    _host_context: Option<&crate::render_request::HostContext>,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "render_experimental_image_at_time_with_format_and_context",
    ))
}

#[cfg(not(windows))]
pub fn render_experimental_image_with_audio_sidecar(
    _repository: &Path,
    _plugin_path: &Path,
    _approved_sha256: &str,
    _input_path: &Path,
    _audio_path: &Path,
    _output_path: &Path,
    _parameters: &[InteractiveParameter],
    _timing: RenderTiming,
) -> io::Result<Value> {
    Err(native_worker_unsupported(
        "render_experimental_image_with_audio_sidecar",
    ))
}

#[cfg(not(windows))]
fn native_worker_unsupported(operation: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("{operation} needs the Windows native worker and is not available on this OS"),
    )
}
