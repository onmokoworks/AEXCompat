// Explicit UI-session support. These suites are unavailable to render sessions.
const UI_PARAM_SUITE: u64 = 0x0000_0030_0000_0000;
const UI_SLOTS: u64 = 128;
const UI_CALLBACK_BASE: u64 = UI_PARAM_SUITE + 0x4000;

#[derive(Default)]
struct UiSessionState {
    enabled: bool,
    draw: UiDrawState,
    return_code: u64,
    stream_values: BTreeSet<u64>,
    time: [u8; 8],
    next_handle: u64,
    effects: BTreeSet<u64>,
    streams: BTreeMap<u64, usize>,
    hidden: BTreeMap<usize, bool>,
    dispatching: bool,
    calls: Vec<serde_json::Value>,
}

impl GuestEngine<'static> {
    pub fn enable_ui_session(&mut self) -> Result<(), GuestError> {
        if self.unicorn.get_data().ui_session.enabled {
            return Err(GuestError::Callback("UI session already enabled".into()));
        }
        uc(
            "map UI suites",
            self.unicorn.mem_map(UI_PARAM_SUITE, 0x10000, Prot::ALL),
        )?;
        for slot in 0..13 * UI_SLOTS {
            let callback = UI_CALLBACK_BASE + slot * STUB_STRIDE;
            uc(
                "write UI callback",
                self.unicorn.mem_write(callback, &[0xc3]),
            )?;
            uc(
                "write UI suite",
                self.unicorn
                    .mem_write(UI_PARAM_SUITE + slot * 8, &callback.to_le_bytes()),
            )?;
        }
        uc(
            "hook UI callbacks",
            self.unicorn.add_code_hook(
                UI_CALLBACK_BASE,
                UI_CALLBACK_BASE + (13 * UI_SLOTS - 1) * STUB_STRIDE,
                emulate_ui_callback,
            ),
        )?;
        uc(
            "seal UI suites",
            self.unicorn
                .mem_protect(UI_PARAM_SUITE, 0x10000, Prot::READ | Prot::EXEC),
        )?;
        self.unicorn.get_data_mut().ui_session.enabled = true;
        Ok(())
    }
    pub fn ui_lifetimes(&self) -> serde_json::Value {
        let state = &self.unicorn.get_data().ui_session;
        serde_json::json!({"effects":state.effects.len(),"streams":state.streams.len(),"stream_values":state.stream_values.len(),"draw_objects":state.draw.objects.len()})
    }
    pub fn ui_state_u32(
        &mut self,
        values: &BTreeMap<u64, u32>,
        write: bool,
    ) -> Result<BTreeMap<u64, u32>, GuestError> {
        if !self.unicorn.get_data().ui_session.enabled || values.len() > 16 {
            return Err(GuestError::Callback("invalid UI state request".into()));
        }
        let mut result = BTreeMap::new();
        for (rva, value) in values {
            let address = self
                .image_base
                .checked_add(*rva)
                .ok_or_else(|| GuestError::Callback("state RVA overflow".into()))?;
            if address
                .checked_add(4)
                .is_none_or(|end| end > self.image_end)
                || !guest_range_has_permission(&self.unicorn, address, 4, Prot::READ | Prot::WRITE)
                    .map_err(GuestError::Callback)?
            {
                return Err(GuestError::Callback(
                    "UI state RVA must refer to writable original image data".into(),
                ));
            }
            let mut bytes = [0u8; 4];
            self.read(address, &mut bytes)?;
            result.insert(*rva, u32::from_le_bytes(bytes));
            if write {
                self.write(address, &value.to_le_bytes())?;
            }
        }
        Ok(result)
    }
    pub fn configure_ui_drawing(
        &mut self,
        context: u64,
        theme: BTreeMap<u16, [u16; 3]>,
        picker: Option<[u32; 4]>,
    ) {
        let draw = &mut self.unicorn.get_data_mut().ui_session.draw;
        draw.context = context;
        draw.theme = theme;
        draw.picker = picker;
    }
    pub fn ui_hidden(&self, slot: usize) -> bool {
        *self
            .unicorn
            .get_data()
            .ui_session
            .hidden
            .get(&slot)
            .unwrap_or(&false)
    }
    pub fn set_ui_time(&mut self, time: i32, scale: u32) {
        let state = &mut self.unicorn.get_data_mut().ui_session;
        state.time[..4].copy_from_slice(&time.to_le_bytes());
        state.time[4..].copy_from_slice(&scale.to_le_bytes());
    }
    pub fn begin_ui_dispatch(&mut self) {
        let state = &mut self.unicorn.get_data_mut().ui_session;
        state.calls.clear();
        state.dispatching = true;
    }
    pub fn end_ui_dispatch(&mut self) -> Vec<serde_json::Value> {
        let state = &mut self.unicorn.get_data_mut().ui_session;
        state.dispatching = false;
        std::mem::take(&mut state.calls)
    }
}

fn emulate_ui_callback(unicorn: &mut Unicorn<'_, GuestState>, address: u64, _: u32) {
    let result = (|| -> Result<(), String> {
        let slot = (address - UI_CALLBACK_BASE) / STUB_STRIDE;
        if !unicorn.get_data().ui_session.dispatching {
            return Err(format!(
                "unsupported UI suite call: PF Param Utils slot {slot}"
            ));
        }
        unicorn.get_data_mut().ui_session.return_code = 0;
        if unicorn.get_data().ui_session.calls.len() >= 200000 {
            return Err("UI recording limit exceeded".into());
        }
        if slot >= 6 * UI_SLOTS && slot < 12 * UI_SLOTS {
            return emulate_ui_draw_call(unicorn, slot / UI_SLOTS, slot % UI_SLOTS);
        }
        if slot >= UI_SLOTS {
            return emulate_ui_stream_call(unicorn, slot / UI_SLOTS, slot % UI_SLOTS);
        }
        if slot != 0 {
            return Err(format!("unsupported ParamUtils slot {slot}"));
        }
        let effect = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|e| e.to_string())?;
        let index = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|e| e.to_string())? as i32;
        let source = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|e| e.to_string())?;
        if effect != 1 || index <= 0 {
            return Err("UpdateParamUI invalid effect or slot".into());
        }
        let target = *unicorn
            .get_data()
            .parameter_definitions
            .get(index as usize - 1)
            .ok_or("UpdateParamUI slot outside parameter table")?;
        let definition = unicorn
            .mem_read_as_vec(source, abi::PF_PARAM_DEF_SIZE)
            .map_err(|e| e.to_string())?;
        let mut current = unicorn
            .mem_read_as_vec(target, abi::PF_PARAM_DEF_SIZE)
            .map_err(|e| e.to_string())?;
        // Cosmetic fields only: never copy a caller's value or handle back.
        for (offset, size) in [
            (abi::PARAM_UI_FLAGS_OFFSET, 4),
            (abi::PARAM_UI_WIDTH_OFFSET, 2),
            (abi::PARAM_UI_HEIGHT_OFFSET, 2),
            (abi::PARAM_NAME_OFFSET, abi::PARAM_NAME_SIZE),
        ] {
            current[offset..offset + size].copy_from_slice(&definition[offset..offset + size]);
        }
        let kind = i32::from_le_bytes(definition[12..16].try_into().unwrap());
        if current[12..16] != definition[12..16] {
            return Err("UpdateParamUI parameter type mismatch".into());
        }
        // AE permits only these dynamic flag bits, never value/ownership fields.
        let source_flags = u32::from_le_bytes(definition[48..52].try_into().unwrap());
        let old_flags = u32::from_le_bytes(current[48..52].try_into().unwrap());
        current[48..52].copy_from_slice(&((old_flags & !32) | (source_flags & 32)).to_le_bytes());
        let mut ranges = Vec::new();
        match kind {
            1 => ranges.push((76, 8)),
            2 => {
                ranges.push((76, 8));
                ranges.push((88, 4));
            }
            10 => {
                ranges.push((56, 8));
                ranges.push((68, 4));
            }
            _ => {}
        }
        let mut slider = Vec::new();
        for (offset, size) in ranges {
            let start = abi::PARAM_U_OFFSET + offset;
            current[start..start + size].copy_from_slice(&definition[start..start + size]);
            slider.push(
                serde_json::json!({"union_offset":offset,"bytes":definition[start..start+size]}),
            );
        }
        let flags = u32::from_le_bytes(definition[4..8].try_into().unwrap());
        let name = definition
            [abi::PARAM_NAME_OFFSET..abi::PARAM_NAME_OFFSET + abi::PARAM_NAME_SIZE]
            .to_vec();
        unicorn
            .mem_write(target, &current)
            .map_err(|e| e.to_string())?;
        unicorn.get_data_mut().ui_session.calls.push(serde_json::json!({"call":"PF_UpdateParamUI", "slot":index, "ui_flags":flags, "name_bytes":name,"flags":u32::from_le_bytes(current[48..52].try_into().unwrap()),"width":u16::from_le_bytes(current[8..10].try_into().unwrap()),"height":u16::from_le_bytes(current[10..12].try_into().unwrap()),"slider_ui":slider}));
        Ok(())
    })();
    match result {
        Ok(()) => {
            let code = unicorn.get_data().ui_session.return_code;
            let _ = unicorn.reg_write(RegisterX86::RAX, code);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn ui_suite_address(name: &str, version: u64) -> Option<u64> {
    let index = match (name, version) {
        ("PF Param Utils Suite", 3) => 0,
        ("AEGP PF Interface Suite", 1) => 1,
        ("AEGP Layer Suite", 11) => 2,
        ("AEGP Effect Suite", 3 | 4) => 3,
        ("AEGP Stream Suite", 7 | 10) => 4,
        ("AEGP Dynamic Stream Suite", 2 | 3) => 5,
        ("AEGP Dynamic Stream Suite", 5) => 12,
        ("PF Effect Custom UI Suite", 1) => 6,
        ("DRAWBOT Draw Suite", 1) => 7,
        ("DRAWBOT Supplier Suite", 1) => 8,
        ("DRAWBOT Surface Suite", 2) => 9,
        ("DRAWBOT Path Suite", 1) => 10,
        ("PF AE App Suite", 1 | 4) => 11,
        _ => return None,
    };
    Some(UI_PARAM_SUITE + index * UI_SLOTS * 8)
}
fn ui_write(
    unicorn: &mut Unicorn<'_, GuestState>,
    address: u64,
    bytes: &[u8],
) -> Result<(), String> {
    unicorn.mem_write(address, bytes).map_err(|e| e.to_string())
}
fn ui_new_handle(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u64, String> {
    let state = &mut unicorn.get_data_mut().ui_session;
    if state.next_handle >= 100000 {
        return Err("UI handle budget exhausted".into());
    }
    state.next_handle += 1;
    Ok(0x554900000000 + state.next_handle)
}
fn emulate_ui_stream_call(
    unicorn: &mut Unicorn<'_, GuestState>,
    suite: u64,
    slot: u64,
) -> Result<(), String> {
    let (suite, slot) = if suite == 12 {
        (
            5,
            slot.checked_sub(1)
                .ok_or("unsupported DynamicStream v5 slot 0")?,
        )
    } else {
        (suite, slot)
    };
    let a = unicorn
        .reg_read(RegisterX86::RCX)
        .map_err(|e| e.to_string())?;
    let b = unicorn
        .reg_read(RegisterX86::RDX)
        .map_err(|e| e.to_string())?;
    let c = unicorn
        .reg_read(RegisterX86::R8)
        .map_err(|e| e.to_string())?;
    let d = unicorn
        .reg_read(RegisterX86::R9)
        .map_err(|e| e.to_string())?;
    match (suite, slot) {
        (1, 0) if a == 1 => ui_write(unicorn, b, &0x554800000001u64.to_le_bytes()),
        (1, 1) if b == 1 => {
            let handle = ui_new_handle(unicorn)?;
            ui_write(unicorn, c, &handle.to_le_bytes())?;
            unicorn.get_data_mut().ui_session.effects.insert(handle);
            Ok(())
        }
        (2, 14) if a == 0x554800000001 && b <= 1 => {
            let time = unicorn.get_data().ui_session.time;
            ui_write(unicorn, c, &time)
        }
        (3, 8) => {
            if !unicorn.get_data_mut().ui_session.effects.remove(&a) {
                return Err("invalid/disposed UI effect".into());
            }
            Ok(())
        }
        (4, 5) if unicorn.get_data().ui_session.effects.contains(&b) => {
            let index = c as usize;
            if index == 0 || index > unicorn.get_data().parameter_definitions.len() {
                return Err("UI stream index outside exact slot table".into());
            }
            let handle = ui_new_handle(unicorn)?;
            ui_write(unicorn, d, &handle.to_le_bytes())?;
            unicorn
                .get_data_mut()
                .ui_session
                .streams
                .insert(handle, index);
            Ok(())
        }
        (4, 13) => {
            let index = *unicorn
                .get_data()
                .ui_session
                .streams
                .get(&b)
                .ok_or("invalid UI value stream")?;
            if c > 1 {
                return Err("unsupported stream time mode".into());
            }
            let time = unicorn.mem_read_as_vec(d, 8).map_err(|e| e.to_string())?;
            if time != unicorn.get_data().ui_session.time {
                return Err("UI stream time differs from injected current time".into());
            }
            let output = aegp_stack_arg(unicorn, 0x30)?;
            if unicorn
                .get_data()
                .ui_session
                .stream_values
                .contains(&output)
            {
                return Err("UI stream value still live".into());
            }
            let address = unicorn.get_data().parameter_definitions[index - 1];
            let bytes = unicorn
                .mem_read_as_vec(address, abi::PF_PARAM_DEF_SIZE)
                .map_err(|e| e.to_string())?;
            let kind = i32::from_le_bytes(bytes[12..16].try_into().unwrap());
            let union = &bytes[abi::PARAM_U_OFFSET..];
            let value = match kind {
                1 | 4 | 7 => f64::from(i32::from_le_bytes(union[..4].try_into().unwrap())),
                2 | 3 => f64::from(i32::from_le_bytes(union[..4].try_into().unwrap())) / 65536.0,
                10 => f64::from_le_bytes(union[..8].try_into().unwrap()),
                _ => return Err(format!("unsupported UI stream value type {kind}")),
            };
            let mut bytes = [0u8; 40];
            bytes[..8].copy_from_slice(&b.to_le_bytes());
            bytes[8..16].copy_from_slice(&value.to_le_bytes());
            ui_write(unicorn, output, &bytes)?;
            unicorn
                .get_data_mut()
                .ui_session
                .stream_values
                .insert(output);
            Ok(())
        }
        (4, 14) => {
            if !unicorn.get_data_mut().ui_session.stream_values.remove(&a) {
                return Err("stale UI stream value".into());
            }
            Ok(())
        }
        (4, 7) => {
            if unicorn
                .get_data_mut()
                .ui_session
                .streams
                .remove(&a)
                .is_none()
            {
                return Err("invalid/disposed UI stream".into());
            }
            Ok(())
        }
        (5, 4 | 5) => {
            let index = *unicorn
                .get_data()
                .ui_session
                .streams
                .get(&a)
                .ok_or("invalid UI stream")?;
            if slot == 4 {
                let flags = if *unicorn
                    .get_data()
                    .ui_session
                    .hidden
                    .get(&index)
                    .unwrap_or(&false)
                {
                    2u32
                } else {
                    0
                };
                ui_write(unicorn, b, &flags.to_le_bytes())
            } else {
                if b as u32 != 2 || c as u8 > 1 || d as u8 > 1 {
                    return Err("unsupported dynamic stream flag/boolean".into());
                }
                let state = &mut unicorn.get_data_mut().ui_session;
                state.hidden.insert(index, d as u8 != 0);
                state.calls.push(serde_json::json!({"call":"AEGP_SetDynamicStreamFlag","slot":index,"flag":b as u32,"undoable":c as u8!=0,"value":d as u8!=0}));
                Ok(())
            }
        }
        _ => Err(format!("unsupported UI suite {suite} slot {slot}")),
    }
}
