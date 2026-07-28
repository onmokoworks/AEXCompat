use aex_abi::x86_64_windows as abi;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

use crate::backend::{
    ExecutionTrace, GuestCensus, GuestEngine, GuestError, TraceStateValue, TraceWatchSpec,
    UnsupportedSuiteCall,
};
use crate::pe::PeImage;
use crate::pixel::FramePixelFormat;

const CMD_GLOBAL_SETUP: u64 = 1;
const CMD_GLOBAL_SETDOWN: u64 = 3;
const CMD_PARAMS_SETUP: u64 = 4;
const CMD_SEQUENCE_SETUP: u64 = 5;
const CMD_SEQUENCE_SETDOWN: u64 = 8;
const CMD_FRAME_SETUP: u64 = 10;
const CMD_RENDER: u64 = 11;
const CMD_FRAME_SETDOWN: u64 = 12;
const CMD_SMART_PRE_RENDER: u64 = 23;
const CMD_SMART_RENDER: u64 = 24;
const PARAM_LAYER: i32 = 0;
const PARAM_SLIDER: i32 = 1;
const PARAM_FIXED_SLIDER: i32 = 2;
const PARAM_ANGLE: i32 = 3;
const PARAM_CHECKBOX: i32 = 4;
pub(crate) const PARAM_COLOR: i32 = 5;
pub(crate) const PARAM_POINT: i32 = 6;
const PARAM_POPUP: i32 = 7;
const PARAM_FLOAT_SLIDER: i32 = 10;
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
}

#[derive(Clone, Debug, Serialize)]
pub struct ParameterReport {
    pub slot: usize,
    pub index: i32,
    pub param_type: i32,
    pub name: String,
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
}

#[derive(Clone, Debug)]
pub struct ParameterValue {
    pub slot: Option<usize>,
    pub name: String,
    pub value: Option<f64>,
    pub color: Option<[u8; 4]>,
    pub point: Option<[f64; 2]>,
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
    pub parameters: Vec<ParameterReport>,
    pub suite_requests: Vec<String>,
    pub unsupported_suite_calls: Vec<UnsupportedSuiteCall>,
    pub dropped_unsupported_suite_calls: u64,
}

#[derive(Debug, Serialize)]
pub struct FailureReport {
    pub schema_version: u32,
    pub execution_backend: &'static str,
    pub error: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub census: Option<GuestCensus>,
    pub argb8: Vec<u8>,
    #[serde(skip)]
    pub raw_pixels: Vec<u8>,
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
    resident_frames: u64,
    resident_frame_setdown_error: i32,
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
}

impl ClassicHost {
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
        for offset in abi::INPUT_CALLBACK_OFFSETS {
            write_u64(&mut input_bytes, offset, engine.poison_callback_address());
        }
        write_u64(
            &mut input_bytes,
            abi::INTER_ADD_PARAM_OFFSET,
            engine.add_param_callback_address(),
        );
        write_u64(
            &mut input_bytes,
            abi::INTER_CHECKOUT_PARAM_OFFSET,
            engine.checkout_param_callback_address(),
        );
        write_u64(
            &mut input_bytes,
            abi::INTER_CHECKIN_PARAM_OFFSET,
            engine.checkin_param_callback_address(),
        );
        write_u64(
            &mut input_bytes,
            abi::INTER_ABORT_OFFSET,
            engine.noop_callback_address(),
        );
        write_u64(
            &mut input_bytes,
            abi::INTER_PROGRESS_OFFSET,
            engine.noop_callback_address(),
        );
        write_u64(
            &mut input_bytes,
            abi::INTER_RESERVED_0_OFFSET,
            engine.extended_alloc_callback_address(),
        );
        write_u64(
            &mut input_bytes,
            abi::INTER_RESERVED_1_OFFSET,
            engine.extended_lookup_callback_address(),
        );
        write_u64(
            &mut input_bytes,
            abi::INTER_RESERVED_2_OFFSET,
            engine.extended_free_callback_address(),
        );
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
        let mut utility_bytes = vec![0u8; abi::PF_UTIL_CALLBACKS_SIZE];
        write_u64(
            &mut utility_bytes,
            abi::UTILS_SUBPIXEL_SAMPLE_OFFSET,
            engine.subpixel_sample8_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_AREA_SAMPLE_OFFSET,
            engine.area_sample8_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_TRANSFER_RECT_OFFSET,
            engine.transfer_rect8_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_ANSI_STRCPY_OFFSET,
            engine.ansi_strcpy_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_ANSI_SPRINTF_OFFSET,
            engine.ansi_sprintf_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_COPY_OFFSET,
            engine.copy_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_BLEND_OFFSET,
            engine.blend_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_FILL_OFFSET,
            engine.fill8_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_NEW_WORLD_OFFSET,
            engine.new_world8_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_DISPOSE_WORLD_OFFSET,
            engine.dispose_world_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_GET_CALLBACK_ADDR_OFFSET,
            engine.get_callback_addr_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_ITERATE_OFFSET,
            engine.iterate8_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_ITERATE_ORIGIN_OFFSET,
            engine.iterate8_origin_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_ITERATE16_OFFSET,
            engine.iterate16_callback_address(),
        );
        for (offset, callback) in [
            (
                abi::UTILS_ANSI_CEIL_OFFSET,
                engine.ansi_ceil_callback_address(),
            ),
            (
                abi::UTILS_ANSI_COS_OFFSET,
                engine.ansi_cos_callback_address(),
            ),
            (
                abi::UTILS_ANSI_FABS_OFFSET,
                engine.ansi_fabs_callback_address(),
            ),
            (
                abi::UTILS_ANSI_HYPOT_OFFSET,
                engine.ansi_hypot_callback_address(),
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
                abi::UTILS_ANSI_ASIN_OFFSET,
                engine.ansi_asin_callback_address(),
            ),
            (
                abi::UTILS_ANSI_ACOS_OFFSET,
                engine.ansi_acos_callback_address(),
            ),
        ] {
            write_u64(&mut utility_bytes, offset, callback);
        }
        for (offset, callback) in [
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
            resident_frames: 0,
            resident_frame_setdown_error: 0,
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
                ParameterReport {
                    slot: offset + 1,
                    index: param.index,
                    param_type: param.param_type,
                    name: param.name.clone(),
                    default_value,
                    valid_min,
                    valid_max,
                    slider_min,
                    slider_max,
                    precision,
                    current_color,
                    default_color,
                }
            })
            .collect();
        let report = SetupReport {
            schema_version: 1,
            execution_backend: self.engine.backend_name(),
            global_setup_error,
            params_setup_error,
            advertised_num_params,
            out_flags,
            out_flags2,
            parameters,
            suite_requests: self.engine.suite_requests().to_vec(),
            unsupported_suite_calls: self.engine.unsupported_suite_calls().to_vec(),
            dropped_unsupported_suite_calls: self.engine.dropped_unsupported_suite_calls(),
        };
        self.setup_report = Some(report.clone());
        Ok(report)
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
        if let Err(error) = self.write_frame_context(width, height, 0, time_scale) {
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
            time_scale,
            format,
            input_pixels,
            parameter_values,
            true,
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
            time_scale,
            format,
            input_pixels,
            &[],
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_resident_pixels_mode(
        &mut self,
        width: u32,
        height: u32,
        current_time: i32,
        time_scale: u32,
        format: FramePixelFormat,
        input_pixels: &[u8],
        parameter_values: &[ParameterValue],
        count_frame: bool,
    ) -> Result<RenderReport, ClassicError> {
        if !self.sequence_active {
            return Err(ClassicError::Input(
                "resident session has not been opened".into(),
            ));
        }
        self.write_frame_context(width, height, current_time, time_scale)?;
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
            )?
            .0;
        if count_frame {
            self.resident_frames += 1;
        }
        Ok(report)
    }

    pub fn close_resident_session(&mut self) -> ResidentCloseReport {
        let sequence_setdown_error = cleanup_error_code(self.end_sequence(false));
        let global_setdown_error = cleanup_error_code(self.end_global());
        ResidentCloseReport {
            schema_version: 1,
            execution_backend: self.engine.backend_name(),
            frames_rendered: self.resident_frames,
            frame_setdown_error: self.resident_frame_setdown_error,
            sequence_setdown_error,
            global_setdown_error,
            suite_requests: self.engine.suite_requests().to_vec(),
            unsupported_suite_calls: self.engine.unsupported_suite_calls().to_vec(),
            dropped_unsupported_suite_calls: self.engine.dropped_unsupported_suite_calls(),
            session_clean: self.resident_frame_setdown_error == 0
                && sequence_setdown_error == 0
                && global_setdown_error == 0,
        }
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
        let (category, selector, error_code, message, crash_reason) = match error {
            ClassicError::Guest(source) => (
                source.diagnostic_category(),
                None,
                None,
                source.diagnostic_message(),
                source.crash_reason().map(str::to_owned),
            ),
            ClassicError::SelectorGuest { selector, source } => (
                source.diagnostic_category(),
                Some(*selector),
                None,
                source.diagnostic_message(),
                source.crash_reason().map(str::to_owned),
            ),
            ClassicError::Selector { selector, error } => (
                "selector",
                Some(*selector),
                Some(*error),
                format!("selector {selector} returned {error}"),
                None,
            ),
            ClassicError::Input(message) => ("input", None, None, message.clone(), None),
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
        self.engine
            .begin_execution_trace(selector_name, self.entry)?;
        let before = self.trace_state_snapshot()?;
        let selector_label = match selector {
            CMD_GLOBAL_SETUP => "GLOBAL_SETUP",
            CMD_PARAMS_SETUP => "PARAMS_SETUP",
            _ => unreachable!("validated setup trace selector"),
        };
        let return_value = self
            .invoke(selector)
            .map_err(|source| selector_guest_error(selector_label, source))?;
        let after = self.trace_state_snapshot()?;
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
        let rgba8 = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let pixels = format
            .promote_rgba8(&rgba8)
            .map_err(|error| ClassicError::Input(error.to_string()))?;
        self.render_pixels(2, 2, format, &pixels, &[])
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
        self.render_pixels_with_request(
            width,
            height,
            format,
            input_pixels,
            parameter_values,
            [0, 0, width as i32, height as i32],
            false,
            false,
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
        self.render_pixels_with_request(
            width,
            height,
            format,
            input_pixels,
            parameter_values,
            [0, 0, width as i32, height as i32],
            false,
            true,
        )
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
        self.engine.configure_trace_watches(watches);
        self.trace_output_pixel = output_pixel;
        self.render_pixels_trace(width, height, format, input_pixels, parameter_values)
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
    ) -> Result<(RenderReport, Vec<ExecutionTrace>), ClassicError> {
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
        let smart_render = setup.out_flags2 & (1 << 10) != 0;
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
        let resources =
            self.ensure_frame_resources(width, height, format, pixel_bytes, captured_params.len())?;
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
        let mut input_definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        input_definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_LAYER_DEF_SIZE]
            .copy_from_slice(&input_world);
        self.engine.write(input_param, &input_definition)?;
        self.engine.write(guest_input_pixels, input_pixels)?;
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
            materialize_default(&mut definition, captured.param_type, width, height);
            if smart_render {
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
                apply_parameter_value(&mut definition, captured.param_type, requested)?;
                applied_requests.insert(request_index);
                applied_values.push(AppliedParameter {
                    slot: index + 1,
                    name: captured.name.clone(),
                    value: requested.value,
                    color: requested.color,
                    point: requested.point,
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
                Ok((return_value as i32, None, trace.into_iter().collect()))
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
                    "smart-cpu-region"
                } else if smart_render {
                    "smart-cpu"
                } else {
                    "classic"
                },
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
                census,
                argb8,
                raw_pixels,
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
    ) -> Result<FrameResources, ClassicError> {
        if let Some(resources) = &self.frame_resources {
            if resources.width != width
                || resources.height != height
                || resources.format != format
                || resources.pixel_bytes != pixel_bytes
                || resources.parameter_definitions.len() != parameter_count
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
        time_scale: u32,
    ) -> Result<(), ClassicError> {
        let mut input = vec![0u8; abi::PF_IN_DATA_SIZE];
        self.engine.read(self.input, &mut input)?;
        write_i32(&mut input, abi::IN_WIDTH_OFFSET, width as i32);
        write_i32(&mut input, abi::IN_HEIGHT_OFFSET, height as i32);
        write_i32(&mut input, abi::IN_CURRENT_TIME_OFFSET, current_time);
        write_i32(&mut input, abi::IN_TIME_STEP_OFFSET, 1);
        write_i32(&mut input, abi::IN_LOCAL_TIME_STEP_OFFSET, 1);
        write_u32(&mut input, abi::IN_TIME_SCALE_OFFSET, time_scale);
        write_rect(&mut input, abi::IN_EXTENT_HINT_OFFSET, width, height);
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
        let result = self.invoke(CMD_GLOBAL_SETDOWN);
        self.global_active = false;
        let clear = self.write_input_pointer(abi::IN_GLOBAL_DATA_OFFSET, 0);
        match result {
            Ok(result) => {
                clear?;
                Ok(result as i32)
            }
            Err(error) => {
                let _ = clear;
                Err(selector_guest_error("GLOBAL_SETDOWN", error))
            }
        }
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
            abi::SMART_INPUT_BITDEPTH_OFFSET,
            format.bit_depth(),
        );
        self.engine.write(pre_input, &pre_input_bytes)?;
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
        let (pre_result, trace) = self.call_with_optional_trace(
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
        let pre_error = pre_result as i32;
        if pre_error != 0 {
            return Err(ClassicError::Selector {
                selector: "SMART_PRE_RENDER",
                error: pre_error,
            });
        }

        let mut pre_output_bytes = vec![0u8; abi::PF_PRE_RENDER_OUTPUT_SIZE];
        self.engine.read(pre_output, &mut pre_output_bytes)?;
        let pre_render_data = self.read_guest_u64(pre_output + 40)?;
        let smart_input = self.engine.allocate(abi::PF_SMART_RENDER_INPUT_SIZE, 8)?;
        let smart_callbacks = self
            .engine
            .allocate(abi::PF_SMART_RENDER_CALLBACKS_SIZE, 8)?;
        let smart_extra = self.engine.allocate(abi::PF_SMART_RENDER_EXTRA_SIZE, 8)?;
        let mut smart_input_bytes = vec![0u8; abi::PF_SMART_RENDER_INPUT_SIZE];
        write_i16(
            &mut smart_input_bytes,
            abi::SMART_INPUT_BITDEPTH_OFFSET,
            format.bit_depth(),
        );
        write_u64(&mut smart_input_bytes, 48, pre_render_data);
        self.engine.write(smart_input, &smart_input_bytes)?;
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
        if census_enabled {
            self.engine.begin_block_census()?;
        }
        let render_result = self.call_with_optional_trace(
            "SMART_RENDER",
            [
                CMD_SMART_RENDER,
                self.input,
                self.output,
                params,
                0,
                smart_extra,
            ],
            trace_enabled,
        );
        let census = if census_enabled {
            Some(
                self.engine
                    .finish_block_census(u64::from(width) * u64::from(height))?,
            )
        } else {
            None
        };
        let (return_value, trace) = render_result?;
        traces.extend(trace);
        let render_error = return_value as i32;
        if render_error != 0 {
            let callbacks = self.engine.smart_callback_counts();
            let result_rect = [
                read_i32(&pre_output_bytes, 0),
                read_i32(&pre_output_bytes, 4),
                read_i32(&pre_output_bytes, 8),
                read_i32(&pre_output_bytes, 12),
            ];
            return Err(ClassicError::Input(format!(
                "SMART_RENDER returned {render_error}; callbacks pre/checkout/output={callbacks:?}, result_rect={result_rect:?}, suite requests={:?}, handle allocations={:?}",
                self.engine.suite_requests(),
                self.engine.handle_allocations()
            )));
        }
        Ok((render_error, census, traces))
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

fn materialize_default(definition: &mut [u8], param_type: i32, width: u32, height: u32) {
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
        _ => {}
    }
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

fn apply_parameter_value(
    definition: &mut [u8],
    param_type: i32,
    requested: &ParameterValue,
) -> Result<(), ClassicError> {
    if param_type == PARAM_COLOR {
        let color = requested.color.ok_or_else(|| {
            ClassicError::Input("color parameter requires four ARGB8 components".into())
        })?;
        if requested.value.is_some() {
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
        for (offset, value) in [(0, x), (4, y)] {
            let fixed = value * 65536.0;
            if fixed < i32::MIN as f64 || fixed > i32::MAX as f64 {
                return Err(ClassicError::Input(format!(
                    "point component is outside 16.16 range: {value}"
                )));
            }
            definition[abi::PARAM_U_OFFSET + offset..abi::PARAM_U_OFFSET + offset + 4]
                .copy_from_slice(&(fixed.round() as i32).to_le_bytes());
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

fn cleanup_error_code(result: Result<i32, ClassicError>) -> i32 {
    result.unwrap_or(CLEANUP_GUEST_ERROR)
}

fn selector_failure(selector: &'static str, error: i32) -> Option<ClassicError> {
    (error != 0).then_some(ClassicError::Selector { selector, error })
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
        materialize_default(&mut slider, PARAM_FIXED_SLIDER, 32, 20);
        assert_eq!(
            &slider[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4],
            &123i32.to_le_bytes()
        );

        let mut float_slider = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        float_slider[abi::PARAM_U_OFFSET + abi::FLOAT_SLIDER_DEFAULT_OFFSET
            ..abi::PARAM_U_OFFSET + abi::FLOAT_SLIDER_DEFAULT_OFFSET + 4]
            .copy_from_slice(&5.0f32.to_le_bytes());
        materialize_default(&mut float_slider, PARAM_FLOAT_SLIDER, 32, 20);
        assert_eq!(
            &float_slider[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 8],
            &5.0f64.to_le_bytes()
        );

        let mut color = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        color[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 8]
            .copy_from_slice(&[1, 2, 3, 4, 255, 64, 128, 192]);
        materialize_default(&mut color, PARAM_COLOR, 32, 20);
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
        materialize_default(&mut point, PARAM_POINT, 32, 20);
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
