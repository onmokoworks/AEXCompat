use aex_abi::x86_64_windows as abi;
use serde::Serialize;
use thiserror::Error;

use crate::pe::PeImage;
use crate::x64::{GuestEngine, GuestError};

const CMD_GLOBAL_SETUP: u64 = 1;
const CMD_PARAMS_SETUP: u64 = 4;
const CMD_RENDER: u64 = 11;
const PARAM_SLIDER: i32 = 1;
const PARAM_FIXED_SLIDER: i32 = 2;
const PARAM_ANGLE: i32 = 3;
const PARAM_CHECKBOX: i32 = 4;
const PARAM_POPUP: i32 = 7;
const PARAM_FLOAT_SLIDER: i32 = 10;
const ANGLE_DEFAULT_OFFSET: usize = 4;

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
}

#[derive(Debug, Serialize)]
pub struct ParameterReport {
    pub index: i32,
    pub param_type: i32,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct SetupReport {
    pub schema_version: u32,
    pub global_setup_error: i32,
    pub params_setup_error: i32,
    pub advertised_num_params: i32,
    pub parameters: Vec<ParameterReport>,
}

#[derive(Debug, Serialize)]
pub struct RenderReport {
    pub schema_version: u32,
    pub setup: SetupReport,
    pub render_error: i32,
    pub width: u32,
    pub height: u32,
    pub default_value: f64,
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
        engine.write(input, &input_bytes)?;
        engine.write(output, &vec![0u8; abi::PF_OUT_DATA_SIZE])?;
        let mut pica_bytes = [0u8; 64];
        write_u64(&mut pica_bytes, 0, engine.poison_callback_address());
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
        let mut output = vec![0u8; abi::PF_OUT_DATA_SIZE];
        self.engine.read(self.output, &mut output)?;
        let advertised_num_params = read_i32(&output, abi::OUT_NUM_PARAMS_OFFSET);
        let parameters = self
            .engine
            .parameters()
            .iter()
            .map(|param| ParameterReport {
                index: param.index,
                param_type: param.param_type,
                name: param.name.clone(),
            })
            .collect();
        Ok(SetupReport {
            schema_version: 1,
            global_setup_error,
            params_setup_error,
            advertised_num_params,
            parameters,
        })
    }

    pub fn render_default_2x2(&mut self) -> Result<RenderReport, ClassicError> {
        let setup = self.setup()?;
        let captured_params = self.engine.parameters().to_vec();
        let captured = captured_params
            .first()
            .expect("PARAMS_SETUP succeeded without capturing its required parameter")
            .bytes
            .clone();
        let default_offset = abi::PARAM_U_OFFSET + abi::FLOAT_SLIDER_DEFAULT_OFFSET;
        let default_value = f32::from_le_bytes(
            captured[default_offset..default_offset + abi::FLOAT_SLIDER_DEFAULT_SIZE]
                .try_into()
                .expect("generated float slider default is four bytes"),
        ) as f64;
        let input_param = self.engine.allocate(abi::PF_PARAM_DEF_SIZE, 8)?;
        let params = self.engine.allocate((captured_params.len() + 1) * 8, 8)?;
        let output_world = self.engine.allocate(abi::PF_LAYER_DEF_SIZE, 8)?;
        let input_pixels = self.engine.allocate(2 * 2 * abi::PF_PIXEL_SIZE, 8)?;
        let output_pixels = self.engine.allocate(2 * 2 * abi::PF_PIXEL_SIZE, 8)?;

        let mut input_world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        write_u64(&mut input_world, abi::LAYER_DATA_OFFSET, input_pixels);
        write_i32(&mut input_world, abi::LAYER_ROWBYTES_OFFSET, 8);
        write_i32(&mut input_world, abi::LAYER_WIDTH_OFFSET, 2);
        write_i32(&mut input_world, abi::LAYER_HEIGHT_OFFSET, 2);
        let mut input_definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        input_definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_LAYER_DEF_SIZE]
            .copy_from_slice(&input_world);
        self.engine.write(input_param, &input_definition)?;
        self.engine.write(
            input_pixels,
            &[
                255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255,
            ],
        )?;
        self.engine.write_u64(params, input_param)?;
        for (index, captured) in captured_params.into_iter().enumerate() {
            let mut definition = captured.bytes;
            materialize_default(&mut definition, captured.param_type);
            let parameter = self.engine.allocate(abi::PF_PARAM_DEF_SIZE, 8)?;
            self.engine.write(parameter, &definition)?;
            self.engine
                .write_u64(params + ((index + 1) * 8) as u64, parameter)?;
        }

        let mut world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        write_u64(&mut world, abi::LAYER_DATA_OFFSET, output_pixels);
        write_i32(&mut world, abi::LAYER_ROWBYTES_OFFSET, 8);
        write_i32(&mut world, abi::LAYER_WIDTH_OFFSET, 2);
        write_i32(&mut world, abi::LAYER_HEIGHT_OFFSET, 2);
        self.engine.write(output_world, &world)?;
        let render_error = self.engine.call_win64(
            self.entry,
            [CMD_RENDER, self.input, self.output, params, output_world, 0],
        )? as i32;
        if render_error != 0 {
            return Err(ClassicError::Selector {
                selector: "RENDER",
                error: render_error,
            });
        }
        let mut argb8 = vec![0u8; 2 * 2 * abi::PF_PIXEL_SIZE];
        self.engine.read(output_pixels, &mut argb8)?;
        Ok(RenderReport {
            schema_version: 1,
            setup,
            render_error,
            width: 2,
            height: 2,
            default_value,
            argb8,
        })
    }

    fn invoke(&mut self, selector: u64) -> Result<u64, GuestError> {
        self.engine
            .call_win64(self.entry, [selector, self.input, self.output, 0, 0, 0])
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

fn read_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
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
}
