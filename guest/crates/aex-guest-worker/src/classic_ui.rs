// Kept separate so the render route never implicitly dispatches UI selectors.
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
            4,
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
            self.engine
                .write(resources.parameter_definitions[index], &definition)?;
            self.engine.write_u64(
                resources.params + (index as u64 + 1) * 8,
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
                let id = read_i16(&bytes, abi::PARAM_U_OFFSET);
                let refcon = read_u64(&bytes, abi::PARAM_U_OFFSET + ARBITRARY_REFCON_OFFSET);
                let handle = read_u64(&bytes, abi::PARAM_U_OFFSET + ARBITRARY_VALUE_HANDLE_OFFSET);
                if handle == 0 {
                    serde_json::Value::Null
                } else {
                    let size_cell = self.engine.allocate(4, 4)?;
                    let code = self.call_arbitrary_callback(
                        3,
                        id,
                        refcon,
                        &[(16, handle), (24, size_cell)],
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
                        4,
                        id,
                        refcon,
                        &[(16, handle), (24, size as u64), (32, buffer)],
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
                    1 | 2 | 3 | 4 | 5 | 7 => 4,
                    6 | 10 => 8,
                    18 => 24,
                    _ => 0,
                };
                serde_json::json!({"bytes":bytes[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET+length]})
            };
            values.push(serde_json::json!({"slot":index+1,"type":kind,"value":value,"change_flags":read_u32(&bytes,0),"hidden":self.engine.ui_hidden(index+1),
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
        let result = self
            .engine
            .call_selector_win64(self.entry, [14, self.input, self.output, params, 0, 0]);
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
            serde_json::json!({"selector":14,"selector_error":code,"before":before,"after":self.ui_parameter_snapshot()?,"calls":calls}),
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
            || ![2, 3, 4, 9, 10].contains(&event.event)
        {
            return Err(ClassicError::Input("invalid UI event or slot".into()));
        }
        if (event.event == 10) != event.key_code.is_some()
            || (event.event != 10 && event.modifiers.is_some())
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
            let address = self.engine.allocate(80, 8)?;
            let mut bytes = [0u8; 80];
            write_u64(&mut bytes, 0, address + 8);
            write_u32(&mut bytes, 8, 0x05ea771e);
            write_i32(&mut bytes, 12, event.window_type);
            self.engine.write(address, &bytes)?;
            self.ui_contexts
                .insert(event.context, (address, event.window_type));
            address
        };
        let extra = if let Some(address) = self.ui_event_extra {
            address
        } else {
            let address = self.engine.allocate(208, 8)?;
            self.ui_event_extra = Some(address);
            address
        };
        let mut bytes = [0u8; 208];
        if event.event == 3 {
            let (slot, continuation) = self.ui_drags.get(&event.context).ok_or_else(|| {
                ClassicError::Input("drag without an active click in this context".into())
            })?;
            if *slot != event.slot {
                return Err(ClassicError::Input(
                    "drag slot differs from originating click".into(),
                ));
            }
            bytes[40..72].copy_from_slice(continuation);
        }
        write_u64(&mut bytes, 0, context);
        write_i32(&mut bytes, 8, event.event);
        if event.event == 2 || event.event == 3 {
            write_i32(&mut bytes, 20, event.x);
            write_i32(&mut bytes, 24, event.y);
            write_i32(&mut bytes, 28, event.clicks);
            bytes[73] = u8::from(event.last);
        } else if event.event == 10 {
            // Same PF_DoKeyDownEventInfo layout as worker_ui_event_execution.cpp.
            write_u32(&mut bytes, 16, 1);
            write_i32(&mut bytes, 20, event.x);
            write_i32(&mut bytes, 24, event.y);
            write_u32(&mut bytes, 28, event.key_code.unwrap());
            write_u32(&mut bytes, 32, event.modifiers.unwrap_or(0));
        } else if event.event == 9 {
            write_i32(&mut bytes, 16, event.x);
            write_i32(&mut bytes, 20, event.y);
        }
        write_i32(&mut bytes, 80, event.slot as i32);
        write_i32(&mut bytes, 84, event.area);
        for (index, value) in event.frame.iter().enumerate() {
            write_i32(&mut bytes, 88 + index * 4, *value);
        }
        if event.event == 4 {
            for (index, value) in event.frame.iter().enumerate() {
                write_i32(&mut bytes, 16 + index * 4, *value);
            }
            write_i32(&mut bytes, 32, 32);
        }
        self.engine.write(extra, &bytes)?;
        self.engine
            .configure_ui_drawing(context, event.theme.clone(), event.picker_argb_bits);
        let before = self.ui_parameter_snapshot()?;
        self.engine.begin_ui_dispatch();
        let result = self.engine.call_selector_win64(
            self.entry,
            [15, self.input, self.output, resources.params, 0, extra],
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
        if event.event == 2 && bytes[72] != 0 || event.event == 3 && !event.last {
            self.ui_drags.insert(
                event.context,
                (event.slot, bytes[40..72].try_into().unwrap()),
            );
        } else if event.event == 2 || event.event == 3 {
            self.ui_drags.remove(&event.context);
        }
        Ok(
            serde_json::json!({"selector":15,"selector_error":code,"input":event,"before":before,"after":self.ui_parameter_snapshot()?,"calls":calls,"send_drag":bytes[72],"event_out_flags":read_u32(&bytes,204)}),
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
