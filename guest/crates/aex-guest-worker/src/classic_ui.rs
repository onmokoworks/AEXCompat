// Kept separate so the render route never implicitly dispatches UI selectors.
use aex_abi::x86_64_windows as abi;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::pixel::FramePixelFormat;

use super::{
    ARBITRARY_DEFAULT_HANDLE_OFFSET, ARBITRARY_REFCON_OFFSET, ARBITRARY_VALUE_HANDLE_OFFSET,
    ArbitraryValue, ClassicError, ClassicHost, PARAM_ANGLE, PARAM_ARBITRARY_DATA, PARAM_CHECKBOX,
    PARAM_COLOR, PARAM_FIXED_SLIDER, PARAM_FLOAT_SLIDER, PARAM_POINT, PARAM_POINT3D, PARAM_POPUP,
    PARAM_SLIDER, SetupReport, materialize_default, read_i16, read_i32, read_u32, read_u64,
    selector_guest_error, write_i32, write_u32, write_u64,
};

// Local ABI values not emitted by aex-abi: arbitrary selectors and UI event IDs.
const ARBITRARY_FLAT_SIZE_FUNC: i32 = 3;
const ARBITRARY_FLATTEN_FUNC: i32 = 4;
const EVENT_DO_CLICK: i32 = 2;
const EVENT_DRAG: i32 = 3;
const EVENT_DRAW: i32 = 4;
const EVENT_ADJUST_CURSOR: i32 = 9;
const EVENT_KEY_DOWN: i32 = 10;
// PF_ContextH is a pointer cell followed by PF_Context in the same allocation.
const CONTEXT_HANDLE_OFFSET: usize = 0;
const CONTEXT_VALUE_OFFSET: usize = abi::POINTER_SIZE;
const CONTEXT_ALLOCATION_SIZE: usize = CONTEXT_VALUE_OFFSET + abi::PF_CONTEXT_SIZE;
// Context magic and PF_DrawEventInfo fields are absent from the generated contract.
const CONTEXT_MAGIC: u32 = 0x05ea771e;
const DRAW_UPDATE_RECT_OFFSET: usize = 0;
const DRAW_DEPTH_OFFSET: usize = 16;
// PF_Point's two 32-bit coordinates are not emitted individually by aex-abi.
const SCREEN_POINT_Y_OFFSET: usize = 4;

impl ClassicHost {
    pub fn begin_ui_session(&mut self, time: i32, scale: u32) -> Result<SetupReport, ClassicError> {
        self.engine.enable_ui_session()?;
        self.engine.set_ui_time(time, scale);
        let setup = self.begin_resident_session(1, 1, scale)?;
        self.write_frame_context(1, 1, time, 1, 0, scale)?;
        let captured = self.engine.parameters().to_vec();
        let resources = self.ensure_frame_resources(
            1,
            1,
            FramePixelFormat::Argb8,
            FramePixelFormat::Argb8,
            abi::PF_PIXEL_SIZE,
            captured.len(),
            &[],
            None,
        )?;
        self.engine
            .write(resources.input_param, &vec![0; abi::PF_PARAM_DEF_SIZE])?;
        self.engine
            .write_u64(resources.params, resources.input_param)?;
        for (index, param) in captured.into_iter().enumerate() {
            let mut definition = param.bytes;
            materialize_default(&mut definition, param.param_type, 1, 1)
                .map_err(ClassicError::Input)?;
            if param.param_type == PARAM_ARBITRARY_DATA {
                let union = abi::PARAM_U_OFFSET;
                let default = read_u64(&definition, union + ARBITRARY_DEFAULT_HANDLE_OFFSET);
                if default != 0 {
                    let id = read_i16(&definition, union + abi::ARBITRARY_ID_OFFSET);
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
            self.engine
                .write(resources.parameter_definitions[index], &definition)?;
            self.engine.write_u64(
                resources.params + (index as u64 + 1) * abi::POINTER_SIZE as u64,
                resources.parameter_definitions[index],
            )?;
        }
        self.engine.configure_parameter_definitions(
            resources.input_param,
            resources.parameter_definitions,
        )?;
        Ok(setup)
    }

    pub fn ui_parameter_snapshot(&mut self) -> Result<Vec<serde_json::Value>, ClassicError> {
        let resources = self
            .frame_resources
            .as_ref()
            .ok_or_else(|| ClassicError::Input("UI session is not initialized".into()))?
            .clone();
        let mut values = Vec::new();
        for (index, address) in resources.parameter_definitions.iter().enumerate() {
            let mut bytes = vec![0; abi::PF_PARAM_DEF_SIZE];
            self.engine.read(*address, &mut bytes)?;
            let kind = read_i32(&bytes, abi::PARAM_PARAM_TYPE_OFFSET);
            let value = if kind == PARAM_ARBITRARY_DATA {
                let id = read_i16(&bytes, abi::PARAM_U_OFFSET + abi::ARBITRARY_ID_OFFSET);
                let refcon = read_u64(&bytes, abi::PARAM_U_OFFSET + ARBITRARY_REFCON_OFFSET);
                let handle = read_u64(&bytes, abi::PARAM_U_OFFSET + ARBITRARY_VALUE_HANDLE_OFFSET);
                if handle == 0 {
                    serde_json::Value::Null
                } else {
                    let size_cell = self.engine.allocate(4, 4)?;
                    let code = self.call_arbitrary_callback(
                        ARBITRARY_FLAT_SIZE_FUNC,
                        id,
                        refcon,
                        &[
                            (abi::ARB_FLAT_SIZE_VALUE_OFFSET, handle),
                            (abi::ARB_FLAT_SIZE_OUTPUT_OFFSET, size_cell),
                        ],
                    )?;
                    if code != 0 {
                        return Err(ClassicError::Input(format!("FLAT_SIZE returned {code}")));
                    }
                    let mut size = [0u8; 4];
                    self.engine.read(size_cell, &mut size)?;
                    let size = u32::from_le_bytes(size) as usize;
                    if size == 0 || size > 65536 {
                        return Err(ClassicError::Input(
                            "arbitrary flat size outside 1..65536".into(),
                        ));
                    }
                    let buffer = self.engine.allocate(size, 8)?;
                    let code = self.call_arbitrary_callback(
                        ARBITRARY_FLATTEN_FUNC,
                        id,
                        refcon,
                        &[
                            (abi::ARB_FLATTEN_VALUE_OFFSET, handle),
                            (abi::ARB_FLATTEN_SIZE_OFFSET, size as u64),
                            (abi::ARB_FLATTEN_BUFFER_OFFSET, buffer),
                        ],
                    )?;
                    if code != 0 {
                        return Err(ClassicError::Input(format!("FLATTEN returned {code}")));
                    }
                    let mut flat = vec![0; size];
                    self.engine.read(buffer, &mut flat)?;
                    serde_json::json!({"flat_bytes":flat})
                }
            } else {
                let length = match kind {
                    PARAM_SLIDER | PARAM_FIXED_SLIDER | PARAM_ANGLE | PARAM_CHECKBOX
                    | PARAM_COLOR | PARAM_POPUP => 4,
                    PARAM_POINT | PARAM_FLOAT_SLIDER => 8,
                    PARAM_POINT3D => 24,
                    _ => 0,
                };
                serde_json::json!({"bytes":bytes[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET+length]})
            };
            values.push(serde_json::json!({"slot":index+1,"type":kind,"value":value,"change_flags":read_u32(&bytes,abi::PARAM_UU_OFFSET),"hidden":self.engine.ui_hidden(index+1),
                "name_bytes":bytes[abi::PARAM_NAME_OFFSET..abi::PARAM_NAME_OFFSET+abi::PARAM_NAME_SIZE],
                "ui_flags":read_u32(&bytes,abi::PARAM_UI_FLAGS_OFFSET),
                "flags":read_u32(&bytes,abi::PARAM_FLAGS_OFFSET)}));
        }
        Ok(values)
    }

    pub fn update_params_ui(&mut self) -> Result<serde_json::Value, ClassicError> {
        let params = self
            .frame_resources
            .as_ref()
            .ok_or_else(|| ClassicError::Input("UI session is not initialized".into()))?
            .params;
        let before = self.ui_parameter_snapshot()?;
        self.engine.begin_ui_dispatch();
        let result = self.engine.call_selector_win64(
            self.entry,
            [
                abi::PF_CMD_UPDATE_PARAMS_UI as u64,
                self.input,
                self.output,
                params,
                0,
                0,
            ],
        );
        let calls = self.engine.end_ui_dispatch();
        let code =
            result.map_err(|source| selector_guest_error("UPDATE_PARAMS_UI", source))? as i32;
        if code != 0 {
            return Err(ClassicError::Selector {
                selector: "UPDATE_PARAMS_UI",
                error: code,
            });
        }
        Ok(
            serde_json::json!({"selector":abi::PF_CMD_UPDATE_PARAMS_UI,"selector_error":code,"before":before,"after":self.ui_parameter_snapshot()?,"calls":calls}),
        )
    }
}

#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UiEvent {
    pub event: i32,
    pub slot: usize,
    pub area: i32,
    /// [left, top, right, bottom] in window coordinates.
    pub frame: [i32; 4],
    pub x: i32,
    pub y: i32,
    pub clicks: i32,
    pub last: bool,
    pub context: u64,
    pub window_type: i32,
    pub theme: BTreeMap<u16, [u16; 3]>,
    pub picker_argb_bits: Option<[u32; 4]>,
    /// Additive keydown payload; absent fields preserve U03 JSON output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_code: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifiers: Option<u32>,
}
impl ClassicHost {
    pub fn ui_event(&mut self, event: &UiEvent) -> Result<serde_json::Value, ClassicError> {
        let resources = self
            .frame_resources
            .as_ref()
            .ok_or_else(|| ClassicError::Input("UI session is not initialized".into()))?
            .clone();
        if event.slot == 0
            || event.slot > resources.parameter_definitions.len()
            || ![
                EVENT_DO_CLICK,
                EVENT_DRAG,
                EVENT_DRAW,
                EVENT_ADJUST_CURSOR,
                EVENT_KEY_DOWN,
            ]
            .contains(&event.event)
        {
            return Err(ClassicError::Input("invalid UI event or slot".into()));
        }
        if (event.event == EVENT_KEY_DOWN) != event.key_code.is_some()
            || (event.event != EVENT_KEY_DOWN && event.modifiers.is_some())
        {
            return Err(ClassicError::Input(
                "key payload requires keydown event 10".into(),
            ));
        }
        let context = if let Some((address, kind)) = self.ui_contexts.get(&event.context) {
            if *kind != event.window_type {
                return Err(ClassicError::Input(
                    "context window type cannot change".into(),
                ));
            }
            *address
        } else {
            let address = self.engine.allocate(CONTEXT_ALLOCATION_SIZE, 8)?;
            let mut bytes = [0u8; CONTEXT_ALLOCATION_SIZE];
            write_u64(
                &mut bytes,
                CONTEXT_HANDLE_OFFSET,
                address + CONTEXT_VALUE_OFFSET as u64,
            );
            write_u32(
                &mut bytes,
                CONTEXT_VALUE_OFFSET + abi::CONTEXT_MAGIC_OFFSET,
                CONTEXT_MAGIC,
            );
            write_i32(
                &mut bytes,
                CONTEXT_VALUE_OFFSET + abi::CONTEXT_WINDOW_TYPE_OFFSET,
                event.window_type,
            );
            self.engine.write(address, &bytes)?;
            self.ui_contexts
                .insert(event.context, (address, event.window_type));
            address
        };
        let extra = if let Some(address) = self.ui_event_extra {
            address
        } else {
            let address = self.engine.allocate(abi::PF_EVENT_EXTRA_SIZE, 8)?;
            self.ui_event_extra = Some(address);
            address
        };
        let mut bytes = [0u8; abi::PF_EVENT_EXTRA_SIZE];
        if event.event == EVENT_DRAG {
            let (slot, continuation) = self.ui_drags.get(&event.context).ok_or_else(|| {
                ClassicError::Input("drag without an active click in this context".into())
            })?;
            if *slot != event.slot {
                return Err(ClassicError::Input(
                    "drag slot differs from originating click".into(),
                ));
            }
            bytes[abi::EVENT_UNION_OFFSET + abi::DO_CLICK_CONTINUE_REFCON_OFFSET
                ..abi::EVENT_UNION_OFFSET
                    + abi::DO_CLICK_CONTINUE_REFCON_OFFSET
                    + abi::DO_CLICK_CONTINUE_REFCON_SIZE]
                .copy_from_slice(continuation);
        }
        write_u64(&mut bytes, abi::EVENT_CONTEXT_OFFSET, context);
        write_i32(&mut bytes, abi::EVENT_TYPE_OFFSET, event.event);
        if event.event == EVENT_DO_CLICK || event.event == EVENT_DRAG {
            write_i32(
                &mut bytes,
                abi::EVENT_UNION_OFFSET + abi::DO_CLICK_SCREEN_POINT_OFFSET,
                event.x,
            );
            write_i32(
                &mut bytes,
                abi::EVENT_UNION_OFFSET + abi::DO_CLICK_SCREEN_POINT_OFFSET + SCREEN_POINT_Y_OFFSET,
                event.y,
            );
            write_i32(
                &mut bytes,
                abi::EVENT_UNION_OFFSET + abi::DO_CLICK_NUM_CLICKS_OFFSET,
                event.clicks,
            );
            bytes[abi::EVENT_UNION_OFFSET + abi::DO_CLICK_LAST_TIME_OFFSET] = u8::from(event.last);
        } else if event.event == EVENT_KEY_DOWN {
            // Same PF_DoKeyDownEventInfo layout as worker_ui_event_execution.cpp.
            write_u32(
                &mut bytes,
                abi::EVENT_UNION_OFFSET + abi::KEY_DOWN_WHEN_OFFSET,
                1,
            );
            write_i32(
                &mut bytes,
                abi::EVENT_UNION_OFFSET + abi::KEY_DOWN_SCREEN_POINT_OFFSET,
                event.x,
            );
            write_i32(
                &mut bytes,
                abi::EVENT_UNION_OFFSET + abi::KEY_DOWN_SCREEN_POINT_OFFSET + SCREEN_POINT_Y_OFFSET,
                event.y,
            );
            write_u32(
                &mut bytes,
                abi::EVENT_UNION_OFFSET + abi::KEY_DOWN_KEYCODE_OFFSET,
                event.key_code.unwrap(),
            );
            write_u32(
                &mut bytes,
                abi::EVENT_UNION_OFFSET + abi::KEY_DOWN_MODIFIERS_OFFSET,
                event.modifiers.unwrap_or(0),
            );
        } else if event.event == EVENT_ADJUST_CURSOR {
            write_i32(
                &mut bytes,
                abi::EVENT_UNION_OFFSET + abi::ADJUST_CURSOR_SCREEN_POINT_OFFSET,
                event.x,
            );
            write_i32(
                &mut bytes,
                abi::EVENT_UNION_OFFSET
                    + abi::ADJUST_CURSOR_SCREEN_POINT_OFFSET
                    + SCREEN_POINT_Y_OFFSET,
                event.y,
            );
        }
        write_i32(
            &mut bytes,
            abi::EVENT_EFFECT_WINDOW_OFFSET + abi::EFFECT_WINDOW_INDEX_OFFSET,
            event.slot as i32,
        );
        write_i32(
            &mut bytes,
            abi::EVENT_EFFECT_WINDOW_OFFSET + abi::EFFECT_WINDOW_AREA_OFFSET,
            event.area,
        );
        for (index, value) in event.frame.iter().enumerate() {
            write_i32(
                &mut bytes,
                abi::EVENT_EFFECT_WINDOW_OFFSET
                    + abi::EFFECT_WINDOW_CURRENT_FRAME_OFFSET
                    + index * 4,
                *value,
            );
        }
        if event.event == EVENT_DRAW {
            for (index, value) in event.frame.iter().enumerate() {
                write_i32(
                    &mut bytes,
                    abi::EVENT_UNION_OFFSET + DRAW_UPDATE_RECT_OFFSET + index * 4,
                    *value,
                );
            }
            write_i32(&mut bytes, abi::EVENT_UNION_OFFSET + DRAW_DEPTH_OFFSET, 32);
        }
        self.engine.write(extra, &bytes)?;
        self.engine
            .configure_ui_drawing(context, event.theme.clone(), event.picker_argb_bits);
        let before = self.ui_parameter_snapshot()?;
        self.engine.begin_ui_dispatch();
        let result = self.engine.call_selector_win64(
            self.entry,
            [
                abi::PF_CMD_EVENT as u64,
                self.input,
                self.output,
                resources.params,
                0,
                extra,
            ],
        );
        let calls = self.engine.end_ui_dispatch();
        let code = result.map_err(|source| selector_guest_error("EVENT", source))? as i32;
        if code != 0 {
            return Err(ClassicError::Selector {
                selector: "EVENT",
                error: code,
            });
        }
        self.engine.read(extra, &mut bytes)?;
        if event.event == EVENT_DO_CLICK
            && bytes[abi::EVENT_UNION_OFFSET + abi::DO_CLICK_SEND_DRAG_OFFSET] != 0
            || event.event == EVENT_DRAG && !event.last
        {
            self.ui_drags.insert(
                event.context,
                (
                    event.slot,
                    bytes[abi::EVENT_UNION_OFFSET + abi::DO_CLICK_CONTINUE_REFCON_OFFSET
                        ..abi::EVENT_UNION_OFFSET
                            + abi::DO_CLICK_CONTINUE_REFCON_OFFSET
                            + abi::DO_CLICK_CONTINUE_REFCON_SIZE]
                        .try_into()
                        .unwrap(),
                ),
            );
        } else if event.event == EVENT_DO_CLICK || event.event == EVENT_DRAG {
            self.ui_drags.remove(&event.context);
        }
        Ok(
            serde_json::json!({"selector":abi::PF_CMD_EVENT,"selector_error":code,"input":event,"before":before,"after":self.ui_parameter_snapshot()?,"calls":calls,"send_drag":bytes[abi::EVENT_UNION_OFFSET + abi::DO_CLICK_SEND_DRAG_OFFSET],"event_out_flags":read_u32(&bytes,abi::EVENT_OUT_FLAGS_OFFSET)}),
        )
    }
}

impl ClassicHost {
    pub fn ui_lifetimes(&self) -> serde_json::Value {
        self.engine.ui_lifetimes()
    }
    pub fn ui_state_u32(
        &mut self,
        values: &BTreeMap<u64, u32>,
        write: bool,
    ) -> Result<BTreeMap<u64, u32>, ClassicError> {
        Ok(self.engine.ui_state_u32(values, write)?)
    }
}
