use aex_abi::x86_64_windows as abi;
use serde::Serialize;
use thiserror::Error;

use crate::backend::{ExecutionTrace, GuestCensus, GuestEngine, GuestError};
use crate::pe::PeImage;

const CMD_GLOBAL_SETUP: u64 = 1;
const CMD_PARAMS_SETUP: u64 = 4;
const CMD_SEQUENCE_SETUP: u64 = 5;
const CMD_SEQUENCE_SETDOWN: u64 = 8;
const CMD_FRAME_SETUP: u64 = 10;
const CMD_RENDER: u64 = 11;
const CMD_FRAME_SETDOWN: u64 = 12;
const CMD_SMART_PRE_RENDER: u64 = 23;
const CMD_SMART_RENDER: u64 = 24;
const PARAM_SLIDER: i32 = 1;
const PARAM_FIXED_SLIDER: i32 = 2;
const PARAM_ANGLE: i32 = 3;
const PARAM_CHECKBOX: i32 = 4;
const PARAM_POPUP: i32 = 7;
const PARAM_FLOAT_SLIDER: i32 = 10;
const ANGLE_DEFAULT_OFFSET: usize = 4;
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
    #[error("invalid ARGB8 input: {0}")]
    Input(String),
}

#[derive(Debug, Serialize)]
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
}

#[derive(Clone, Debug)]
pub struct ParameterValue {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Serialize)]
pub struct AppliedParameter {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Serialize)]
pub struct SetupReport {
    pub schema_version: u32,
    pub execution_backend: &'static str,
    pub global_setup_error: i32,
    pub params_setup_error: i32,
    pub advertised_num_params: i32,
    pub out_flags: u32,
    pub out_flags2: u32,
    pub parameters: Vec<ParameterReport>,
}

#[derive(Debug, Serialize)]
pub struct RenderReport {
    pub schema_version: u32,
    pub setup: SetupReport,
    pub render_error: i32,
    pub render_mode: &'static str,
    pub width: u32,
    pub height: u32,
    pub parameter_values: Vec<AppliedParameter>,
    pub output_request: [i32; 4],
    pub input_requests: Vec<[i32; 4]>,
    pub suite_requests: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub census: Option<GuestCensus>,
    pub argb8: Vec<u8>,
}

pub struct ClassicHost {
    engine: GuestEngine<'static>,
    entry: u64,
    input: u64,
    output: u64,
}

impl ClassicHost {
    pub fn new(image: &PeImage) -> Result<Self, ClassicError> {
        let entry = image.entry_address();
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
            abi::UTILS_ANSI_STRCPY_OFFSET,
            engine.ansi_strcpy_callback_address(),
        );
        write_u64(
            &mut utility_bytes,
            abi::UTILS_COPY_OFFSET,
            engine.copy_callback_address(),
        );
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
        Ok(Self {
            engine,
            entry,
            input,
            output,
        })
    }

    pub fn setup(&mut self) -> Result<SetupReport, ClassicError> {
        let global_setup_error =
            self.invoke(CMD_GLOBAL_SETUP)
                .map_err(|source| ClassicError::SelectorGuest {
                    selector: "GLOBAL_SETUP",
                    source,
                })? as i32;
        if global_setup_error != 0 {
            return Err(ClassicError::Selector {
                selector: "GLOBAL_SETUP",
                error: global_setup_error,
            });
        }
        let mut output = vec![0u8; abi::PF_OUT_DATA_SIZE];
        self.engine.read(self.output, &mut output)?;
        let global_data = read_u64(&output, abi::OUT_GLOBAL_DATA_OFFSET);
        let mut input = vec![0u8; abi::PF_IN_DATA_SIZE];
        self.engine.read(self.input, &mut input)?;
        write_u64(&mut input, abi::IN_GLOBAL_DATA_OFFSET, global_data);
        self.engine.write(self.input, &input)?;
        let params_setup_error =
            self.invoke(CMD_PARAMS_SETUP)
                .map_err(|source| ClassicError::SelectorGuest {
                    selector: "PARAMS_SETUP",
                    source,
                })? as i32;
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
                }
            })
            .collect();
        Ok(SetupReport {
            schema_version: 1,
            execution_backend: self.engine.backend_name(),
            global_setup_error,
            params_setup_error,
            advertised_num_params,
            out_flags,
            out_flags2,
            parameters,
        })
    }

    pub fn trace_setup_selector(
        &mut self,
        selector_name: &str,
    ) -> Result<ExecutionTrace, ClassicError> {
        let selector = match selector_name {
            "GLOBAL_SETUP" => CMD_GLOBAL_SETUP,
            "PARAMS_SETUP" => {
                let error = self.invoke(CMD_GLOBAL_SETUP)? as i32;
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
        let return_value = self.invoke(selector)?;
        self.engine
            .finish_execution_trace(return_value)
            .map_err(ClassicError::from)
    }

    pub fn render_default_2x2(&mut self) -> Result<RenderReport, ClassicError> {
        self.render_argb8(
            2,
            2,
            &[
                255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255,
            ],
            &[],
        )
    }

    pub fn render_argb8(
        &mut self,
        width: u32,
        height: u32,
        input_argb8: &[u8],
        parameter_values: &[ParameterValue],
    ) -> Result<RenderReport, ClassicError> {
        self.render_argb8_with_request(
            width,
            height,
            input_argb8,
            parameter_values,
            [0, 0, width as i32, height as i32],
            false,
        )
    }

    pub fn render_argb8_census(
        &mut self,
        width: u32,
        height: u32,
        input_argb8: &[u8],
        parameter_values: &[ParameterValue],
    ) -> Result<RenderReport, ClassicError> {
        self.render_argb8_with_request(
            width,
            height,
            input_argb8,
            parameter_values,
            [0, 0, width as i32, height as i32],
            true,
        )
    }

    pub fn render_argb8_region(
        &mut self,
        width: u32,
        height: u32,
        input_argb8: &[u8],
        parameter_values: &[ParameterValue],
        output_request: [i32; 4],
    ) -> Result<RenderReport, ClassicError> {
        self.render_argb8_with_request(
            width,
            height,
            input_argb8,
            parameter_values,
            output_request,
            false,
        )
    }

    fn render_argb8_with_request(
        &mut self,
        width: u32,
        height: u32,
        input_argb8: &[u8],
        parameter_values: &[ParameterValue],
        output_request: [i32; 4],
        census_enabled: bool,
    ) -> Result<RenderReport, ClassicError> {
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
        let rowbytes = width
            .checked_mul(abi::PF_PIXEL_SIZE as u32)
            .ok_or_else(|| ClassicError::Input("rowbytes overflow".into()))?;
        let pixel_bytes = usize::try_from(rowbytes)
            .ok()
            .and_then(|row| row.checked_mul(height as usize))
            .ok_or_else(|| ClassicError::Input("pixel byte count overflow".into()))?;
        if input_argb8.len() != pixel_bytes {
            return Err(ClassicError::Input(format!(
                "expected {pixel_bytes} ARGB8 bytes for {width}x{height}, got {}",
                input_argb8.len()
            )));
        }
        let setup = self.setup()?;
        let smart_render = setup.out_flags2 & (1 << 10) != 0;
        if census_enabled && !smart_render {
            return Err(ClassicError::Input(
                "guest census currently requires Smart Render support".into(),
            ));
        }
        let captured_params = self.engine.parameters().to_vec();
        let mut applied_values = Vec::with_capacity(parameter_values.len());
        let input_param = self.engine.allocate(abi::PF_PARAM_DEF_SIZE, 8)?;
        let params = self.engine.allocate((captured_params.len() + 1) * 8, 8)?;
        let output_world = self.engine.allocate(abi::PF_LAYER_DEF_SIZE, 8)?;
        let input_pixels = self.engine.allocate(pixel_bytes, 64)?;
        let output_pixels = self.engine.allocate(pixel_bytes, 64)?;

        let mut input_world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        write_u64(&mut input_world, abi::LAYER_DATA_OFFSET, input_pixels);
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
        self.engine.write(input_pixels, input_argb8)?;
        self.engine.write_u64(params, input_param)?;
        let mut parameter_definitions = Vec::with_capacity(captured_params.len());
        for (index, captured) in captured_params.into_iter().enumerate() {
            let mut definition = captured.bytes;
            materialize_default(&mut definition, captured.param_type);
            if let Some(requested) = parameter_values
                .iter()
                .find(|requested| requested.name == captured.name)
            {
                apply_parameter_value(&mut definition, captured.param_type, requested.value)?;
                applied_values.push(AppliedParameter {
                    name: captured.name.clone(),
                    value: requested.value,
                });
            }
            let parameter = self.engine.allocate(abi::PF_PARAM_DEF_SIZE, 8)?;
            self.engine.write(parameter, &definition)?;
            parameter_definitions.push(parameter);
            self.engine
                .write_u64(params + ((index + 1) * 8) as u64, parameter)?;
        }
        self.engine
            .configure_parameter_definitions(parameter_definitions);
        if applied_values.len() != parameter_values.len() {
            let missing = parameter_values
                .iter()
                .find(|requested| {
                    !applied_values
                        .iter()
                        .any(|applied| applied.name == requested.name)
                })
                .expect("parameter count mismatch has a missing value");
            return Err(ClassicError::Input(format!(
                "AEX did not declare a supported parameter named {:?}",
                missing.name
            )));
        }

        let mut world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        write_u64(&mut world, abi::LAYER_DATA_OFFSET, output_pixels);
        write_i32(&mut world, abi::LAYER_ROWBYTES_OFFSET, rowbytes as i32);
        write_i32(&mut world, abi::LAYER_WIDTH_OFFSET, width as i32);
        write_i32(&mut world, abi::LAYER_HEIGHT_OFFSET, height as i32);
        write_rect(&mut world, abi::LAYER_EXTENT_HINT_OFFSET, width, height);
        self.engine.write(output_world, &world)?;
        let mut input_data = vec![0u8; abi::PF_IN_DATA_SIZE];
        self.engine.read(self.input, &mut input_data)?;
        write_i32(&mut input_data, abi::IN_WIDTH_OFFSET, width as i32);
        write_i32(&mut input_data, abi::IN_HEIGHT_OFFSET, height as i32);
        write_rect(&mut input_data, abi::IN_EXTENT_HINT_OFFSET, width, height);
        self.engine.write(self.input, &input_data)?;
        let sequence_setup_error =
            self.invoke(CMD_SEQUENCE_SETUP)
                .map_err(|source| ClassicError::SelectorGuest {
                    selector: "SEQUENCE_SETUP",
                    source,
                })? as i32;
        if sequence_setup_error != 0 {
            return Err(ClassicError::Selector {
                selector: "SEQUENCE_SETUP",
                error: sequence_setup_error,
            });
        }
        let sequence_data = self.read_output_pointer(abi::OUT_SEQUENCE_DATA_OFFSET)?;
        self.write_input_pointer(abi::IN_SEQUENCE_DATA_OFFSET, sequence_data)?;

        let frame_setup_error = self.engine.call_win64(
            self.entry,
            [
                CMD_FRAME_SETUP,
                self.input,
                self.output,
                params,
                output_world,
                0,
            ],
        )? as i32;
        if frame_setup_error != 0 {
            let _ = self.invoke(CMD_SEQUENCE_SETDOWN);
            return Err(ClassicError::Selector {
                selector: "FRAME_SETUP",
                error: frame_setup_error,
            });
        }
        let frame_data = self.read_output_pointer(abi::OUT_FRAME_DATA_OFFSET)?;
        self.write_input_pointer(abi::IN_FRAME_DATA_OFFSET, frame_data)?;

        let (mut render_error, census) = if smart_render {
            self.render_smart(
                params,
                input_param,
                output_world,
                width,
                height,
                rowbytes,
                output_request,
                census_enabled,
            )?
        } else {
            if output_request != [0, 0, width as i32, height as i32] {
                return Err(ClassicError::Input(
                    "region rendering requires Smart Render support".into(),
                ));
            }
            (
                self.engine.call_win64(
                    self.entry,
                    [CMD_RENDER, self.input, self.output, params, output_world, 0],
                )? as i32,
                None,
            )
        };
        let frame_setdown_error = self.engine.call_win64(
            self.entry,
            [
                CMD_FRAME_SETDOWN,
                self.input,
                self.output,
                params,
                output_world,
                0,
            ],
        )? as i32;
        if render_error == 0 {
            render_error = frame_setdown_error;
        }
        self.write_input_pointer(abi::IN_FRAME_DATA_OFFSET, 0)?;
        let sequence_setdown_error = self.invoke(CMD_SEQUENCE_SETDOWN)? as i32;
        if render_error == 0 {
            render_error = sequence_setdown_error;
        }
        self.write_input_pointer(abi::IN_SEQUENCE_DATA_OFFSET, 0)?;
        if render_error != 0 {
            return Err(ClassicError::Selector {
                selector: "RENDER",
                error: render_error,
            });
        }
        let mut argb8 = vec![0u8; pixel_bytes];
        self.engine.read(output_pixels, &mut argb8)?;
        Ok(RenderReport {
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
            parameter_values: applied_values,
            output_request,
            input_requests: self.engine.pre_checkout_requests().to_vec(),
            suite_requests: self.engine.suite_requests().to_vec(),
            census,
            argb8,
        })
    }

    fn invoke(&mut self, selector: u64) -> Result<u64, GuestError> {
        self.engine
            .call_win64(self.entry, [selector, self.input, self.output, 0, 0, 0])
    }

    fn render_smart(
        &mut self,
        params: u64,
        input_param: u64,
        output_world: u64,
        width: u32,
        height: u32,
        _rowbytes: u32,
        output_request: [i32; 4],
        census_enabled: bool,
    ) -> Result<(i32, Option<GuestCensus>), ClassicError> {
        let input_world = input_param + abi::PARAM_U_OFFSET as u64;
        self.engine
            .configure_smart_render(input_world, output_world, width, height);

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
        write_i16(&mut pre_input_bytes, abi::SMART_INPUT_BITDEPTH_OFFSET, 8);
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
        let pre_error = self.engine.call_win64(
            self.entry,
            [
                CMD_SMART_PRE_RENDER,
                self.input,
                self.output,
                params,
                0,
                pre_extra,
            ],
        )? as i32;
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
        write_i16(&mut smart_input_bytes, abi::SMART_INPUT_BITDEPTH_OFFSET, 8);
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
        let render_result = self.engine.call_win64(
            self.entry,
            [
                CMD_SMART_RENDER,
                self.input,
                self.output,
                params,
                0,
                smart_extra,
            ],
        );
        let census = if census_enabled {
            Some(
                self.engine
                    .finish_block_census(u64::from(width) * u64::from(height))?,
            )
        } else {
            None
        };
        let render_error = render_result? as i32;
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
        Ok((render_error, census))
    }

    fn read_guest_u64(&mut self, address: u64) -> Result<u64, GuestError> {
        let mut bytes = [0u8; 8];
        self.engine.read(address, &mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
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

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn materialize_default(definition: &mut [u8], param_type: i32) {
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
        _ => {}
    }
}

fn apply_parameter_value(
    definition: &mut [u8],
    param_type: i32,
    value: f64,
) -> Result<(), ClassicError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materializes_supported_parameter_defaults() {
        let mut slider = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        slider[abi::PARAM_U_OFFSET + abi::SLIDER_DEFAULT_OFFSET
            ..abi::PARAM_U_OFFSET + abi::SLIDER_DEFAULT_OFFSET + 4]
            .copy_from_slice(&123i32.to_le_bytes());
        materialize_default(&mut slider, PARAM_FIXED_SLIDER);
        assert_eq!(
            &slider[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4],
            &123i32.to_le_bytes()
        );

        let mut float_slider = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        float_slider[abi::PARAM_U_OFFSET + abi::FLOAT_SLIDER_DEFAULT_OFFSET
            ..abi::PARAM_U_OFFSET + abi::FLOAT_SLIDER_DEFAULT_OFFSET + 4]
            .copy_from_slice(&5.0f32.to_le_bytes());
        materialize_default(&mut float_slider, PARAM_FLOAT_SLIDER);
        assert_eq!(
            &float_slider[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 8],
            &5.0f64.to_le_bytes()
        );
    }

    #[test]
    fn applies_generic_editable_parameter_values() {
        let union = abi::PARAM_U_OFFSET;

        let mut fixed = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        apply_parameter_value(&mut fixed, PARAM_FIXED_SLIDER, 12.5).unwrap();
        assert_eq!(read_i32(&fixed, union), 12 * 65536 + 32768);

        let mut checkbox = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        apply_parameter_value(&mut checkbox, PARAM_CHECKBOX, 1.0).unwrap();
        assert_eq!(read_i32(&checkbox, union), 1);

        let mut popup = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        apply_parameter_value(&mut popup, PARAM_POPUP, 2.0).unwrap();
        assert_eq!(read_i32(&popup, union), 2);

        let mut float_slider = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        apply_parameter_value(&mut float_slider, PARAM_FLOAT_SLIDER, 42.25).unwrap();
        assert_eq!(
            f64::from_le_bytes(
                float_slider[union..union + 8]
                    .try_into()
                    .expect("parameter value is eight bytes")
            ),
            42.25
        );
    }
}
