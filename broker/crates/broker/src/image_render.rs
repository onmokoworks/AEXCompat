#![cfg_attr(not(windows), allow(dead_code, unused_imports, unused_variables))]
// Public entry points remain at image_render; implementation dependencies are
// explicit within the private submodules below.
pub use crate::parameter_animation::{
    AnimationInterpolation, AnimationTime, AnimationValue, ParameterAnimation,
    ParameterAnimationKey, parameter_animation_sidecar_json,
};

pub use crate::render_artifacts::{
    CapturedWorldRecord, RenderArtifactConditions, RenderArtifactKind, read_captured_world,
    write_float32_exr_artifact, write_raw_world_artifact, write_raw_world_checkpoint_artifact,
    write_strided_world_checkpoint_artifact,
};
pub use crate::render_fixture::InteractiveParameter;
pub use crate::render_pixel_format::RenderPixelFormat;

mod declarative_fixture;
mod diagnostics;
mod independent_cases;
mod inspection_and_probes;
mod native_input;
mod render_operations;
mod session;
mod types_and_transport;
#[cfg(not(windows))]
mod unsupported;
mod visual_diagnostics;

#[cfg(test)]
mod tests;

pub(crate) use crate::decoded_image::native_rgba_to_preview;
#[cfg(windows)]
pub use declarative_fixture::render_declarative_fixture;
#[cfg(windows)]
pub(crate) use diagnostics::isolated_worker_diagnostics;
#[cfg(test)]
pub(crate) use diagnostics::stderr_tail;
pub use diagnostics::{
    FRAME_DEADLINE_ENV, MAX_FRAME_DEADLINE_MS, MIN_FRAME_DEADLINE_MS,
    PF_OUTFLAG2_SUPPORTS_SMART_RENDER, PreparedGpuRuntimePolicy, decode_bounded_image,
    decode_bounded_image_for_format, interactive_frame_deadline_ms, prepare_gpu_runtime_policy,
    smart_render_advertised,
};
pub(crate) use diagnostics::{
    INTERACTIVE_RENDER_TIMEOUT_MS, MAX_DIMENSION, MAX_PIXELS, MAX_STDERR_TAIL_BYTES,
    decode_sha256_hex, propagate_bee_facade,
};
#[cfg(windows)]
pub use independent_cases::IndependentCaseBatch;
pub use inspection_and_probes::{
    dispatch_experimental_aegp_active_idle_roundtrip, dispatch_experimental_aegp_command_roundtrip,
    dispatch_experimental_aegp_comp_idle_roundtrip, dispatch_experimental_aegp_idle,
    dispatch_experimental_aegp_keyframe_roundtrip, dispatch_experimental_aegp_seek_roundtrip,
    dispatch_experimental_aegp_switch_roundtrip, dispatch_experimental_aegp_trim_roundtrip,
    dispatch_experimental_aegp_update_menu, encode_default_interactive_payload,
    encode_interactive_payload, normalize_default_interactive_parameters,
    probe_experimental_custom_ui_click, probe_experimental_custom_ui_cursor,
    probe_experimental_custom_ui_drag, probe_experimental_custom_ui_draw,
    probe_experimental_custom_ui_idle, probe_experimental_custom_ui_keydown,
    probe_experimental_custom_ui_lifecycle, probe_experimental_custom_ui_mouse_exited,
    trigger_experimental_button,
};
#[cfg(windows)]
pub use inspection_and_probes::{
    initialize_experimental_aegp, initialize_experimental_aegp_in_place,
};
pub use native_input::NativeImageInput;
#[cfg(windows)]
pub use native_input::{
    render_native_image_with_dependencies, render_native_image_with_timed_layers,
};
#[cfg(windows)]
pub use render_operations::{
    inspect_experimental, inspect_experimental_cleanup_contained_in_place,
    inspect_experimental_external_dependencies, inspect_experimental_in_place,
    inspect_experimental_in_place_plugin_data_effect, inspect_experimental_via_discovery_in_place,
    inspect_experimental_with_approved_dependencies,
    inspect_experimental_with_approved_dependencies_and_diagnostics,
    inspect_experimental_with_diagnostics, inspect_experimental_with_runtime_policy,
    probe_experimental_automatic_options_dialog, probe_experimental_copied_flattened_sequence,
    probe_experimental_expand_buffer, probe_experimental_flattened_sequence,
    probe_experimental_input_buffer_write, probe_experimental_nop_render,
    probe_experimental_options_dialog, probe_experimental_persistent_sequence,
    probe_experimental_shrink_buffer, probe_experimental_smart_input_buffer_write,
    probe_experimental_smart_nop_render, render_experimental_artifact_at_time,
    render_experimental_audio, render_experimental_image, render_experimental_image_at_time,
    render_experimental_image_at_time_with_deep16_png,
    render_experimental_image_at_time_with_format,
    render_experimental_image_at_time_with_format_and_context,
    render_experimental_image_at_time_with_format_context_and_click,
    render_experimental_image_at_time_with_format_context_and_ui_action,
    render_experimental_image_at_time_with_format_context_ui_action_and_gpu_backend,
    render_experimental_image_with_approved_dependencies,
    render_experimental_image_with_approved_dependencies_and_deep16_png,
    render_experimental_image_with_approved_dependencies_and_gpu_runtime_policy,
    render_experimental_image_with_approved_dependencies_and_gpu_runtime_policy_and_search_dirs,
    render_experimental_image_with_approved_dependencies_and_search_dirs,
    render_experimental_image_with_audio_sidecar,
    render_experimental_image_with_auto_opencl_policy,
    render_experimental_image_with_parameter_animation,
    render_experimental_image_with_timed_layers, render_experimental_smart_image,
    render_experimental_smart_image_at_time, render_scattermap_fixture,
};
#[cfg(debug_assertions)]
pub use session::FORCE_SESSION_FALLBACK_ENV;
pub use session::{
    InteractiveCapabilitySource, InteractiveRenderPath, InteractiveSessionSelection,
    RENDER_SESSION_WRAPPER_RENDERS, annotate_interactive_selection,
};
#[cfg(windows)]
pub use session::{InteractiveRenderSession, InteractiveSessionOpen};
pub use types_and_transport::{
    GpuRuntimePolicyInput, RenderGpuBackend, RenderTiming, RenderUiAction, TimedLayerImage,
};
pub(crate) use types_and_transport::{
    resolve_managed_dump_dir, runtime_backend, validate_animation_bindings,
};
#[cfg(not(windows))]
pub use unsupported::{
    initialize_experimental_aegp, inspect_experimental_external_dependencies,
    inspect_experimental_with_approved_dependencies_and_diagnostics,
    probe_experimental_automatic_options_dialog, probe_experimental_copied_flattened_sequence,
    probe_experimental_expand_buffer, probe_experimental_flattened_sequence,
    probe_experimental_input_buffer_write, probe_experimental_nop_render,
    probe_experimental_options_dialog, probe_experimental_persistent_sequence,
    probe_experimental_shrink_buffer, probe_experimental_smart_input_buffer_write,
    probe_experimental_smart_nop_render, render_experimental_audio,
    render_experimental_image_at_time_with_format_and_context,
    render_experimental_image_with_audio_sidecar,
};
