use aex_abi::x86_64_windows as abi;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

use crate::backend::{
    CustomUiRegistration, ExecutionTrace, GuestCensus, GuestEngine, GuestError,
    SmartCheckoutDiskIdFallback, TraceStateValue, TraceWatchSpec, UnsupportedSuiteCall,
};
use crate::gpu_lifecycle::{
    GpuRenderDiagnostic, GpuRuntimeBackendKind, LifecycleCall, LifecycleFailure, LifecycleReply,
    RenderBackendRequest, run_smart_lifecycle,
};
use crate::pe::PeImage;
use crate::pixel::FramePixelFormat;

const CMD_GLOBAL_SETUP: u64 = abi::PF_CMD_GLOBAL_SETUP as u64;
const CMD_GLOBAL_SETDOWN: u64 = abi::PF_CMD_GLOBAL_SETDOWN as u64;
const CMD_PARAMS_SETUP: u64 = abi::PF_CMD_PARAMS_SETUP as u64;
const CMD_SEQUENCE_SETUP: u64 = abi::PF_CMD_SEQUENCE_SETUP as u64;
const CMD_SEQUENCE_SETDOWN: u64 = abi::PF_CMD_SEQUENCE_SETDOWN as u64;
const CMD_FRAME_SETUP: u64 = abi::PF_CMD_FRAME_SETUP as u64;
const CMD_RENDER: u64 = abi::PF_CMD_RENDER as u64;
const CMD_FRAME_SETDOWN: u64 = abi::PF_CMD_FRAME_SETDOWN as u64;
const CMD_USER_CHANGED_PARAM: u64 = abi::PF_CMD_USER_CHANGED_PARAM as u64;
const CMD_SMART_PRE_RENDER: u64 = abi::PF_CMD_SMART_PRE_RENDER as u64;
const CMD_SMART_RENDER: u64 = abi::PF_CMD_SMART_RENDER as u64;
const CMD_SMART_RENDER_GPU: u64 = abi::PF_CMD_SMART_RENDER_GPU as u64;
const CMD_GPU_DEVICE_SETUP: u64 = abi::PF_CMD_GPU_DEVICE_SETUP as u64;
const CMD_GPU_DEVICE_SETDOWN: u64 = abi::PF_CMD_GPU_DEVICE_SETDOWN as u64;
const CMD_ARBITRARY_CALLBACK: u64 = abi::PF_CMD_ARBITRARY_CALLBACK as u64;
pub const PARAM_LAYER: i32 = 0;
const PARAM_SLIDER: i32 = 1;
const PARAM_FIXED_SLIDER: i32 = 2;
pub(crate) const PARAM_ANGLE: i32 = 3;
const PARAM_CHECKBOX: i32 = 4;
pub(crate) const PARAM_COLOR: i32 = 5;
pub(crate) const PARAM_POINT: i32 = 6;
pub(crate) const PARAM_POINT3D: i32 = 18;
const PARAM_POPUP: i32 = 7;
const PARAM_FLOAT_SLIDER: i32 = 10;
const PARAM_ARBITRARY_DATA: i32 = 11;
const ARBITRARY_DEFAULT_HANDLE_OFFSET: usize = 8;
const ARBITRARY_VALUE_HANDLE_OFFSET: usize = 16;
const ARBITRARY_REFCON_OFFSET: usize = 24;
/// `PF_ArbParamsExtra`: `which_function` at 0, then the per-function union
/// (`id` at 4, `refconPV` at 8, source handle at 16, destination handle
/// pointer at 24). The host keeps the COPY destination cell inside the same
/// scratch block, after the union.
const ARBITRARY_EXTRA_BYTES: usize = 48;
const ARBITRARY_EXTRA_ID_OFFSET: usize = 4;
const ARBITRARY_EXTRA_REFCON_OFFSET: usize = 8;
const ARBITRARY_EXTRA_HANDLE_OFFSET: usize = 16;
const ARBITRARY_EXTRA_DESTINATION_POINTER_OFFSET: usize = 24;
const ARBITRARY_EXTRA_DESTINATION_CELL_OFFSET: usize = 32;
const ARBITRARY_DISPOSE_FUNC: i32 = 1;
const ARBITRARY_COPY_FUNC: i32 = 2;
const LAYER_DEFAULT_OFFSET: usize = 116;
const ANGLE_DEFAULT_OFFSET: usize = 4;
const POINT_DEFAULT_X_OFFSET: usize = 12;
const POINT_DEFAULT_Y_OFFSET: usize = 16;
const CLEANUP_GUEST_ERROR: i32 = -40;
const OUTPUT_GUARD_BYTES: usize = 64;
const OUTPUT_GUARD_PATTERN: u8 = 0xa5;
pub(crate) const MAX_FAILURE_TEXT_BYTES: usize = 1024;
pub(crate) const MAX_FAILURE_SUITE_REQUEST_BYTES: usize = 256;
const MAX_FAILURE_SUITE_REQUESTS: usize = 64;
const MAX_FAILURE_UNSUPPORTED_SUITE_CALLS: usize = 64;
pub(crate) const MAX_FAILURE_CRASH_SNAPSHOT_BYTES: usize = 8 * 1024;
pub const MAX_RENDER_WIDTH: u32 = 1920;
pub const MAX_RENDER_HEIGHT: u32 = 1080;

#[derive(Debug, Error)]
pub enum ClassicError {
    #[error(transparent)]
    Guest(#[from] GuestError),
    #[error("selector {selector} failed in guest: {source}")]
    SelectorGuest {
        selector: &'static str,
        source: GuestError,
    },
    #[error("selector {selector} returned {error}")]
    Selector { selector: &'static str, error: i32 },
    #[error("invalid frame input: {0}")]
    Input(String),
    /// A `PF_Cmd_ARBITRARY_CALLBACK` round trip (COPY/DISPOSE) the host
    /// issued on the plug-in's behalf did not produce the contract result.
    #[error("arbitrary parameter id={id} {operation} failed: {message}")]
    Arbitrary {
        operation: &'static str,
        id: i16,
        message: String,
    },
    /// Two independent failures from one lifecycle step. `primary` keeps the
    /// error whose kind (guest crash, selector code) categorizes the failure;
    /// `secondary` stays visible in the message instead of being dropped.
    #[error("{primary}; additionally: {secondary}")]
    Compound {
        primary: Box<ClassicError>,
        secondary: Box<ClassicError>,
    },
}

impl ClassicError {
    /// The selector error code this failure carries, looking through a
    /// compound failure to its primary error.
    pub fn selector_error_code(&self) -> Option<i32> {
        match self {
            ClassicError::Selector { error, .. } => Some(*error),
            ClassicError::Compound { primary, .. } => primary.selector_error_code(),
            _ => None,
        }
    }
}

fn combine_failures(failures: Vec<ClassicError>) -> Result<(), ClassicError> {
    match failures
        .into_iter()
        .reduce(|primary, secondary| ClassicError::Compound {
            primary: Box::new(primary),
            secondary: Box::new(secondary),
        }) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ParameterReport {
    pub slot: usize,
    pub index: i32,
    pub param_type: i32,
    pub name: String,
    pub ui_flags: u32,
    pub flags: u32,
    pub ui_width: u16,
    pub ui_height: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<String>,
    pub default_value: Option<f64>,
    pub valid_min: Option<f64>,
    pub valid_max: Option<f64>,
    pub slider_min: Option<f64>,
    pub slider_max: Option<f64>,
    pub precision: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_color: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_color: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_components: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_components: Option<Vec<f64>>,
}

#[derive(Clone, Debug)]
pub struct ParameterValue {
    pub slot: Option<usize>,
    pub name: String,
    pub value: Option<f64>,
    pub color: Option<[u8; 4]>,
    pub point: Option<[f64; 2]>,
    pub angle: Option<f64>,
    pub point3d: Option<[f64; 3]>,
}

#[derive(Clone, Copy, Debug)]
pub struct ResidentLayer<'a> {
    pub slot: usize,
    pub width: u32,
    pub height: u32,
    pub pixels: &'a [u8],
}

#[derive(Debug)]
pub struct ResidentWorldDump {
    pub slot: usize,
    pub width: u32,
    pub height: u32,
    pub raw_pixels: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct AppliedParameter {
    pub slot: usize,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point3d: Option<[f64; 3]>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SetupReport {
    pub schema_version: u32,
    pub execution_backend: &'static str,
    pub global_setup_error: i32,
    pub params_setup_error: i32,
    pub advertised_num_params: i32,
    pub out_flags: u32,
    pub out_flags2: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_ui: Option<CustomUiRegistration>,
    pub parameters: Vec<ParameterReport>,
    pub suite_requests: Vec<String>,
    pub unsupported_suite_calls: Vec<UnsupportedSuiteCall>,
    pub dropped_unsupported_suite_calls: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ParameterUiState {
    pub slot: usize,
    pub ui_flags: u32,
    pub flags: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct UserChangedReport {
    pub slot: usize,
    pub selector_error: i32,
    pub parameters: Vec<ParameterUiState>,
}

#[derive(Debug, Serialize)]
pub struct FailureReport {
    pub schema_version: u32,
    pub execution_backend: &'static str,
    pub error: String,
    pub gpu: GpuRenderDiagnostic,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_message: Option<String>,
    pub suite_requests: Vec<String>,
    pub unsupported_suite_calls: Vec<UnsupportedSuiteCall>,
    pub dropped_unsupported_suite_calls: u64,
}

#[derive(Debug, Serialize)]
pub struct ResidentFailureDiagnostic {
    pub schema_version: u32,
    pub stage: &'static str,
    pub execution_backend: &'static str,
    pub category: &'static str,
    pub selector: Option<&'static str>,
    pub error_code: Option<i32>,
    pub message: String,
    pub crash_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crash_snapshot: Option<serde_json::Value>,
    pub suite_requests: Vec<String>,
    pub dropped_suite_requests: u64,
    pub unsupported_suite_calls: Vec<UnsupportedSuiteCall>,
    pub dropped_unsupported_suite_calls: u64,
}

#[derive(Debug, Serialize)]
pub struct ResidentCloseReport {
    pub schema_version: u32,
    pub execution_backend: &'static str,
    pub frames_rendered: u64,
    pub frame_setdown_error: i32,
    pub sequence_setdown_error: i32,
    pub global_setdown_error: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_setdown_diagnostic: Option<ResidentFailureDiagnostic>,
    pub suite_requests: Vec<String>,
    pub unsupported_suite_calls: Vec<UnsupportedSuiteCall>,
    pub dropped_unsupported_suite_calls: u64,
    pub session_clean: bool,
}

#[derive(Debug, Serialize)]
pub struct RenderReport {
    pub schema_version: u32,
    pub setup: SetupReport,
    pub render_error: i32,
    pub render_mode: &'static str,
    pub gpu: GpuRenderDiagnostic,
    pub width: u32,
    pub height: u32,
    pub pixel_format: &'static str,
    pub raw_pixel_bytes: usize,
    pub raw_pixel_sha256: String,
    pub guards_intact: bool,
    pub parameter_values: Vec<AppliedParameter>,
    pub output_request: [i32; 4],
    pub input_requests: Vec<[i32; 4]>,
    pub suite_requests: Vec<String>,
    pub unsupported_suite_calls: Vec<UnsupportedSuiteCall>,
    pub dropped_unsupported_suite_calls: u64,
    /// Smart checkouts the host resolved by disk id instead of positionally.
    /// AE and the minihost resolve `PF_CHECKOUT_LAYER` positionally only, so
    /// every entry here is a host-side substitution rather than observed AE
    /// behavior; an empty list means the render used no such substitution.
    pub smart_checkout_disk_id_fallbacks: Vec<SmartCheckoutDiskIdFallback>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub census: Option<GuestCensus>,
    pub argb8: Vec<u8>,
    #[serde(skip)]
    pub raw_pixels: Vec<u8>,
    #[serde(skip)]
    pub raw_input_pixels: Vec<u8>,
    #[serde(skip)]
    pub raw_secondary_layers: Vec<ResidentWorldDump>,
}

fn write_ansi_numeric_callbacks(engine: &GuestEngine<'static>, utility_bytes: &mut [u8]) {
    for (offset, callback) in [
        (
            abi::UTILS_ANSI_ATAN_OFFSET,
            engine.ansi_atan_callback_address(),
        ),
        (
            abi::UTILS_ANSI_ATAN2_OFFSET,
            engine.ansi_atan2_callback_address(),
        ),
        (
            abi::UTILS_ANSI_CEIL_OFFSET,
            engine.ansi_ceil_callback_address(),
        ),
        (
            abi::UTILS_ANSI_COS_OFFSET,
            engine.ansi_cos_callback_address(),
        ),
        (
            abi::UTILS_ANSI_EXP_OFFSET,
            engine.ansi_exp_callback_address(),
        ),
        (
            abi::UTILS_ANSI_FABS_OFFSET,
            engine.ansi_fabs_callback_address(),
        ),
        (
            abi::UTILS_ANSI_FLOOR_OFFSET,
            engine.ansi_floor_callback_address(),
        ),
        (
            abi::UTILS_ANSI_FMOD_OFFSET,
            engine.ansi_fmod_callback_address(),
        ),
        (
            abi::UTILS_ANSI_HYPOT_OFFSET,
            engine.ansi_hypot_callback_address(),
        ),
        (
            abi::UTILS_ANSI_LOG_OFFSET,
            engine.ansi_log_callback_address(),
        ),
        (
            abi::UTILS_ANSI_LOG10_OFFSET,
            engine.ansi_log10_callback_address(),
        ),
        (
            abi::UTILS_ANSI_POW_OFFSET,
            engine.ansi_pow_callback_address(),
        ),
        (
            abi::UTILS_ANSI_SIN_OFFSET,
            engine.ansi_sin_callback_address(),
        ),
        (
            abi::UTILS_ANSI_SQRT_OFFSET,
            engine.ansi_sqrt_callback_address(),
        ),
        (
            abi::UTILS_ANSI_TAN_OFFSET,
            engine.ansi_tan_callback_address(),
        ),
        (
            abi::UTILS_ANSI_ASIN_OFFSET,
            engine.ansi_asin_callback_address(),
        ),
        (
            abi::UTILS_ANSI_ACOS_OFFSET,
            engine.ansi_acos_callback_address(),
        ),
    ] {
        write_u64(utility_bytes, offset, callback);
    }
}

pub(crate) fn build_utility_callbacks(engine: &GuestEngine<'static>) -> Vec<u8> {
    let mut utility_bytes = vec![0u8; abi::PF_UTIL_CALLBACKS_SIZE];
    for (offset, callback) in [
        (
            abi::UTILS_SUBPIXEL_SAMPLE_OFFSET,
            engine.subpixel_sample8_callback_address(),
        ),
        (
            abi::UTILS_AREA_SAMPLE_OFFSET,
            engine.area_sample8_callback_address(),
        ),
        (
            abi::UTILS_TRANSFER_RECT_OFFSET,
            engine.transfer_rect8_callback_address(),
        ),
        (
            abi::UTILS_ANSI_STRCPY_OFFSET,
            engine.ansi_strcpy_callback_address(),
        ),
        (
            abi::UTILS_ANSI_SPRINTF_OFFSET,
            engine.ansi_sprintf_callback_address(),
        ),
        (abi::UTILS_COPY_OFFSET, engine.copy_callback_address()),
        (abi::UTILS_BLEND_OFFSET, engine.blend_callback_address()),
        (abi::UTILS_FILL_OFFSET, engine.fill8_callback_address()),
        (
            abi::UTILS_NEW_WORLD_OFFSET,
            engine.new_world8_callback_address(),
        ),
        (
            abi::UTILS_DISPOSE_WORLD_OFFSET,
            engine.dispose_world_callback_address(),
        ),
        (
            abi::UTILS_GET_CALLBACK_ADDR_OFFSET,
            engine.get_callback_addr_callback_address(),
        ),
        (
            abi::UTILS_ITERATE_OFFSET,
            engine.iterate8_callback_address(),
        ),
        (
            abi::UTILS_ITERATE_ORIGIN_OFFSET,
            engine.iterate8_origin_callback_address(),
        ),
        (
            abi::UTILS_ITERATE16_OFFSET,
            engine.iterate16_callback_address(),
        ),
        (
            abi::UTILS_HOST_NEW_HANDLE_OFFSET,
            engine.new_handle_callback_address(),
        ),
        (
            abi::UTILS_HOST_LOCK_HANDLE_OFFSET,
            engine.lock_handle_callback_address(),
        ),
        (
            abi::UTILS_HOST_UNLOCK_HANDLE_OFFSET,
            engine.unlock_handle_callback_address(),
        ),
        (
            abi::UTILS_HOST_DISPOSE_HANDLE_OFFSET,
            engine.dispose_handle_callback_address(),
        ),
        (
            abi::UTILS_HOST_GET_HANDLE_SIZE_OFFSET,
            engine.handle_size_callback_address(),
        ),
        (
            abi::UTILS_HOST_RESIZE_HANDLE_OFFSET,
            engine.resize_handle_callback_address(),
        ),
    ] {
        write_u64(&mut utility_bytes, offset, callback);
    }
    write_ansi_numeric_callbacks(engine, &mut utility_bytes);
    utility_bytes
}

pub struct ClassicHost {
    engine: GuestEngine<'static>,
    entry: u64,
    input: u64,
    output: u64,
    trace_output_pixel: Option<[u32; 2]>,
    setup_report: Option<SetupReport>,
    global_active: bool,
    sequence_active: bool,
    frame_resources: Option<FrameResources>,
    user_changed_extra: Option<u64>,
    arbitrary_extra: Option<u64>,
    arbitrary_values: Vec<ArbitraryValue>,
    resident_frames: u64,
    resident_frame_setdown_error: i32,
    last_gpu_diagnostic: GpuRenderDiagnostic,
}

/// A private copy of an arbitrary parameter's default, made through
/// `PF_Arbitrary_COPY_FUNC` for one render and disposed through
/// `PF_Arbitrary_DISPOSE_FUNC` when that render ends.
#[derive(Clone, Copy, Debug)]
struct ArbitraryValue {
    id: i16,
    refcon: u64,
    handle: u64,
    /// Guest address of the `PF_ParamDef` whose value slot holds `handle`.
    definition: u64,
}

#[derive(Clone)]
struct FrameResources {
    width: u32,
    height: u32,
    format: FramePixelFormat,
    pixel_bytes: usize,
    input_param: u64,
    params: u64,
    output_world: u64,
    input_pixels: u64,
    output_guard_base: u64,
    output_pixels: u64,
    parameter_definitions: Vec<u64>,
    secondary_layers: Vec<ResidentLayerResources>,
}

#[derive(Clone)]
struct ResidentLayerResources {
    slot: usize,
    width: u32,
    height: u32,
    pixels: u64,
}

pub(crate) fn build_interact_callbacks(
    engine: &GuestEngine<'static>,
) -> [u8; abi::PF_INTERACT_CALLBACKS_SIZE] {
    let mut callbacks = [0u8; abi::PF_INTERACT_CALLBACKS_SIZE];
    for offset in abi::INPUT_CALLBACK_OFFSETS {
        write_u64(&mut callbacks, offset, engine.poison_callback_address());
    }
    for (offset, callback) in [
        (
            abi::INTER_ADD_PARAM_OFFSET,
            engine.add_param_callback_address(),
        ),
        (
            abi::INTER_REGISTER_UI_OFFSET,
            engine.register_ui_callback_address(),
        ),
        (
            abi::INTER_CHECKOUT_PARAM_OFFSET,
            engine.checkout_param_callback_address(),
        ),
        (
            abi::INTER_CHECKIN_PARAM_OFFSET,
            engine.checkin_param_callback_address(),
        ),
        (abi::INTER_ABORT_OFFSET, engine.noop_callback_address()),
        (abi::INTER_PROGRESS_OFFSET, engine.noop_callback_address()),
        (
            abi::INTER_RESERVED_0_OFFSET,
            engine.extended_alloc_callback_address(),
        ),
        (
            abi::INTER_RESERVED_1_OFFSET,
            engine.extended_lookup_callback_address(),
        ),
        (
            abi::INTER_RESERVED_2_OFFSET,
            engine.extended_free_callback_address(),
        ),
    ] {
        write_u64(&mut callbacks, offset, callback);
    }
    callbacks
}

impl ClassicHost {
    #[cfg(test)]
    pub(crate) fn from_test_engine(
        mut engine: GuestEngine<'static>,
        entry: u64,
    ) -> Result<Self, ClassicError> {
        let input = engine.allocate(abi::PF_IN_DATA_SIZE, 8)?;
        let output = engine.allocate(abi::PF_OUT_DATA_SIZE, 8)?;
        let mut input_bytes = vec![0u8; abi::PF_IN_DATA_SIZE];
        input_bytes[..abi::PF_INTERACT_CALLBACKS_SIZE]
            .copy_from_slice(&build_interact_callbacks(&engine));
        write_u64(&mut input_bytes, abi::IN_EFFECT_REF_OFFSET, 1);
        write_i32(&mut input_bytes, abi::IN_QUALITY_OFFSET, 1);
        write_i16(&mut input_bytes, abi::IN_VERSION_OFFSET, 13);
        write_i16(&mut input_bytes, abi::IN_VERSION_OFFSET + 2, 29);
        write_u32(&mut input_bytes, abi::IN_APPL_ID_OFFSET, 0x4658_5443);
        write_i32(&mut input_bytes, abi::IN_NUM_PARAMS_OFFSET, 1);
        engine.write(input, &input_bytes)?;
        engine.write(output, &vec![0u8; abi::PF_OUT_DATA_SIZE])?;
        Ok(Self {
            engine,
            entry,
            input,
            output,
            trace_output_pixel: None,
            setup_report: None,
            global_active: false,
            sequence_active: false,
            frame_resources: None,
            user_changed_extra: None,
            arbitrary_extra: None,
            arbitrary_values: Vec::new(),
            resident_frames: 0,
            resident_frame_setdown_error: 0,
            last_gpu_diagnostic: GpuRenderDiagnostic::pending(RenderBackendRequest::Cpu),
        })
    }

    pub fn new(image: &PeImage) -> Result<Self, ClassicError> {
        Self::new_with_effect(image, None)
    }

    pub fn new_with_effect(
        image: &PeImage,
        effect_selector: Option<&str>,
    ) -> Result<Self, ClassicError> {
        let mut engine = GuestEngine::load(image)?;
        let input = engine.allocate(abi::PF_IN_DATA_SIZE, 8)?;
        let output = engine.allocate(abi::PF_OUT_DATA_SIZE, 8)?;
        let utils = engine.allocate(abi::PF_UTIL_CALLBACKS_SIZE, 8)?;
        let pica_basic = engine.allocate(64, 8)?;

        let mut input_bytes = vec![0u8; abi::PF_IN_DATA_SIZE];
        input_bytes[..abi::PF_INTERACT_CALLBACKS_SIZE]
            .copy_from_slice(&build_interact_callbacks(&engine));
        write_u64(&mut input_bytes, abi::IN_UTILS_OFFSET, utils);
        write_u64(&mut input_bytes, abi::IN_PICA_BASICP_OFFSET, pica_basic);
        write_u64(&mut input_bytes, abi::IN_EFFECT_REF_OFFSET, 1);
        write_i32(&mut input_bytes, abi::IN_QUALITY_OFFSET, 1);
        write_i16(&mut input_bytes, abi::IN_VERSION_OFFSET, 13);
        write_i16(&mut input_bytes, abi::IN_VERSION_OFFSET + 2, 29);
        write_u32(&mut input_bytes, abi::IN_APPL_ID_OFFSET, 0x4658_5443);
        write_i32(&mut input_bytes, abi::IN_NUM_PARAMS_OFFSET, 1);
        write_i32(&mut input_bytes, abi::IN_TIME_STEP_OFFSET, 1);
        write_i32(&mut input_bytes, abi::IN_LOCAL_TIME_STEP_OFFSET, 1);
        write_u32(&mut input_bytes, abi::IN_TIME_SCALE_OFFSET, 1);
        for offset in [
            abi::IN_DOWNSAMPLE_X_OFFSET,
            abi::IN_DOWNSAMPLE_Y_OFFSET,
            abi::IN_PIXEL_ASPECT_RATIO_OFFSET,
        ] {
            write_i32(&mut input_bytes, offset, 1);
            write_i32(&mut input_bytes, offset + 4, 1);
        }
        engine.write(input, &input_bytes)?;
        engine.write(output, &vec![0u8; abi::PF_OUT_DATA_SIZE])?;
        let mut pica_bytes = [0u8; 64];
        write_u64(&mut pica_bytes, 0, engine.acquire_suite_callback_address());
        write_u64(&mut pica_bytes, 8, engine.noop_callback_address());
        engine.write(pica_basic, &pica_bytes)?;
        let utility_bytes = build_utility_callbacks(&engine);
        engine.write(utils, &utility_bytes)?;
        let entry = engine.resolve_effect_entry(image, effect_selector, pica_basic)?;
        Ok(Self {
            engine,
            entry,
            input,
            output,
            trace_output_pixel: None,
            setup_report: None,
            global_active: false,
            sequence_active: false,
            frame_resources: None,
            user_changed_extra: None,
            arbitrary_extra: None,
            arbitrary_values: Vec::new(),
            resident_frames: 0,
            resident_frame_setdown_error: 0,
            last_gpu_diagnostic: GpuRenderDiagnostic::pending(RenderBackendRequest::Cpu),
        })
    }

    pub fn setup(&mut self) -> Result<SetupReport, ClassicError> {
        if let Some(report) = &self.setup_report {
            return Ok(report.clone());
        }
        let global_setup_error =
            self.invoke(CMD_GLOBAL_SETUP)
                .map_err(|source| selector_guest_error("GLOBAL_SETUP", source))? as i32;
        if global_setup_error != 0 {
            return Err(ClassicError::Selector {
                selector: "GLOBAL_SETUP",
                error: global_setup_error,
            });
        }
        self.global_active = true;
        let mut output = vec![0u8; abi::PF_OUT_DATA_SIZE];
        self.engine.read(self.output, &mut output)?;
        let global_data = read_u64(&output, abi::OUT_GLOBAL_DATA_OFFSET);
        let mut input = vec![0u8; abi::PF_IN_DATA_SIZE];
        self.engine.read(self.input, &mut input)?;
        write_u64(&mut input, abi::IN_GLOBAL_DATA_OFFSET, global_data);
        self.engine.write(self.input, &input)?;
        let params_setup_error =
            self.invoke(CMD_PARAMS_SETUP)
                .map_err(|source| selector_guest_error("PARAMS_SETUP", source))? as i32;
        if params_setup_error != 0 {
            return Err(ClassicError::Selector {
                selector: "PARAMS_SETUP",
                error: params_setup_error,
            });
        }
        self.engine.read(self.output, &mut output)?;
        let advertised_num_params = read_i32(&output, abi::OUT_NUM_PARAMS_OFFSET);
        let out_flags = read_u32(&output, abi::OUT_OUT_FLAGS_OFFSET);
        let out_flags2 = read_u32(&output, abi::OUT_OUT_FLAGS2_OFFSET);
        self.engine.read(self.input, &mut input)?;
        write_i32(&mut input, abi::IN_NUM_PARAMS_OFFSET, advertised_num_params);
        self.engine.write(self.input, &input)?;
        let parameters = self
            .engine
            .parameters()
            .iter()
            .enumerate()
            .map(|(offset, param)| {
                let (default_value, valid_min, valid_max, slider_min, slider_max, precision) =
                    numeric_descriptor(&param.bytes, param.param_type);
                let (current_color, default_color) =
                    color_descriptor(&param.bytes, param.param_type);
                let (current_components, default_components) =
                    component_descriptor(&param.bytes, param.param_type);
                let choices = if param.param_type == PARAM_POPUP {
                    let pointer =
                        read_u64(&param.bytes, abi::PARAM_U_OFFSET + abi::POPUP_NAMES_OFFSET);
                    Some(self.read_guest_text(pointer, 4096)?)
                } else {
                    None
                };
                Ok(ParameterReport {
                    slot: offset + 1,
                    index: param.index,
                    param_type: param.param_type,
                    name: param.name.clone(),
                    ui_flags: read_u32(&param.bytes, abi::PARAM_UI_FLAGS_OFFSET),
                    flags: read_u32(&param.bytes, abi::PARAM_FLAGS_OFFSET),
                    ui_width: read_i16(&param.bytes, abi::PARAM_UI_WIDTH_OFFSET).max(0) as u16,
                    ui_height: read_i16(&param.bytes, abi::PARAM_UI_HEIGHT_OFFSET).max(0) as u16,
                    choices,
                    default_value,
                    valid_min,
                    valid_max,
                    slider_min,
                    slider_max,
                    precision,
                    current_color,
                    default_color,
                    current_components,
                    default_components,
                })
            })
            .collect::<Result<Vec<_>, ClassicError>>()?;
        let report = SetupReport {
            schema_version: 1,
            execution_backend: self.engine.backend_name(),
            global_setup_error,
            params_setup_error,
            advertised_num_params,
            out_flags,
            out_flags2,
            custom_ui: self.engine.custom_ui_registration(),
            parameters,
            suite_requests: self.engine.suite_requests().to_vec(),
            unsupported_suite_calls: self.engine.unsupported_suite_calls().to_vec(),
            dropped_unsupported_suite_calls: self.engine.dropped_unsupported_suite_calls(),
        };
        self.setup_report = Some(report.clone());
        Ok(report)
    }

    fn read_guest_text(&self, address: u64, limit: usize) -> Result<String, ClassicError> {
        if address == 0 {
            return Ok(String::new());
        }
        let mut bytes = Vec::new();
        for offset in 0..limit {
            let mut byte = [0u8; 1];
            self.engine.read(address + offset as u64, &mut byte)?;
            if byte[0] == 0 {
                return Ok(String::from_utf8_lossy(&bytes).into_owned());
            }
            bytes.push(byte[0]);
        }
        Err(ClassicError::Input(
            "popup choice text exceeds the 4096-byte setup bound".into(),
        ))
    }

    pub fn begin_resident_session(
        &mut self,
        width: u32,
        height: u32,
        time_scale: u32,
    ) -> Result<SetupReport, ClassicError> {
        if width == 0
            || height == 0
            || width > MAX_RENDER_WIDTH
            || height > MAX_RENDER_HEIGHT
            || time_scale == 0
        {
            return Err(ClassicError::Input(format!(
                "resident session requires dimensions within 1x1..={MAX_RENDER_WIDTH}x{MAX_RENDER_HEIGHT} and nonzero time scale"
            )));
        }
        let setup = match self.setup() {
            Ok(setup) => setup,
            Err(error) => {
                let _ = self.end_global();
                return Err(error);
            }
        };
        if let Err(error) = self.write_frame_context(width, height, 0, 1, 0, time_scale) {
            let _ = self.end_global();
            return Err(error);
        }
        if !self.sequence_active {
            let result = match self.invoke(CMD_SEQUENCE_SETUP) {
                Ok(result) => result as i32,
                Err(error) => {
                    let _ = self.end_global();
                    return Err(selector_guest_error("SEQUENCE_SETUP", error));
                }
            };
            if result != 0 {
                let _ = self.end_global();
                return Err(ClassicError::Selector {
                    selector: "SEQUENCE_SETUP",
                    error: result,
                });
            }
            self.sequence_active = true;
            let sequence_data = match self.read_output_pointer(abi::OUT_SEQUENCE_DATA_OFFSET) {
                Ok(sequence_data) => sequence_data,
                Err(error) => {
                    let _ = self.end_sequence(false);
                    let _ = self.end_global();
                    return Err(error.into());
                }
            };
            if let Err(error) =
                self.write_input_pointer(abi::IN_SEQUENCE_DATA_OFFSET, sequence_data)
            {
                let _ = self.end_sequence(false);
                let _ = self.end_global();
                return Err(error.into());
            }
        }
        Ok(setup)
    }

    pub fn render_resident_argb8(
        &mut self,
        width: u32,
        height: u32,
        current_time: i32,
        time_scale: u32,
        input_argb8: &[u8],
        parameter_values: &[ParameterValue],
    ) -> Result<RenderReport, ClassicError> {
        self.render_resident_pixels(
            width,
            height,
            current_time,
            time_scale,
            FramePixelFormat::Argb8,
            input_argb8,
            parameter_values,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_resident_pixels(
        &mut self,
        width: u32,
        height: u32,
        current_time: i32,
        time_scale: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
    ) -> Result<RenderReport, ClassicError> {
        self.render_resident_pixels_mode(
            width,
            height,
            current_time,
            1,
            0,
            time_scale,
            format,
            input_pixels,
            parameter_values,
            true,
            &[],
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_resident_fixture_pixels(
        &mut self,
        width: u32,
        height: u32,
        current_time: i32,
        time_step: i32,
        total_time: i32,
        time_scale: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
        secondary_layers: &[ResidentLayer<'_>],
        smart: bool,
    ) -> Result<RenderReport, ClassicError> {
        self.render_resident_pixels_mode(
            width,
            height,
            current_time,
            time_step,
            total_time,
            time_scale,
            format,
            input_pixels,
            parameter_values,
            true,
            secondary_layers,
            Some(smart),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn probe_resident_fixture_pixels(
        &mut self,
        width: u32,
        height: u32,
        time_scale: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        secondary_layers: &[ResidentLayer<'_>],
        smart: bool,
    ) -> Result<RenderReport, ClassicError> {
        self.render_resident_pixels_mode(
            width,
            height,
            0,
            1,
            0,
            time_scale,
            format,
            input_pixels,
            &[],
            false,
            secondary_layers,
            Some(smart),
        )
    }

    pub fn probe_resident_argb8(
        &mut self,
        width: u32,
        height: u32,
        time_scale: u32,
        input_argb8: &[u8],
    ) -> Result<RenderReport, ClassicError> {
        self.probe_resident_pixels(
            width,
            height,
            time_scale,
            FramePixelFormat::Argb8,
            input_argb8,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn probe_resident_pixels(
        &mut self,
        width: u32,
        height: u32,
        time_scale: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
    ) -> Result<RenderReport, ClassicError> {
        self.render_resident_pixels_mode(
            width,
            height,
            0,
            1,
            0,
            time_scale,
            format,
            input_pixels,
            &[],
            false,
            &[],
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_resident_pixels_mode(
        &mut self,
        width: u32,
        height: u32,
        current_time: i32,
        time_step: i32,
        total_time: i32,
        time_scale: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
        count_frame: bool,
        secondary_layers: &[ResidentLayer<'_>],
        smart_override: Option<bool>,
    ) -> Result<RenderReport, ClassicError> {
        if !self.sequence_active {
            return Err(ClassicError::Input(
                "resident session has not been opened".into(),
            ));
        }
        self.write_frame_context(
            width,
            height,
            current_time,
            time_step,
            total_time,
            time_scale,
        )?;
        let report = self
            .render_pixels_with_request_mode(
                width,
                height,
                format,
                input_pixels,
                parameter_values,
                [0, 0, width as i32, height as i32],
                false,
                false,
                true,
                RenderBackendRequest::Cpu,
                secondary_layers,
                smart_override,
            )?
            .0;
        if count_frame {
            self.resident_frames += 1;
        }
        Ok(report)
    }

    pub fn close_resident_session(&mut self) -> ResidentCloseReport {
        let sequence_setdown_error = cleanup_error_code(self.end_sequence(false));
        let global_setdown_result = self.end_global();
        let global_setdown_diagnostic = global_setdown_result
            .as_ref()
            .err()
            .map(|error| self.resident_failure_diagnostic("global_setdown", error));
        let global_setdown_error = cleanup_error_code(global_setdown_result);
        ResidentCloseReport {
            schema_version: 1,
            execution_backend: self.engine.backend_name(),
            frames_rendered: self.resident_frames,
            frame_setdown_error: self.resident_frame_setdown_error,
            sequence_setdown_error,
            global_setdown_error,
            global_setdown_diagnostic,
            suite_requests: self.engine.suite_requests().to_vec(),
            unsupported_suite_calls: self.engine.unsupported_suite_calls().to_vec(),
            dropped_unsupported_suite_calls: self.engine.dropped_unsupported_suite_calls(),
            session_clean: self.resident_frame_setdown_error == 0
                && sequence_setdown_error == 0
                && global_setdown_error == 0,
        }
    }

    pub fn apply_resident_parameter_values(
        &mut self,
        parameter_values: &[ParameterValue],
    ) -> Result<(), ClassicError> {
        let resources = self.frame_resources.as_ref().ok_or_else(|| {
            ClassicError::Input("resident parameters have not been prepared".into())
        })?;
        let mut seen_slots = BTreeSet::new();
        let mut pending_writes = Vec::with_capacity(parameter_values.len());
        for requested in parameter_values {
            let slot = requested.slot.ok_or_else(|| {
                ClassicError::Input("resident parameter update requires an exact slot".into())
            })?;
            if slot == 0 || slot > resources.parameter_definitions.len() || !seen_slots.insert(slot)
            {
                return Err(ClassicError::Input(format!(
                    "resident parameter slot {slot} is invalid or duplicated"
                )));
            }
            let mut definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
            self.engine
                .read(resources.parameter_definitions[slot - 1], &mut definition)?;
            let param_type = read_i32(&definition, abi::PARAM_PARAM_TYPE_OFFSET);
            apply_parameter_value_for_layer(
                &mut definition,
                param_type,
                requested,
                resources.width,
                resources.height,
            )?;
            pending_writes.push((resources.parameter_definitions[slot - 1], definition));
        }
        for (address, definition) in pending_writes {
            self.engine.write(address, &definition)?;
        }
        Ok(())
    }

    pub fn user_changed_parameter(
        &mut self,
        slot: usize,
    ) -> Result<UserChangedReport, ClassicError> {
        if !self.sequence_active {
            return Err(ClassicError::Input(
                "resident session has not been opened".into(),
            ));
        }
        let resources = self.frame_resources.as_ref().ok_or_else(|| {
            ClassicError::Input("resident parameters have not been prepared".into())
        })?;
        if slot == 0 || slot > resources.parameter_definitions.len() {
            return Err(ClassicError::Input(format!(
                "user-changed parameter slot {slot} is invalid"
            )));
        }
        let params = resources.params;
        let definitions = resources.parameter_definitions.clone();
        let extra = match self.user_changed_extra {
            Some(extra) => extra,
            None => {
                let extra = self
                    .engine
                    .allocate(abi::PF_USER_CHANGED_PARAM_EXTRA_SIZE, 4)?;
                self.user_changed_extra = Some(extra);
                extra
            }
        };
        let mut extra_bytes = vec![0u8; abi::PF_USER_CHANGED_PARAM_EXTRA_SIZE];
        write_i32(
            &mut extra_bytes,
            abi::USER_CHANGED_PARAM_INDEX_OFFSET,
            slot as i32,
        );
        self.engine.write(extra, &extra_bytes)?;
        let selector_error = self
            .engine
            .call_selector_win64(
                self.entry,
                [
                    CMD_USER_CHANGED_PARAM,
                    self.input,
                    self.output,
                    params,
                    0,
                    extra,
                ],
            )
            .map_err(|source| selector_guest_error("USER_CHANGED_PARAM", source))?
            as i32;
        if selector_error != 0 {
            return Err(ClassicError::Selector {
                selector: "USER_CHANGED_PARAM",
                error: selector_error,
            });
        }
        let mut parameters = Vec::with_capacity(definitions.len());
        for (index, definition) in definitions.into_iter().enumerate() {
            let mut bytes = vec![0u8; abi::PF_PARAM_DEF_SIZE];
            self.engine.read(definition, &mut bytes)?;
            parameters.push(ParameterUiState {
                slot: index + 1,
                ui_flags: read_u32(&bytes, abi::PARAM_UI_FLAGS_OFFSET),
                flags: read_u32(&bytes, abi::PARAM_FLAGS_OFFSET),
            });
        }
        Ok(UserChangedReport {
            slot,
            selector_error,
            parameters,
        })
    }

    #[cfg(test)]
    pub(crate) fn prepare_test_user_changed_parameters(
        &mut self,
        definitions: Vec<Vec<u8>>,
    ) -> Result<(), ClassicError> {
        let params = self.engine.allocate((definitions.len() + 1) * 8, 8)?;
        let mut parameter_definitions = Vec::with_capacity(definitions.len());
        for (index, definition) in definitions.into_iter().enumerate() {
            if definition.len() != abi::PF_PARAM_DEF_SIZE {
                return Err(ClassicError::Input(
                    "test parameter definition has the wrong size".into(),
                ));
            }
            let address = self.engine.allocate(abi::PF_PARAM_DEF_SIZE, 8)?;
            self.engine.write(address, &definition)?;
            self.engine
                .write_u64(params + ((index + 1) * 8) as u64, address)?;
            parameter_definitions.push(address);
        }
        let placeholder = self.engine.allocate(1, 1)?;
        self.sequence_active = true;
        self.frame_resources = Some(FrameResources {
            width: 1,
            height: 1,
            format: FramePixelFormat::Argb8,
            pixel_bytes: 4,
            input_param: placeholder,
            params,
            output_world: placeholder,
            input_pixels: placeholder,
            output_guard_base: placeholder,
            output_pixels: placeholder,
            parameter_definitions,
            secondary_layers: Vec::new(),
        });
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn test_user_changed_extra(&self) -> Option<u64> {
        self.user_changed_extra
    }

    pub fn failure_report(&self, error: &ClassicError) -> FailureReport {
        let mut return_message = [0u8; abi::OUT_RETURN_MSG_SIZE];
        let return_message = self
            .engine
            .read(
                self.output + abi::OUT_RETURN_MSG_OFFSET as u64,
                &mut return_message,
            )
            .ok()
            .and_then(|_| decode_return_message(&return_message));
        FailureReport {
            schema_version: 1,
            execution_backend: self.engine.backend_name(),
            error: error.to_string(),
            gpu: self.last_gpu_diagnostic.clone(),
            return_message,
            suite_requests: self.engine.suite_requests().to_vec(),
            unsupported_suite_calls: self.engine.unsupported_suite_calls().to_vec(),
            dropped_unsupported_suite_calls: self.engine.dropped_unsupported_suite_calls(),
        }
    }

    pub fn resident_failure_diagnostic(
        &self,
        stage: &'static str,
        error: &ClassicError,
    ) -> ResidentFailureDiagnostic {
        if let ClassicError::Compound { primary, secondary } = error {
            let mut diagnostic = self.resident_failure_diagnostic(stage, primary);
            diagnostic.message = bounded_failure_text(&format!(
                "{}; additionally: {secondary}",
                diagnostic.message
            ));
            return diagnostic;
        }
        let (category, selector, error_code, message, crash_reason, crash_snapshot) = match error {
            ClassicError::Guest(source) => (
                source.diagnostic_category(),
                None,
                None,
                source.diagnostic_message(),
                source.crash_reason().map(str::to_owned),
                source.crash_snapshot(),
            ),
            ClassicError::SelectorGuest { selector, source } => (
                source.diagnostic_category(),
                Some(*selector),
                None,
                source.diagnostic_message(),
                source.crash_reason().map(str::to_owned),
                source.crash_snapshot(),
            ),
            ClassicError::Selector { selector, error } => (
                "selector",
                Some(*selector),
                Some(*error),
                format!("selector {selector} returned {error}"),
                None,
                None,
            ),
            ClassicError::Input(message) => ("input", None, None, message.clone(), None, None),
            ClassicError::Arbitrary { .. } => {
                ("arbitrary", None, None, error.to_string(), None, None)
            }
            ClassicError::Compound { .. } => unreachable!("compound failures are unwrapped above"),
        };
        let suite_requests = self.engine.suite_requests();
        let unsupported_suite_calls = self.engine.unsupported_suite_calls();
        ResidentFailureDiagnostic {
            schema_version: 1,
            stage,
            execution_backend: self.engine.backend_name(),
            category,
            selector,
            error_code,
            message: bounded_failure_text(&message),
            crash_reason: crash_reason.map(|reason| bounded_failure_text(&reason)),
            crash_snapshot: crash_snapshot.map(bounded_crash_snapshot),
            suite_requests: suite_requests
                .iter()
                .take(MAX_FAILURE_SUITE_REQUESTS)
                .map(|request| bounded_text(request, MAX_FAILURE_SUITE_REQUEST_BYTES))
                .collect(),
            dropped_suite_requests: suite_requests
                .len()
                .saturating_sub(MAX_FAILURE_SUITE_REQUESTS)
                as u64,
            unsupported_suite_calls: unsupported_suite_calls
                .iter()
                .take(MAX_FAILURE_UNSUPPORTED_SUITE_CALLS)
                .cloned()
                .collect(),
            dropped_unsupported_suite_calls: self
                .engine
                .dropped_unsupported_suite_calls()
                .saturating_add(
                    unsupported_suite_calls
                        .len()
                        .saturating_sub(MAX_FAILURE_UNSUPPORTED_SUITE_CALLS)
                        as u64,
                ),
        }
    }

    pub fn trace_setup_selector(
        &mut self,
        selector_name: &str,
    ) -> Result<ExecutionTrace, ClassicError> {
        let selector = match selector_name {
            "GLOBAL_SETUP" => CMD_GLOBAL_SETUP,
            "PARAMS_SETUP" => {
                let error = self
                    .invoke(CMD_GLOBAL_SETUP)
                    .map_err(|source| selector_guest_error("GLOBAL_SETUP", source))?
                    as i32;
                if error != 0 {
                    return Err(ClassicError::Selector {
                        selector: "GLOBAL_SETUP",
                        error,
                    });
                }
                let mut output = vec![0u8; abi::PF_OUT_DATA_SIZE];
                self.engine.read(self.output, &mut output)?;
                let global_data = read_u64(&output, abi::OUT_GLOBAL_DATA_OFFSET);
                let mut input = vec![0u8; abi::PF_IN_DATA_SIZE];
                self.engine.read(self.input, &mut input)?;
                write_u64(&mut input, abi::IN_GLOBAL_DATA_OFFSET, global_data);
                self.engine.write(self.input, &input)?;
                CMD_PARAMS_SETUP
            }
            _ => {
                return Err(ClassicError::Input(
                    "trace selector must be GLOBAL_SETUP or PARAMS_SETUP".into(),
                ));
            }
        };
        let before = self.trace_state_snapshot()?;
        self.engine
            .begin_execution_trace(selector_name, self.entry)?;
        let selector_label = match selector {
            CMD_GLOBAL_SETUP => "GLOBAL_SETUP",
            CMD_PARAMS_SETUP => "PARAMS_SETUP",
            _ => unreachable!("validated setup trace selector"),
        };
        let return_value = match self.invoke(selector) {
            Ok(return_value) => return_value,
            Err(source) => {
                self.engine.discard_execution_trace()?;
                return Err(selector_guest_error(selector_label, source));
            }
        };
        let after = match self.trace_state_snapshot() {
            Ok(after) => after,
            Err(error) => {
                self.engine.discard_execution_trace()?;
                return Err(error.into());
            }
        };
        let mut trace = self
            .engine
            .finish_execution_trace(return_value)
            .map_err(ClassicError::from)?;
        trace.set_state_changes(diff_trace_state(before, after));
        Ok(trace)
    }

    pub fn render_default_2x2(&mut self) -> Result<RenderReport, ClassicError> {
        self.render_default_2x2_format(FramePixelFormat::Argb8)
    }

    pub fn render_default_2x2_format(
        &mut self,
        format: FramePixelFormat,
    ) -> Result<RenderReport, ClassicError> {
        self.render_default_2x2_format_with_backend(format, RenderBackendRequest::Cpu)
    }

    pub fn render_default_2x2_format_with_backend(
        &mut self,
        format: FramePixelFormat,
        backend: RenderBackendRequest,
    ) -> Result<RenderReport, ClassicError> {
        let rgba8 = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let pixels = format
            .promote_rgba8(&rgba8)
            .map_err(|error| ClassicError::Input(error.to_string()))?;
        self.render_pixels_with_backend(2, 2, format, &pixels, &[], backend)
    }

    pub fn render_argb8(
        &mut self,
        width: u32,
        height: u32,
        input_argb8: &[u8],
        parameter_values: &[ParameterValue],
    ) -> Result<RenderReport, ClassicError> {
        self.render_pixels(
            width,
            height,
            FramePixelFormat::Argb8,
            input_argb8,
            parameter_values,
        )
    }

    pub fn render_pixels(
        &mut self,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
    ) -> Result<RenderReport, ClassicError> {
        self.render_pixels_with_backend(
            width,
            height,
            format,
            input_pixels,
            parameter_values,
            RenderBackendRequest::Cpu,
        )
    }

    pub fn render_pixels_with_backend(
        &mut self,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
        backend: RenderBackendRequest,
    ) -> Result<RenderReport, ClassicError> {
        self.render_pixels_with_request(
            width,
            height,
            format,
            input_pixels,
            parameter_values,
            [0, 0, width as i32, height as i32],
            false,
            false,
            backend,
        )
        .map(|(report, _)| report)
    }

    pub fn render_argb8_census(
        &mut self,
        width: u32,
        height: u32,
        input_argb8: &[u8],
        parameter_values: &[ParameterValue],
    ) -> Result<RenderReport, ClassicError> {
        self.render_pixels_census(
            width,
            height,
            FramePixelFormat::Argb8,
            input_argb8,
            parameter_values,
        )
    }

    pub fn render_pixels_census(
        &mut self,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
    ) -> Result<RenderReport, ClassicError> {
        self.render_pixels_with_request(
            width,
            height,
            format,
            input_pixels,
            parameter_values,
            [0, 0, width as i32, height as i32],
            true,
            false,
            RenderBackendRequest::Cpu,
        )
        .map(|(report, _)| report)
    }

    pub fn render_argb8_region(
        &mut self,
        width: u32,
        height: u32,
        input_argb8: &[u8],
        parameter_values: &[ParameterValue],
        output_request: [i32; 4],
    ) -> Result<RenderReport, ClassicError> {
        self.render_pixels_region(
            width,
            height,
            FramePixelFormat::Argb8,
            input_argb8,
            parameter_values,
            output_request,
        )
    }

    pub fn render_pixels_region(
        &mut self,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
        output_request: [i32; 4],
    ) -> Result<RenderReport, ClassicError> {
        self.render_pixels_with_request(
            width,
            height,
            format,
            input_pixels,
            parameter_values,
            output_request,
            false,
            false,
            RenderBackendRequest::Cpu,
        )
        .map(|(report, _)| report)
    }

    pub fn render_argb8_trace(
        &mut self,
        width: u32,
        height: u32,
        input_argb8: &[u8],
        parameter_values: &[ParameterValue],
    ) -> Result<(RenderReport, Vec<ExecutionTrace>), ClassicError> {
        self.render_pixels_trace(
            width,
            height,
            FramePixelFormat::Argb8,
            input_argb8,
            parameter_values,
        )
    }

    pub fn render_pixels_trace(
        &mut self,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
    ) -> Result<(RenderReport, Vec<ExecutionTrace>), ClassicError> {
        // The engine keeps trace configuration between renders, so a plain
        // trace resets whatever an earlier watched render configured: it is
        // always a full capture with no watches.
        self.configure_trace(Vec::new(), None);
        self.render_pixels_with_request(
            width,
            height,
            format,
            input_pixels,
            parameter_values,
            [0, 0, width as i32, height as i32],
            false,
            true,
            RenderBackendRequest::Cpu,
        )
    }

    fn configure_trace(&mut self, watches: Vec<TraceWatchSpec>, output_pixel: Option<[u32; 2]>) {
        self.engine
            .configure_trace_checkpoint_only(!watches.is_empty());
        self.engine.configure_trace_watches(watches);
        self.trace_output_pixel = output_pixel;
    }

    pub fn render_argb8_trace_with_watches(
        &mut self,
        width: u32,
        height: u32,
        input_argb8: &[u8],
        parameter_values: &[ParameterValue],
        watches: Vec<TraceWatchSpec>,
        output_pixel: Option<[u32; 2]>,
    ) -> Result<(RenderReport, Vec<ExecutionTrace>), ClassicError> {
        self.render_pixels_trace_with_watches(
            width,
            height,
            FramePixelFormat::Argb8,
            input_argb8,
            parameter_values,
            watches,
            output_pixel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_pixels_trace_with_watches(
        &mut self,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
        watches: Vec<TraceWatchSpec>,
        output_pixel: Option<[u32; 2]>,
    ) -> Result<(RenderReport, Vec<ExecutionTrace>), ClassicError> {
        self.configure_trace(watches, output_pixel);
        self.render_pixels_with_request(
            width,
            height,
            format,
            input_pixels,
            parameter_values,
            [0, 0, width as i32, height as i32],
            false,
            true,
            RenderBackendRequest::Cpu,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_pixels_with_request(
        &mut self,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
        output_request: [i32; 4],
        census_enabled: bool,
        trace_enabled: bool,
        backend: RenderBackendRequest,
    ) -> Result<(RenderReport, Vec<ExecutionTrace>), ClassicError> {
        self.render_pixels_with_request_mode(
            width,
            height,
            format,
            input_pixels,
            parameter_values,
            output_request,
            census_enabled,
            trace_enabled,
            false,
            backend,
            &[],
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_pixels_with_request_mode(
        &mut self,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
        output_request: [i32; 4],
        census_enabled: bool,
        trace_enabled: bool,
        persistent_sequence: bool,
        backend: RenderBackendRequest,
        secondary_layers: &[ResidentLayer<'_>],
        smart_override: Option<bool>,
    ) -> Result<(RenderReport, Vec<ExecutionTrace>), ClassicError> {
        let result = self.render_pixels_with_request_mode_body(
            width,
            height,
            format,
            input_pixels,
            parameter_values,
            output_request,
            census_enabled,
            trace_enabled,
            persistent_sequence,
            backend,
            secondary_layers,
            smart_override,
        );
        // The arbitrary value copies live exactly as long as this render, the
        // minihost ArbitraryValuesScope: dispose them on every exit path, and
        // keep a render failure primary over a disposal failure.
        match (result, self.dispose_arbitrary_values()) {
            (result, Ok(())) => result,
            (Ok(_), Err(cleanup)) => Err(cleanup),
            (Err(primary), Err(secondary)) => Err(ClassicError::Compound {
                primary: Box::new(primary),
                secondary: Box::new(secondary),
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_pixels_with_request_mode_body(
        &mut self,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
        output_request: [i32; 4],
        census_enabled: bool,
        trace_enabled: bool,
        persistent_sequence: bool,
        backend: RenderBackendRequest,
        secondary_layers: &[ResidentLayer<'_>],
        smart_override: Option<bool>,
    ) -> Result<(RenderReport, Vec<ExecutionTrace>), ClassicError> {
        self.last_gpu_diagnostic = GpuRenderDiagnostic::pending(backend);
        if width == 0 || height == 0 || width > MAX_RENDER_WIDTH || height > MAX_RENDER_HEIGHT {
            return Err(ClassicError::Input(format!(
                "dimensions must be within 1x1..={MAX_RENDER_WIDTH}x{MAX_RENDER_HEIGHT}, got {width}x{height}"
            )));
        }
        if output_request[0] < 0
            || output_request[1] < 0
            || output_request[2] > width as i32
            || output_request[3] > height as i32
            || output_request[0] >= output_request[2]
            || output_request[1] >= output_request[3]
        {
            return Err(ClassicError::Input(format!(
                "output request must be a non-empty rectangle inside {width}x{height}, got {output_request:?}"
            )));
        }
        let rowbytes = format
            .rowbytes(width)
            .map_err(|error| ClassicError::Input(error.to_string()))?;
        let pixel_bytes = format
            .byte_count(width, height)
            .map_err(|error| ClassicError::Input(error.to_string()))?;
        format
            .validate_bytes(width, height, input_pixels)
            .map_err(|error| ClassicError::Input(error.to_string()))?;
        let setup = self.setup()?;
        if !format.advertised_by(setup.out_flags, setup.out_flags2) {
            return Err(ClassicError::Input(format!(
                "AEX did not advertise support for {} pixel depth",
                format.name()
            )));
        }
        let smart_capable = setup.out_flags2 & (1 << 10) != 0;
        let smart_render = smart_override.unwrap_or(smart_capable);
        if smart_render && !smart_capable {
            return Err(ClassicError::Input(
                "fixture requested Smart Render but the AEX did not advertise it".into(),
            ));
        }
        if backend.is_gpu() && !smart_render {
            return Err(ClassicError::Input(
                "OpenCL GPU rendering requires Smart Render support".into(),
            ));
        }
        if backend.is_gpu() && format != FramePixelFormat::Argb32f {
            return Err(ClassicError::Input(
                "OpenCL GPU rendering requires argb32f pixel transport".into(),
            ));
        }
        if census_enabled && !smart_render {
            return Err(ClassicError::Input(
                "guest census currently requires Smart Render support".into(),
            ));
        }
        let captured_params = self.engine.parameters().to_vec();
        for requested in parameter_values {
            if let Some(slot) = requested.slot {
                if slot == 0 || slot > captured_params.len() {
                    return Err(ClassicError::Input(format!(
                        "AEX did not declare parameter slot #{slot}"
                    )));
                }
            } else {
                let matches = captured_params
                    .iter()
                    .filter(|captured| captured.name == requested.name)
                    .count();
                if matches == 0 {
                    return Err(ClassicError::Input(format!(
                        "AEX did not declare a supported parameter named {:?}",
                        requested.name
                    )));
                }
                if matches > 1 {
                    return Err(ClassicError::Input(format!(
                        "parameter name {:?} is ambiguous; use Name@slot=value",
                        requested.name
                    )));
                }
            }
        }
        let mut applied_values = Vec::with_capacity(parameter_values.len());
        let mut applied_requests = BTreeSet::new();
        let resources = self.ensure_frame_resources(
            width,
            height,
            format,
            pixel_bytes,
            captured_params.len(),
            secondary_layers,
        )?;
        let input_param = resources.input_param;
        let params = resources.params;
        let output_world = resources.output_world;
        let guest_input_pixels = resources.input_pixels;
        let output_pixels = resources.output_pixels;
        if let Some([x, y]) = self.trace_output_pixel {
            if x >= width || y >= height {
                return Err(ClassicError::Input(format!(
                    "watch output pixel ({x},{y}) is outside {width}x{height}"
                )));
            }
            let row_offset = u64::from(y) * u64::from(rowbytes);
            self.engine.add_trace_watch(TraceWatchSpec {
                id: format!("output-pixel-{x}-{y}"),
                function_rva: None,
                instruction_rva: None,
                absolute_address: Some(
                    output_pixels + row_offset + u64::from(x) * format.bytes_per_pixel() as u64,
                ),
                register: "absolute",
                dereference_offset: None,
                size: format.bytes_per_pixel(),
                occurrence: None,
                image_coordinate: Some([x, y]),
                image_row_offset: Some(row_offset),
                image_format: Some(format.name()),
            });
        }

        let mut input_world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        write_i32(
            &mut input_world,
            abi::LAYER_WORLD_FLAGS_OFFSET,
            format.world_flags(),
        );
        write_u64(&mut input_world, abi::LAYER_DATA_OFFSET, guest_input_pixels);
        write_i32(
            &mut input_world,
            abi::LAYER_ROWBYTES_OFFSET,
            rowbytes as i32,
        );
        write_i32(&mut input_world, abi::LAYER_WIDTH_OFFSET, width as i32);
        write_i32(&mut input_world, abi::LAYER_HEIGHT_OFFSET, height as i32);
        write_rect(
            &mut input_world,
            abi::LAYER_EXTENT_HINT_OFFSET,
            width,
            height,
        );
        write_i32(&mut input_world, abi::LAYER_PIX_ASPECT_RATIO_OFFSET, 1);
        write_u32(&mut input_world, abi::LAYER_PIX_ASPECT_RATIO_OFFSET + 4, 1);
        let mut input_definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        input_definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_LAYER_DEF_SIZE]
            .copy_from_slice(&input_world);
        self.engine.write(input_param, &input_definition)?;
        self.engine.write(guest_input_pixels, input_pixels)?;
        for (layer, allocated) in secondary_layers.iter().zip(&resources.secondary_layers) {
            self.engine.write(allocated.pixels, layer.pixels)?;
        }
        let output_guard = vec![OUTPUT_GUARD_PATTERN; OUTPUT_GUARD_BYTES];
        self.engine
            .write(resources.output_guard_base, &output_guard)?;
        self.engine
            .write(output_pixels + pixel_bytes as u64, &output_guard)?;
        // A resident frame must not inherit pixels the previous frame left in
        // a partially written result region.
        self.engine.write(output_pixels, &vec![0u8; pixel_bytes])?;
        self.engine.write_u64(params, input_param)?;
        for (index, captured) in captured_params.into_iter().enumerate() {
            let mut definition = captured.bytes;
            materialize_default(&mut definition, captured.param_type, width, height)
                .map_err(ClassicError::Input)?;
            if captured.param_type == PARAM_ARBITRARY_DATA {
                let union = abi::PARAM_U_OFFSET;
                let default = read_u64(&definition, union + ARBITRARY_DEFAULT_HANDLE_OFFSET);
                // PF_ADD_ARBITRARY2 leaves the value null when there is no
                // default; otherwise the value is a private COPY of it, never
                // the default handle itself (minihost initialize_arbitrary_values).
                if default != 0 {
                    let id = read_i16(&definition, union);
                    let refcon = read_u64(&definition, union + ARBITRARY_REFCON_OFFSET);
                    let handle = self.copy_arbitrary_default(id, refcon, default)?;
                    write_u64(
                        &mut definition,
                        union + ARBITRARY_VALUE_HANDLE_OFFSET,
                        handle,
                    );
                    self.arbitrary_values.push(ArbitraryValue {
                        id,
                        refcon,
                        handle,
                        definition: resources.parameter_definitions[index],
                    });
                }
            }
            if let Some(layer) = resources
                .secondary_layers
                .iter()
                .find(|layer| layer.slot == index + 1)
            {
                let layer_rowbytes = format
                    .rowbytes(layer.width)
                    .map_err(|error| ClassicError::Input(error.to_string()))?;
                let mut layer_world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
                write_i32(
                    &mut layer_world,
                    abi::LAYER_WORLD_FLAGS_OFFSET,
                    format.world_flags(),
                );
                write_u64(&mut layer_world, abi::LAYER_DATA_OFFSET, layer.pixels);
                write_i32(
                    &mut layer_world,
                    abi::LAYER_ROWBYTES_OFFSET,
                    layer_rowbytes as i32,
                );
                write_i32(
                    &mut layer_world,
                    abi::LAYER_WIDTH_OFFSET,
                    layer.width as i32,
                );
                write_i32(
                    &mut layer_world,
                    abi::LAYER_HEIGHT_OFFSET,
                    layer.height as i32,
                );
                write_rect(
                    &mut layer_world,
                    abi::LAYER_EXTENT_HINT_OFFSET,
                    layer.width,
                    layer.height,
                );
                write_i32(&mut layer_world, abi::LAYER_PIX_ASPECT_RATIO_OFFSET, 1);
                write_u32(&mut layer_world, abi::LAYER_PIX_ASPECT_RATIO_OFFSET + 4, 1);
                if captured.param_type != PARAM_LAYER {
                    return Err(ClassicError::Input(format!(
                        "secondary layer slot {} is not a layer parameter",
                        index + 1
                    )));
                }
                definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_LAYER_DEF_SIZE]
                    .copy_from_slice(&layer_world);
            } else if smart_render {
                materialize_layer_world(&mut definition, captured.param_type, &input_world);
            }
            if let Some((request_index, requested)) =
                parameter_values
                    .iter()
                    .enumerate()
                    .find(|(request_index, requested)| {
                        !applied_requests.contains(request_index)
                            && requested
                                .slot
                                .map_or(requested.name == captured.name, |slot| slot == index + 1)
                    })
            {
                apply_parameter_value_for_layer(
                    &mut definition,
                    captured.param_type,
                    requested,
                    width,
                    height,
                )?;
                applied_requests.insert(request_index);
                applied_values.push(AppliedParameter {
                    slot: index + 1,
                    name: captured.name.clone(),
                    value: requested.value,
                    color: requested.color,
                    point: requested.point,
                    angle: requested.angle,
                    point3d: requested.point3d,
                });
            }
            let parameter = resources.parameter_definitions[index];
            self.engine.write(parameter, &definition)?;
            self.engine
                .write_u64(params + ((index + 1) * 8) as u64, parameter)?;
        }
        self.engine
            .configure_parameter_definitions(input_param, resources.parameter_definitions)?;
        if applied_requests.len() != parameter_values.len() {
            return Err(ClassicError::Input(format!(
                "parameter application count mismatch: requested {}, applied {}",
                parameter_values.len(),
                applied_values.len()
            )));
        }

        let mut world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        write_i32(
            &mut world,
            abi::LAYER_WORLD_FLAGS_OFFSET,
            format.world_flags(),
        );
        write_u64(&mut world, abi::LAYER_DATA_OFFSET, output_pixels);
        write_i32(&mut world, abi::LAYER_ROWBYTES_OFFSET, rowbytes as i32);
        write_i32(&mut world, abi::LAYER_WIDTH_OFFSET, width as i32);
        write_i32(&mut world, abi::LAYER_HEIGHT_OFFSET, height as i32);
        write_rect(&mut world, abi::LAYER_EXTENT_HINT_OFFSET, width, height);
        write_i32(&mut world, abi::LAYER_PIX_ASPECT_RATIO_OFFSET, 1);
        write_u32(&mut world, abi::LAYER_PIX_ASPECT_RATIO_OFFSET + 4, 1);
        self.engine.write(output_world, &world)?;
        self.engine
            .configure_render_pixel_format(format.pf_pixel_format());
        let mut input_data = vec![0u8; abi::PF_IN_DATA_SIZE];
        self.engine.read(self.input, &mut input_data)?;
        write_i32(&mut input_data, abi::IN_WIDTH_OFFSET, width as i32);
        write_i32(&mut input_data, abi::IN_HEIGHT_OFFSET, height as i32);
        write_rect(&mut input_data, abi::IN_EXTENT_HINT_OFFSET, width, height);
        self.engine.write(self.input, &input_data)?;
        let mut traces = Vec::new();
        if !self.sequence_active {
            let (sequence_setup_result, trace) = self.call_with_optional_trace(
                "SEQUENCE_SETUP",
                [CMD_SEQUENCE_SETUP, self.input, self.output, 0, 0, 0],
                trace_enabled,
            )?;
            traces.extend(trace);
            let sequence_setup_error = sequence_setup_result as i32;
            if sequence_setup_error != 0 {
                return Err(ClassicError::Selector {
                    selector: "SEQUENCE_SETUP",
                    error: sequence_setup_error,
                });
            }
            let sequence_data = self.read_output_pointer(abi::OUT_SEQUENCE_DATA_OFFSET)?;
            self.write_input_pointer(abi::IN_SEQUENCE_DATA_OFFSET, sequence_data)?;
            self.sequence_active = true;
        }

        let (frame_setup_result, trace) = self.call_with_optional_trace(
            "FRAME_SETUP",
            [
                CMD_FRAME_SETUP,
                self.input,
                self.output,
                params,
                output_world,
                0,
            ],
            trace_enabled,
        )?;
        traces.extend(trace);
        let frame_setup_error = frame_setup_result as i32;
        if frame_setup_error != 0 {
            if !persistent_sequence {
                let _ = self.end_sequence(false);
            }
            return Err(ClassicError::Selector {
                selector: "FRAME_SETUP",
                error: frame_setup_error,
            });
        }
        let frame_execution = (|| {
            let frame_data = self.read_output_pointer(abi::OUT_FRAME_DATA_OFFSET)?;
            self.write_input_pointer(abi::IN_FRAME_DATA_OFFSET, frame_data)?;
            if smart_render {
                self.render_smart(
                    params,
                    input_param,
                    output_world,
                    width,
                    height,
                    format,
                    output_request,
                    census_enabled,
                    trace_enabled,
                    backend,
                )
            } else {
                if output_request != [0, 0, width as i32, height as i32] {
                    return Err(ClassicError::Input(
                        "region rendering requires Smart Render support".into(),
                    ));
                }
                let (return_value, trace) = self.call_with_optional_trace(
                    "RENDER",
                    [CMD_RENDER, self.input, self.output, params, output_world, 0],
                    trace_enabled,
                )?;
                let render_error = return_value as i32;
                self.last_gpu_diagnostic = GpuRenderDiagnostic::classic_cpu(render_error);
                Ok((render_error, None, trace.into_iter().collect()))
            }
        })();
        let frame_setdown = self.call_with_optional_trace(
            "FRAME_SETDOWN",
            [
                CMD_FRAME_SETDOWN,
                self.input,
                self.output,
                params,
                output_world,
                0,
            ],
            trace_enabled,
        );
        let (mut frame_setdown_error, mut frame_setdown_failure) = match frame_setdown {
            Ok((result, trace)) => {
                traces.extend(trace);
                let error = result as i32;
                (error, selector_failure("FRAME_SETDOWN", error))
            }
            Err(error) => (CLEANUP_GUEST_ERROR, Some(error)),
        };
        if let Err(source) = self.write_input_pointer(abi::IN_FRAME_DATA_OFFSET, 0) {
            if frame_setdown_failure.is_none() {
                frame_setdown_error = CLEANUP_GUEST_ERROR;
                frame_setdown_failure = Some(ClassicError::SelectorGuest {
                    selector: "FRAME_SETDOWN",
                    source,
                });
            }
        }
        if persistent_sequence && self.resident_frame_setdown_error == 0 && frame_setdown_error != 0
        {
            self.resident_frame_setdown_error = frame_setdown_error;
        }
        let (render_error, census, render_traces) = match frame_execution {
            Ok(result) => result,
            Err(error) => {
                if !persistent_sequence {
                    let _ = self.end_sequence(false);
                }
                return Err(error);
            }
        };
        traces.extend(render_traces);
        let mut failure = selector_failure("RENDER", render_error).or(frame_setdown_failure);
        if !persistent_sequence {
            let sequence_setdown = self.call_with_optional_trace(
                "SEQUENCE_SETDOWN",
                [CMD_SEQUENCE_SETDOWN, self.input, self.output, 0, 0, 0],
                trace_enabled,
            );
            match sequence_setdown {
                Ok((result, trace)) => {
                    traces.extend(trace);
                    let error = result as i32;
                    if failure.is_none() {
                        failure = selector_failure("SEQUENCE_SETDOWN", error);
                    }
                }
                Err(error) if failure.is_none() => failure = Some(error),
                Err(_) => {}
            }
            self.sequence_active = false;
            if let Err(source) = self.write_input_pointer(abi::IN_SEQUENCE_DATA_OFFSET, 0) {
                if failure.is_none() {
                    failure = Some(ClassicError::SelectorGuest {
                        selector: "SEQUENCE_SETDOWN",
                        source,
                    });
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        let mut leading_guard = vec![0u8; OUTPUT_GUARD_BYTES];
        let mut trailing_guard = vec![0u8; OUTPUT_GUARD_BYTES];
        self.engine
            .read(resources.output_guard_base, &mut leading_guard)?;
        self.engine
            .read(output_pixels + pixel_bytes as u64, &mut trailing_guard)?;
        if leading_guard
            .iter()
            .chain(&trailing_guard)
            .any(|byte| *byte != OUTPUT_GUARD_PATTERN)
        {
            return Err(ClassicError::Input(
                "guest render corrupted an output pixel guard".into(),
            ));
        }
        let mut raw_pixels = vec![0u8; pixel_bytes];
        self.engine.read(output_pixels, &mut raw_pixels)?;
        let mut raw_input_pixels = vec![0u8; pixel_bytes];
        self.engine
            .read(guest_input_pixels, &mut raw_input_pixels)?;
        let mut raw_secondary_layers = Vec::with_capacity(resources.secondary_layers.len());
        for layer in &resources.secondary_layers {
            let layer_bytes = format
                .byte_count(layer.width, layer.height)
                .map_err(|error| ClassicError::Input(error.to_string()))?;
            let mut pixels = vec![0u8; layer_bytes];
            self.engine.read(layer.pixels, &mut pixels)?;
            raw_secondary_layers.push(ResidentWorldDump {
                slot: layer.slot,
                width: layer.width,
                height: layer.height,
                raw_pixels: pixels,
            });
        }
        let argb8 = format
            .to_argb8_preview(&raw_pixels)
            .map_err(|error| ClassicError::Input(error.to_string()))?;
        let raw_pixel_sha256 = format!("{:x}", Sha256::digest(&raw_pixels));
        Ok((
            RenderReport {
                schema_version: 1,
                setup,
                render_error,
                render_mode: if output_request != [0, 0, width as i32, height as i32] {
                    if backend.is_gpu() {
                        "smart-opencl-region"
                    } else {
                        "smart-cpu-region"
                    }
                } else if backend.is_gpu() {
                    "smart-opencl"
                } else if smart_render {
                    "smart-cpu"
                } else {
                    "classic"
                },
                gpu: self.last_gpu_diagnostic.clone(),
                width,
                height,
                pixel_format: format.name(),
                raw_pixel_bytes: raw_pixels.len(),
                raw_pixel_sha256,
                guards_intact: true,
                parameter_values: applied_values,
                output_request,
                input_requests: self.engine.pre_checkout_requests().to_vec(),
                suite_requests: self.engine.suite_requests().to_vec(),
                unsupported_suite_calls: self.engine.unsupported_suite_calls().to_vec(),
                dropped_unsupported_suite_calls: self.engine.dropped_unsupported_suite_calls(),
                smart_checkout_disk_id_fallbacks: self
                    .engine
                    .smart_checkout_disk_id_fallbacks()
                    .to_vec(),
                census,
                argb8,
                raw_pixels,
                raw_input_pixels,
                raw_secondary_layers,
            },
            traces,
        ))
    }

    fn ensure_frame_resources(
        &mut self,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        pixel_bytes: usize,
        parameter_count: usize,
        secondary_layers: &[ResidentLayer<'_>],
    ) -> Result<FrameResources, ClassicError> {
        if let Some(resources) = &self.frame_resources {
            if resources.width != width
                || resources.height != height
                || resources.format != format
                || resources.pixel_bytes != pixel_bytes
                || resources.parameter_definitions.len() != parameter_count
                || resources.secondary_layers.len() != secondary_layers.len()
                || resources.secondary_layers.iter().zip(secondary_layers).any(
                    |(allocated, requested)| {
                        allocated.slot != requested.slot
                            || allocated.width != requested.width
                            || allocated.height != requested.height
                    },
                )
            {
                return Err(ClassicError::Input(
                    "resident frame structure changed; reopen the session".into(),
                ));
            }
            return Ok(resources.clone());
        }
        let input_param = self.engine.allocate(abi::PF_PARAM_DEF_SIZE, 8)?;
        let params = self.engine.allocate((parameter_count + 1) * 8, 8)?;
        let output_world = self.engine.allocate(abi::PF_LAYER_DEF_SIZE, 8)?;
        let input_pixels = self.engine.allocate(pixel_bytes, 64)?;
        let guarded_output_bytes = pixel_bytes
            .checked_add(OUTPUT_GUARD_BYTES * 2)
            .ok_or_else(|| ClassicError::Input("guarded output size overflow".into()))?;
        let output_guard_base = self.engine.allocate(guarded_output_bytes, 64)?;
        let output_pixels = output_guard_base + OUTPUT_GUARD_BYTES as u64;
        let mut parameter_definitions = Vec::with_capacity(parameter_count);
        for _ in 0..parameter_count {
            parameter_definitions.push(self.engine.allocate(abi::PF_PARAM_DEF_SIZE, 8)?);
        }
        let mut allocated_layers = Vec::with_capacity(secondary_layers.len());
        let mut seen_slots = BTreeSet::new();
        for layer in secondary_layers {
            if layer.slot == 0
                || layer.slot > parameter_count
                || !seen_slots.insert(layer.slot)
                || layer.width == 0
                || layer.height == 0
            {
                return Err(ClassicError::Input(
                    "secondary layer identity or dimensions are invalid".into(),
                ));
            }
            format
                .validate_bytes(layer.width, layer.height, layer.pixels)
                .map_err(|error| ClassicError::Input(error.to_string()))?;
            let bytes = format
                .byte_count(layer.width, layer.height)
                .map_err(|error| ClassicError::Input(error.to_string()))?;
            allocated_layers.push(ResidentLayerResources {
                slot: layer.slot,
                width: layer.width,
                height: layer.height,
                pixels: self.engine.allocate(bytes, 64)?,
            });
        }
        let resources = FrameResources {
            width,
            height,
            format,
            pixel_bytes,
            input_param,
            params,
            output_world,
            input_pixels,
            output_guard_base,
            output_pixels,
            parameter_definitions,
            secondary_layers: allocated_layers,
        };
        self.frame_resources = Some(resources.clone());
        Ok(resources)
    }

    fn invoke(&mut self, selector: u64) -> Result<u64, GuestError> {
        self.engine
            .call_selector_win64(self.entry, [selector, self.input, self.output, 0, 0, 0])
    }

    fn write_frame_context(
        &mut self,
        width: u32,
        height: u32,
        current_time: i32,
        time_step: i32,
        total_time: i32,
        time_scale: u32,
    ) -> Result<(), ClassicError> {
        let mut input = vec![0u8; abi::PF_IN_DATA_SIZE];
        self.engine.read(self.input, &mut input)?;
        populate_frame_context(
            &mut input,
            width,
            height,
            current_time,
            time_step,
            total_time,
            time_scale,
        );
        self.engine.write(self.input, &input)?;
        Ok(())
    }

    fn end_sequence(&mut self, trace_enabled: bool) -> Result<i32, ClassicError> {
        if !self.sequence_active {
            return Ok(0);
        }
        let result = self.call_with_optional_trace(
            "SEQUENCE_SETDOWN",
            [CMD_SEQUENCE_SETDOWN, self.input, self.output, 0, 0, 0],
            trace_enabled,
        );
        self.sequence_active = false;
        let clear = self.write_input_pointer(abi::IN_SEQUENCE_DATA_OFFSET, 0);
        match result {
            Ok((result, _)) => {
                clear?;
                Ok(result as i32)
            }
            Err(error) => {
                let _ = clear;
                Err(error)
            }
        }
    }

    fn end_global(&mut self) -> Result<i32, ClassicError> {
        if !self.global_active {
            return Ok(0);
        }
        // Both disposals go through PF_Cmd_ARBITRARY_CALLBACK and need the
        // plug-in's global data, so they run before GLOBAL_SETDOWN.
        let cleanup = combine_failures(
            [
                self.dispose_arbitrary_values(),
                self.dispose_arbitrary_defaults(),
            ]
            .into_iter()
            .filter_map(Result::err)
            .collect(),
        );
        let result = self.invoke(CMD_GLOBAL_SETDOWN);
        self.global_active = false;
        let clear = self.write_input_pointer(abi::IN_GLOBAL_DATA_OFFSET, 0);
        let setdown = match result {
            Ok(result) => clear.map(|()| result as i32).map_err(ClassicError::from),
            Err(error) => Err(selector_guest_error("GLOBAL_SETDOWN", error)),
        };
        // A GLOBAL_SETDOWN failure (possibly a guest crash) stays the primary
        // error; a disposal failure is reported alongside it, never instead.
        match (setdown, cleanup) {
            (setdown, Ok(())) => setdown,
            (Ok(0), Err(cleanup)) => Err(cleanup),
            (Ok(error), Err(cleanup)) => Err(ClassicError::Compound {
                primary: Box::new(cleanup),
                secondary: Box::new(ClassicError::Selector {
                    selector: "GLOBAL_SETDOWN",
                    error,
                }),
            }),
            (Err(setdown), Err(cleanup)) => Err(ClassicError::Compound {
                primary: Box::new(setdown),
                secondary: Box::new(cleanup),
            }),
        }
    }

    fn arbitrary_extra(&mut self) -> Result<u64, ClassicError> {
        if let Some(extra) = self.arbitrary_extra {
            return Ok(extra);
        }
        let extra = self.engine.allocate(ARBITRARY_EXTRA_BYTES, 8)?;
        self.arbitrary_extra = Some(extra);
        Ok(extra)
    }

    /// Issues one `PF_Cmd_ARBITRARY_CALLBACK` with a freshly zeroed
    /// `PF_ArbParamsExtra` and returns the plug-in's error code.
    fn call_arbitrary_callback(
        &mut self,
        which_function: i32,
        id: i16,
        refcon: u64,
        payload: &[(usize, u64)],
    ) -> Result<i32, ClassicError> {
        let extra = self.arbitrary_extra()?;
        let mut bytes = vec![0u8; ARBITRARY_EXTRA_BYTES];
        write_i32(&mut bytes, 0, which_function);
        write_i16(&mut bytes, ARBITRARY_EXTRA_ID_OFFSET, id);
        write_u64(&mut bytes, ARBITRARY_EXTRA_REFCON_OFFSET, refcon);
        for (offset, value) in payload {
            write_u64(&mut bytes, *offset, *value);
        }
        self.engine.write(extra, &bytes)?;
        let error = self
            .engine
            .call_selector_win64(
                self.entry,
                [CMD_ARBITRARY_CALLBACK, self.input, self.output, 0, 0, extra],
            )
            .map_err(|source| selector_guest_error("ARBITRARY_CALLBACK", source))?;
        Ok(error as i32)
    }

    /// `PF_Arbitrary_COPY_FUNC`: duplicates `source` into a handle the render
    /// owns. A null or aliased destination is a contract failure, not a value.
    fn copy_arbitrary_default(
        &mut self,
        id: i16,
        refcon: u64,
        source: u64,
    ) -> Result<u64, ClassicError> {
        let destination_cell =
            self.arbitrary_extra()? + ARBITRARY_EXTRA_DESTINATION_CELL_OFFSET as u64;
        let error = self.call_arbitrary_callback(
            ARBITRARY_COPY_FUNC,
            id,
            refcon,
            &[
                (ARBITRARY_EXTRA_HANDLE_OFFSET, source),
                (ARBITRARY_EXTRA_DESTINATION_POINTER_OFFSET, destination_cell),
            ],
        )?;
        if error != 0 {
            return Err(ClassicError::Arbitrary {
                operation: "COPY",
                id,
                message: format!("ARBITRARY_CALLBACK returned {error}"),
            });
        }
        let destination = self.read_guest_u64(destination_cell)?;
        if destination == 0 {
            return Err(ClassicError::Arbitrary {
                operation: "COPY",
                id,
                message: "plug-in returned a null destination handle".into(),
            });
        }
        if destination == source {
            return Err(ClassicError::Arbitrary {
                operation: "COPY",
                id,
                message: "plug-in returned the source handle instead of a copy".into(),
            });
        }
        Ok(destination)
    }

    /// `PF_Arbitrary_DISPOSE_FUNC` for one handle.
    fn dispose_arbitrary_handle(
        &mut self,
        id: i16,
        refcon: u64,
        handle: u64,
    ) -> Result<(), ClassicError> {
        let error = self.call_arbitrary_callback(
            ARBITRARY_DISPOSE_FUNC,
            id,
            refcon,
            &[(ARBITRARY_EXTRA_HANDLE_OFFSET, handle)],
        )?;
        if error != 0 {
            return Err(ClassicError::Arbitrary {
                operation: "DISPOSE",
                id,
                message: format!("ARBITRARY_CALLBACK returned {error}"),
            });
        }
        Ok(())
    }

    /// Disposes every render-owned value copy. Each slot is released exactly
    /// once whether or not its DISPOSE succeeds, and every failure is kept.
    fn dispose_arbitrary_values(&mut self) -> Result<(), ClassicError> {
        let mut failures = Vec::new();
        for value in std::mem::take(&mut self.arbitrary_values) {
            if let Err(error) = self.dispose_arbitrary_handle(value.id, value.refcon, value.handle)
            {
                failures.push(error);
            }
            if let Err(error) = self.engine.write_u64(
                value.definition + (abi::PARAM_U_OFFSET + ARBITRARY_VALUE_HANDLE_OFFSET) as u64,
                0,
            ) {
                failures.push(ClassicError::Guest(error));
            }
        }
        combine_failures(failures)
    }

    /// Disposes every captured arbitrary default (minihost
    /// `dispose_arbitrary_defaults`): all slots are visited, each default slot
    /// is nulled whether or not its DISPOSE succeeded so it is never disposed
    /// twice, and the failures are reported together.
    fn dispose_arbitrary_defaults(&mut self) -> Result<(), ClassicError> {
        let union = abi::PARAM_U_OFFSET;
        let owned = self
            .engine
            .parameters()
            .iter()
            .enumerate()
            .filter(|(_, parameter)| parameter.param_type == PARAM_ARBITRARY_DATA)
            .map(|(index, parameter)| {
                (
                    index,
                    read_i16(&parameter.bytes, union),
                    read_u64(&parameter.bytes, union + ARBITRARY_DEFAULT_HANDLE_OFFSET),
                    read_u64(&parameter.bytes, union + ARBITRARY_REFCON_OFFSET),
                )
            })
            .collect::<Vec<_>>();
        let mut failures = Vec::new();
        for (index, id, handle, refcon) in owned {
            if handle == 0 {
                continue;
            }
            if let Err(error) = self.dispose_arbitrary_handle(id, refcon, handle) {
                failures.push(error);
            }
            if let Some(parameter) = self.engine.parameters_mut().get_mut(index) {
                write_u64(
                    &mut parameter.bytes,
                    union + ARBITRARY_DEFAULT_HANDLE_OFFSET,
                    0,
                );
            }
        }
        combine_failures(failures)
    }

    fn call_with_optional_trace(
        &mut self,
        selector_name: &'static str,
        args: [u64; 6],
        trace_enabled: bool,
    ) -> Result<(u64, Option<ExecutionTrace>), ClassicError> {
        let before = trace_enabled
            .then(|| self.trace_state_snapshot())
            .transpose()?;
        if trace_enabled {
            self.engine
                .begin_execution_trace(selector_name, self.entry)?;
        }
        let return_value = match self.engine.call_selector_win64(self.entry, args) {
            Ok(return_value) => return_value,
            Err(source) => {
                if trace_enabled {
                    self.engine.discard_execution_trace()?;
                }
                return Err(selector_guest_error(selector_name, source));
            }
        };
        let after = trace_enabled
            .then(|| self.trace_state_snapshot())
            .transpose()?;
        let trace = if trace_enabled {
            let mut trace = self.engine.finish_execution_trace(return_value)?;
            trace.set_state_changes(diff_trace_state(
                before.expect("trace snapshot"),
                after.expect("trace snapshot"),
            ));
            Some(trace)
        } else {
            None
        };
        Ok((return_value, trace))
    }

    fn render_smart(
        &mut self,
        params: u64,
        input_param: u64,
        output_world: u64,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        output_request: [i32; 4],
        census_enabled: bool,
        trace_enabled: bool,
        backend: RenderBackendRequest,
    ) -> Result<(i32, Option<GuestCensus>, Vec<ExecutionTrace>), ClassicError> {
        let input_world = input_param + abi::PARAM_U_OFFSET as u64;
        let mut current_time = [0u8; 4];
        let mut current_time_scale = [0u8; 4];
        self.engine.read(
            self.input + abi::IN_CURRENT_TIME_OFFSET as u64,
            &mut current_time,
        )?;
        self.engine.read(
            self.input + abi::IN_TIME_SCALE_OFFSET as u64,
            &mut current_time_scale,
        )?;
        self.engine.configure_smart_render(
            input_world,
            output_world,
            width,
            height,
            format.pf_pixel_format(),
            i32::from_le_bytes(current_time),
            u32::from_le_bytes(current_time_scale),
        );
        let result = self.render_smart_configured(
            params,
            width,
            height,
            format,
            output_request,
            census_enabled,
            trace_enabled,
            backend,
        );
        // PF Smart Render plug-ins in the frozen corpus commonly retain a
        // checked-out host world until the selector returns. The world is
        // host-owned, so selector-scope cleanup is the ownership boundary.
        self.engine.finish_smart_checkout_scope();
        result
    }

    fn render_smart_configured(
        &mut self,
        params: u64,
        width: u32,
        height: u32,
        format: FramePixelFormat,
        output_request: [i32; 4],
        census_enabled: bool,
        trace_enabled: bool,
        backend: RenderBackendRequest,
    ) -> Result<(i32, Option<GuestCensus>, Vec<ExecutionTrace>), ClassicError> {
        let mut traces = Vec::new();
        let pre_input = self.engine.allocate(abi::PF_PRE_RENDER_INPUT_SIZE, 8)?;
        let pre_output = self.engine.allocate(abi::PF_PRE_RENDER_OUTPUT_SIZE, 8)?;
        let pre_callbacks = self.engine.allocate(abi::PF_PRE_RENDER_CALLBACKS_SIZE, 8)?;
        let pre_extra = self.engine.allocate(abi::PF_PRE_RENDER_EXTRA_SIZE, 8)?;
        let mut pre_input_bytes = vec![0u8; abi::PF_PRE_RENDER_INPUT_SIZE];
        write_rect_values(
            &mut pre_input_bytes,
            abi::PRE_INPUT_OUTPUT_REQUEST_OFFSET,
            output_request,
        );
        write_i16(
            &mut pre_input_bytes,
            abi::PRE_INPUT_BITDEPTH_OFFSET,
            format.bit_depth(),
        );
        self.engine
            .write(pre_output, &vec![0u8; abi::PF_PRE_RENDER_OUTPUT_SIZE])?;
        let mut pre_callbacks_bytes = vec![0u8; abi::PF_PRE_RENDER_CALLBACKS_SIZE];
        write_u64(
            &mut pre_callbacks_bytes,
            abi::PRE_CALLBACKS_CHECKOUT_LAYER_OFFSET,
            self.engine.pre_checkout_layer_callback_address(),
        );
        write_u64(
            &mut pre_callbacks_bytes,
            8,
            self.engine.noop_callback_address(),
        );
        self.engine.write(pre_callbacks, &pre_callbacks_bytes)?;
        let mut pre_extra_bytes = vec![0u8; abi::PF_PRE_RENDER_EXTRA_SIZE];
        write_u64(&mut pre_extra_bytes, abi::PRE_EXTRA_INPUT_OFFSET, pre_input);
        write_u64(
            &mut pre_extra_bytes,
            abi::PRE_EXTRA_OUTPUT_OFFSET,
            pre_output,
        );
        write_u64(
            &mut pre_extra_bytes,
            abi::PRE_EXTRA_CALLBACKS_OFFSET,
            pre_callbacks,
        );
        self.engine.write(pre_extra, &pre_extra_bytes)?;
        let mut pre_output_bytes = vec![0u8; abi::PF_PRE_RENDER_OUTPUT_SIZE];
        let smart_input = self.engine.allocate(abi::PF_SMART_RENDER_INPUT_SIZE, 8)?;
        let smart_callbacks = self
            .engine
            .allocate(abi::PF_SMART_RENDER_CALLBACKS_SIZE, 8)?;
        let smart_extra = self.engine.allocate(abi::PF_SMART_RENDER_EXTRA_SIZE, 8)?;
        let mut smart_callbacks_bytes = vec![0u8; abi::PF_SMART_RENDER_CALLBACKS_SIZE];
        write_u64(
            &mut smart_callbacks_bytes,
            abi::SMART_CALLBACKS_CHECKOUT_LAYER_PIXELS_OFFSET,
            self.engine.checkout_layer_pixels_callback_address(),
        );
        write_u64(
            &mut smart_callbacks_bytes,
            abi::SMART_CALLBACKS_CHECKIN_LAYER_PIXELS_OFFSET,
            self.engine.checkin_layer_pixels_callback_address(),
        );
        write_u64(
            &mut smart_callbacks_bytes,
            abi::SMART_CALLBACKS_CHECKOUT_OUTPUT_OFFSET,
            self.engine.checkout_output_callback_address(),
        );
        self.engine.write(smart_callbacks, &smart_callbacks_bytes)?;
        let mut smart_extra_bytes = vec![0u8; abi::PF_SMART_RENDER_EXTRA_SIZE];
        write_u64(
            &mut smart_extra_bytes,
            abi::SMART_EXTRA_INPUT_OFFSET,
            smart_input,
        );
        write_u64(
            &mut smart_extra_bytes,
            abi::SMART_EXTRA_CALLBACKS_OFFSET,
            smart_callbacks,
        );
        self.engine.write(smart_extra, &smart_extra_bytes)?;

        let gpu_buffers = if backend.is_gpu() {
            Some((
                self.engine
                    .allocate(abi::PF_GPU_DEVICE_SETUP_INPUT_SIZE, 8)?,
                self.engine
                    .allocate(abi::PF_GPU_DEVICE_SETUP_OUTPUT_SIZE, 8)?,
                self.engine
                    .allocate(abi::PF_GPU_DEVICE_SETUP_EXTRA_SIZE, 8)?,
                self.engine
                    .allocate(abi::PF_GPU_DEVICE_SETDOWN_INPUT_SIZE, 8)?,
                self.engine
                    .allocate(abi::PF_GPU_DEVICE_SETDOWN_EXTRA_SIZE, 8)?,
            ))
        } else {
            None
        };

        let mut runtime_started = false;
        let mut transport_prepared = false;
        let mut transport_finished = false;
        let mut transport_prepare_error = None;
        let mut transport_cleanup_error = None;
        if let Some(runtime_backend) = backend.runtime_backend() {
            let device_index = backend
                .device_index()
                .expect("GPU runtime request carries a device index");
            if let Err(source) = self.engine.begin_gpu_runtime(runtime_backend, device_index) {
                let mut diagnostic = GpuRenderDiagnostic::pending(backend);
                diagnostic.runtime_begin_error = Some(source.to_string());
                diagnostic.device_suite = Some(self.engine.gpu_suite_evidence());
                diagnostic.opencl = Some(self.engine.opencl_bridge_evidence());
                diagnostic.wgpu = self.engine.wgpu_runtime_evidence();
                diagnostic.cleanup_complete = !self.engine.gpu_runtime_active();
                self.last_gpu_diagnostic = diagnostic;
                return Err(ClassicError::Guest(source));
            }
            runtime_started = true;
        }

        let mut execution = run_smart_lifecycle(
            backend,
            abi::PF_GPU_FRAMEWORK_OPENCL as i32,
            abi::PF_RENDER_OUTPUT_FLAG_GPU_RENDER_POSSIBLE as u16,
            |call| -> Result<LifecycleReply<Option<GuestCensus>, ClassicError>, ClassicError> {
                match call {
                    LifecycleCall::Setup {
                        framework,
                        device_index,
                    } => {
                        let (
                            setup_input,
                            setup_output,
                            setup_extra,
                            _setdown_input,
                            _setdown_extra,
                        ) = gpu_buffers.expect("GPU lifecycle allocated setup buffers");
                        let mut input_bytes = vec![0u8; abi::PF_GPU_DEVICE_SETUP_INPUT_SIZE];
                        write_i32(
                            &mut input_bytes,
                            abi::GPU_SETUP_INPUT_WHAT_GPU_OFFSET,
                            framework,
                        );
                        write_u32(
                            &mut input_bytes,
                            abi::GPU_SETUP_INPUT_DEVICE_INDEX_OFFSET,
                            device_index,
                        );
                        self.engine.write(setup_input, &input_bytes)?;
                        self.engine.write(
                            setup_output,
                            &vec![0u8; abi::PF_GPU_DEVICE_SETUP_OUTPUT_SIZE],
                        )?;
                        let mut extra_bytes = vec![0u8; abi::PF_GPU_DEVICE_SETUP_EXTRA_SIZE];
                        write_u64(
                            &mut extra_bytes,
                            abi::GPU_SETUP_EXTRA_INPUT_OFFSET,
                            setup_input,
                        );
                        write_u64(
                            &mut extra_bytes,
                            abi::GPU_SETUP_EXTRA_OUTPUT_OFFSET,
                            setup_output,
                        );
                        self.engine.write(setup_extra, &extra_bytes)?;
                        let (return_value, trace) = self.call_with_optional_trace(
                            "GPU_DEVICE_SETUP",
                            [
                                CMD_GPU_DEVICE_SETUP,
                                self.input,
                                self.output,
                                params,
                                0,
                                setup_extra,
                            ],
                            trace_enabled,
                        )?;
                        traces.extend(trace);
                        let error = return_value as i32;
                        let gpu_data = if error == 0 {
                            self.read_guest_u64(
                                setup_output + abi::GPU_SETUP_OUTPUT_GPU_DATA_OFFSET as u64,
                            )
                            .map_err(ClassicError::from)
                        } else {
                            Ok(0)
                        };
                        Ok(LifecycleReply::Setup { error, gpu_data })
                    }
                    LifecycleCall::PreRender { context } => {
                        if let Some(context) = context {
                            write_u64(
                                &mut pre_input_bytes,
                                abi::PRE_INPUT_GPU_DATA_OFFSET,
                                context.gpu_data,
                            );
                            write_i32(
                                &mut pre_input_bytes,
                                abi::PRE_INPUT_WHAT_GPU_OFFSET,
                                context.framework,
                            );
                            write_u32(
                                &mut pre_input_bytes,
                                abi::PRE_INPUT_DEVICE_INDEX_OFFSET,
                                context.device_index,
                            );
                        }
                        self.engine.write(pre_input, &pre_input_bytes)?;
                        let (return_value, trace) = self.call_with_optional_trace(
                            "SMART_PRE_RENDER",
                            [
                                CMD_SMART_PRE_RENDER,
                                self.input,
                                self.output,
                                params,
                                0,
                                pre_extra,
                            ],
                            trace_enabled,
                        )?;
                        traces.extend(trace);
                        self.engine.read(pre_output, &mut pre_output_bytes)?;
                        let output_flags = u16::from_le_bytes(
                            pre_output_bytes[abi::PRE_OUTPUT_FLAGS_OFFSET
                                ..abi::PRE_OUTPUT_FLAGS_OFFSET + abi::PRE_OUTPUT_FLAGS_SIZE]
                                .try_into()
                                .expect("PF_PreRenderOutput flags are two bytes"),
                        );
                        Ok(LifecycleReply::PreRender {
                            error: return_value as i32,
                            output_flags,
                        })
                    }
                    LifecycleCall::RenderCpu | LifecycleCall::RenderGpu { .. } => {
                        let context = match call {
                            LifecycleCall::RenderGpu { context } => Some(context),
                            LifecycleCall::RenderCpu => None,
                            _ => unreachable!(),
                        };
                        let mut smart_input_bytes = vec![0u8; abi::PF_SMART_RENDER_INPUT_SIZE];
                        write_i16(
                            &mut smart_input_bytes,
                            abi::SMART_INPUT_BITDEPTH_OFFSET,
                            format.bit_depth(),
                        );
                        write_u64(
                            &mut smart_input_bytes,
                            abi::SMART_INPUT_PRE_RENDER_DATA_OFFSET,
                            read_u64(&pre_output_bytes, abi::PRE_OUTPUT_PRE_RENDER_DATA_OFFSET),
                        );
                        if let Some(context) = context {
                            write_u64(
                                &mut smart_input_bytes,
                                abi::SMART_INPUT_GPU_DATA_OFFSET,
                                context.gpu_data,
                            );
                            write_i32(
                                &mut smart_input_bytes,
                                abi::SMART_INPUT_WHAT_GPU_OFFSET,
                                context.framework,
                            );
                            write_u32(
                                &mut smart_input_bytes,
                                abi::SMART_INPUT_DEVICE_INDEX_OFFSET,
                                context.device_index,
                            );
                        }
                        self.engine.write(smart_input, &smart_input_bytes)?;
                        if census_enabled {
                            self.engine.begin_block_census()?;
                        }
                        let (selector_name, selector) = if context.is_some() {
                            ("SMART_RENDER_GPU", CMD_SMART_RENDER_GPU)
                        } else {
                            ("SMART_RENDER", CMD_SMART_RENDER)
                        };
                        if context.is_some() {
                            match self.engine.prepare_gpu_render_transport() {
                                Ok(()) => transport_prepared = true,
                                Err(source) => {
                                    transport_prepare_error = Some(source.to_string());
                                    return Err(ClassicError::Guest(source));
                                }
                            }
                        }
                        let render_result = self.call_with_optional_trace(
                            selector_name,
                            [selector, self.input, self.output, params, 0, smart_extra],
                            trace_enabled,
                        );
                        let transport_result = if context.is_some() {
                            match self.engine.finish_gpu_render_transport() {
                                Ok(()) => {
                                    transport_finished = true;
                                    Ok(())
                                }
                                Err(source) => {
                                    transport_cleanup_error = Some(source.to_string());
                                    Err(ClassicError::Guest(source))
                                }
                            }
                        } else {
                            Ok(())
                        };
                        let census_result = if census_enabled {
                            self.engine
                                .finish_block_census(u64::from(width) * u64::from(height))
                                .map(Some)
                                .map_err(ClassicError::from)
                        } else {
                            Ok(None)
                        };
                        let (return_value, trace) = render_result?;
                        traces.extend(trace);
                        if return_value as i32 != 0 {
                            return Ok(LifecycleReply::Render {
                                error: return_value as i32,
                                output: census_result.ok().flatten(),
                            });
                        }
                        let census = census_result?;
                        transport_result?;
                        Ok(LifecycleReply::Render {
                            error: return_value as i32,
                            output: census,
                        })
                    }
                    LifecycleCall::Setdown { context } => {
                        let (
                            _setup_input,
                            _setup_output,
                            _setup_extra,
                            setdown_input,
                            setdown_extra,
                        ) = gpu_buffers.expect("GPU lifecycle allocated setdown buffers");
                        let mut input_bytes = vec![0u8; abi::PF_GPU_DEVICE_SETDOWN_INPUT_SIZE];
                        write_u64(
                            &mut input_bytes,
                            abi::GPU_SETDOWN_INPUT_GPU_DATA_OFFSET,
                            context.gpu_data,
                        );
                        write_i32(
                            &mut input_bytes,
                            abi::GPU_SETDOWN_INPUT_WHAT_GPU_OFFSET,
                            context.framework,
                        );
                        write_u32(
                            &mut input_bytes,
                            abi::GPU_SETDOWN_INPUT_DEVICE_INDEX_OFFSET,
                            context.device_index,
                        );
                        self.engine.write(setdown_input, &input_bytes)?;
                        let mut extra_bytes = vec![0u8; abi::PF_GPU_DEVICE_SETDOWN_EXTRA_SIZE];
                        write_u64(
                            &mut extra_bytes,
                            abi::GPU_SETDOWN_EXTRA_INPUT_OFFSET,
                            setdown_input,
                        );
                        self.engine.write(setdown_extra, &extra_bytes)?;
                        let (return_value, trace) = self.call_with_optional_trace(
                            "GPU_DEVICE_SETDOWN",
                            [
                                CMD_GPU_DEVICE_SETDOWN,
                                self.input,
                                self.output,
                                params,
                                0,
                                setdown_extra,
                            ],
                            trace_enabled,
                        )?;
                        traces.extend(trace);
                        Ok(LifecycleReply::Setdown {
                            error: return_value as i32,
                        })
                    }
                }
            },
        );
        let mut runtime_end_error = None;
        if runtime_started && let Err(source) = self.engine.end_gpu_runtime() {
            runtime_end_error = Some(source.to_string());
            apply_runtime_end_failure(
                &mut execution.result,
                backend
                    .runtime_backend()
                    .expect("started GPU request has a runtime backend"),
                source,
            );
        }
        if backend.is_gpu() {
            execution.diagnostic.runtime_backend = if runtime_started {
                backend.runtime_backend()
            } else {
                None
            };
            execution.diagnostic.runtime_started = runtime_started;
            execution.diagnostic.transport_prepared = transport_prepared;
            execution.diagnostic.transport_finished = transport_finished;
            execution.diagnostic.runtime_ended = !self.engine.gpu_runtime_active();
            execution.diagnostic.transport_prepare_error = transport_prepare_error;
            execution.diagnostic.transport_cleanup_error = transport_cleanup_error;
            execution.diagnostic.runtime_end_error = runtime_end_error;
            let device_suite = self.engine.gpu_suite_evidence();
            let opencl = self.engine.opencl_bridge_evidence();
            let wgpu = self.engine.wgpu_runtime_evidence();
            execution
                .diagnostic
                .finish_runtime_evidence(device_suite, opencl, wgpu);
        }
        self.last_gpu_diagnostic = execution.diagnostic;
        match execution.result {
            Ok(census) => Ok((0, census, traces)),
            Err(LifecycleFailure::Dispatch { selector, source }) => Err(match source {
                ClassicError::Guest(source) => ClassicError::SelectorGuest { selector, source },
                source => source,
            }),
            Err(LifecycleFailure::Selector {
                selector: "SMART_RENDER",
                error,
            }) => {
                let callbacks = self.engine.smart_callback_counts();
                let result_rect = [
                    read_i32(&pre_output_bytes, abi::PRE_OUTPUT_RESULT_RECT_OFFSET),
                    read_i32(&pre_output_bytes, abi::PRE_OUTPUT_RESULT_RECT_OFFSET + 4),
                    read_i32(&pre_output_bytes, abi::PRE_OUTPUT_RESULT_RECT_OFFSET + 8),
                    read_i32(&pre_output_bytes, abi::PRE_OUTPUT_RESULT_RECT_OFFSET + 12),
                ];
                Err(ClassicError::Input(format!(
                    "SMART_RENDER returned {error}; callbacks pre/checkout/output={callbacks:?}, result_rect={result_rect:?}, suite requests={:?}, handle allocations={:?}",
                    self.engine.suite_requests(),
                    self.engine.handle_allocations()
                )))
            }
            Err(LifecycleFailure::Selector { selector, error }) => {
                Err(ClassicError::Selector { selector, error })
            }
            Err(LifecycleFailure::GpuRenderNotPossible) => Err(ClassicError::Input(
                "SMART_PRE_RENDER did not set GPU_RENDER_POSSIBLE for the explicit OpenCL request"
                    .into(),
            )),
            Err(LifecycleFailure::InternalContract(message)) => Err(ClassicError::Input(format!(
                "internal GPU lifecycle contract failure: {message}"
            ))),
        }
    }

    fn read_guest_u64(&mut self, address: u64) -> Result<u64, GuestError> {
        let mut bytes = [0u8; 8];
        self.engine.read(address, &mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn trace_state_snapshot(&mut self) -> Result<BTreeMap<String, String>, GuestError> {
        let mut input = vec![0u8; abi::PF_IN_DATA_SIZE];
        let mut output = vec![0u8; abi::PF_OUT_DATA_SIZE];
        self.engine.read(self.input, &mut input)?;
        self.engine.read(self.output, &mut output)?;
        Ok(BTreeMap::from([
            (
                "input.global_data".into(),
                format!("{:#x}", read_u64(&input, abi::IN_GLOBAL_DATA_OFFSET)),
            ),
            (
                "input.sequence_data".into(),
                format!("{:#x}", read_u64(&input, abi::IN_SEQUENCE_DATA_OFFSET)),
            ),
            (
                "input.frame_data".into(),
                format!("{:#x}", read_u64(&input, abi::IN_FRAME_DATA_OFFSET)),
            ),
            (
                "output.global_data".into(),
                format!("{:#x}", read_u64(&output, abi::OUT_GLOBAL_DATA_OFFSET)),
            ),
            (
                "output.sequence_data".into(),
                format!("{:#x}", read_u64(&output, abi::OUT_SEQUENCE_DATA_OFFSET)),
            ),
            (
                "output.frame_data".into(),
                format!("{:#x}", read_u64(&output, abi::OUT_FRAME_DATA_OFFSET)),
            ),
            (
                "output.num_params".into(),
                read_i32(&output, abi::OUT_NUM_PARAMS_OFFSET).to_string(),
            ),
            (
                "output.out_flags".into(),
                format!("{:#x}", read_u32(&output, abi::OUT_OUT_FLAGS_OFFSET)),
            ),
            (
                "output.out_flags2".into(),
                format!("{:#x}", read_u32(&output, abi::OUT_OUT_FLAGS2_OFFSET)),
            ),
            (
                "host.captured_parameters".into(),
                self.engine.parameters().len().to_string(),
            ),
        ]))
    }

    fn read_output_pointer(&mut self, offset: usize) -> Result<u64, GuestError> {
        let mut output = vec![0u8; abi::PF_OUT_DATA_SIZE];
        self.engine.read(self.output, &mut output)?;
        Ok(read_u64(&output, offset))
    }

    fn write_input_pointer(&mut self, offset: usize, value: u64) -> Result<(), GuestError> {
        let mut input = vec![0u8; abi::PF_IN_DATA_SIZE];
        self.engine.read(self.input, &mut input)?;
        write_u64(&mut input, offset, value);
        self.engine.write(self.input, &input)
    }
}

fn write_i16(bytes: &mut [u8], offset: usize, value: i16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_i32(bytes: &mut [u8], offset: usize, value: i32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn write_rect(bytes: &mut [u8], offset: usize, width: u32, height: u32) {
    write_rect_values(bytes, offset, [0, 0, width as i32, height as i32]);
}

fn write_rect_values(bytes: &mut [u8], offset: usize, rect: [i32; 4]) {
    write_i32(bytes, offset, rect[0]);
    write_i32(bytes, offset + 4, rect[1]);
    write_i32(bytes, offset + 8, rect[2]);
    write_i32(bytes, offset + 12, rect[3]);
}

fn read_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_i16(bytes: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_f64(bytes: &[u8], offset: usize) -> f64 {
    f64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn diff_trace_state(
    before: BTreeMap<String, String>,
    after: BTreeMap<String, String>,
) -> Vec<TraceStateValue> {
    before
        .into_iter()
        .filter_map(|(name, before)| {
            let after = after.get(&name)?.clone();
            (before != after).then_some(TraceStateValue {
                name,
                before,
                after,
            })
        })
        .collect()
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn materialize_default(
    definition: &mut [u8],
    param_type: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let union = abi::PARAM_U_OFFSET;
    match param_type {
        PARAM_SLIDER | PARAM_FIXED_SLIDER => {
            definition.copy_within(
                union + abi::SLIDER_DEFAULT_OFFSET
                    ..union + abi::SLIDER_DEFAULT_OFFSET + abi::SLIDER_DEFAULT_SIZE,
                union,
            );
        }
        PARAM_ANGLE => {
            definition.copy_within(
                union + ANGLE_DEFAULT_OFFSET..union + ANGLE_DEFAULT_OFFSET + 4,
                union,
            );
        }
        PARAM_CHECKBOX => {
            let value = definition[union + abi::CHECKBOX_DEFAULT_OFFSET] as u32;
            definition[union..union + 4].copy_from_slice(&value.to_le_bytes());
        }
        PARAM_COLOR => {
            definition.copy_within(
                union + abi::PF_PIXEL_SIZE..union + abi::PF_PIXEL_SIZE * 2,
                union,
            );
        }
        PARAM_ARBITRARY_DATA => {
            // PF_ADD_ARBITRARY2 initializes the value to null independently of
            // the optional default handle. The render-time value is a private
            // COPY of the default that ClassicHost obtains through
            // PF_Cmd_ARBITRARY_CALLBACK; it is never the default handle itself,
            // and a null default simply leaves the value uninitialized.
            definition
                [union + ARBITRARY_VALUE_HANDLE_OFFSET..union + ARBITRARY_VALUE_HANDLE_OFFSET + 8]
                .copy_from_slice(&0u64.to_le_bytes());
        }
        PARAM_POPUP => {
            let value = i16::from_le_bytes(
                definition[union + abi::POPUP_DEFAULT_OFFSET
                    ..union + abi::POPUP_DEFAULT_OFFSET + abi::POPUP_DEFAULT_SIZE]
                    .try_into()
                    .expect("generated popup default is two bytes"),
            ) as i32;
            definition[union..union + 4].copy_from_slice(&value.to_le_bytes());
        }
        PARAM_FLOAT_SLIDER => {
            let value = f32::from_le_bytes(
                definition[union + abi::FLOAT_SLIDER_DEFAULT_OFFSET
                    ..union + abi::FLOAT_SLIDER_DEFAULT_OFFSET + abi::FLOAT_SLIDER_DEFAULT_SIZE]
                    .try_into()
                    .expect("generated float slider default is four bytes"),
            ) as f64;
            definition[union..union + 8].copy_from_slice(&value.to_le_bytes());
        }
        PARAM_POINT => {
            let coordinate = |default_offset, extent: u32| {
                let percent = read_i32(definition, union + default_offset) as f64 / 65536.0;
                (percent * f64::from(extent) * 65536.0 / 100.0).round() as i32
            };
            let x = coordinate(POINT_DEFAULT_X_OFFSET, width);
            let y = coordinate(POINT_DEFAULT_Y_OFFSET, height);
            definition[union..union + 4].copy_from_slice(&x.to_le_bytes());
            definition[union + 4..union + 8].copy_from_slice(&y.to_le_bytes());
        }
        PARAM_POINT3D => {
            for (component, extent) in [width, height, height].into_iter().enumerate() {
                let offset = union + 24 + component * 8;
                let percent = f64::from_le_bytes(
                    definition[offset..offset + 8]
                        .try_into()
                        .expect("point3d default is eight bytes"),
                );
                let pixels = if width > 0 && height > 0 {
                    percent / 100.0 * f64::from(extent)
                } else {
                    percent
                };
                definition[union + component * 8..union + component * 8 + 8]
                    .copy_from_slice(&pixels.to_le_bytes());
            }
        }
        _ => {}
    }
    Ok(())
}

fn materialize_layer_world(definition: &mut [u8], param_type: i32, input_world: &[u8]) {
    if param_type == PARAM_LAYER
        && definition.len() >= abi::PARAM_U_OFFSET + abi::PF_LAYER_DEF_SIZE
        && read_i32(definition, abi::PARAM_U_OFFSET + LAYER_DEFAULT_OFFSET) == -1
        && input_world.len() == abi::PF_LAYER_DEF_SIZE
    {
        definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_LAYER_DEF_SIZE]
            .copy_from_slice(input_world);
    }
}

#[cfg(test)]
fn apply_parameter_value(
    definition: &mut [u8],
    param_type: i32,
    requested: &ParameterValue,
) -> Result<(), ClassicError> {
    apply_parameter_value_for_layer(definition, param_type, requested, 0, 0)
}

fn apply_parameter_value_for_layer(
    definition: &mut [u8],
    param_type: i32,
    requested: &ParameterValue,
    width: u32,
    height: u32,
) -> Result<(), ClassicError> {
    if param_type == PARAM_COLOR {
        let color = requested.color.ok_or_else(|| {
            ClassicError::Input("color parameter requires four ARGB8 components".into())
        })?;
        if requested.value.is_some()
            || requested.point.is_some()
            || requested.angle.is_some()
            || requested.point3d.is_some()
        {
            return Err(ClassicError::Input(
                "color parameter accepts only ARGB8 components".into(),
            ));
        }
        definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_PIXEL_SIZE]
            .copy_from_slice(&color);
        return Ok(());
    }
    if param_type == PARAM_POINT {
        let [x, y] = requested.point.ok_or_else(|| {
            ClassicError::Input("point parameter requires two comma-separated values".into())
        })?;
        if requested.value.is_some()
            || requested.color.is_some()
            || !x.is_finite()
            || !y.is_finite()
        {
            return Err(ClassicError::Input(
                "point parameter components must be finite and typed".into(),
            ));
        }
        for (offset, value, extent) in [(0, x, width), (4, y, height)] {
            let pixels = if width > 0 && height > 0 {
                value / 100.0 * f64::from(extent)
            } else {
                value
            };
            let fixed = pixels.clamp(-32768.0, 32767.0) * 65536.0;
            definition[abi::PARAM_U_OFFSET + offset..abi::PARAM_U_OFFSET + offset + 4]
                .copy_from_slice(&(fixed.round() as i32).to_le_bytes());
        }
        return Ok(());
    }
    if param_type == PARAM_ANGLE {
        let angle = requested.angle.ok_or_else(|| {
            ClassicError::Input("angle parameter requires one typed component".into())
        })?;
        if requested.value.is_some()
            || requested.color.is_some()
            || requested.point.is_some()
            || requested.point3d.is_some()
            || !angle.is_finite()
        {
            return Err(ClassicError::Input(
                "angle parameter component must be finite and typed".into(),
            ));
        }
        let fixed = angle * 65536.0;
        if fixed < i32::MIN as f64 || fixed > i32::MAX as f64 {
            return Err(ClassicError::Input(format!(
                "angle component is outside 16.16 range: {angle}"
            )));
        }
        definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4]
            .copy_from_slice(&(fixed.round() as i32).to_le_bytes());
        return Ok(());
    }
    if param_type == PARAM_POINT3D {
        let values = requested.point3d.ok_or_else(|| {
            ClassicError::Input("point3d parameter requires three typed components".into())
        })?;
        if requested.value.is_some()
            || requested.color.is_some()
            || requested.point.is_some()
            || requested.angle.is_some()
            || values.iter().any(|value| !value.is_finite())
        {
            return Err(ClassicError::Input(
                "point3d parameter components must be finite and typed".into(),
            ));
        }
        for (index, (value, extent)) in values.into_iter().zip([width, height, height]).enumerate()
        {
            let pixels = if width > 0 && height > 0 {
                value / 100.0 * f64::from(extent)
            } else {
                value
            };
            definition[abi::PARAM_U_OFFSET + index * 8..abi::PARAM_U_OFFSET + index * 8 + 8]
                .copy_from_slice(&pixels.to_le_bytes());
        }
        return Ok(());
    }
    if requested.color.is_some() {
        return Err(ClassicError::Input(format!(
            "parameter type {param_type} does not accept an ARGB8 color value"
        )));
    }
    if requested.point.is_some() {
        return Err(ClassicError::Input(format!(
            "parameter type {param_type} does not accept a point value"
        )));
    }
    if requested.angle.is_some() || requested.point3d.is_some() {
        return Err(ClassicError::Input(format!(
            "parameter type {param_type} does not accept component values"
        )));
    }
    let value = requested.value.ok_or_else(|| {
        ClassicError::Input(format!(
            "parameter type {param_type} requires a numeric value"
        ))
    })?;
    if !value.is_finite() {
        return Err(ClassicError::Input(
            "parameter values must be finite".into(),
        ));
    }
    let union = abi::PARAM_U_OFFSET;
    match param_type {
        PARAM_SLIDER | PARAM_POPUP => {
            definition[union..union + 4].copy_from_slice(&(value.round() as i32).to_le_bytes());
        }
        PARAM_FIXED_SLIDER => {
            let fixed = value * 65536.0;
            if fixed < i32::MIN as f64 || fixed > i32::MAX as f64 {
                return Err(ClassicError::Input(format!(
                    "fixed-slider value is outside 16.16 range: {value}"
                )));
            }
            definition[union..union + 4].copy_from_slice(&(fixed.round() as i32).to_le_bytes());
        }
        PARAM_CHECKBOX => {
            let checked = i32::from(value != 0.0);
            definition[union..union + 4].copy_from_slice(&checked.to_le_bytes());
        }
        PARAM_FLOAT_SLIDER => {
            definition[union..union + 8].copy_from_slice(&value.to_le_bytes());
        }
        _ => {
            return Err(ClassicError::Input(format!(
                "parameter type {param_type} is not yet editable"
            )));
        }
    }
    Ok(())
}

fn color_descriptor(definition: &[u8], param_type: i32) -> (Option<[u8; 4]>, Option<[u8; 4]>) {
    if param_type != PARAM_COLOR {
        return (None, None);
    }
    let union = abi::PARAM_U_OFFSET;
    let default = definition[union + abi::PF_PIXEL_SIZE..union + abi::PF_PIXEL_SIZE * 2]
        .try_into()
        .expect("PF color default is four bytes");
    // AE materializes `dephault` into `value` before the first render. Report
    // that effective initial state rather than the add-param scratch bytes.
    (Some(default), Some(default))
}

fn component_descriptor(
    definition: &[u8],
    param_type: i32,
) -> (Option<Vec<f64>>, Option<Vec<f64>>) {
    let union = abi::PARAM_U_OFFSET;
    let defaults = match param_type {
        PARAM_ANGLE => vec![read_i32(definition, union + ANGLE_DEFAULT_OFFSET) as f64 / 65536.0],
        PARAM_POINT => vec![
            read_i32(definition, union + POINT_DEFAULT_X_OFFSET) as f64 / 65536.0,
            read_i32(definition, union + POINT_DEFAULT_Y_OFFSET) as f64 / 65536.0,
        ],
        PARAM_POINT3D => (0..3)
            .map(|component| read_f64(definition, union + 24 + component * 8))
            .collect(),
        _ => return (None, None),
    };
    (Some(defaults.clone()), Some(defaults))
}

fn numeric_descriptor(
    definition: &[u8],
    param_type: i32,
) -> (
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<i16>,
) {
    let union = abi::PARAM_U_OFFSET;
    match param_type {
        PARAM_SLIDER => (
            Some(read_i32(definition, union + abi::SLIDER_DEFAULT_OFFSET) as f64),
            Some(read_i32(definition, union + abi::SLIDER_VALID_MIN_OFFSET) as f64),
            Some(read_i32(definition, union + abi::SLIDER_VALID_MAX_OFFSET) as f64),
            Some(read_i32(definition, union + abi::SLIDER_SLIDER_MIN_OFFSET) as f64),
            Some(read_i32(definition, union + abi::SLIDER_SLIDER_MAX_OFFSET) as f64),
            None,
        ),
        PARAM_FIXED_SLIDER => {
            let fixed = |offset| read_i32(definition, union + offset) as f64 / 65536.0;
            (
                Some(fixed(abi::SLIDER_DEFAULT_OFFSET)),
                Some(fixed(abi::SLIDER_VALID_MIN_OFFSET)),
                Some(fixed(abi::SLIDER_VALID_MAX_OFFSET)),
                Some(fixed(abi::SLIDER_SLIDER_MIN_OFFSET)),
                Some(fixed(abi::SLIDER_SLIDER_MAX_OFFSET)),
                Some(read_i16(definition, union + 88)),
            )
        }
        PARAM_CHECKBOX => (
            Some(if definition[union + abi::CHECKBOX_DEFAULT_OFFSET] != 0 {
                1.0
            } else {
                0.0
            }),
            Some(0.0),
            Some(1.0),
            Some(0.0),
            Some(1.0),
            None,
        ),
        PARAM_POPUP => {
            let choices = read_i16(definition, union + abi::POPUP_NUM_CHOICES_OFFSET) as f64;
            (
                Some(read_i16(definition, union + abi::POPUP_DEFAULT_OFFSET) as f64),
                Some(1.0),
                Some(choices),
                Some(1.0),
                Some(choices),
                None,
            )
        }
        PARAM_FLOAT_SLIDER => (
            Some(read_f32(definition, union + abi::FLOAT_SLIDER_DEFAULT_OFFSET) as f64),
            Some(read_f32(definition, union + abi::FLOAT_SLIDER_VALID_MIN_OFFSET) as f64),
            Some(read_f32(definition, union + abi::FLOAT_SLIDER_VALID_MAX_OFFSET) as f64),
            Some(read_f32(definition, union + abi::FLOAT_SLIDER_SLIDER_MIN_OFFSET) as f64),
            Some(read_f32(definition, union + abi::FLOAT_SLIDER_SLIDER_MAX_OFFSET) as f64),
            Some(read_i16(
                definition,
                union + abi::FLOAT_SLIDER_PRECISION_OFFSET,
            )),
        ),
        _ => (None, None, None, None, None, None),
    }
}

fn populate_frame_context(
    input: &mut [u8],
    width: u32,
    height: u32,
    current_time: i32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
) {
    write_i32(input, abi::IN_WIDTH_OFFSET, width as i32);
    write_i32(input, abi::IN_HEIGHT_OFFSET, height as i32);
    write_i32(input, abi::IN_CURRENT_TIME_OFFSET, current_time);
    write_i32(input, abi::IN_TIME_STEP_OFFSET, time_step);
    write_i32(input, abi::IN_TOTAL_TIME_OFFSET, total_time);
    write_i32(input, abi::IN_LOCAL_TIME_STEP_OFFSET, time_step);
    write_u32(input, abi::IN_TIME_SCALE_OFFSET, time_scale);
    write_rect(input, abi::IN_EXTENT_HINT_OFFSET, width, height);
}

fn cleanup_error_code(result: Result<i32, ClassicError>) -> i32 {
    result.unwrap_or(CLEANUP_GUEST_ERROR)
}

fn selector_failure(selector: &'static str, error: i32) -> Option<ClassicError> {
    (error != 0).then_some(ClassicError::Selector { selector, error })
}

fn apply_runtime_end_failure<T>(
    result: &mut Result<T, LifecycleFailure<ClassicError>>,
    backend_kind: GpuRuntimeBackendKind,
    source: GuestError,
) {
    if result.is_ok() {
        *result = Err(LifecycleFailure::Dispatch {
            selector: match backend_kind {
                GpuRuntimeBackendKind::AppleOpenCl => "OPENCL_RUNTIME_END",
                GpuRuntimeBackendKind::WgpuMetal => "WGPU_METAL_RUNTIME_END",
            },
            source: ClassicError::Guest(source),
        });
    }
}

fn selector_guest_error(selector: &'static str, source: GuestError) -> ClassicError {
    match source {
        GuestError::SelectorAbort { error, .. } => ClassicError::Selector { selector, error },
        source => ClassicError::SelectorGuest { selector, source },
    }
}

fn bounded_failure_text(value: &str) -> String {
    bounded_text(value, MAX_FAILURE_TEXT_BYTES)
}

fn bounded_crash_snapshot(snapshot: serde_json::Value) -> serde_json::Value {
    if serde_json::to_vec(&snapshot)
        .is_ok_and(|bytes| bytes.len() <= MAX_FAILURE_CRASH_SNAPSHOT_BYTES)
    {
        return snapshot;
    }

    let scalar = |name: &str| {
        snapshot
            .get(name)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };
    serde_json::json!({
        "truncated": true,
        "reason": bounded_failure_text(snapshot.get("reason").and_then(|value| value.as_str()).unwrap_or("")),
        "registers": scalar("registers"),
        "instruction_address": scalar("instruction_address"),
        "instruction_rva": scalar("instruction_rva"),
        "instruction_bytes": bounded_text(snapshot.get("instruction_bytes").and_then(|value| value.as_str()).unwrap_or(""), 256),
        "runtime_target": scalar("runtime_target"),
        "live_handle_count": scalar("live_handle_count"),
        "next_pf_handle_data": scalar("next_pf_handle_data"),
        "pf_handle_data_end": scalar("pf_handle_data_end"),
        "handle_allocation_count": snapshot.get("handle_allocations").and_then(|value| value.as_array()).map_or(0, Vec::len),
        "handle_allocation_failure_count": snapshot.get("handle_allocation_failures").and_then(|value| value.as_array()).map_or(0, Vec::len),
    })
}

fn decode_return_message(bytes: &[u8]) -> Option<String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    (end != 0).then(|| String::from_utf8_lossy(&bytes[..end]).into_owned())
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_text_is_utf8_safe_and_bounded() {
        let text = "界".repeat(MAX_FAILURE_TEXT_BYTES);
        let bounded = bounded_failure_text(&text);
        assert!(bounded.len() <= MAX_FAILURE_TEXT_BYTES);
        assert!(bounded.is_char_boundary(bounded.len()));
        assert_eq!(bounded, "界".repeat(MAX_FAILURE_TEXT_BYTES / 3));
    }

    #[test]
    fn resident_crash_snapshot_drops_unbounded_history_but_keeps_crash_location() {
        let snapshot = serde_json::json!({
            "reason": "unmapped write",
            "registers": {"rip": 0x18000539bu64, "rcx": 0x100001000u64},
            "instruction_address": 0x18000539bu64,
            "instruction_rva": 0x539bu64,
            "instruction_bytes": "48".repeat(32),
            "runtime_target": null,
            "handle_allocations": vec![4096u64; 16_384],
            "handle_allocation_failures": vec!["failure"; 1024],
            "live_handle_count": 1,
            "next_pf_handle_data": 0x100002000u64,
            "pf_handle_data_end": 0x110000000u64,
        });

        let bounded = bounded_crash_snapshot(snapshot);
        assert_eq!(bounded["truncated"], true);
        assert_eq!(bounded["instruction_rva"], 0x539bu64);
        assert_eq!(bounded["handle_allocation_count"], 16_384);
        assert!(serde_json::to_vec(&bounded).unwrap().len() <= MAX_FAILURE_CRASH_SNAPSHOT_BYTES);
    }

    #[test]
    fn return_message_is_bounded_nul_terminated_and_utf8_safe() {
        assert_eq!(decode_return_message(&[0; 256]), None);
        assert_eq!(
            decode_return_message(b"could not be initialized\0ignored"),
            Some("could not be initialized".into())
        );
        assert_eq!(
            decode_return_message(&[b'a', 0xff, b'b', 0]),
            Some("a\u{fffd}b".into())
        );
        let full = vec![b'x'; abi::OUT_RETURN_MSG_SIZE];
        assert_eq!(
            decode_return_message(&full).unwrap(),
            "x".repeat(abi::OUT_RETURN_MSG_SIZE)
        );
    }

    #[test]
    fn cleanup_selector_failure_preserves_its_selector() {
        let failure = selector_failure("FRAME_SETDOWN", -40).unwrap();
        assert!(matches!(
            failure,
            ClassicError::Selector {
                selector: "FRAME_SETDOWN",
                error: -40
            }
        ));
        assert!(selector_failure("FRAME_SETDOWN", 0).is_none());
    }

    #[test]
    fn runtime_end_failure_is_promoted_after_a_successful_render_body() {
        let mut result: Result<&str, LifecycleFailure<ClassicError>> = Ok("rendered");
        apply_runtime_end_failure(
            &mut result,
            GpuRuntimeBackendKind::AppleOpenCl,
            GuestError::Callback("unbalanced OpenCL objects".into()),
        );
        assert!(matches!(
            result,
            Err(LifecycleFailure::Dispatch {
                selector: "OPENCL_RUNTIME_END",
                source: ClassicError::Guest(GuestError::Callback(ref message)),
            }) if message == "unbalanced OpenCL objects"
        ));
    }

    #[test]
    fn runtime_end_failure_does_not_replace_the_primary_selector_error() {
        let mut result: Result<(), LifecycleFailure<ClassicError>> =
            Err(LifecycleFailure::Selector {
                selector: "SMART_RENDER_GPU",
                error: 25,
            });
        apply_runtime_end_failure(
            &mut result,
            GpuRuntimeBackendKind::AppleOpenCl,
            GuestError::Callback("unbalanced OpenCL objects".into()),
        );
        assert!(matches!(
            result,
            Err(LifecycleFailure::Selector {
                selector: "SMART_RENDER_GPU",
                error: 25,
            })
        ));
    }

    #[test]
    fn unsupported_suite_throw_maps_to_the_active_selector_error() {
        let failure = selector_guest_error(
            "SMART_RENDER",
            GuestError::SelectorAbort {
                error: 13,
                suite_name: "FLT Blur Suite".into(),
                suite_version: 1,
                acquire_error: -1,
            },
        );
        assert!(matches!(
            failure,
            ClassicError::Selector {
                selector: "SMART_RENDER",
                error: 13
            }
        ));
    }

    #[test]
    fn materializes_supported_parameter_defaults() {
        let mut slider = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        slider[abi::PARAM_U_OFFSET + abi::SLIDER_DEFAULT_OFFSET
            ..abi::PARAM_U_OFFSET + abi::SLIDER_DEFAULT_OFFSET + 4]
            .copy_from_slice(&123i32.to_le_bytes());
        materialize_default(&mut slider, PARAM_FIXED_SLIDER, 32, 20).unwrap();
        assert_eq!(
            &slider[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4],
            &123i32.to_le_bytes()
        );

        let mut float_slider = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        float_slider[abi::PARAM_U_OFFSET + abi::FLOAT_SLIDER_DEFAULT_OFFSET
            ..abi::PARAM_U_OFFSET + abi::FLOAT_SLIDER_DEFAULT_OFFSET + 4]
            .copy_from_slice(&5.0f32.to_le_bytes());
        materialize_default(&mut float_slider, PARAM_FLOAT_SLIDER, 32, 20).unwrap();
        assert_eq!(
            &float_slider[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 8],
            &5.0f64.to_le_bytes()
        );

        let mut color = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        color[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 8]
            .copy_from_slice(&[1, 2, 3, 4, 255, 64, 128, 192]);
        materialize_default(&mut color, PARAM_COLOR, 32, 20).unwrap();
        assert_eq!(
            &color[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4],
            &[255, 64, 128, 192]
        );

        let mut point = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        point[abi::PARAM_U_OFFSET + POINT_DEFAULT_X_OFFSET
            ..abi::PARAM_U_OFFSET + POINT_DEFAULT_X_OFFSET + 4]
            .copy_from_slice(&(50 * 65536i32).to_le_bytes());
        point[abi::PARAM_U_OFFSET + POINT_DEFAULT_Y_OFFSET
            ..abi::PARAM_U_OFFSET + POINT_DEFAULT_Y_OFFSET + 4]
            .copy_from_slice(&(25 * 65536i32).to_le_bytes());
        materialize_default(&mut point, PARAM_POINT, 32, 20).unwrap();
        assert_eq!(
            read_i32(&point, abi::PARAM_U_OFFSET),
            16 * 65536,
            "point x percentage default must become a source coordinate"
        );
        assert_eq!(
            read_i32(&point, abi::PARAM_U_OFFSET + 4),
            5 * 65536,
            "point y percentage default must become a source coordinate"
        );

        let mut point3d = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        for (index, value) in [50.0f64, 25.0, 75.0].into_iter().enumerate() {
            point3d[abi::PARAM_U_OFFSET + 24 + index * 8..abi::PARAM_U_OFFSET + 32 + index * 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        materialize_default(&mut point3d, PARAM_POINT3D, 32, 20).unwrap();
        for (index, expected) in [16.0f64, 5.0, 15.0].into_iter().enumerate() {
            assert_eq!(
                f64::from_le_bytes(
                    point3d[abi::PARAM_U_OFFSET + index * 8..abi::PARAM_U_OFFSET + index * 8 + 8]
                        .try_into()
                        .unwrap()
                ),
                expected
            );
        }
    }

    #[test]
    fn setup_component_descriptors_preserve_angle_point_and_point3d_defaults() {
        let mut angle = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        angle[abi::PARAM_U_OFFSET + ANGLE_DEFAULT_OFFSET
            ..abi::PARAM_U_OFFSET + ANGLE_DEFAULT_OFFSET + 4]
            .copy_from_slice(&(45i32 * 65536).to_le_bytes());
        assert_eq!(
            component_descriptor(&angle, PARAM_ANGLE),
            (Some(vec![45.0]), Some(vec![45.0]))
        );

        let mut point = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        point[abi::PARAM_U_OFFSET + POINT_DEFAULT_X_OFFSET
            ..abi::PARAM_U_OFFSET + POINT_DEFAULT_X_OFFSET + 4]
            .copy_from_slice(&(25i32 * 65536).to_le_bytes());
        point[abi::PARAM_U_OFFSET + POINT_DEFAULT_Y_OFFSET
            ..abi::PARAM_U_OFFSET + POINT_DEFAULT_Y_OFFSET + 4]
            .copy_from_slice(&(-10i32 * 65536).to_le_bytes());
        assert_eq!(
            component_descriptor(&point, PARAM_POINT),
            (Some(vec![25.0, -10.0]), Some(vec![25.0, -10.0]))
        );

        let mut point3d = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        for (index, value) in [10.5f64, -20.25, 30.75].into_iter().enumerate() {
            let offset = abi::PARAM_U_OFFSET + 24 + index * 8;
            point3d[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            component_descriptor(&point3d, PARAM_POINT3D),
            (
                Some(vec![10.5, -20.25, 30.75]),
                Some(vec![10.5, -20.25, 30.75])
            )
        );
    }

    #[test]
    fn materialize_default_never_aliases_arbitrary_default_into_value() {
        let union = abi::PARAM_U_OFFSET;
        let mut arbitrary = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        arbitrary[union..union + 4].copy_from_slice(&[7, 0, 0, 0]);
        arbitrary
            [union + ARBITRARY_DEFAULT_HANDLE_OFFSET..union + ARBITRARY_DEFAULT_HANDLE_OFFSET + 8]
            .copy_from_slice(&0x1234_5678_9abc_def0u64.to_le_bytes());
        arbitrary[union + ARBITRARY_VALUE_HANDLE_OFFSET..union + ARBITRARY_VALUE_HANDLE_OFFSET + 8]
            .copy_from_slice(&0x5555_5555_5555_5555u64.to_le_bytes());
        arbitrary[union + ARBITRARY_REFCON_OFFSET..union + ARBITRARY_REFCON_OFFSET + 8]
            .copy_from_slice(&0x0fed_cba9_8765_4321u64.to_le_bytes());

        materialize_default(&mut arbitrary, PARAM_ARBITRARY_DATA, 32, 20).unwrap();

        // The value is left null for the host's COPY; the default and refcon
        // metadata stay intact for that callback.
        assert_eq!(
            read_u64(&arbitrary, union + ARBITRARY_VALUE_HANDLE_OFFSET),
            0
        );
        assert_eq!(
            read_u64(&arbitrary, union + ARBITRARY_DEFAULT_HANDLE_OFFSET),
            0x1234_5678_9abc_def0
        );
        assert_eq!(&arbitrary[union..union + 4], &[7, 0, 0, 0]);
        assert_eq!(
            read_u64(&arbitrary, union + ARBITRARY_REFCON_OFFSET),
            0x0fed_cba9_8765_4321
        );
    }

    #[test]
    fn arbitrary_null_default_leaves_value_uninitialized() {
        let union = abi::PARAM_U_OFFSET;
        let mut arbitrary = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        arbitrary[union + ARBITRARY_VALUE_HANDLE_OFFSET..union + ARBITRARY_VALUE_HANDLE_OFFSET + 8]
            .copy_from_slice(&0x5555_5555_5555_5555u64.to_le_bytes());
        materialize_default(&mut arbitrary, PARAM_ARBITRARY_DATA, 32, 20).unwrap();
        assert_eq!(
            read_u64(&arbitrary, union + ARBITRARY_VALUE_HANDLE_OFFSET),
            0
        );
    }

    #[test]
    fn combine_failures_keeps_every_error_visible_in_order() {
        assert!(combine_failures(Vec::new()).is_ok());
        let single = combine_failures(vec![ClassicError::Selector {
            selector: "GLOBAL_SETDOWN",
            error: 5,
        }])
        .unwrap_err();
        assert!(matches!(
            single,
            ClassicError::Selector {
                selector: "GLOBAL_SETDOWN",
                error: 5
            }
        ));
        let combined = combine_failures(vec![
            ClassicError::Arbitrary {
                operation: "DISPOSE",
                id: 3,
                message: "ARBITRARY_CALLBACK returned 9".into(),
            },
            ClassicError::Selector {
                selector: "GLOBAL_SETDOWN",
                error: 5,
            },
            ClassicError::Input("third".into()),
        ])
        .unwrap_err();
        assert_eq!(combined.selector_error_code(), None);
        assert_eq!(
            combined.to_string(),
            "arbitrary parameter id=3 DISPOSE failed: ARBITRARY_CALLBACK returned 9; additionally: selector GLOBAL_SETDOWN returned 5; additionally: invalid frame input: third"
        );
        let selector_primary = ClassicError::Compound {
            primary: Box::new(ClassicError::Selector {
                selector: "GLOBAL_SETDOWN",
                error: 5,
            }),
            secondary: Box::new(ClassicError::Input("cleanup".into())),
        };
        assert_eq!(selector_primary.selector_error_code(), Some(5));
    }

    #[test]
    fn applies_generic_editable_parameter_values() {
        let union = abi::PARAM_U_OFFSET;

        let mut fixed = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        let scalar = |value| ParameterValue {
            slot: None,
            name: "fixture".into(),
            value: Some(value),
            color: None,
            point: None,
            angle: None,
            point3d: None,
        };
        apply_parameter_value(&mut fixed, PARAM_FIXED_SLIDER, &scalar(12.5)).unwrap();
        assert_eq!(read_i32(&fixed, union), 12 * 65536 + 32768);

        let mut checkbox = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        apply_parameter_value(&mut checkbox, PARAM_CHECKBOX, &scalar(1.0)).unwrap();
        assert_eq!(read_i32(&checkbox, union), 1);

        let mut popup = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        apply_parameter_value(&mut popup, PARAM_POPUP, &scalar(2.0)).unwrap();
        assert_eq!(read_i32(&popup, union), 2);

        let mut float_slider = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        apply_parameter_value(&mut float_slider, PARAM_FLOAT_SLIDER, &scalar(42.25)).unwrap();
        assert_eq!(
            f64::from_le_bytes(
                float_slider[union..union + 8]
                    .try_into()
                    .expect("parameter value is eight bytes")
            ),
            42.25
        );

        let mut point = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        apply_parameter_value(
            &mut point,
            PARAM_POINT,
            &ParameterValue {
                name: "point".into(),
                slot: None,
                value: None,
                color: None,
                point: Some([42.5, -7.25]),
                angle: None,
                point3d: None,
            },
        )
        .unwrap();
        assert_eq!(
            read_i32(&point, abi::PARAM_U_OFFSET),
            (42.5 * 65536.0) as i32
        );
        assert_eq!(
            read_i32(&point, abi::PARAM_U_OFFSET + 4),
            (-7.25 * 65536.0) as i32
        );

        let mut angle = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        apply_parameter_value_for_layer(
            &mut angle,
            PARAM_ANGLE,
            &ParameterValue {
                name: "angle".into(),
                slot: None,
                value: None,
                color: None,
                point: None,
                angle: Some(12.5),
                point3d: None,
            },
            32,
            20,
        )
        .unwrap();
        assert_eq!(read_i32(&angle, union), (12.5 * 65536.0) as i32);

        let mut point3d = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        apply_parameter_value_for_layer(
            &mut point3d,
            PARAM_POINT3D,
            &ParameterValue {
                name: "point3d".into(),
                slot: None,
                value: None,
                color: None,
                point: None,
                angle: None,
                point3d: Some([50.0, 25.0, 75.0]),
            },
            32,
            20,
        )
        .unwrap();
        for (index, expected) in [16.0f64, 5.0, 15.0].into_iter().enumerate() {
            assert_eq!(
                f64::from_le_bytes(
                    point3d[union + index * 8..union + index * 8 + 8]
                        .try_into()
                        .unwrap()
                ),
                expected
            );
        }
    }

    #[test]
    fn fixture_frame_context_preserves_non_unit_time_step() {
        let mut input = vec![0u8; abi::PF_IN_DATA_SIZE];
        populate_frame_context(&mut input, 640, 360, 42, 7, 210, 30);
        assert_eq!(read_i32(&input, abi::IN_CURRENT_TIME_OFFSET), 42);
        assert_eq!(read_i32(&input, abi::IN_TIME_STEP_OFFSET), 7);
        assert_eq!(read_i32(&input, abi::IN_TOTAL_TIME_OFFSET), 210);
        assert_eq!(read_i32(&input, abi::IN_LOCAL_TIME_STEP_OFFSET), 7);
        assert_eq!(read_u32(&input, abi::IN_TIME_SCALE_OFFSET), 30);
    }

    #[test]
    fn reports_and_applies_argb8_color_values() {
        let union = abi::PARAM_U_OFFSET;
        let mut definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        definition[union..union + 8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            color_descriptor(&definition, PARAM_COLOR),
            (Some([5, 6, 7, 8]), Some([5, 6, 7, 8]))
        );

        apply_parameter_value(
            &mut definition,
            PARAM_COLOR,
            &ParameterValue {
                slot: Some(2),
                name: "Key Color".into(),
                value: None,
                color: Some([255, 64, 128, 192]),
                point: None,
                angle: None,
                point3d: None,
            },
        )
        .unwrap();
        assert_eq!(
            &definition[union..union + abi::PF_PIXEL_SIZE],
            &[255, 64, 128, 192]
        );
        let applied = serde_json::to_value(AppliedParameter {
            slot: 2,
            name: "Color".into(),
            value: None,
            color: Some([255, 64, 128, 192]),
            point: None,
            angle: None,
            point3d: None,
        })
        .unwrap();
        assert_eq!(applied["slot"], 2);
        assert_eq!(applied["color"], serde_json::json!([255, 64, 128, 192]));
        assert!(applied.get("point").is_none());
    }

    #[test]
    fn rejects_color_payload_for_numeric_parameter() {
        let mut definition = [0u8; abi::PF_PARAM_DEF_SIZE];
        let error = apply_parameter_value(
            &mut definition,
            PARAM_SLIDER,
            &ParameterValue {
                slot: None,
                name: "Amount".into(),
                value: None,
                color: Some([255, 1, 2, 3]),
                point: None,
                angle: None,
                point3d: None,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("does not accept an ARGB8 color"));
    }

    #[test]
    fn cleanup_guest_failures_are_reported_as_unclean() {
        assert_eq!(cleanup_error_code(Ok(0)), 0);
        assert_eq!(cleanup_error_code(Ok(17)), 17);
        assert_eq!(
            cleanup_error_code(Err(ClassicError::Input("fixture failure".into()))),
            CLEANUP_GUEST_ERROR
        );
    }

    #[test]
    fn public_iterate16_callback_uses_the_observed_utility_slot() {
        assert_eq!(abi::UTILS_ITERATE16_OFFSET, 0x1f8);
        assert_eq!(abi::UTILS_ITERATE16_SIZE, std::mem::size_of::<u64>());
    }

    #[test]
    fn declared_layer_parameters_inherit_the_active_input_world() {
        let input_world = (0..abi::PF_LAYER_DEF_SIZE as u8).collect::<Vec<_>>();
        let mut definition = vec![0xa5; abi::PF_PARAM_DEF_SIZE];
        definition[abi::PARAM_U_OFFSET + LAYER_DEFAULT_OFFSET
            ..abi::PARAM_U_OFFSET + LAYER_DEFAULT_OFFSET + 4]
            .copy_from_slice(&(-1i32).to_le_bytes());
        materialize_layer_world(&mut definition, PARAM_LAYER, &input_world);
        assert_eq!(
            &definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_LAYER_DEF_SIZE],
            input_world
        );

        let mut scalar = vec![0xa5; abi::PF_PARAM_DEF_SIZE];
        materialize_layer_world(&mut scalar, PARAM_SLIDER, &input_world);
        assert_eq!(
            &scalar[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_LAYER_DEF_SIZE],
            vec![0xa5; abi::PF_LAYER_DEF_SIZE]
        );

        let mut explicit_none = vec![0xa5; abi::PF_PARAM_DEF_SIZE];
        explicit_none[abi::PARAM_U_OFFSET + LAYER_DEFAULT_OFFSET
            ..abi::PARAM_U_OFFSET + LAYER_DEFAULT_OFFSET + 4]
            .copy_from_slice(&0i32.to_le_bytes());
        let explicit_before = explicit_none.clone();
        materialize_layer_world(&mut explicit_none, PARAM_LAYER, &input_world);
        assert_eq!(
            &explicit_none[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_LAYER_DEF_SIZE],
            &explicit_before[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_LAYER_DEF_SIZE]
        );
    }
}
