fn read_guest_u64(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    description: &str,
) -> Result<u64, String> {
    let mut bytes = [0u8; 8];
    unicorn
        .mem_read(address, &mut bytes)
        .map_err(|error| format!("{description}: {error}"))?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_guest_i32(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    description: &str,
) -> Result<i32, String> {
    let mut bytes = [0u8; 4];
    unicorn
        .mem_read(address, &mut bytes)
        .map_err(|error| format!("{description}: {error}"))?;
    Ok(i32::from_le_bytes(bytes))
}

fn schedule_iterate_pixel(unicorn: &mut Unicorn<'_, GuestState>) -> Result<(), String> {
    let pending = unicorn
        .get_data()
        .pending_iterate
        .as_ref()
        .cloned()
        .ok_or_else(|| "PF iterate continuation has no pending call".to_string())?;
    let output = pending.destination_data
        + pending.y as u64 * pending.destination_rowbytes
        + pending.x as u64 * pending.pixel_bytes;
    let input = if pending.zero_outside_source
        && (pending.source_data == 0
            || pending.x < 0
            || pending.y < 0
            || pending.x >= pending.source_width
            || pending.y >= pending.source_height)
    {
        HOST_ZERO_PIXEL
    } else if pending.source_data == 0 {
        output
    } else {
        pending.source_data
            + pending.y as u64 * pending.source_rowbytes
            + pending.x as u64 * pending.pixel_bytes
    };
    let callback_rsp = pending
        .caller_rsp
        .checked_sub(0x30)
        .ok_or_else(|| format!("{} callback stack underflow", pending.callback_name))?;
    unicorn
        .mem_write(callback_rsp, &pending.continuation.to_le_bytes())
        .map_err(|error| format!("{} callback return address: {error}", pending.callback_name))?;
    unicorn
        .mem_write(callback_rsp + 0x28, &output.to_le_bytes())
        .map_err(|error| {
            format!(
                "{} callback output argument: {error}",
                pending.callback_name
            )
        })?;
    for (register, value) in [
        (RegisterX86::RSP, callback_rsp),
        (RegisterX86::RCX, pending.refcon),
        (
            RegisterX86::RDX,
            pending.x.wrapping_add(pending.origin_x) as u32 as u64,
        ),
        (
            RegisterX86::R8,
            pending.y.wrapping_add(pending.origin_y) as u32 as u64,
        ),
        (RegisterX86::R9, input),
        (RegisterX86::RIP, pending.pixel_function),
    ] {
        unicorn
            .reg_write(register, value)
            .map_err(|error| format!("{} callback register: {error}", pending.callback_name))?;
    }
    unicorn
        .get_data_mut()
        .pending_iterate
        .as_mut()
        .ok_or_else(|| "PF iterate continuation has no pending call".to_string())?
        .callback_phase = IterateCallbackPhase::Pixel;
    Ok(())
}

fn schedule_iterate_progress(unicorn: &mut Unicorn<'_, GuestState>) -> Result<(), String> {
    let pending = unicorn
        .get_data()
        .pending_iterate
        .as_ref()
        .cloned()
        .ok_or_else(|| "PF iterate progress has no pending call".to_string())?;
    let rows = i64::from(pending.bottom - pending.top);
    let completed_rows = i64::from(pending.y - pending.top);
    let Some((current, total)) = crate::compose_iterate_progress(
        pending.progress_base,
        pending.progress_final,
        completed_rows as i32,
        rows as i32,
    )
    .map_err(|()| format!("{} progress span exceeds i32", pending.callback_name))?
    else {
        return Err(format!(
            "{} progress callback scheduled without a positive total",
            pending.callback_name
        ));
    };
    schedule_iterate_host_callback(
        unicorn,
        &pending,
        pending.progress_function,
        pending.effect_ref,
        current as u32 as u64,
        total as u32 as u64,
        IterateCallbackPhase::Progress,
    )
}

fn schedule_iterate_abort(unicorn: &mut Unicorn<'_, GuestState>) -> Result<(), String> {
    let pending = unicorn
        .get_data()
        .pending_iterate
        .as_ref()
        .cloned()
        .ok_or_else(|| "PF iterate abort has no pending call".to_string())?;
    schedule_iterate_host_callback(
        unicorn,
        &pending,
        pending.abort_function,
        pending.effect_ref,
        0,
        0,
        IterateCallbackPhase::Abort,
    )
}

fn schedule_iterate_host_callback(
    unicorn: &mut Unicorn<'_, GuestState>,
    pending: &PendingIterate,
    function: u64,
    rcx: u64,
    rdx: u64,
    r8: u64,
    phase: IterateCallbackPhase,
) -> Result<(), String> {
    let callback_rsp = pending
        .caller_rsp
        .checked_sub(0x30)
        .ok_or_else(|| format!("{} callback stack underflow", pending.callback_name))?;
    unicorn
        .mem_write(callback_rsp, &pending.continuation.to_le_bytes())
        .map_err(|error| format!("{} callback return address: {error}", pending.callback_name))?;
    for (register, value) in [
        (RegisterX86::RSP, callback_rsp),
        (RegisterX86::RCX, rcx),
        (RegisterX86::RDX, rdx),
        (RegisterX86::R8, r8),
        (RegisterX86::RIP, function),
    ] {
        unicorn
            .reg_write(register, value)
            .map_err(|error| format!("{} callback register: {error}", pending.callback_name))?;
    }
    unicorn
        .get_data_mut()
        .pending_iterate
        .as_mut()
        .ok_or_else(|| "PF iterate continuation has no pending call".to_string())?
        .callback_phase = phase;
    Ok(())
}

fn finish_iterate(unicorn: &mut Unicorn<'_, GuestState>, result: u64) -> Result<(), String> {
    let pending = unicorn
        .get_data_mut()
        .pending_iterate
        .take()
        .ok_or_else(|| "PF iterate completion has no pending call".to_string())?;
    for (register, value) in [
        (RegisterX86::RSP, pending.caller_rsp + 8),
        (RegisterX86::RIP, pending.return_address),
        (RegisterX86::RAX, result),
    ] {
        unicorn
            .reg_write(register, value)
            .map_err(|error| format!("{} completion register: {error}", pending.callback_name))?;
    }
    Ok(())
}

fn emulate_iterate8(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    emulate_iterate_common(unicorn, false, 4, HOST_ITERATE8_CONTINUE, "Iterate8");
}

fn emulate_iterate8_origin(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    emulate_iterate_common(unicorn, true, 4, HOST_ITERATE8_CONTINUE, "Iterate8 origin");
}

fn emulate_iterate16(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    emulate_iterate_common(unicorn, false, 8, HOST_ITERATE16_CONTINUE, "Iterate16");
}

fn emulate_iterate_float(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    emulate_iterate_common(
        unicorn,
        false,
        16,
        HOST_ITERATE_FLOAT_CONTINUE,
        "IterateFloat",
    );
}

fn emulate_iterate_common(
    unicorn: &mut Unicorn<'_, GuestState>,
    has_origin: bool,
    pixel_bytes: i32,
    continuation: u64,
    callback_name: &'static str,
) {
    let result = (|| {
        if unicorn.get_data().pending_iterate.is_some() {
            return Err("nested PF iterate calls are unsupported".to_string());
        }
        let caller_rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("{callback_name} stack: {error}"))?;
        let return_address = read_guest_u64(
            unicorn,
            caller_rsp,
            &format!("{callback_name} return address"),
        )?;
        let in_data = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("{callback_name} in_data: {error}"))?;
        let progress_base = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("{callback_name} progress base: {error}"))?
            as u32 as i32;
        let progress_final = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("{callback_name} progress final: {error}"))?
            as u32 as i32;
        let source_world = unicorn
            .reg_read(RegisterX86::R9)
            .map_err(|error| format!("{callback_name} source world: {error}"))?;
        let area = read_guest_u64(unicorn, caller_rsp + 0x28, &format!("{callback_name} area"))?;
        let (origin_x, origin_y, stack_shift) = if has_origin {
            let origin = read_guest_u64(unicorn, caller_rsp + 0x30, "Iterate8 origin")?;
            if origin == 0 {
                unicorn
                    .reg_write(RegisterX86::RAX, 4)
                    .map_err(|error| format!("{callback_name} invalid-origin return: {error}"))?;
                return Ok(());
            }
            (
                read_guest_i32(unicorn, origin, "Iterate8 origin x")?,
                read_guest_i32(unicorn, origin + 4, "Iterate8 origin y")?,
                8,
            )
        } else {
            (0, 0, 0)
        };
        let (abort_function, progress_function, effect_ref) = if !has_origin && in_data != 0 {
            (
                read_guest_u64(
                    unicorn,
                    in_data + abi::INTER_ABORT_OFFSET as u64,
                    &format!("{callback_name} abort callback"),
                )?,
                read_guest_u64(
                    unicorn,
                    in_data + abi::INTER_PROGRESS_OFFSET as u64,
                    &format!("{callback_name} progress callback"),
                )?,
                read_guest_u64(
                    unicorn,
                    in_data + abi::IN_EFFECT_REF_OFFSET as u64,
                    &format!("{callback_name} effect ref"),
                )?,
            )
        } else {
            (0, 0, 0)
        };
        let refcon = read_guest_u64(unicorn, caller_rsp + 0x30 + stack_shift, "Iterate8 refcon")?;
        let pixel_function = read_guest_u64(
            unicorn,
            caller_rsp + 0x38 + stack_shift,
            "Iterate8 pixel callback",
        )?;
        let destination_world = read_guest_u64(
            unicorn,
            caller_rsp + 0x40 + stack_shift,
            "Iterate8 destination world",
        )?;
        if pixel_function == 0 || destination_world == 0 {
            unicorn
                .reg_write(RegisterX86::RAX, 4)
                .map_err(|error| format!("{callback_name} invalid-call return: {error}"))?;
            return Ok(());
        }
        let destination_data = read_guest_u64(
            unicorn,
            destination_world + abi::LAYER_DATA_OFFSET as u64,
            "Iterate8 destination data",
        )?;
        let destination_rowbytes = read_guest_i32(
            unicorn,
            destination_world + abi::LAYER_ROWBYTES_OFFSET as u64,
            "Iterate8 destination rowbytes",
        )?;
        let destination_width = read_guest_i32(
            unicorn,
            destination_world + abi::LAYER_WIDTH_OFFSET as u64,
            "Iterate8 destination width",
        )?;
        let destination_height = read_guest_i32(
            unicorn,
            destination_world + abi::LAYER_HEIGHT_OFFSET as u64,
            "Iterate8 destination height",
        )?;
        let (source_data, source_rowbytes, source_width, source_height) = if source_world == 0 {
            (0, 0, 0, 0)
        } else {
            let data = read_guest_u64(
                unicorn,
                source_world + abi::LAYER_DATA_OFFSET as u64,
                "Iterate8 source data",
            )?;
            let rowbytes = read_guest_i32(
                unicorn,
                source_world + abi::LAYER_ROWBYTES_OFFSET as u64,
                "Iterate8 source rowbytes",
            )?;
            let source_width = read_guest_i32(
                unicorn,
                source_world + abi::LAYER_WIDTH_OFFSET as u64,
                "Iterate8 source width",
            )?;
            let source_height = read_guest_i32(
                unicorn,
                source_world + abi::LAYER_HEIGHT_OFFSET as u64,
                "Iterate8 source height",
            )?;
            (data, rowbytes, source_width, source_height)
        };
        let (width, height) = if has_origin || source_world == 0 {
            (destination_width, destination_height)
        } else {
            (
                source_width.min(destination_width),
                source_height.min(destination_height),
            )
        };
        if destination_data == 0
            || source_world != 0 && source_data == 0
            || source_world != 0 && source_rowbytes < source_width.saturating_mul(pixel_bytes)
            || destination_rowbytes < destination_width.saturating_mul(pixel_bytes)
            || source_world != 0 && (source_width <= 0 || source_height <= 0)
            || width <= 0
            || height <= 0
        {
            unicorn
                .reg_write(RegisterX86::RAX, 4)
                .map_err(|error| format!("{callback_name} invalid-world return: {error}"))?;
            return Ok(());
        }
        let mut bounds = [0, 0, width, height];
        if area != 0 {
            for (index, value) in bounds.iter_mut().enumerate() {
                *value = read_guest_i32(unicorn, area + (index * 4) as u64, "Iterate8 area field")?;
            }
            if bounds[0] < 0
                || bounds[1] < 0
                || bounds[2] < bounds[0]
                || bounds[3] < bounds[1]
                || bounds[2] > width
                || bounds[3] > height
            {
                unicorn
                    .reg_write(RegisterX86::RAX, 4)
                    .map_err(|error| format!("{callback_name} invalid-area return: {error}"))?;
                return Ok(());
            }
        }
        if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
            unicorn
                .reg_write(RegisterX86::RAX, 0)
                .map_err(|error| format!("{callback_name} empty-area return: {error}"))?;
            return Ok(());
        }
        let iterations = i64::from(bounds[2] - bounds[0]) * i64::from(bounds[3] - bounds[1]);
        if iterations <= 0 || iterations > MAX_ITERATE_PIXELS {
            unicorn
                .reg_write(RegisterX86::RAX, 4)
                .map_err(|error| format!("{callback_name} iteration-budget return: {error}"))?;
            return Ok(());
        }
        unicorn.get_data_mut().pending_iterate = Some(PendingIterate {
            caller_rsp,
            return_address,
            refcon,
            pixel_function,
            source_data,
            source_rowbytes: source_rowbytes as u64,
            source_width,
            source_height,
            zero_outside_source: has_origin,
            destination_data,
            destination_rowbytes: destination_rowbytes as u64,
            left: bounds[0],
            right: bounds[2],
            bottom: bounds[3],
            x: bounds[0],
            y: bounds[1],
            origin_x,
            origin_y,
            pixel_bytes: pixel_bytes as u64,
            continuation,
            callback_name,
            callback_phase: IterateCallbackPhase::Pixel,
            abort_function,
            progress_function,
            effect_ref,
            progress_base,
            progress_final,
            top: bounds[1],
        });
        schedule_iterate_pixel(unicorn)
    })();
    if let Err(error) = result {
        unicorn.get_data_mut().callback_error = Some(error);
        let _ = unicorn.emu_stop();
    }
}

fn continue_iterate(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let callback_error = unicorn
            .reg_read(RegisterX86::RAX)
            .map_err(|error| format!("PF iterate callback return: {error}"))?;
        if callback_error as u32 != 0 {
            return finish_iterate(unicorn, callback_error as u32 as u64);
        }
        let phase = unicorn
            .get_data()
            .pending_iterate
            .as_ref()
            .ok_or_else(|| "PF iterate continuation has no pending call".to_string())?
            .callback_phase;
        match phase {
            IterateCallbackPhase::Pixel => {
                let row_completed = {
                    let pending = unicorn
                        .get_data_mut()
                        .pending_iterate
                        .as_mut()
                        .ok_or_else(|| "PF iterate continuation has no pending call".to_string())?;
                    pending.x += 1;
                    if pending.x >= pending.right {
                        pending.x = pending.left;
                        pending.y += 1;
                        true
                    } else {
                        false
                    }
                };
                let pending = unicorn
                    .get_data()
                    .pending_iterate
                    .as_ref()
                    .ok_or_else(|| "PF iterate continuation has no pending call".to_string())?;
                let progress_is_reportable = row_completed
                    && pending.progress_function != 0
                    && crate::compose_iterate_progress(
                        pending.progress_base,
                        pending.progress_final,
                        pending.y - pending.top,
                        pending.bottom - pending.top,
                    )
                    .map_err(|()| format!("{} progress span exceeds i32", pending.callback_name))?
                    .is_some();
                if progress_is_reportable {
                    schedule_iterate_progress(unicorn)
                } else if pending.y >= pending.bottom {
                    finish_iterate(unicorn, 0)
                } else if row_completed && pending.abort_function != 0 {
                    schedule_iterate_abort(unicorn)
                } else {
                    schedule_iterate_pixel(unicorn)
                }
            }
            IterateCallbackPhase::Progress => {
                let pending = unicorn
                    .get_data()
                    .pending_iterate
                    .as_ref()
                    .ok_or_else(|| "PF iterate continuation has no pending call".to_string())?;
                if pending.y < pending.bottom && pending.abort_function != 0 {
                    schedule_iterate_abort(unicorn)
                } else if pending.y >= pending.bottom {
                    finish_iterate(unicorn, 0)
                } else {
                    schedule_iterate_pixel(unicorn)
                }
            }
            IterateCallbackPhase::Abort => schedule_iterate_pixel(unicorn),
        }
    })();
    if let Err(error) = result {
        unicorn.get_data_mut().callback_error = Some(error);
        let _ = unicorn.emu_stop();
    }
}

fn install_iterate8_suites(unicorn: &mut Unicorn<'_, GuestState>) -> Result<(), GuestError> {
    for version in [1u32, 2] {
        let table =
            iterate8_suite_table_address(version as u64).expect("known PF Iterate8 Suite version");
        let mut bytes = [0u8; 40];
        bytes[..8].copy_from_slice(&HOST_ITERATE8.to_le_bytes());
        for slot in 1..5usize {
            let stub = HOST_ITERATE8_UNSUPPORTED_STUBS
                + (version as u64 - 1) * 0x100
                + (slot as u64 - 1) * STUB_STRIDE;
            uc(
                "write Iterate8 unsupported callback",
                unicorn.mem_write(stub, &[0xc3]),
            )?;
            uc(
                "install Iterate8 unsupported callback",
                unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                    let state = unicorn.get_data_mut();
                    record_named_unsupported_suite_call(
                        &mut state.unsupported_suite_calls,
                        &mut state.dropped_unsupported_suite_calls,
                        "PF Iterate8 Suite",
                        version,
                        slot,
                    );
                    let _ = unicorn.reg_write(RegisterX86::RAX, 4);
                }),
            )?;
            bytes[slot * 8..slot * 8 + 8].copy_from_slice(&stub.to_le_bytes());
        }
        uc("write PF Iterate8 Suite", unicorn.mem_write(table, &bytes))?;
    }
    Ok(())
}

fn install_typed_iterate_suites(
    unicorn: &mut Unicorn<'_, GuestState>,
) -> Result<(), GuestError> {
    for (name, table, callback) in [
        ("PF iterate16 Suite", HOST_ITERATE16_SUITE, HOST_ITERATE16),
        (
            "PF iterateFloat Suite",
            HOST_ITERATE_FLOAT_SUITE,
            HOST_ITERATE_FLOAT,
        ),
    ] {
        let expected = typed_iterate_suite_table_address(name, 1)
            .expect("known typed PF iterate Suite version");
        debug_assert_eq!(table, expected);
        uc(
            "write typed PF iterate Suite",
            unicorn.mem_write(table, &callback.to_le_bytes()),
        )?;
    }
    Ok(())
}

fn install_aegp_utility_suites(unicorn: &mut Unicorn<'_, GuestState>) -> Result<(), GuestError> {
    for version in [3u32, 7, 11, 13] {
        let (slot_count, register_slot, window_slot) =
            utility_suite_layout(version).expect("known utility suite version");
        let table =
            utility_suite_table_address(version).expect("known utility suite table address");
        let mut bytes = vec![0u8; slot_count * 8];
        for slot in 0..slot_count {
            let callback = if slot == register_slot {
                HOST_AEGP_REGISTER
            } else if slot == window_slot {
                HOST_AEGP_GET_MAIN_WINDOW
            } else {
                let stub = unsupported_suite_stub_address(version, slot)
                    .expect("known unsupported suite stub");
                uc(
                    "write AEGP unsupported callback",
                    unicorn.mem_write(stub, &[0xc3]),
                )?;
                uc(
                    "install AEGP unsupported callback",
                    unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
                        let guest = unicorn.get_data_mut();
                        let GuestState {
                            unsupported_suite_calls,
                            dropped_unsupported_suite_calls,
                            ..
                        } = guest;
                        record_unsupported_suite_call(
                            unsupported_suite_calls,
                            dropped_unsupported_suite_calls,
                            version,
                            slot,
                        );
                        let _ = unicorn.reg_write(RegisterX86::RAX, 4);
                    }),
                )?;
                stub
            };
            bytes[slot * 8..slot * 8 + 8].copy_from_slice(&callback.to_le_bytes());
        }
        uc("write AEGP Utility Suite", unicorn.mem_write(table, &bytes))?;
    }
    Ok(())
}

fn ansi_finite_unary(value: f64, operation: fn(f64) -> f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let result = operation(value);
    if result.is_finite() { result } else { 0.0 }
}

fn ansi_finite_binary(left: f64, right: f64, operation: fn(f64, f64) -> f64) -> f64 {
    if !left.is_finite() || !right.is_finite() {
        return 0.0;
    }
    let result = operation(left, right);
    if result.is_finite() { result } else { 0.0 }
}

fn ansi_atan(value: f64) -> f64 {
    ansi_finite_unary(value, f64::atan)
}

fn ansi_ceil(value: f64) -> f64 {
    ansi_finite_unary(value, f64::ceil)
}

fn ansi_cos(value: f64) -> f64 {
    ansi_finite_unary(value, f64::cos)
}

fn ansi_exp(value: f64) -> f64 {
    ansi_finite_unary(value, f64::exp)
}

fn ansi_fabs(value: f64) -> f64 {
    ansi_finite_unary(value, f64::abs)
}

fn ansi_floor(value: f64) -> f64 {
    ansi_finite_unary(value, f64::floor)
}

fn ansi_log(value: f64) -> f64 {
    if value > 0.0 {
        ansi_finite_unary(value, f64::ln)
    } else {
        0.0
    }
}

fn ansi_log10(value: f64) -> f64 {
    if value > 0.0 {
        ansi_finite_unary(value, f64::log10)
    } else {
        0.0
    }
}

fn ansi_sin(value: f64) -> f64 {
    ansi_finite_unary(value, f64::sin)
}

fn ansi_sqrt(value: f64) -> f64 {
    if value >= 0.0 {
        ansi_finite_unary(value, f64::sqrt)
    } else {
        0.0
    }
}

fn ansi_tan(value: f64) -> f64 {
    ansi_finite_unary(value, f64::tan)
}

fn ansi_asin(value: f64) -> f64 {
    if (-1.0..=1.0).contains(&value) {
        ansi_finite_unary(value, f64::asin)
    } else {
        0.0
    }
}

fn ansi_acos(value: f64) -> f64 {
    if (-1.0..=1.0).contains(&value) {
        ansi_finite_unary(value, f64::acos)
    } else {
        0.0
    }
}

fn ansi_atan2(left: f64, right: f64) -> f64 {
    ansi_finite_binary(left, right, f64::atan2)
}

fn ansi_fmod(left: f64, right: f64) -> f64 {
    if right == 0.0 {
        0.0
    } else {
        ansi_finite_binary(left, right, |value, divisor| value % divisor)
    }
}

fn ansi_hypot(left: f64, right: f64) -> f64 {
    ansi_finite_binary(left, right, f64::hypot)
}

fn ansi_pow(left: f64, right: f64) -> f64 {
    ansi_finite_binary(left, right, f64::powf)
}

fn read_ansi_xmm_f64(
    unicorn: &Unicorn<'_, GuestState>,
    register: RegisterX86,
) -> Result<f64, String> {
    let bytes = unicorn
        .reg_read_long(register)
        .map_err(|error| format!("PF ANSI XMM read: {error}"))?;
    let bits = bytes
        .get(..8)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| "PF ANSI XMM register is shorter than 8 bytes".to_string())?;
    Ok(f64::from_bits(bits))
}

fn write_ansi_xmm_f64(
    unicorn: &Unicorn<'_, GuestState>,
    register: RegisterX86,
    value: f64,
) -> Result<(), String> {
    let mut bytes = unicorn
        .reg_read_long(register)
        .map_err(|error| format!("PF ANSI XMM preserve read: {error}"))?;
    let lane = bytes
        .get_mut(..8)
        .ok_or_else(|| "PF ANSI XMM register is shorter than 8 bytes".to_string())?;
    lane.copy_from_slice(&value.to_le_bytes());
    unicorn
        .reg_write_long(register, &bytes)
        .map_err(|error| format!("PF ANSI XMM write: {error}"))
}

fn emulate_ansi_numeric_callback(unicorn: &mut Unicorn<'_, GuestState>, address: u64, _: u32) {
    let result = (|| {
        let left = read_ansi_xmm_f64(unicorn, RegisterX86::XMM0)?;
        let output = match address {
            HOST_PF_ANSI_ATAN => ansi_atan(left),
            HOST_PF_ANSI_CEIL => ansi_ceil(left),
            HOST_PF_ANSI_COS => ansi_cos(left),
            HOST_PF_ANSI_EXP => ansi_exp(left),
            HOST_PF_ANSI_FABS => ansi_fabs(left),
            HOST_PF_ANSI_FLOOR => ansi_floor(left),
            HOST_PF_ANSI_LOG => ansi_log(left),
            HOST_PF_ANSI_LOG10 => ansi_log10(left),
            HOST_PF_ANSI_SIN => ansi_sin(left),
            HOST_PF_ANSI_SQRT => ansi_sqrt(left),
            HOST_PF_ANSI_TAN => ansi_tan(left),
            HOST_PF_ANSI_ASIN => ansi_asin(left),
            HOST_PF_ANSI_ACOS => ansi_acos(left),
            HOST_PF_ANSI_ATAN2 | HOST_PF_ANSI_FMOD | HOST_PF_ANSI_HYPOT | HOST_PF_ANSI_POW => {
                let right = read_ansi_xmm_f64(unicorn, RegisterX86::XMM1)?;
                match address {
                    HOST_PF_ANSI_ATAN2 => ansi_atan2(left, right),
                    HOST_PF_ANSI_FMOD => ansi_fmod(left, right),
                    HOST_PF_ANSI_HYPOT => ansi_hypot(left, right),
                    HOST_PF_ANSI_POW => ansi_pow(left, right),
                    _ => unreachable!(),
                }
            }
            _ => return Ok(()),
        };
        write_ansi_xmm_f64(unicorn, RegisterX86::XMM0, output)
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.emu_stop();
    };
}

fn install_pf_ansi_suite_v2(unicorn: &mut Unicorn<'static, GuestState>) -> Result<(), GuestError> {
    for address in [
        HOST_PF_ANSI_ATAN,
        HOST_PF_ANSI_ATAN2,
        HOST_PF_ANSI_CEIL,
        HOST_PF_ANSI_COS,
        HOST_PF_ANSI_EXP,
        HOST_PF_ANSI_FABS,
        HOST_PF_ANSI_FLOOR,
        HOST_PF_ANSI_FMOD,
        HOST_PF_ANSI_HYPOT,
        HOST_PF_ANSI_LOG,
        HOST_PF_ANSI_LOG10,
        HOST_PF_ANSI_POW,
        HOST_PF_ANSI_SIN,
        HOST_PF_ANSI_SQRT,
        HOST_PF_ANSI_TAN,
        HOST_PF_ANSI_ASIN,
        HOST_PF_ANSI_ACOS,
    ] {
        uc(
            "write PF ANSI numeric callback",
            unicorn.mem_write(address, &[0xc3]),
        )?;
    }
    uc(
        "install PF ANSI numeric callbacks 0 through 14",
        unicorn.add_code_hook(
            HOST_PF_ANSI_ATAN,
            HOST_PF_ANSI_TAN,
            emulate_ansi_numeric_callback,
        ),
    )?;
    uc(
        "install PF ANSI numeric callbacks 17 through 18",
        unicorn.add_code_hook(
            HOST_PF_ANSI_ASIN,
            HOST_PF_ANSI_ACOS,
            emulate_ansi_numeric_callback,
        ),
    )?;
    for (operation, address) in [
        ("write PF ANSI sprintf callback", HOST_PF_ANSI_SPRINTF),
        ("write PF ANSI strcpy callback", HOST_PF_ANSI_STRCPY),
        (
            "write PF ANSI bounded strcpy callback",
            HOST_PF_ANSI_STRCPY_BOUNDED,
        ),
    ] {
        uc(operation, unicorn.mem_write(address, &[0xc3]))?;
    }
    uc(
        "install PF ANSI sprintf callback",
        unicorn.add_code_hook(
            HOST_PF_ANSI_SPRINTF,
            HOST_PF_ANSI_SPRINTF,
            |unicorn, _, _| emulate_ansi_sprintf_literal(unicorn),
        ),
    )?;
    uc(
        "install PF ANSI strcpy callback",
        unicorn.add_code_hook(HOST_PF_ANSI_STRCPY, HOST_PF_ANSI_STRCPY, |unicorn, _, _| {
            emulate_strcpy(unicorn)
        }),
    )?;
    uc(
        "install PF ANSI bounded strcpy callback",
        unicorn.add_code_hook(
            HOST_PF_ANSI_STRCPY_BOUNDED,
            HOST_PF_ANSI_STRCPY_BOUNDED,
            |unicorn, _, _| emulate_ansi_strcpy_bounded(unicorn),
        ),
    )?;

    let callbacks = [
        HOST_PF_ANSI_ATAN,
        HOST_PF_ANSI_ATAN2,
        HOST_PF_ANSI_CEIL,
        HOST_PF_ANSI_COS,
        HOST_PF_ANSI_EXP,
        HOST_PF_ANSI_FABS,
        HOST_PF_ANSI_FLOOR,
        HOST_PF_ANSI_FMOD,
        HOST_PF_ANSI_HYPOT,
        HOST_PF_ANSI_LOG,
        HOST_PF_ANSI_LOG10,
        HOST_PF_ANSI_POW,
        HOST_PF_ANSI_SIN,
        HOST_PF_ANSI_SQRT,
        HOST_PF_ANSI_TAN,
        HOST_PF_ANSI_SPRINTF,
        HOST_PF_ANSI_STRCPY,
        HOST_PF_ANSI_ASIN,
        HOST_PF_ANSI_ACOS,
        0,
        HOST_PF_ANSI_STRCPY_BOUNDED,
    ];
    let mut table = [0u8; 21 * 8];
    for (slot, callback) in callbacks.into_iter().enumerate() {
        table[slot * 8..slot * 8 + 8].copy_from_slice(&callback.to_le_bytes());
    }
    uc(
        "write PF ANSI Suite v2",
        unicorn.mem_write(HOST_PF_ANSI_SUITE_V2, &table),
    )?;
    Ok(())
}

fn finish_acquire_suite_success(unicorn: &mut Unicorn<'_, GuestState>) {
    unicorn.get_data_mut().pending_unsupported_suite = None;
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn image_executable_address(state: &GuestState, address: u64) -> bool {
    state
        .image_executable_ranges
        .iter()
        .any(|(start, end)| (*start..*end).contains(&address))
}

fn read_guest_u32(unicorn: &Unicorn<'_, GuestState>, address: u64) -> Option<u32> {
    let mut bytes = [0u8; 4];
    unicorn.mem_read(address, &mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn image_rva_address(unicorn: &Unicorn<'_, GuestState>, rva: u32, byte_count: u64) -> Option<u64> {
    let (image_start, image_end) = unicorn.get_data().image_region?;
    let address = image_start.checked_add(u64::from(rva))?;
    let end = address.checked_add(byte_count)?;
    (address >= image_start && end <= image_end).then_some(address)
}

fn is_msvc_i32_throw_info(unicorn: &Unicorn<'_, GuestState>, throw_info: u64) -> bool {
    let Some((image_start, image_end)) = unicorn.get_data().image_region else {
        return false;
    };
    let Some(throw_info_end) = throw_info.checked_add(16) else {
        return false;
    };
    if throw_info < image_start || throw_info_end > image_end {
        return false;
    }

    // Win64 MSVC exception metadata stores image-relative 32-bit pointers.
    // Require one simple, four-byte catchable type whose TypeDescriptor is
    // exactly the built-in `int` encoding. PF_Err is an A_long/int32.
    let Some(catchable_array_rva) = read_guest_u32(unicorn, throw_info + 12) else {
        return false;
    };
    let Some(catchable_array) = image_rva_address(unicorn, catchable_array_rva, 8) else {
        return false;
    };
    if read_guest_u32(unicorn, catchable_array) != Some(1) {
        return false;
    }
    let Some(catchable_type_rva) = read_guest_u32(unicorn, catchable_array + 4) else {
        return false;
    };
    let Some(catchable_type) = image_rva_address(unicorn, catchable_type_rva, 28) else {
        return false;
    };
    if read_guest_u32(unicorn, catchable_type) != Some(1)
        || read_guest_u32(unicorn, catchable_type + 20) != Some(4)
    {
        return false;
    }
    let Some(type_descriptor_rva) = read_guest_u32(unicorn, catchable_type + 4) else {
        return false;
    };
    let Some(type_name) = image_rva_address(unicorn, type_descriptor_rva, 19) else {
        return false;
    };
    let mut name = [0u8; 3];
    unicorn.mem_read(type_name + 16, &mut name).is_ok() && name == *b".H\0"
}

fn msvc_throw_type_name(unicorn: &Unicorn<'_, GuestState>, throw_info: u64) -> Option<String> {
    const MAX_TYPE_NAME_BYTES: u64 = 128;

    let (image_start, image_end) = unicorn.get_data().image_region?;
    if throw_info < image_start || throw_info.checked_add(16)? > image_end {
        return None;
    }
    let catchable_array_rva = read_guest_u32(unicorn, throw_info.checked_add(12)?)?;
    let catchable_array = image_rva_address(unicorn, catchable_array_rva, 8)?;
    let catchable_count = read_guest_u32(unicorn, catchable_array)?;
    if catchable_count == 0 || catchable_count > 32 {
        return None;
    }
    let catchable_type_rva = read_guest_u32(unicorn, catchable_array.checked_add(4)?)?;
    let catchable_type = image_rva_address(unicorn, catchable_type_rva, 28)?;
    let type_descriptor_rva = read_guest_u32(unicorn, catchable_type.checked_add(4)?)?;
    let type_descriptor = image_rva_address(unicorn, type_descriptor_rva, 17)?;
    let name_start = type_descriptor.checked_add(16)?;
    let mut bytes = Vec::new();
    for offset in 0..MAX_TYPE_NAME_BYTES {
        let address = name_start.checked_add(offset)?;
        if address >= image_end {
            return None;
        }
        let byte = unicorn.mem_read_as_vec(address, 1).ok()?[0];
        if byte == 0 {
            return if bytes.is_empty() {
                None
            } else {
                String::from_utf8(bytes).ok()
            };
        }
        if !byte.is_ascii_graphic() {
            return None;
        }
        bytes.push(byte);
    }
    None
}

fn read_msvc_x64_string(unicorn: &Unicorn<'_, GuestState>, object: u64) -> Option<String> {
    const SSO_CAPACITY: u64 = 15;
    const MAX_STRING_BYTES: u64 = 512;
    const MAX_CAPACITY: u64 = 1 << 20;

    let bytes = unicorn.mem_read_as_vec(object, 32).ok()?;
    let size = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
    let capacity = u64::from_le_bytes(bytes[24..32].try_into().ok()?);
    if size > MAX_STRING_BYTES || capacity < size || capacity > MAX_CAPACITY {
        return None;
    }
    let data = if capacity == SSO_CAPACITY {
        if size > SSO_CAPACITY {
            return None;
        }
        object
    } else {
        if capacity < SSO_CAPACITY + 1 {
            return None;
        }
        u64::from_le_bytes(bytes[..8].try_into().ok()?)
    };
    let byte_count = size.checked_add(1)?;
    let text = unicorn
        .mem_read_as_vec(data, usize::try_from(byte_count).ok()?)
        .ok()?;
    if text.last() != Some(&0) {
        return None;
    }
    let value = std::str::from_utf8(&text[..usize::try_from(size).ok()?]).ok()?;
    let mut sanitized = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            if matches!(character, '\n' | '\r' | '\t') {
                sanitized.push(' ');
            } else {
                return None;
            }
        } else {
            sanitized.push(character);
        }
    }
    Some(sanitized.trim().to_string())
}

fn cv_exception_message(
    unicorn: &Unicorn<'_, GuestState>,
    exception: u64,
    throw_type: &str,
) -> Option<String> {
    const CV_EXCEPTION_TYPE: &str = ".?AVException@cv@@";
    const OBJECT_PREFIX_BYTES: u64 = 256;
    const STRING_ALIGNMENT: u64 = 8;

    if throw_type != CV_EXCEPTION_TYPE || exception == 0 {
        return None;
    }
    for offset in (0..OBJECT_PREFIX_BYTES).step_by(STRING_ALIGNMENT as usize) {
        let object = exception.checked_add(offset)?;
        let Some(candidate) = read_msvc_x64_string(unicorn, object) else {
            continue;
        };
        if candidate.starts_with("OpenCV(") && candidate.contains("error:") {
            return Some(candidate);
        }
    }
    None
}

fn emulate_cxx_throw_exception(unicorn: &mut Unicorn<'_, GuestState>) {
    if try_emulate_selector_abort(unicorn) {
        return;
    }
    // `_CxxThrowException` is noreturn. Returning through the import stub for
    // an exception we cannot faithfully dispatch would execute compiler
    // unreachable code and can corrupt selector/session state. Keep all
    // non-selector, stale, malformed, differently typed, and unrelated throws
    // fail-closed instead.
    if unicorn.get_data().callback_error.is_none() {
        let throw_type = unicorn
            .reg_read(RegisterX86::RDX)
            .ok()
            .and_then(|throw_info| msvc_throw_type_name(unicorn, throw_info))
            .unwrap_or_else(|| "unavailable".into());
        let cv_message = unicorn
            .reg_read(RegisterX86::RCX)
            .ok()
            .and_then(|exception| cv_exception_message(unicorn, exception, &throw_type));
        let message_suffix = cv_message
            .map(|message| format!(", cv_message={message}"))
            .unwrap_or_default();
        unicorn.get_data_mut().callback_error = Some(format!(
            "guest called _CxxThrowException outside the supported selector-abort contract (msvc_type={throw_type}{message_suffix})"
        ));
    }
    let _ = unicorn.emu_stop();
}

fn try_emulate_selector_abort(unicorn: &mut Unicorn<'_, GuestState>) -> bool {
    if !unicorn.get_data().selector_dispatch_active
        || unicorn.get_data().pending_unsupported_suite.is_none()
    {
        return false;
    }
    let exception = match unicorn.reg_read(RegisterX86::RCX) {
        Ok(exception) if exception != 0 => exception,
        _ => return false,
    };
    let mut bytes = [0u8; 4];
    if unicorn.mem_read(exception, &mut bytes).is_err() {
        return false;
    }
    let error = i32::from_le_bytes(bytes);
    // PF_Err is an A_long. Keep this trap deliberately narrower than the
    // language exception ABI: only a small, positive host error thrown after
    // an unsupported AcquireSuite is a selector-level abort.
    if !(1..=0x7fff).contains(&error) {
        return false;
    }
    if !unicorn
        .reg_read(RegisterX86::RDX)
        .is_ok_and(|throw_info| is_msvc_i32_throw_info(unicorn, throw_info))
    {
        return false;
    }
    let caller_rsp = match unicorn.reg_read(RegisterX86::RSP) {
        Ok(caller_rsp) => caller_rsp,
        _ => return false,
    };
    let mut return_bytes = [0u8; 8];
    if unicorn.mem_read(caller_rsp, &mut return_bytes).is_err() {
        return false;
    }
    let return_address = u64::from_le_bytes(return_bytes);
    let pending = unicorn
        .get_data()
        .pending_unsupported_suite
        .as_ref()
        .expect("checked above");
    let close_same_frame_throw = caller_rsp == pending.caller_rsp
        && image_executable_address(unicorn.get_data(), return_address)
        && return_address
            .checked_sub(pending.return_address)
            .is_some_and(|distance| (1..=0x400).contains(&distance));
    if !close_same_frame_throw {
        return false;
    }
    let Some(suite) = unicorn.get_data_mut().pending_unsupported_suite.take() else {
        return false;
    };
    unicorn.get_data_mut().selector_abort = Some(SelectorAbortRecord { error, suite });
    let _ = unicorn.emu_stop();
    true
}

fn emulate_acquire_suite(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let name_pointer = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let version = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let mut bytes = Vec::new();
    if name_pointer != 0 {
        for offset in 0..256u64 {
            let mut byte = [0u8; 1];
            if unicorn.mem_read(name_pointer + offset, &mut byte).is_err() || byte[0] == 0 {
                break;
            }
            bytes.push(byte[0]);
        }
    }
    let name = String::from_utf8_lossy(&bytes);
    record_suite_request(
        &mut unicorn.get_data_mut().suite_requests,
        format!("{name} v{version}"),
    );
    let output = unicorn.reg_read(RegisterX86::R8).unwrap_or_default();
    if output != 0 {
        let _ = unicorn.mem_write(output, &0u64.to_le_bytes());
    }
    if name == "PF Handle Suite"
        && version == 2
        && output != 0
        && unicorn
            .mem_write(output, &HOST_HANDLE_SUITE.to_le_bytes())
            .is_ok()
    {
        finish_acquire_suite_success(unicorn);
        return;
    }
    if name == "AEGP Memory Suite"
        && version == 1
        && output != 0
        && unicorn
            .mem_write(output, &HOST_AEGP_MEMORY_SUITE.to_le_bytes())
            .is_ok()
    {
        finish_acquire_suite_success(unicorn);
        return;
    }
    if name == "PF World Suite"
        && version == 2
        && output != 0
        && unicorn
            .mem_write(output, &HOST_WORLD_SUITE.to_le_bytes())
            .is_ok()
    {
        finish_acquire_suite_success(unicorn);
        return;
    }
    if name == "PF GPU Device Suite"
        && version == 1
        && output != 0
        && unicorn
            .mem_write(output, &HOST_GPU_DEVICE_SUITE_V1.to_le_bytes())
            .is_ok()
    {
        finish_acquire_suite_success(unicorn);
        return;
    }
    if name == "PF ANSI Suite"
        && version == 2
        && output != 0
        && unicorn
            .mem_write(output, &HOST_PF_ANSI_SUITE_V2.to_le_bytes())
            .is_ok()
    {
        finish_acquire_suite_success(unicorn);
        return;
    }
    if name == "PF Iterate8 Suite"
        && output != 0
        && let Some(table) = iterate8_suite_table_address(version)
        && unicorn.mem_write(output, &table.to_le_bytes()).is_ok()
    {
        finish_acquire_suite_success(unicorn);
        return;
    }
    if output != 0
        && let Some(table) = typed_iterate_suite_table_address(name.as_ref(), version)
        && unicorn.mem_write(output, &table.to_le_bytes()).is_ok()
    {
        finish_acquire_suite_success(unicorn);
        return;
    }
    if name == "PF ColorParamSuite"
        && version == 1
        && output != 0
        && unicorn
            .mem_write(output, &HOST_COLOR_PARAM_SUITE.to_le_bytes())
            .is_ok()
    {
        finish_acquire_suite_success(unicorn);
        return;
    }
    if name == "PF PointParamSuite"
        && version == 1
        && output != 0
        && unicorn
            .mem_write(output, &HOST_POINT_PARAM_SUITE.to_le_bytes())
            .is_ok()
    {
        finish_acquire_suite_success(unicorn);
        return;
    }
    if name == "AEGP Utility Suite"
        && output != 0
        && let Some(table) = u32::try_from(version)
            .ok()
            .and_then(utility_suite_table_address)
        && unicorn.mem_write(output, &table.to_le_bytes()).is_ok()
    {
        finish_acquire_suite_success(unicorn);
        return;
    }
    if name == "AEGP Compute Cache"
        && version == 1
        && output != 0
        && unicorn
            .mem_write(output, &HOST_AEGP_COMPUTE_CACHE_SUITE_V1.to_le_bytes())
            .is_ok()
    {
        finish_acquire_suite_success(unicorn);
        return;
    }
    if unicorn.get_data().selector_dispatch_active {
        let caller_rsp = unicorn.reg_read(RegisterX86::RSP).unwrap_or_default();
        let mut return_bytes = [0u8; 8];
        let return_address = if unicorn.mem_read(caller_rsp, &mut return_bytes).is_ok() {
            u64::from_le_bytes(return_bytes)
        } else {
            0
        };
        if !image_executable_address(unicorn.get_data(), return_address) {
            let _ = unicorn.reg_write(RegisterX86::RAX, u32::MAX as u64);
            return;
        }
        unicorn.get_data_mut().pending_unsupported_suite = Some(PendingUnsupportedSuite {
            name: name.into_owned(),
            version,
            acquire_error: -1,
            caller_rsp,
            return_address,
        });
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, u32::MAX as u64);
}

fn install_aegp_compute_cache_suite(
    unicorn: &mut Unicorn<'_, GuestState>,
) -> Result<(), GuestError> {
    let mut table = [0u8; 48];
    for (slot, address) in HOST_AEGP_COMPUTE_CACHE_CALLBACKS.into_iter().enumerate() {
        uc(
            "write AEGP Compute Cache callback",
            unicorn.mem_write(address, &[0xc3]),
        )?;
        if slot == 0 {
            uc(
                "install AEGP Compute Cache class-register callback",
                unicorn.add_code_hook(address, address, emulate_aegp_compute_cache_class_register),
            )?;
        } else {
            uc(
                "install unsupported AEGP Compute Cache callback",
                unicorn.add_code_hook(address, address, move |unicorn, _, _| {
                    let state = unicorn.get_data_mut();
                    record_named_unsupported_suite_call(
                        &mut state.unsupported_suite_calls,
                        &mut state.dropped_unsupported_suite_calls,
                        "AEGP Compute Cache",
                        1,
                        slot,
                    );
                    let _ = unicorn.reg_write(RegisterX86::RAX, 1);
                }),
            )?;
        }
        table[slot * 8..slot * 8 + 8].copy_from_slice(&address.to_le_bytes());
    }
    uc(
        "write AEGP Compute Cache v1 table",
        unicorn.mem_write(HOST_AEGP_COMPUTE_CACHE_SUITE_V1, &table),
    )
}

fn emulate_aegp_compute_cache_class_register(
    unicorn: &mut Unicorn<'_, GuestState>,
    _: u64,
    _: u32,
) {
    const MAX_CLASS_ID_BYTES: u64 = 256;
    const MAX_CLASSES: usize = 64;
    const A_ERR_STRUCT: u64 = 2;
    const A_ERR_PARAMETER: u64 = 3;
    const A_ERR_ALLOC: u64 = 4;

    let class_pointer = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let callbacks_pointer = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    if class_pointer == 0 || callbacks_pointer == 0 {
        let _ = unicorn.reg_write(RegisterX86::RAX, A_ERR_PARAMETER);
        return;
    }
    let mut class_bytes = Vec::new();
    for offset in 0..MAX_CLASS_ID_BYTES {
        let mut byte = [0u8; 1];
        if unicorn.mem_read(class_pointer + offset, &mut byte).is_err() {
            let _ = unicorn.reg_write(RegisterX86::RAX, A_ERR_PARAMETER);
            return;
        }
        if byte[0] == 0 {
            break;
        }
        class_bytes.push(byte[0]);
    }
    if class_bytes.is_empty() || class_bytes.len() == MAX_CLASS_ID_BYTES as usize {
        let _ = unicorn.reg_write(RegisterX86::RAX, A_ERR_PARAMETER);
        return;
    }
    let class_id = class_bytes;
    let mut callback_bytes = [0u8; 32];
    if unicorn
        .mem_read(callbacks_pointer, &mut callback_bytes)
        .is_err()
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, A_ERR_PARAMETER);
        return;
    }
    let callbacks = std::array::from_fn(|index| {
        u64::from_le_bytes(
            callback_bytes[index * 8..index * 8 + 8]
                .try_into()
                .expect("eight-byte callback slot"),
        )
    });
    if callbacks
        .iter()
        .any(|callback| !image_executable_address(unicorn.get_data(), *callback))
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, A_ERR_STRUCT);
        return;
    }
    let classes = &mut unicorn.get_data_mut().aegp_compute_cache_classes;
    if classes.contains_key(&class_id) {
        let _ = unicorn.reg_write(RegisterX86::RAX, A_ERR_STRUCT);
    } else if classes.len() >= MAX_CLASSES {
        let _ = unicorn.reg_write(RegisterX86::RAX, A_ERR_ALLOC);
    } else {
        classes.insert(class_id, callbacks);
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    }
}

fn emulate_color_param_value(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    const PF_INVALID_INDEX: u64 = 513;
    const PF_UNRECOGNIZED_PARAM_TYPE: u64 = 514;
    const PF_BAD_CALLBACK_PARAM: u64 = 516;
    let effect_ref = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let definition = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let output = unicorn.reg_read(RegisterX86::R8).unwrap_or_default();
    if effect_ref != 1 || definition == 0 || output == 0 {
        let _ = unicorn.reg_write(RegisterX86::RAX, PF_BAD_CALLBACK_PARAM);
        return;
    }
    let result = (|| {
        let mut disk_id = [0u8; 4];
        let mut param_type = [0u8; 4];
        let mut argb = [0u8; abi::PF_PIXEL_SIZE];
        unicorn
            .mem_read(definition, &mut disk_id)
            .map_err(|error| format!("color-param disk id read: {error}"))?;
        unicorn
            .mem_read(
                definition + abi::PARAM_PARAM_TYPE_OFFSET as u64,
                &mut param_type,
            )
            .map_err(|error| format!("color-param type read: {error}"))?;
        unicorn
            .mem_read(definition + abi::PARAM_U_OFFSET as u64, &mut argb)
            .map_err(|error| format!("color-param value read: {error}"))?;
        let disk_id = i32::from_le_bytes(disk_id);
        let param_type = i32::from_le_bytes(param_type);
        let Some(source) = unicorn.get_data().params.iter().find(|parameter| {
            parameter
                .bytes
                .get(..4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(i32::from_le_bytes)
                == Some(disk_id)
        }) else {
            return Ok(PF_INVALID_INDEX);
        };
        if param_type != 5 || source.param_type != 5 {
            return Ok(PF_UNRECOGNIZED_PARAM_TYPE);
        }
        let current: [u8; 4] = source.bytes
            [abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_PIXEL_SIZE]
            .try_into()
            .expect("PF_Pixel is four bytes");
        let default: [u8; 4] = source.bytes[abi::PARAM_U_OFFSET + abi::PF_PIXEL_SIZE
            ..abi::PARAM_U_OFFSET + abi::PF_PIXEL_SIZE * 2]
            .try_into()
            .expect("PF color default is four bytes");
        if argb != current && argb != default {
            return Ok(PF_BAD_CALLBACK_PARAM);
        }
        let mut pixel_float = [0u8; abi::PF_PIXEL_FLOAT_SIZE];
        for (index, channel) in argb.into_iter().enumerate() {
            let offset = index * 4;
            pixel_float[offset..offset + 4]
                .copy_from_slice(&(f32::from(channel) / 255.0).to_le_bytes());
        }
        unicorn
            .mem_write(output, &pixel_float)
            .map_err(|error| format!("color-param output write: {error}"))?;
        Ok(0)
    })();
    match result {
        Ok(error) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, error);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.reg_write(RegisterX86::RAX, PF_BAD_CALLBACK_PARAM);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_aegp_register(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let output = unicorn.reg_read(RegisterX86::R8).unwrap_or_default();
    if output != 0 && unicorn.mem_write(output, &1i32.to_le_bytes()).is_ok() {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    } else {
        let _ = unicorn.reg_write(RegisterX86::RAX, 4);
    }
}

fn emulate_aegp_get_main_window(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let output = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    if output != 0 && unicorn.mem_write(output, &0u64.to_le_bytes()).is_ok() {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    } else {
        let _ = unicorn.reg_write(RegisterX86::RAX, 4);
    }
}

fn aegp_stack_arg(unicorn: &Unicorn<'_, GuestState>, offset: u64) -> Result<u64, String> {
    let rsp = unicorn
        .reg_read(RegisterX86::RSP)
        .map_err(|error| format!("AEGP Memory stack pointer: {error}"))?;
    let mut bytes = [0u8; 8];
    unicorn
        .mem_read(rsp + offset, &mut bytes)
        .map_err(|error| format!("AEGP Memory stack argument: {error}"))?;
    Ok(u64::from_le_bytes(bytes))
}

fn aegp_memory_error(unicorn: &mut Unicorn<'_, GuestState>) {
    let _ = unicorn.reg_write(RegisterX86::RAX, 4);
}

fn valid_aegp_memory_size(size: u64) -> Option<u64> {
    (size <= i32::MAX as u64 && size <= MAX_AEGP_MEMORY_BYTES).then_some(size)
}

fn checked_aegp_memory_live_bytes_after(
    state: &GuestState,
    replaced_size: u64,
    size: u64,
) -> Option<u64> {
    let live_bytes = state
        .aegp_memory_handles
        .values()
        .try_fold(0u64, |total, record| total.checked_add(record.size))?;
    let total = live_bytes.checked_sub(replaced_size)?.checked_add(size)?;
    (total <= MAX_AEGP_MEMORY_BYTES).then_some(total)
}

fn aegp_memory_capacity(size: u64) -> Option<u64> {
    size.max(1).checked_add(15).map(|size| size & !15)
}

fn select_aegp_memory_block(
    state: &GuestState,
    size: u64,
) -> Option<(AegpMemoryBlock, Option<usize>, Option<AegpMemoryBlock>)> {
    let capacity = aegp_memory_capacity(size)?;
    if let Some((index, block)) = state
        .aegp_memory_free
        .iter()
        .copied()
        .enumerate()
        .find(|(_, block)| block.end - block.data >= capacity)
    {
        let end = block.data.checked_add(capacity)?;
        let remainder = (end < block.end).then_some(AegpMemoryBlock {
            data: end,
            end: block.end,
        });
        return Some((
            AegpMemoryBlock {
                data: block.data,
                end,
            },
            Some(index),
            remainder,
        ));
    }
    let data = (state.next_handle_data + 15) & !15;
    let end = data.checked_add(capacity)?;
    (end <= HANDLE_DATA_END).then_some((AegpMemoryBlock { data, end }, None, None))
}

fn commit_aegp_memory_block(
    state: &mut GuestState,
    block: AegpMemoryBlock,
    free_index: Option<usize>,
    remainder: Option<AegpMemoryBlock>,
) {
    if let Some(index) = free_index {
        state.aegp_memory_free.remove(index);
        if let Some(remainder) = remainder {
            state.aegp_memory_free.push(remainder);
        }
    } else {
        state.next_handle_data = block.end;
    }
}

fn reclaim_aegp_memory_block(state: &mut GuestState, block: AegpMemoryBlock) {
    state.aegp_memory_free.push(block);
    state.aegp_memory_free.sort_by_key(|block| block.data);
    let mut merged: Vec<AegpMemoryBlock> = Vec::with_capacity(state.aegp_memory_free.len());
    for block in state.aegp_memory_free.drain(..) {
        if let Some(last) = merged.last_mut()
            && block.data <= last.end
        {
            last.end = last.end.max(block.end);
        } else {
            merged.push(block);
        }
    }
    state.aegp_memory_free = merged;
}

fn emulate_aegp_new_mem_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let plugin_id = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| error.to_string())?;
        let what = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| error.to_string())?;
        let size = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| error.to_string())?;
        let flags = unicorn
            .reg_read(RegisterX86::R9)
            .map_err(|error| error.to_string())?;
        let output = aegp_stack_arg(unicorn, 0x28)?;
        if output != 0 {
            let _ = unicorn.mem_write(output, &0u64.to_le_bytes());
        }
        let Some(size) = valid_aegp_memory_size(size) else {
            return Err("AEGP Memory NewMemHandle size is invalid".to_string());
        };
        if plugin_id != 1 || what == 0 || output == 0 || flags > u32::MAX as u64 || flags & !3 != 0
        {
            return Err("AEGP Memory NewMemHandle arguments are invalid".to_string());
        }
        let state = unicorn.get_data();
        if state.aegp_memory_handles.len() >= MAX_AEGP_MEMORY_HANDLES {
            return Err("AEGP Memory handle limit reached".to_string());
        }
        if checked_aegp_memory_live_bytes_after(state, 0, size).is_none() {
            return Err("AEGP Memory live-byte budget exceeded".to_string());
        }
        let handle = state.next_aegp_memory_handle;
        let next_handle = handle
            .checked_add(8)
            .filter(|next| *next != 0)
            .ok_or("AEGP Memory handle space exhausted")?;
        let (block, free_index, remainder) =
            select_aegp_memory_block(state, size).ok_or("AEGP Memory arena exhausted")?;
        if size != 0 {
            unicorn
                .mem_write(block.data, &vec![0u8; size as usize])
                .map_err(|error| error.to_string())?;
        }
        unicorn
            .mem_write(output, &handle.to_le_bytes())
            .map_err(|error| error.to_string())?;
        let state = unicorn.get_data_mut();
        commit_aegp_memory_block(state, block, free_index, remainder);
        state.next_aegp_memory_handle = next_handle;
        state.aegp_memory_handles.insert(
            handle,
            AegpMemoryHandle {
                data: block.data,
                size,
                locks: 0,
                end: block.end,
            },
        );
        Ok(())
    })();
    if result.is_ok() {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    } else {
        aegp_memory_error(unicorn);
    }
}

fn emulate_aegp_free_mem_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let can_free = unicorn
        .get_data()
        .aegp_memory_handles
        .get(&handle)
        .is_some_and(|record| record.locks == 0);
    if can_free {
        let state = unicorn.get_data_mut();
        let record = state
            .aegp_memory_handles
            .remove(&handle)
            .expect("checked handle exists");
        reclaim_aegp_memory_block(
            state,
            AegpMemoryBlock {
                data: record.data,
                end: record.end,
            },
        );
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    } else {
        aegp_memory_error(unicorn);
    }
}

fn emulate_aegp_lock_mem_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let output = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let data = unicorn
        .get_data()
        .aegp_memory_handles
        .get(&handle)
        .map(|record| record.data);
    if output == 0
        || data.is_none_or(|data| unicorn.mem_write(output, &data.to_le_bytes()).is_err())
    {
        aegp_memory_error(unicorn);
        return;
    }
    if let Some(record) = unicorn.get_data_mut().aegp_memory_handles.get_mut(&handle) {
        if record.locks == u32::MAX {
            aegp_memory_error(unicorn);
        } else {
            record.locks += 1;
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
    } else {
        aegp_memory_error(unicorn);
    }
}

fn emulate_aegp_unlock_mem_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let unlocked = unicorn
        .get_data_mut()
        .aegp_memory_handles
        .get_mut(&handle)
        .is_some_and(|record| {
            if record.locks == 0 {
                false
            } else {
                record.locks -= 1;
                true
            }
        });
    if unlocked {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    } else {
        aegp_memory_error(unicorn);
    }
}

fn emulate_aegp_mem_handle_size(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let output = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let size = unicorn
        .get_data()
        .aegp_memory_handles
        .get(&handle)
        .map(|record| record.size as u32);
    if let Some(size) = size
        && output != 0
        && unicorn.mem_write(output, &size.to_le_bytes()).is_ok()
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    } else {
        aegp_memory_error(unicorn);
    }
}

fn emulate_aegp_resize_mem_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let what = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| error.to_string())?;
        let size = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| error.to_string())?;
        let handle = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| error.to_string())?;
        let Some(size) = valid_aegp_memory_size(size) else {
            return Err("AEGP Memory ResizeMemHandle size is invalid".to_string());
        };
        if what == 0 {
            return Err("AEGP Memory ResizeMemHandle label is null".to_string());
        }
        let old = unicorn
            .get_data()
            .aegp_memory_handles
            .get(&handle)
            .cloned()
            .ok_or("AEGP Memory handle is stale")?;
        if old.locks != 0 {
            return Err("AEGP Memory handle is locked".to_string());
        }
        if checked_aegp_memory_live_bytes_after(unicorn.get_data(), old.size, size).is_none() {
            return Err("AEGP Memory live-byte budget exceeded".to_string());
        }
        let old_capacity = old.end - old.data;
        let new_capacity = aegp_memory_capacity(size).ok_or("AEGP Memory resize overflow")?;
        if new_capacity <= old_capacity {
            if size > old.size {
                unicorn
                    .mem_write(old.data + old.size, &vec![0u8; (size - old.size) as usize])
                    .map_err(|error| error.to_string())?;
            }
            let new_end = old.data + new_capacity;
            let state = unicorn.get_data_mut();
            let record = state
                .aegp_memory_handles
                .get_mut(&handle)
                .ok_or("AEGP Memory handle disappeared")?;
            record.size = size;
            record.end = new_end;
            if new_end < old.end {
                reclaim_aegp_memory_block(
                    state,
                    AegpMemoryBlock {
                        data: new_end,
                        end: old.end,
                    },
                );
            }
            return Ok(());
        }
        let (block, free_index, remainder) = select_aegp_memory_block(unicorn.get_data(), size)
            .ok_or("AEGP Memory arena exhausted")?;
        let mut bytes = vec![0u8; size as usize];
        let copied = old.size.min(size) as usize;
        if copied != 0 {
            unicorn
                .mem_read(old.data, &mut bytes[..copied])
                .map_err(|error| error.to_string())?;
        }
        if size != 0 {
            unicorn
                .mem_write(block.data, &bytes)
                .map_err(|error| error.to_string())?;
        }
        let state = unicorn.get_data_mut();
        commit_aegp_memory_block(state, block, free_index, remainder);
        let record = state
            .aegp_memory_handles
            .get_mut(&handle)
            .ok_or("AEGP Memory handle disappeared")?;
        record.data = block.data;
        record.size = size;
        record.end = block.end;
        reclaim_aegp_memory_block(
            state,
            AegpMemoryBlock {
                data: old.data,
                end: old.end,
            },
        );
        Ok(())
    })();
    if result.is_ok() {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    } else {
        aegp_memory_error(unicorn);
    }
}

fn emulate_point_param_value(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let definition = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let output = unicorn.reg_read(RegisterX86::R8).unwrap_or_default();
    if definition == 0 || output == 0 {
        let _ = unicorn.reg_write(RegisterX86::RAX, 4);
        return;
    }
    let mut fixed = [0u8; 8];
    if unicorn
        .mem_read(definition + abi::PARAM_U_OFFSET as u64, &mut fixed)
        .is_err()
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, 4);
        return;
    }
    let x = i32::from_le_bytes(fixed[..4].try_into().unwrap()) as f64 / 65536.0;
    let y = i32::from_le_bytes(fixed[4..].try_into().unwrap()) as f64 / 65536.0;
    let mut values = [0u8; 16];
    values[..8].copy_from_slice(&x.to_le_bytes());
    values[8..].copy_from_slice(&y.to_le_bytes());
    let result = if unicorn.mem_write(output, &values).is_ok() {
        0
    } else {
        4
    };
    let _ = unicorn.reg_write(RegisterX86::RAX, result);
}

fn emulate_checkout_param(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let index = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("checkout-param index: {error}"))?
            as usize;
        let source = if index == 0 {
            Some(unicorn.get_data().input_parameter_definition)
        } else {
            index
                .checked_sub(1)
                .and_then(|offset| unicorn.get_data().parameter_definitions.get(offset))
                .copied()
        }
        .filter(|source| *source != 0)
        .ok_or_else(|| format!("checkout-param index is outside definitions: {index}"))?;
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("checkout-param stack: {error}"))?;
        let mut destination = [0u8; 8];
        unicorn
            .mem_read(rsp + 0x30, &mut destination)
            .map_err(|error| format!("checkout-param destination pointer: {error}"))?;
        let destination = u64::from_le_bytes(destination);
        if destination == 0 {
            return Err("checkout-param destination is null".to_string());
        }
        let mut definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        unicorn
            .mem_read(source, &mut definition)
            .map_err(|error| format!("checkout-param definition read: {error}"))?;
        unicorn
            .mem_write(destination, &definition)
            .map_err(|error| format!("checkout-param definition write: {error}"))?;
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn aligned_pf_region_size(size: u64) -> Result<u64, String> {
    size.max(1)
        .checked_add(PAGE_SIZE - 1)
        .map(|end| end & !(PAGE_SIZE - 1))
        .ok_or_else(|| "PF Handle region size overflow".to_string())
}

fn validate_pf_handle_budget(
    state: &GuestState,
    size: u64,
    replacing: Option<&GuestHandle>,
) -> Result<(), String> {
    if size > MAX_PF_HANDLE_SIZE {
        return Err(format!(
            "handle allocation exceeds {} bytes: {size}",
            MAX_PF_HANDLE_SIZE
        ));
    }
    let live_bytes = state
        .handles
        .values()
        .map(|record| record.size)
        .sum::<u64>();
    let replaced_bytes = replacing.map_or(0, |record| record.size);
    let live_count = state.handles.len() - usize::from(replacing.is_some());
    if live_count >= MAX_PF_HANDLE_COUNT || live_bytes - replaced_bytes > MAX_PF_HANDLE_SIZE - size
    {
        return Err(format!(
            "PF Handle live budget exceeded: size={size}, live_bytes={live_bytes}, live_count={}",
            state.handles.len()
        ));
    }
    Ok(())
}

fn find_pf_region(
    state: &GuestState,
    mapped_size: u64,
    reserved: Option<(u64, u64)>,
) -> Result<u64, String> {
    let mut occupied = state
        .handles
        .values()
        .flat_map(|record| {
            [
                (record.handle_region, record.handle_region + PAGE_SIZE),
                (
                    record.data_region,
                    record.data_region + record.data_mapped_size,
                ),
            ]
        })
        .collect::<Vec<_>>();
    occupied.extend(state.image_region);
    occupied.extend(state.image_region);
    occupied.extend(reserved);
    occupied.sort_unstable();
    let mut candidate = PF_HANDLE_DATA_BASE;
    for (start, end) in occupied {
        if candidate
            .checked_add(mapped_size)
            .is_some_and(|candidate_end| candidate_end <= start)
        {
            return Ok(candidate);
        }
        candidate = candidate.max(end);
    }
    if candidate
        .checked_add(mapped_size)
        .is_some_and(|candidate_end| candidate_end <= PF_HANDLE_DATA_END)
    {
        Ok(candidate)
    } else {
        Err("PF Handle address space exhausted".into())
    }
}

fn emulate_new_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let size = unicorn.reg_read(RegisterX86::RCX).unwrap_or(u64::MAX);
    unicorn.get_data_mut().handle_allocations.push(size);
    let allocation = (|| {
        validate_pf_handle_budget(unicorn.get_data(), size, None)?;
        let handle_region = find_pf_region(unicorn.get_data(), PAGE_SIZE, None)?;
        unicorn
            .mem_map(handle_region, PAGE_SIZE, Prot::READ | Prot::WRITE)
            .map_err(|error| format!("PF Handle header map: {error}"))?;
        let data_mapped_size = aligned_pf_region_size(size)?;
        let data_region = match find_pf_region(
            unicorn.get_data(),
            data_mapped_size,
            Some((handle_region, handle_region + PAGE_SIZE)),
        ) {
            Ok(region) => region,
            Err(error) => {
                let _ = unicorn.mem_unmap(handle_region, PAGE_SIZE);
                return Err(error);
            }
        };
        if let Err(error) = unicorn.mem_map(data_region, data_mapped_size, Prot::READ | Prot::WRITE)
        {
            let _ = unicorn.mem_unmap(handle_region, PAGE_SIZE);
            return Err(format!("PF Handle data map: {error}"));
        }
        let handle = handle_region;
        let data = data_region;
        let state = unicorn.get_data_mut();
        state.next_pf_handle_data = state
            .next_pf_handle_data
            .max(data_region + data_mapped_size);
        state.handles.insert(
            handle,
            GuestHandle {
                data,
                size,
                locks: 0,
                pending_dispose: false,
                handle_region,
                data_region,
                data_mapped_size,
            },
        );
        Ok((handle, data, handle_region, data_region, data_mapped_size))
    })();
    match allocation {
        Ok((handle, data, handle_region, data_region, data_mapped_size)) => {
            if let Err(error) = unicorn.mem_write(handle, &data.to_le_bytes()) {
                unicorn.get_data_mut().handles.remove(&handle);
                let _ = unicorn.mem_unmap(data_region, data_mapped_size);
                let _ = unicorn.mem_unmap(handle_region, PAGE_SIZE);
                unicorn.get_data_mut().callback_error =
                    Some(format!("PF Handle header write failed: {error}"));
                let _ = unicorn.emu_stop();
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, handle);
        }
        Err(error) => {
            unicorn
                .get_data_mut()
                .handle_allocation_failures
                .push(format!("size={size}: {error}"));
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            unicorn.get_data_mut().callback_error = Some(format!(
                "PF Handle allocation failed for {size} bytes: {error}"
            ));
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_lock_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let data = unicorn
        .get_data_mut()
        .handles
        .get_mut(&handle)
        .filter(|record| !record.pending_dispose)
        .map(|record| {
            record.locks = record.locks.saturating_add(1);
            record.data
        });
    if let Some(data) = data {
        let _ = unicorn.reg_write(RegisterX86::RAX, data);
    } else {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        unicorn.get_data_mut().callback_error = Some(format!(
            "PF Handle lock received unknown handle {handle:#x}"
        ));
        let _ = unicorn.emu_stop();
    }
}

fn emulate_unlock_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let (valid, release) = {
        let state = unicorn.get_data_mut();
        match state.handles.get_mut(&handle) {
            Some(record) if record.locks != 0 => {
                record.locks -= 1;
                let release = record.locks == 0 && record.pending_dispose;
                (true, release)
            }
            _ => (false, false),
        }
    };
    if !valid {
        unicorn.get_data_mut().callback_error = Some(format!(
            "PF Handle unlock received stale or unlocked handle {handle:#x}"
        ));
        let _ = unicorn.emu_stop();
    } else if release
        && let Some(record) = unicorn.get_data_mut().handles.remove(&handle)
        && let Err(error) = unmap_pf_handle(unicorn, record)
    {
        unicorn.get_data_mut().callback_error = Some(error);
        let _ = unicorn.emu_stop();
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn unmap_pf_handle(
    unicorn: &mut Unicorn<'_, GuestState>,
    record: GuestHandle,
) -> Result<(), String> {
    let data_error = unicorn
        .mem_unmap(record.data_region, record.data_mapped_size)
        .err();
    let header_error = unicorn.mem_unmap(record.handle_region, PAGE_SIZE).err();
    match (data_error, header_error) {
        (None, None) => Ok(()),
        (Some(error), None) => Err(format!("PF Handle data unmap failed: {error}")),
        (None, Some(error)) => Err(format!("PF Handle header unmap failed: {error}")),
        (Some(data), Some(header)) => Err(format!(
            "PF Handle data/header unmap failed: data={data}, header={header}"
        )),
    }
}

fn emulate_dispose_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let disposition = {
        let state = unicorn.get_data_mut();
        match state.handles.get_mut(&handle) {
            Some(record) if record.pending_dispose => None,
            Some(record) if record.locks != 0 => {
                record.pending_dispose = true;
                Some(None)
            }
            Some(_) => Some(state.handles.remove(&handle)),
            None => None,
        }
    };
    match disposition {
        Some(Some(record)) => {
            if let Err(error) = unmap_pf_handle(unicorn, record) {
                unicorn.get_data_mut().callback_error = Some(error);
                let _ = unicorn.emu_stop();
            }
        }
        Some(None) => {}
        None => {
            unicorn.get_data_mut().callback_error = Some(format!(
                "PF Handle dispose received stale or foreign handle {handle:#x}"
            ));
            let _ = unicorn.emu_stop();
        }
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_handle_size(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    if let Some(size) = unicorn
        .get_data()
        .handles
        .get(&handle)
        .filter(|record| !record.pending_dispose)
        .map(|record| record.size)
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, size);
    } else {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        unicorn.get_data_mut().callback_error = Some(format!(
            "PF Handle size received unknown handle {handle:#x}"
        ));
        let _ = unicorn.emu_stop();
    }
}

fn emulate_resize_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let size = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("resize-handle size: {error}"))?;
        let handle_pointer = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("resize-handle pointer: {error}"))?;
        if size > MAX_PF_HANDLE_SIZE || handle_pointer == 0 {
            return Err("invalid resize-handle request".to_string());
        }
        let mut handle_bytes = [0u8; 8];
        unicorn
            .mem_read(handle_pointer, &mut handle_bytes)
            .map_err(|error| format!("resize-handle read: {error}"))?;
        let handle = u64::from_le_bytes(handle_bytes);
        let old = unicorn
            .get_data()
            .handles
            .get(&handle)
            .cloned()
            .ok_or_else(|| "resize-handle unknown handle".to_string())?;
        if old.locks != 0 {
            return Err("resize-handle locked handle".to_string());
        }
        validate_pf_handle_budget(unicorn.get_data(), size, Some(&old))?;
        let data_mapped_size = aligned_pf_region_size(size)?;
        let data_region = find_pf_region(unicorn.get_data(), data_mapped_size, None)?;
        unicorn
            .mem_map(data_region, data_mapped_size, Prot::READ | Prot::WRITE)
            .map_err(|error| format!("resize-handle memory map: {error}"))?;
        let data = data_region;
        unicorn.get_data_mut().next_pf_handle_data = unicorn
            .get_data()
            .next_pf_handle_data
            .max(data_region + data_mapped_size);
        let copied = old.size.min(size);
        let mut offset = 0u64;
        let mut buffer = vec![0u8; 1024 * 1024];
        while offset < copied {
            let length = (copied - offset).min(buffer.len() as u64) as usize;
            if let Err(error) = unicorn.mem_read(old.data + offset, &mut buffer[..length]) {
                let _ = unicorn.mem_unmap(data_region, data_mapped_size);
                return Err(format!("resize-handle old data: {error}"));
            }
            if let Err(error) = unicorn.mem_write(data + offset, &buffer[..length]) {
                let _ = unicorn.mem_unmap(data_region, data_mapped_size);
                return Err(format!("resize-handle new data: {error}"));
            }
            offset += length as u64;
        }
        if let Err(error) = unicorn.mem_write(handle, &data.to_le_bytes()) {
            let _ = unicorn.mem_unmap(data_region, data_mapped_size);
            return Err(format!("resize-handle record: {error}"));
        }
        unicorn
            .mem_unmap(old.data_region, old.data_mapped_size)
            .map_err(|error| format!("resize-handle old data unmap: {error}"))?;
        if let Some(record) = unicorn.get_data_mut().handles.get_mut(&handle) {
            record.data = data;
            record.size = size;
            record.data_region = data_region;
            record.data_mapped_size = data_mapped_size;
        }
        Ok(())
    })();
    let _ = unicorn.reg_write(RegisterX86::RAX, if result.is_ok() { 0 } else { 4 });
}

fn world_pixel_bytes(pixel_format: i32) -> Option<u64> {
    match pixel_format as u32 {
        0x6267_7261 => Some(abi::PF_PIXEL_SIZE as u64),
        0x3631_6561 => Some(abi::PF_PIXEL16_SIZE as u64),
        0x3233_6561 => Some(abi::PF_PIXEL_FLOAT_SIZE as u64),
        _ => None,
    }
}

fn read_world_i32(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
) -> Result<i32, unicorn_engine::unicorn_const::uc_error> {
    let mut bytes = [0u8; 4];
    unicorn.mem_read(address, &mut bytes)?;
    Ok(i32::from_le_bytes(bytes))
}

fn find_world_region(state: &GuestState, mapped_size: u64) -> Result<u64, String> {
    let mut occupied = state
        .worlds
        .values()
        .map(|record| {
            (
                record.data_region,
                record.data_region + record.data_mapped_size,
            )
        })
        .collect::<Vec<_>>();
    occupied.sort_unstable();
    let mut candidate = WORLD_DATA_BASE;
    for (start, end) in occupied {
        if candidate
            .checked_add(mapped_size)
            .is_some_and(|candidate_end| candidate_end <= start)
        {
            return Ok(candidate);
        }
        candidate = candidate.max(end);
    }
    if candidate
        .checked_add(mapped_size)
        .is_some_and(|candidate_end| candidate_end <= WORLD_DATA_END)
    {
        Ok(candidate)
    } else {
        Err("PF World address space exhausted".into())
    }
}

fn emulate_new_world(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    emulate_new_world_common(unicorn, false);
}

fn emulate_new_world8(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    emulate_new_world_common(unicorn, true);
}

fn emulate_new_world_common(unicorn: &mut Unicorn<'_, GuestState>, legacy_argb8: bool) {
    let flags = unicorn.reg_read(RegisterX86::R9).unwrap_or_default() as u32;
    if legacy_argb8 && flags & 0x2 != 0 {
        unicorn.get_data_mut().callback_error =
            Some("legacy new-world requested unsupported DEEP_PIXELS".into());
        let _ = unicorn.reg_write(RegisterX86::RAX, 4);
        let _ = unicorn.emu_stop();
        return;
    }
    let result = (|| {
        let width = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("new-world width: {error}"))? as u32
            as i32;
        let height = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("new-world height: {error}"))? as u32
            as i32;
        let clear = if legacy_argb8 {
            flags & 0x1 != 0
        } else {
            flags as u8 != 0
        };
        let (pixel_format, world) = if legacy_argb8 {
            (0x6267_7261u32 as i32, aegp_stack_arg(unicorn, 0x28)?)
        } else {
            (
                aegp_stack_arg(unicorn, 0x28)? as u32 as i32,
                aegp_stack_arg(unicorn, 0x30)?,
            )
        };
        let pixel_bytes = world_pixel_bytes(pixel_format)
            .ok_or_else(|| format!("unsupported PF pixel format {pixel_format:#x}"))?;
        if width <= 0 || height <= 0 || world == 0 {
            return Err("invalid new-world dimensions or destination".to_string());
        }
        let rowbytes = u64::try_from(width)
            .ok()
            .and_then(|value| value.checked_mul(pixel_bytes))
            .ok_or_else(|| "new-world rowbytes overflow".to_string())?;
        let size = rowbytes
            .checked_mul(height as u64)
            .ok_or_else(|| "new-world allocation overflow".to_string())?;
        if rowbytes > i32::MAX as u64 || size > MAX_WORLD_SIZE {
            return Err("new-world allocation exceeds worker bounds".to_string());
        }
        let state = unicorn.get_data();
        if state.worlds.contains_key(&world) {
            return Err("new-world destination is already owned".to_string());
        }
        if state.worlds.len() >= MAX_WORLD_COUNT
            || state.worlds.values().map(|record| record.size).sum::<u64>() > MAX_WORLD_SIZE - size
        {
            return Err("PF World live budget exceeded".to_string());
        }
        unicorn
            .mem_read_as_vec(world, abi::PF_LAYER_DEF_SIZE)
            .map_err(|error| format!("new-world destination: {error}"))?;
        let mapped_size = aligned_pf_region_size(size)?;
        let data = find_world_region(state, mapped_size)?;
        unicorn
            .mem_map(data, mapped_size, Prot::READ | Prot::WRITE)
            .map_err(|error| format!("new-world data map: {error}"))?;
        if !clear {
            let fill = vec![0xcd; size.min(1024 * 1024) as usize];
            let mut offset = 0;
            while offset < size {
                let length = (size - offset).min(fill.len() as u64) as usize;
                if let Err(error) = unicorn.mem_write(data + offset, &fill[..length]) {
                    let _ = unicorn.mem_unmap(data, mapped_size);
                    return Err(format!("new-world deterministic fill: {error}"));
                }
                offset += length as u64;
            }
        }
        let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        let flags = 2 | i32::from(pixel_format as u32 != 0x6267_7261);
        definition[abi::LAYER_WORLD_FLAGS_OFFSET..abi::LAYER_WORLD_FLAGS_OFFSET + 4]
            .copy_from_slice(&flags.to_le_bytes());
        definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&data.to_le_bytes());
        definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&(rowbytes as i32).to_le_bytes());
        definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&width.to_le_bytes());
        definition[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&height.to_le_bytes());
        for (index, value) in [0, 0, width, height].into_iter().enumerate() {
            let offset = abi::LAYER_EXTENT_HINT_OFFSET + index * 4;
            definition[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        definition[88..92].copy_from_slice(&1i32.to_le_bytes());
        definition[92..96].copy_from_slice(&1u32.to_le_bytes());
        if let Err(error) = unicorn.mem_write(world, &definition) {
            let _ = unicorn.mem_unmap(data, mapped_size);
            return Err(format!("new-world definition: {error}"));
        }
        unicorn.get_data_mut().worlds.insert(
            world,
            GuestWorld {
                pixel_format,
                size,
                data_region: data,
                data_mapped_size: mapped_size,
            },
        );
        Ok(())
    })();
    let _ = unicorn.reg_write(RegisterX86::RAX, if result.is_ok() { 0 } else { 4 });
}

fn emulate_dispose_world(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let world = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let result: Result<(), String> = (|| {
        let record = unicorn
            .get_data()
            .worlds
            .get(&world)
            .cloned()
            .ok_or_else(|| "dispose-world received a stale world".to_string())?;
        let old_definition = unicorn
            .mem_read_as_vec(world, abi::PF_LAYER_DEF_SIZE)
            .map_err(|error| format!("dispose-world destination: {error}"))?;
        unicorn
            .mem_write(world, &vec![0u8; abi::PF_LAYER_DEF_SIZE])
            .map_err(|error| format!("dispose-world definition: {error}"))?;
        if let Err(error) = unicorn.mem_unmap(record.data_region, record.data_mapped_size) {
            let _ = unicorn.mem_write(world, &old_definition);
            return Err(format!("dispose-world data unmap: {error}"));
        }
        unicorn.get_data_mut().worlds.remove(&world);
        Ok(())
    })();
    let _ = unicorn.reg_write(RegisterX86::RAX, if result.is_ok() { 0 } else { 4 });
}

fn emulate_get_world_pixel_format(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let world = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let output = unicorn.reg_read(RegisterX86::RDX).unwrap_or_default();
    let pixel_format = unicorn
        .get_data()
        .gpu_suite
        .worlds
        .get(&world)
        .map(|_| PF_PIXEL_FORMAT_GPU_BGRA128)
        .or_else(|| {
            unicorn
                .get_data()
                .worlds
                .get(&world)
                .map(|record| record.pixel_format)
        })
        .or_else(|| {
            (world != 0
                && (world == unicorn.get_data().smart_input_world
                    || world == unicorn.get_data().smart_output_world))
                .then_some(unicorn.get_data().render_pixel_format)
        })
        .or_else(|| {
            if world == 0 {
                return None;
            }
            let width = read_world_i32(unicorn, world + abi::LAYER_WIDTH_OFFSET as u64).ok()?;
            let rowbytes =
                read_world_i32(unicorn, world + abi::LAYER_ROWBYTES_OFFSET as u64).ok()?;
            if width <= 0 || rowbytes <= 0 || rowbytes % width != 0 {
                return None;
            }
            let bytes_per_pixel = rowbytes / width;
            match bytes_per_pixel {
                4 => Some(0x6267_7261u32 as i32),
                8 => Some(0x3631_6561u32 as i32),
                16 => Some(0x3233_6561u32 as i32),
                _ => None,
            }
        });
    let result = if let Some(pixel_format) = pixel_format
        && output != 0
        && unicorn
            .mem_write(output, &pixel_format.to_le_bytes())
            .is_ok()
    {
        0
    } else {
        4
    };
    let _ = unicorn.reg_write(RegisterX86::RAX, result);
}
