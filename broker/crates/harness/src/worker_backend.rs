// GUI とワーカーの間の窓口。画面はこのモジュールの関数だけを呼び、OS ごとの
// ワーカー（Windows はネイティブ worker、macOS は x64 guest）はここで切り替える。

#[cfg(windows)]
pub(crate) use aexcompat_broker::image_render::{
    inspect_experimental_via_discovery_in_place,
    render_experimental_image_with_approved_dependencies_and_search_dirs,
    render_experimental_image_with_auto_opencl_policy, render_native_image_with_dependencies,
};

#[cfg(target_os = "macos")]
pub(crate) use crate::macos::backend::{
    inspect_experimental_via_discovery_in_place,
    render_experimental_image_with_approved_dependencies_and_search_dirs,
    render_experimental_image_with_auto_opencl_policy, render_native_image_with_dependencies,
};

/// The worker executable a native render needs on Windows.
#[cfg(windows)]
pub(crate) fn required_render_worker_path(
    repository: &std::path::Path,
    _smart: bool,
) -> std::path::PathBuf {
    repository.join("target/minihost-build/aex_worker.exe")
}

/// Describes the missing worker when a render cannot start, or `None` when the
/// worker this OS renders with is in place.
#[cfg(windows)]
pub(crate) fn missing_render_worker(repository: &std::path::Path, smart: bool) -> Option<String> {
    let worker = required_render_worker_path(repository, smart);
    (!worker.is_file()).then(|| worker.display().to_string())
}

#[cfg(target_os = "macos")]
pub(crate) use crate::macos::backend::missing_render_worker;
