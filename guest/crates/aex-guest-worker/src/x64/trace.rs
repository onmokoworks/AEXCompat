fn push_trace_event(capture: &mut TraceCapture, mut event: TraceEvent) {
    let key = if matches!(event.kind, "selector_enter" | "selector_exit") {
        None
    } else {
        Some(TraceEventKey {
            depth: event.depth,
            kind: event.kind,
            function_rva: event.function_rva,
            pc_rva: event.pc_rva,
            target_rva: event.target_rva,
            name: event.name.clone(),
            call_kind: event.call_kind,
        })
    };
    if let Some(key) = &key
        && let Some(index) = capture.event_index.get(key).copied()
    {
        let observation_number = capture.events[index].observed_count + 1;
        if let Some(observation) = trace_observation(&event, observation_number) {
            update_exemplars(
                &mut capture.events[index].exemplars,
                capture.event_fingerprints.entry(index).or_default(),
                observation,
            );
        }
        capture.events[index].observed_count = observation_number;
        return;
    }
    if capture.events.len() >= MAX_TRACE_EVENTS {
        capture.truncated = true;
        capture.dropped_events += 1;
        return;
    }
    if let Some(key) = key {
        let index = capture.events.len();
        capture.event_index.insert(key, index);
        if let Some(observation) = trace_observation(&event, 1) {
            let mut fingerprints = HashSet::new();
            update_exemplars(&mut event.exemplars, &mut fingerprints, observation);
            capture.event_fingerprints.insert(index, fingerprints);
        }
    }
    event.sequence = capture.events.len();
    capture.events.push(event);
}

fn trace_observation(event: &TraceEvent, observation: u64) -> Option<TraceObservation> {
    if event.arguments.is_empty()
        && event.xmm_arguments.is_empty()
        && event.stack_arguments.is_empty()
        && event.return_value.is_none()
    {
        return None;
    }
    let mut hasher = Sha256::new();
    for argument in &event.arguments {
        hasher.update(argument.register.as_bytes());
        hasher.update(argument.value.raw.to_le_bytes());
    }
    for xmm in &event.xmm_arguments {
        hasher.update(xmm.register.as_bytes());
        hasher.update(xmm.raw_hex.as_bytes());
    }
    for argument in &event.stack_arguments {
        hasher.update(argument.index.to_le_bytes());
        hasher.update(argument.value.raw.to_le_bytes());
    }
    if let Some(returned) = &event.return_value {
        hasher.update(returned.rax.raw.to_le_bytes());
        hasher.update(returned.xmm0.raw_hex.as_bytes());
    }
    let digest = hasher.finalize();
    let fingerprint = u64::from_be_bytes(digest[..8].try_into().expect("eight-byte digest"));
    Some(TraceObservation {
        observation,
        fingerprint: format!("{fingerprint:016x}"),
        call_id: event.call_id,
        arguments: event.arguments.clone(),
        xmm_arguments: event.xmm_arguments.clone(),
        stack_arguments: event.stack_arguments.clone(),
        return_value: event.return_value.clone(),
    })
}

fn update_exemplars(
    exemplars: &mut TraceExemplars,
    fingerprints: &mut HashSet<u64>,
    observation: TraceObservation,
) {
    if exemplars.first.len() < TRACE_FIRST_SAMPLES {
        exemplars.first.push(observation.clone());
    }
    exemplars.last.push(observation.clone());
    if exemplars.last.len() > TRACE_LAST_SAMPLES {
        exemplars.last.remove(0);
    }
    let fingerprint = u64::from_str_radix(&observation.fingerprint, 16)
        .expect("trace fingerprint is hexadecimal");
    if fingerprints.contains(&fingerprint) {
        // Already represented by the bounded set.
    } else if fingerprints.len() < TRACE_DISTINCT_FINGERPRINTS {
        fingerprints.insert(fingerprint);
        exemplars.distinct_fingerprints += 1;
        if exemplars.distinct.len() < TRACE_DISTINCT_SAMPLES {
            exemplars.distinct.push(observation.clone());
        } else {
            exemplars.dropped_distinct_fingerprints += 1;
        }
    } else {
        exemplars.fingerprint_tracking_truncated = true;
        exemplars.untracked_fingerprint_observations += 1;
    }
    update_numeric_ranges(exemplars, &observation);
}

fn update_numeric_ranges(exemplars: &mut TraceExemplars, observation: &TraceObservation) {
    let mut values = Vec::new();
    values.extend(
        observation
            .arguments
            .iter()
            .map(|argument| (argument.register.to_string(), argument.value.raw as f64)),
    );
    values.extend(observation.stack_arguments.iter().map(|argument| {
        (
            format!("stack_arg_{}", argument.index),
            argument.value.raw as f64,
        )
    }));
    for xmm in &observation.xmm_arguments {
        for (lane, value) in xmm.f32_lanes.iter().enumerate() {
            if let Some(value) = value {
                values.push((format!("{}.f32[{lane}]", xmm.register), f64::from(*value)));
            }
        }
        for (lane, value) in xmm.f64_lanes.iter().enumerate() {
            if let Some(value) = value {
                values.push((format!("{}.f64[{lane}]", xmm.register), *value));
            }
        }
    }
    if let Some(returned) = &observation.return_value {
        values.push(("rax".into(), returned.rax.raw as f64));
        for (lane, value) in returned.xmm0.f32_lanes.iter().enumerate() {
            if let Some(value) = value {
                values.push((format!("xmm0.f32[{lane}]"), f64::from(*value)));
            }
        }
        for (lane, value) in returned.xmm0.f64_lanes.iter().enumerate() {
            if let Some(value) = value {
                values.push((format!("xmm0.f64[{lane}]"), *value));
            }
        }
    }
    for (field, value) in values {
        if let Some(range) = exemplars
            .numeric_ranges
            .iter_mut()
            .find(|range| range.field == field)
        {
            range.minimum = range.minimum.min(value);
            range.maximum = range.maximum.max(value);
        } else {
            exemplars.numeric_ranges.push(TraceNumericRange {
                field,
                minimum: value,
                maximum: value,
            });
        }
    }
}

fn selected_watch_occurrence(capture: &mut TraceCapture, spec: &TraceWatchSpec) -> bool {
    let count = capture
        .watch_occurrence_counts
        .entry(spec.id.clone())
        .or_default();
    *count += 1;
    spec.occurrence
        .is_none_or(|occurrence| occurrence == *count)
}

fn select_function_watches(capture: &mut TraceCapture, function_rva: u64) -> Vec<TraceWatchSpec> {
    let matches = capture
        .watch_specs
        .iter()
        .filter(|spec| spec.function_rva == Some(function_rva))
        .cloned()
        .collect::<Vec<_>>();
    matches
        .into_iter()
        .filter(|spec| selected_watch_occurrence(capture, spec))
        .collect()
}

fn select_call_watches(
    capture: &mut TraceCapture,
    target_rva: Option<u64>,
    pc_rva: Option<u64>,
) -> Vec<TraceWatchSpec> {
    let matches = capture
        .watch_specs
        .iter()
        .filter(|spec| {
            spec.function_rva.is_some_and(|rva| Some(rva) == target_rva)
                || spec.instruction_rva.is_some_and(|rva| Some(rva) == pc_rva)
        })
        .cloned()
        .collect::<Vec<_>>();
    matches
        .into_iter()
        .filter(|spec| selected_watch_occurrence(capture, spec))
        .collect()
}

fn trace_instruction(
    unicorn: &mut Unicorn<'_, GuestState>,
    address: u64,
    size: u32,
    image_base: u64,
    image_end: u64,
) {
    let rsp = unicorn.reg_read(RegisterX86::RSP).unwrap_or(0);
    let entry_rva = address.saturating_sub(image_base);
    let entry_watches = if unicorn.get_data().trace.as_ref().is_some_and(|capture| {
        address == image_base + capture.entry_rva
            && !capture
                .selector_watches
                .iter()
                .any(|pending| pending.function_rva == Some(entry_rva))
    }) {
        unicorn
            .get_data_mut()
            .trace
            .as_mut()
            .map(|capture| select_function_watches(capture, entry_rva))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if !entry_watches.is_empty() {
        let pending = entry_watches
            .into_iter()
            .map(|spec| {
                let watch_address = trace_register_value(unicorn, spec.register, 0x28).unwrap_or(0);
                PendingTraceWatch {
                    spec_id: spec.id,
                    register: spec.register,
                    call_id: None,
                    function_rva: Some(entry_rva),
                    pc_rva: Some(entry_rva),
                    address: watch_address,
                    before: trace_memory_snapshot(
                        unicorn,
                        watch_address,
                        spec.size,
                        image_base,
                        image_end,
                    ),
                    image_coordinate: spec.image_coordinate,
                    image_row_offset: spec.image_row_offset,
                    image_format: spec.image_format,
                }
            })
            .collect::<Vec<_>>();
        if let Some(capture) = unicorn.get_data_mut().trace.as_mut() {
            capture.selector_watches.extend(pending);
        }
    }
    let return_value = trace_return_value(unicorn, image_base, image_end);
    loop {
        let should_infer_return = unicorn
            .get_data()
            .trace
            .as_ref()
            .and_then(|capture| capture.call_rsp_stack.last())
            .is_some_and(|caller_rsp| rsp >= *caller_rsp);
        if !should_infer_return {
            break;
        }
        let pending = unicorn
            .get_data()
            .trace
            .as_ref()
            .and_then(|capture| capture.watch_stack.last().cloned())
            .unwrap_or_default();
        let completed = complete_trace_watches(unicorn, pending, image_base, image_end);
        let Some(capture) = unicorn.get_data_mut().trace.as_mut() else {
            break;
        };
        capture.call_rsp_stack.pop();
        let call_id = capture.call_id_stack.pop();
        capture.watch_stack.pop();
        append_trace_witnesses(capture, completed);
        let target = capture.return_stack.pop();
        let function_rva = capture.function_stack.pop().flatten();
        push_trace_event(
            capture,
            TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth: capture.return_stack.len(),
                kind: "guest_return",
                call_id,
                function_rva,
                pc_rva: None,
                target_rva: target
                    .filter(|target| (image_base..image_end).contains(target))
                    .map(|target| target - image_base),
                name: Some("inferred_from_stack".into()),
                arguments: Vec::new(),
                xmm_arguments: Vec::new(),
                stack_arguments: Vec::new(),
                return_value: Some(return_value.clone()),
                exemplars: TraceExemplars::default(),
                call_kind: None,
                instruction_bytes: None,
            },
        );
    }
    let arguments = trace_arguments(unicorn, image_base, image_end);
    let xmm_arguments = trace_xmm_arguments(unicorn);
    let entry_stack_arguments = trace_stack_arguments(unicorn, rsp, 0x28, image_base, image_end);
    let call_stack_arguments = trace_stack_arguments(unicorn, rsp, 0x20, image_base, image_end);
    let label = unicorn.get_data().trace_labels.get(&address).cloned();
    if let Some(label) = label
        && let Some(capture) = unicorn.get_data_mut().trace.as_mut()
    {
        push_trace_event(
            capture,
            TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth: capture.return_stack.len(),
                kind: match label.kind {
                    TraceLabelKind::Import => "import_call",
                    TraceLabelKind::HostCallback => "host_callback",
                },
                call_id: capture.call_id_stack.last().copied(),
                function_rva: capture
                    .function_stack
                    .iter()
                    .rev()
                    .flatten()
                    .copied()
                    .next(),
                pc_rva: None,
                target_rva: None,
                name: Some(label.name),
                arguments: arguments.clone(),
                xmm_arguments: xmm_arguments.clone(),
                stack_arguments: entry_stack_arguments,
                return_value: None,
                exemplars: TraceExemplars::default(),
                call_kind: None,
                instruction_bytes: None,
            },
        );
    }

    let mut bytes = vec![0u8; size.clamp(1, 15) as usize];
    if unicorn.mem_read(address, &mut bytes).is_err() {
        return;
    }
    let instruction = Decoder::with_ip(64, &bytes, address, DecoderOptions::NONE).decode();
    if instruction.is_invalid() {
        return;
    }
    let mnemonic = instruction.mnemonic();
    if instruction.is_ip_rel_memory_operand() && !matches!(mnemonic, Mnemonic::Call | Mnemonic::Jmp)
    {
        let memory_address = instruction.ip_rel_memory_address();
        let mut raw = [0; 8];
        let value = unicorn
            .mem_read(memory_address, &mut raw)
            .is_ok()
            .then(|| u64::from_le_bytes(raw));
        if let Some(capture) = unicorn.get_data_mut().trace.as_mut() {
            push_trace_event(
                capture,
                TraceEvent {
                    sequence: 0,
                    observed_count: 1,
                    depth: capture.return_stack.len(),
                    kind: "rip_constant",
                    call_id: capture.call_id_stack.last().copied(),
                    function_rva: capture
                        .function_stack
                        .iter()
                        .rev()
                        .flatten()
                        .copied()
                        .next(),
                    pc_rva: Some(address.saturating_sub(image_base)),
                    target_rva: (image_base..image_end)
                        .contains(&memory_address)
                        .then(|| memory_address - image_base),
                    name: value
                        .map(|value| format!("address={memory_address:#x} value={value:#x}")),
                    arguments: Vec::new(),
                    xmm_arguments: Vec::new(),
                    stack_arguments: Vec::new(),
                    return_value: None,
                    exemplars: TraceExemplars::default(),
                    call_kind: None,
                    instruction_bytes: Some(bytes_to_hex(
                        &bytes[..instruction.len().min(bytes.len())],
                    )),
                },
            );
        }
    }
    if mnemonic == Mnemonic::Call {
        let instruction_bytes = bytes_to_hex(&bytes[..instruction.len().min(bytes.len())]);
        let runtime_target = capture_runtime_target(
            unicorn,
            &instruction,
            address,
            instruction_bytes.clone(),
            image_base,
            image_end,
        );
        let target = runtime_target.effective_target;
        unicorn.get_data_mut().latest_runtime_target = Some(runtime_target);
        let target_rva = (image_base..image_end)
            .contains(&target.unwrap_or(0))
            .then(|| target.expect("range-checked target") - image_base);
        let return_address = address.saturating_add(instruction.len() as u64);
        let pc_rva = (image_base..image_end)
            .contains(&address)
            .then(|| address - image_base);
        let matching_watches = unicorn
            .get_data_mut()
            .trace
            .as_mut()
            .map(|capture| select_call_watches(capture, target_rva, pc_rva))
            .unwrap_or_default();
        if let Some(capture) = unicorn.get_data_mut().trace.as_mut() {
            let depth = capture.return_stack.len();
            let call_id = capture.next_call_id;
            capture.next_call_id += 1;
            let function_rva = capture
                .function_stack
                .iter()
                .rev()
                .flatten()
                .copied()
                .next();
            push_trace_event(
                capture,
                TraceEvent {
                    sequence: 0,
                    observed_count: 1,
                    depth,
                    kind: "guest_call",
                    call_id: Some(call_id),
                    function_rva,
                    pc_rva,
                    target_rva,
                    name: None,
                    arguments,
                    xmm_arguments,
                    stack_arguments: call_stack_arguments,
                    return_value: None,
                    exemplars: TraceExemplars::default(),
                    call_kind: Some(
                        if matches!(
                            instruction.op0_kind(),
                            OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64
                        ) {
                            "direct"
                        } else {
                            "indirect"
                        },
                    ),
                    instruction_bytes: Some(instruction_bytes),
                },
            );
            capture.return_stack.push(return_address);
            capture.function_stack.push(target_rva);
            if let Some(target_rva) = target_rva {
                capture.known_function_entries.insert(target_rva);
            }
            capture.call_rsp_stack.push(rsp);
            capture.call_id_stack.push(call_id);
        }
        let pending = matching_watches
            .into_iter()
            .map(|spec| {
                let address = trace_register_value(unicorn, spec.register, 0x20).unwrap_or(0);
                PendingTraceWatch {
                    spec_id: spec.id,
                    register: spec.register,
                    call_id: unicorn
                        .get_data()
                        .trace
                        .as_ref()
                        .and_then(|capture| capture.call_id_stack.last().copied()),
                    function_rva: target_rva,
                    pc_rva,
                    address,
                    before: trace_memory_snapshot(
                        unicorn, address, spec.size, image_base, image_end,
                    ),
                    image_coordinate: spec.image_coordinate,
                    image_row_offset: spec.image_row_offset,
                    image_format: spec.image_format,
                }
            })
            .collect::<Vec<_>>();
        if let Some(capture) = unicorn.get_data_mut().trace.as_mut() {
            capture.watch_stack.push(pending);
        }
    } else if mnemonic == Mnemonic::Jmp {
        let instruction_bytes = bytes_to_hex(&bytes[..instruction.len().min(bytes.len())]);
        let runtime_target = capture_runtime_target(
            unicorn,
            &instruction,
            address,
            instruction_bytes.clone(),
            image_base,
            image_end,
        );
        let target = runtime_target.effective_target;
        let target_rva = target
            .filter(|target| (image_base..image_end).contains(target))
            .map(|target| target - image_base);
        let pc_rva = (image_base..image_end)
            .contains(&address)
            .then(|| address - image_base);
        let is_tail_target = target.is_some_and(|_| target_rva.is_none())
            || unicorn.get_data().trace.as_ref().is_some_and(|capture| {
                target_rva.is_some_and(|rva| {
                    capture.known_function_entries.contains(&rva)
                        || capture
                            .watch_specs
                            .iter()
                            .any(|spec| spec.function_rva == Some(rva))
                })
            });
        let mut runtime_target = runtime_target;
        runtime_target.transfer_kind = if is_tail_target { "tail_call" } else { "jump" };
        unicorn.get_data_mut().latest_runtime_target = Some(runtime_target);
        let matching_watches = unicorn
            .get_data_mut()
            .trace
            .as_mut()
            .map(|capture| select_call_watches(capture, target_rva, pc_rva))
            .unwrap_or_default();
        let tail_stack_arguments = trace_stack_arguments(unicorn, rsp, 0x28, image_base, image_end);
        if is_tail_target && let Some(capture) = unicorn.get_data_mut().trace.as_mut() {
            push_trace_event(
                capture,
                TraceEvent {
                    sequence: 0,
                    observed_count: 1,
                    depth: capture.return_stack.len(),
                    kind: "tail_call",
                    call_id: capture.call_id_stack.last().copied(),
                    function_rva: capture
                        .function_stack
                        .iter()
                        .rev()
                        .flatten()
                        .copied()
                        .next(),
                    pc_rva,
                    target_rva,
                    name: target.map(|target| format!("runtime_target={target:#x}")),
                    arguments,
                    xmm_arguments,
                    stack_arguments: tail_stack_arguments,
                    return_value: None,
                    exemplars: TraceExemplars::default(),
                    call_kind: Some("runtime_jmp"),
                    instruction_bytes: Some(instruction_bytes),
                },
            );
            if let Some(target_rva) = target_rva
                && let Some(active_function) = capture.function_stack.last_mut()
            {
                *active_function = Some(target_rva);
            }
        }
        if is_tail_target {
            let pending = matching_watches
                .into_iter()
                .map(|spec| {
                    let address = trace_register_value(unicorn, spec.register, 0x28).unwrap_or(0);
                    PendingTraceWatch {
                        spec_id: spec.id,
                        register: spec.register,
                        call_id: unicorn
                            .get_data()
                            .trace
                            .as_ref()
                            .and_then(|capture| capture.call_id_stack.last().copied()),
                        function_rva: target_rva,
                        pc_rva,
                        address,
                        before: trace_memory_snapshot(
                            unicorn, address, spec.size, image_base, image_end,
                        ),
                        image_coordinate: spec.image_coordinate,
                        image_row_offset: spec.image_row_offset,
                        image_format: spec.image_format,
                    }
                })
                .collect::<Vec<_>>();
            if let Some(capture) = unicorn.get_data_mut().trace.as_mut() {
                if let Some(active_watches) = capture.watch_stack.last_mut() {
                    active_watches.extend(pending);
                } else {
                    capture.watch_stack.push(pending);
                }
            }
        }
    } else if matches!(
        mnemonic,
        Mnemonic::Ret | Mnemonic::Retf | Mnemonic::Iret | Mnemonic::Iretd | Mnemonic::Iretq
    ) {
        let pending = unicorn
            .get_data()
            .trace
            .as_ref()
            .and_then(|capture| capture.watch_stack.last().cloned())
            .unwrap_or_default();
        let completed = complete_trace_watches(unicorn, pending, image_base, image_end);
        let Some(capture) = unicorn.get_data_mut().trace.as_mut() else {
            return;
        };
        let depth = capture.return_stack.len().saturating_sub(1);
        let target = capture.return_stack.pop();
        let function_rva = capture.function_stack.pop().flatten();
        capture.call_rsp_stack.pop();
        let call_id = capture.call_id_stack.pop();
        capture.watch_stack.pop();
        append_trace_witnesses(capture, completed);
        push_trace_event(
            capture,
            TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth,
                kind: "guest_return",
                call_id,
                function_rva,
                pc_rva: (image_base..image_end)
                    .contains(&address)
                    .then(|| address - image_base),
                target_rva: target
                    .filter(|target| (image_base..image_end).contains(target))
                    .map(|target| target - image_base),
                name: None,
                arguments: Vec::new(),
                xmm_arguments: Vec::new(),
                stack_arguments: Vec::new(),
                return_value: Some(return_value),
                exemplars: TraceExemplars::default(),
                call_kind: None,
                instruction_bytes: Some(bytes_to_hex(&bytes[..instruction.len().min(bytes.len())])),
            },
        );
    }
}

fn trace_arguments(
    unicorn: &mut Unicorn<'_, GuestState>,
    image_base: u64,
    image_end: u64,
) -> Vec<TraceArgument> {
    [
        ("rcx", RegisterX86::RCX),
        ("rdx", RegisterX86::RDX),
        ("r8", RegisterX86::R8),
        ("r9", RegisterX86::R9),
    ]
    .into_iter()
    .filter_map(|(register, id)| {
        unicorn.reg_read(id).ok().map(|raw| TraceArgument {
            register,
            value: classify_trace_value(raw, image_base, image_end),
        })
    })
    .collect()
}

fn trace_xmm_arguments(unicorn: &Unicorn<'_, GuestState>) -> Vec<TraceXmmValue> {
    [
        ("xmm0", RegisterX86::XMM0),
        ("xmm1", RegisterX86::XMM1),
        ("xmm2", RegisterX86::XMM2),
        ("xmm3", RegisterX86::XMM3),
    ]
    .into_iter()
    .filter_map(|(register, id)| read_xmm(unicorn, register, id))
    .collect()
}

fn read_xmm(
    unicorn: &Unicorn<'_, GuestState>,
    register: &'static str,
    id: RegisterX86,
) -> Option<TraceXmmValue> {
    let bytes = unicorn.reg_read_long(id).ok()?;
    let f32_lanes = bytes
        .chunks_exact(4)
        .map(|chunk| {
            let value = f32::from_le_bytes(chunk.try_into().expect("four-byte XMM lane"));
            value.is_finite().then_some(value)
        })
        .collect();
    let f64_lanes = bytes
        .chunks_exact(8)
        .map(|chunk| {
            let value = f64::from_le_bytes(chunk.try_into().expect("eight-byte XMM lane"));
            value.is_finite().then_some(value)
        })
        .collect();
    Some(TraceXmmValue {
        register,
        raw_hex: bytes_to_hex(&bytes),
        f32_lanes,
        f64_lanes,
    })
}

fn trace_stack_arguments(
    unicorn: &Unicorn<'_, GuestState>,
    rsp: u64,
    first_offset: u64,
    image_base: u64,
    image_end: u64,
) -> Vec<TraceStackArgument> {
    (0..TRACE_STACK_ARGUMENTS)
        .filter_map(|offset| {
            let stack_offset = first_offset + (offset * 8) as u64;
            let raw = if first_offset == 0x28 {
                read_win64_import_argument(unicorn, offset + 4).ok()?
            } else {
                let mut bytes = [0u8; 8];
                unicorn
                    .mem_read(rsp.checked_add(stack_offset)?, &mut bytes)
                    .ok()?;
                u64::from_le_bytes(bytes)
            };
            Some(TraceStackArgument {
                index: offset + 5,
                stack_offset,
                value: classify_trace_value(raw, image_base, image_end),
            })
        })
        .collect()
}

fn trace_return_value(
    unicorn: &Unicorn<'_, GuestState>,
    image_base: u64,
    image_end: u64,
) -> TraceReturnValue {
    let rax = unicorn.reg_read(RegisterX86::RAX).unwrap_or(0);
    let xmm0 = read_xmm(unicorn, "xmm0", RegisterX86::XMM0).unwrap_or(TraceXmmValue {
        register: "xmm0",
        raw_hex: String::new(),
        f32_lanes: Vec::new(),
        f64_lanes: Vec::new(),
    });
    TraceReturnValue {
        rax: classify_trace_value(rax, image_base, image_end),
        xmm0,
    }
}

fn trace_register_value(
    unicorn: &Unicorn<'_, GuestState>,
    register: &str,
    stack_argument_offset: u64,
) -> Option<u64> {
    if let Some(index) = register
        .strip_prefix("stack")
        .and_then(|index| index.parse::<u64>().ok())
        && (5..=8).contains(&index)
    {
        let rsp = unicorn.reg_read(RegisterX86::RSP).ok()?;
        let mut bytes = [0; 8];
        unicorn
            .mem_read(rsp + stack_argument_offset + (index - 5) * 8, &mut bytes)
            .ok()?;
        return Some(u64::from_le_bytes(bytes));
    }
    let register = match register {
        "rcx" => RegisterX86::RCX,
        "rdx" => RegisterX86::RDX,
        "r8" => RegisterX86::R8,
        "r9" => RegisterX86::R9,
        "rax" => RegisterX86::RAX,
        _ => return None,
    };
    unicorn.reg_read(register).ok()
}

fn trace_memory_snapshot(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    size: usize,
    image_base: u64,
    image_end: u64,
) -> TraceMemorySnapshot {
    let classification = classify_trace_value(address, image_base, image_end).classification;
    if size == 0 || size > MAX_TRACE_WATCH_BYTES {
        return TraceMemorySnapshot {
            status: "oversized",
            address,
            size,
            classification,
            sha256: None,
            hex: None,
            u8_values: Vec::new(),
            u16_values: Vec::new(),
            u32_values: Vec::new(),
            u64_values: Vec::new(),
            f32_values: Vec::new(),
            f64_values: Vec::new(),
            pointer_chain: Vec::new(),
        };
    }
    let mut bytes = vec![0; size];
    if unicorn.mem_read(address, &mut bytes).is_err() {
        return TraceMemorySnapshot {
            status: "unmapped",
            address,
            size,
            classification,
            sha256: None,
            hex: None,
            u8_values: Vec::new(),
            u16_values: Vec::new(),
            u32_values: Vec::new(),
            u64_values: Vec::new(),
            f32_values: Vec::new(),
            f64_values: Vec::new(),
            pointer_chain: Vec::new(),
        };
    }
    let u16_values = bytes
        .chunks_exact(2)
        .take(32)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    let f32_values = bytes
        .chunks_exact(4)
        .take(16)
        .map(|chunk| {
            let value = f32::from_le_bytes(chunk.try_into().expect("four-byte chunk"));
            value.is_finite().then_some(value)
        })
        .collect();
    let u32_values = bytes
        .chunks_exact(4)
        .take(16)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("four-byte chunk")))
        .collect();
    let u64_values = bytes
        .chunks_exact(8)
        .take(8)
        .map(|chunk| u64::from_le_bytes(chunk.try_into().expect("eight-byte chunk")))
        .collect();
    let f64_values = bytes
        .chunks_exact(8)
        .take(8)
        .map(|chunk| {
            let value = f64::from_le_bytes(chunk.try_into().expect("eight-byte chunk"));
            value.is_finite().then_some(value)
        })
        .collect();
    let mut pointer_chain = Vec::new();
    let mut pointer = address;
    for _ in 0..3 {
        if size < 8 {
            break;
        }
        let mut raw = [0; 8];
        if unicorn.mem_read(pointer, &mut raw).is_err() {
            break;
        }
        let next = u64::from_le_bytes(raw);
        if next == 0
            || pointer_chain
                .iter()
                .any(|hop: &TracePointerHop| hop.address == next)
        {
            break;
        }
        let next_value = classify_trace_value(next, image_base, image_end);
        if next_value.classification == "scalar" {
            break;
        }
        pointer_chain.push(TracePointerHop {
            address: next,
            classification: next_value.classification,
        });
        pointer = next;
    }
    TraceMemorySnapshot {
        status: "captured",
        address,
        size,
        classification,
        sha256: Some(format!("{:x}", Sha256::digest(&bytes))),
        hex: Some(bytes_to_hex(&bytes)),
        u8_values: bytes.iter().copied().take(64).collect(),
        u16_values,
        u32_values,
        u64_values,
        f32_values,
        f64_values,
        pointer_chain,
    }
}

fn trace_changed_ranges(
    before: &TraceMemorySnapshot,
    after: &TraceMemorySnapshot,
) -> Vec<TraceChangedRange> {
    let (Some(before), Some(after)) = (&before.hex, &after.hex) else {
        return Vec::new();
    };
    let before = before.as_bytes().chunks_exact(2);
    let after = after.as_bytes().chunks_exact(2);
    let changed = before
        .zip(after)
        .enumerate()
        .filter_map(|(index, (left, right))| (left != right).then_some(index))
        .collect::<Vec<_>>();
    let mut ranges = Vec::<TraceChangedRange>::new();
    for offset in changed {
        if let Some(last) = ranges.last_mut()
            && last.offset + last.size == offset
        {
            last.size += 1;
        } else {
            ranges.push(TraceChangedRange { offset, size: 1 });
        }
    }
    ranges
}

fn complete_trace_watches(
    unicorn: &Unicorn<'_, GuestState>,
    pending: Vec<PendingTraceWatch>,
    image_base: u64,
    image_end: u64,
) -> Vec<TraceMemoryWitness> {
    pending
        .into_iter()
        .map(|pending| {
            let after = trace_memory_snapshot(
                unicorn,
                pending.address,
                pending.before.size,
                image_base,
                image_end,
            );
            TraceMemoryWitness {
                watch_id: pending.spec_id,
                call_id: pending.call_id,
                function_rva: pending.function_rva,
                pc_rva: pending.pc_rva,
                register: pending.register,
                image_coordinate: pending.image_coordinate,
                image_row_offset: pending.image_row_offset,
                image_format: pending.image_format,
                changed_ranges: trace_changed_ranges(&pending.before, &after),
                before: pending.before,
                after,
            }
        })
        .collect()
}

fn append_trace_witnesses(
    capture: &mut TraceCapture,
    witnesses: impl IntoIterator<Item = TraceMemoryWitness>,
) {
    for witness in witnesses {
        if capture.witnesses.len() < MAX_TRACE_WITNESSES {
            capture.witnesses.push(witness);
        } else {
            capture.dropped_witnesses += 1;
        }
    }
}

#[derive(Clone, Debug)]
struct RuntimeTargetResolution {
    target: Option<u64>,
    operand_kind: &'static str,
    register: Option<TraceTargetRegister>,
    memory: Option<TraceTargetMemory>,
}

fn iced_register_name(register: Register) -> String {
    format!("{register:?}").to_ascii_lowercase()
}

fn resolve_runtime_target_provenance(
    unicorn: &Unicorn<'_, GuestState>,
    instruction: &iced_x86::Instruction,
) -> RuntimeTargetResolution {
    match instruction.op0_kind() {
        OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64 => {
            RuntimeTargetResolution {
                target: Some(instruction.near_branch_target()),
                operand_kind: "direct",
                register: None,
                memory: None,
            }
        }
        OpKind::Register => {
            let register = instruction.op0_register();
            let value = read_iced_register(unicorn, register);
            RuntimeTargetResolution {
                target: value,
                operand_kind: "register",
                register: value.map(|value| TraceTargetRegister {
                    name: iced_register_name(register),
                    value,
                }),
                memory: None,
            }
        }
        OpKind::Memory => {
            let base_register = instruction.memory_base();
            let index_register = instruction.memory_index();
            let base_value = read_iced_register(unicorn, base_register);
            let index_value = read_iced_register(unicorn, index_register);
            let address = if instruction.is_ip_rel_memory_operand() {
                Some(instruction.ip_rel_memory_address())
            } else {
                base_value.zip(index_value).map(|(base, index)| {
                    base.wrapping_add(index.wrapping_mul(instruction.memory_index_scale() as u64))
                        .wrapping_add(instruction.memory_displacement64())
                })
            };
            let mut bytes = [0u8; 8];
            let target = address.and_then(|address| {
                unicorn
                    .mem_read(address, &mut bytes)
                    .ok()
                    .map(|()| u64::from_le_bytes(bytes))
            });
            RuntimeTargetResolution {
                target,
                operand_kind: "memory",
                register: None,
                memory: Some(TraceTargetMemory {
                    address,
                    base_register: (base_register != Register::None)
                        .then(|| iced_register_name(base_register)),
                    base_value,
                    index_register: (index_register != Register::None)
                        .then(|| iced_register_name(index_register)),
                    index_value,
                    scale: instruction.memory_index_scale() as u32,
                    displacement: instruction.memory_displacement64(),
                    dereferenced_target: target,
                }),
            }
        }
        _ => RuntimeTargetResolution {
            target: None,
            operand_kind: "unsupported",
            register: None,
            memory: None,
        },
    }
}

fn is_suite_address(address: u64) -> bool {
    (HOST_AEGP_UTILITY_TABLES..HOST_AEGP_UTILITY_TABLES + 0x400).contains(&address)
        || (HOST_AEGP_UNSUPPORTED_STUBS..HOST_AEGP_UNSUPPORTED_STUBS + 0x4000).contains(&address)
        || (HOST_ITERATE8_UNSUPPORTED_STUBS..HOST_ITERATE8_UNSUPPORTED_STUBS + 0x1000)
            .contains(&address)
        || [
            HOST_HANDLE_SUITE,
            HOST_ITERATE8_SUITE,
            HOST_COLOR_PARAM_SUITE,
            HOST_POINT_PARAM_SUITE,
            HOST_AEGP_MEMORY_SUITE,
            HOST_WORLD_SUITE,
            HOST_PF_ANSI_SUITE_V2,
        ]
        .into_iter()
        .any(|start| (start..start + 0x100).contains(&address))
}

fn classify_runtime_target(
    unicorn: &Unicorn<'_, GuestState>,
    target: Option<u64>,
) -> TraceTargetClassification {
    let Some(target) = target else {
        return TraceTargetClassification {
            kind: "unresolved",
            name: None,
        };
    };
    if target == 0 {
        return TraceTargetClassification {
            kind: "null",
            name: None,
        };
    }
    if let Some(label) = unicorn.get_data().trace_labels.get(&target) {
        return TraceTargetClassification {
            kind: match label.kind {
                TraceLabelKind::Import => "import",
                TraceLabelKind::HostCallback => "callback",
            },
            name: Some(label.name.clone()),
        };
    }
    if is_suite_address(target) {
        return TraceTargetClassification {
            kind: "suite",
            name: None,
        };
    }
    if unicorn
        .get_data()
        .image_executable_ranges
        .iter()
        .any(|(start, end)| (*start..*end).contains(&target))
    {
        return TraceTargetClassification {
            kind: "image_executable",
            name: None,
        };
    }
    if unicorn
        .get_data()
        .image_region
        .is_some_and(|(start, end)| (start..end).contains(&target))
    {
        return TraceTargetClassification {
            kind: "image_nonexec",
            name: None,
        };
    }
    let mut byte = [0u8; 1];
    TraceTargetClassification {
        kind: if unicorn.mem_read(target, &mut byte).is_ok() {
            "other_mapped"
        } else {
            "unmapped"
        },
        name: None,
    }
}

fn capture_runtime_target(
    unicorn: &Unicorn<'_, GuestState>,
    instruction: &iced_x86::Instruction,
    source_address: u64,
    instruction_bytes: String,
    image_base: u64,
    image_end: u64,
) -> TraceRuntimeTarget {
    let resolution = resolve_runtime_target_provenance(unicorn, instruction);
    let classification = classify_runtime_target(unicorn, resolution.target);
    TraceRuntimeTarget {
        source_address,
        source_rva: (image_base..image_end)
            .contains(&source_address)
            .then(|| source_address - image_base),
        transfer_kind: match instruction.mnemonic() {
            Mnemonic::Call => "call",
            Mnemonic::Jmp => "tail_call",
            _ => "branch",
        },
        operand_kind: resolution.operand_kind,
        instruction_bytes,
        effective_target: resolution.target,
        target: classification,
        register: resolution.register,
        memory: resolution.memory,
    }
}

fn advance_runtime_target_lifecycle(state: &mut GuestState, block_address: u64) {
    let should_clear = state.latest_runtime_target.as_ref().is_some_and(|target| {
        block_address != target.source_address && Some(block_address) != target.effective_target
    });
    if should_clear {
        state.latest_runtime_target = None;
    }
}

fn read_iced_register(unicorn: &Unicorn<'_, GuestState>, register: Register) -> Option<u64> {
    let register = match register {
        Register::RAX => RegisterX86::RAX,
        Register::RCX => RegisterX86::RCX,
        Register::RDX => RegisterX86::RDX,
        Register::RBX => RegisterX86::RBX,
        Register::RSP => RegisterX86::RSP,
        Register::RBP => RegisterX86::RBP,
        Register::RSI => RegisterX86::RSI,
        Register::RDI => RegisterX86::RDI,
        Register::R8 => RegisterX86::R8,
        Register::R9 => RegisterX86::R9,
        Register::R10 => RegisterX86::R10,
        Register::R11 => RegisterX86::R11,
        Register::R12 => RegisterX86::R12,
        Register::R13 => RegisterX86::R13,
        Register::R14 => RegisterX86::R14,
        Register::R15 => RegisterX86::R15,
        Register::RIP => RegisterX86::RIP,
        Register::None => return Some(0),
        _ => return None,
    };
    unicorn.reg_read(register).ok()
}

fn iced_ymm_register(register: Register) -> Option<RegisterX86> {
    Some(match register {
        Register::YMM0 => RegisterX86::YMM0,
        Register::YMM1 => RegisterX86::YMM1,
        Register::YMM2 => RegisterX86::YMM2,
        Register::YMM3 => RegisterX86::YMM3,
        Register::YMM4 => RegisterX86::YMM4,
        Register::YMM5 => RegisterX86::YMM5,
        Register::YMM6 => RegisterX86::YMM6,
        Register::YMM7 => RegisterX86::YMM7,
        Register::YMM8 => RegisterX86::YMM8,
        Register::YMM9 => RegisterX86::YMM9,
        Register::YMM10 => RegisterX86::YMM10,
        Register::YMM11 => RegisterX86::YMM11,
        Register::YMM12 => RegisterX86::YMM12,
        Register::YMM13 => RegisterX86::YMM13,
        Register::YMM14 => RegisterX86::YMM14,
        Register::YMM15 => RegisterX86::YMM15,
        _ => return None,
    })
}

fn iced_xmm_register(register: Register) -> Option<RegisterX86> {
    Some(match register {
        Register::XMM0 => RegisterX86::XMM0,
        Register::XMM1 => RegisterX86::XMM1,
        Register::XMM2 => RegisterX86::XMM2,
        Register::XMM3 => RegisterX86::XMM3,
        Register::XMM4 => RegisterX86::XMM4,
        Register::XMM5 => RegisterX86::XMM5,
        Register::XMM6 => RegisterX86::XMM6,
        Register::XMM7 => RegisterX86::XMM7,
        Register::XMM8 => RegisterX86::XMM8,
        Register::XMM9 => RegisterX86::XMM9,
        Register::XMM10 => RegisterX86::XMM10,
        Register::XMM11 => RegisterX86::XMM11,
        Register::XMM12 => RegisterX86::XMM12,
        Register::XMM13 => RegisterX86::XMM13,
        Register::XMM14 => RegisterX86::XMM14,
        Register::XMM15 => RegisterX86::XMM15,
        _ => return None,
    })
}

fn iced_xmm_index(register: Register) -> Option<usize> {
    match register {
        Register::XMM0 => Some(0),
        Register::XMM1 => Some(1),
        Register::XMM2 => Some(2),
        Register::XMM3 => Some(3),
        Register::XMM4 => Some(4),
        Register::XMM5 => Some(5),
        Register::XMM6 => Some(6),
        Register::XMM7 => Some(7),
        Register::XMM8 => Some(8),
        Register::XMM9 => Some(9),
        Register::XMM10 => Some(10),
        Register::XMM11 => Some(11),
        Register::XMM12 => Some(12),
        Register::XMM13 => Some(13),
        Register::XMM14 => Some(14),
        Register::XMM15 => Some(15),
        _ => None,
    }
}

fn iced_ymm_index(register: Register) -> Option<usize> {
    match register {
        Register::YMM0 => Some(0),
        Register::YMM1 => Some(1),
        Register::YMM2 => Some(2),
        Register::YMM3 => Some(3),
        Register::YMM4 => Some(4),
        Register::YMM5 => Some(5),
        Register::YMM6 => Some(6),
        Register::YMM7 => Some(7),
        Register::YMM8 => Some(8),
        Register::YMM9 => Some(9),
        Register::YMM10 => Some(10),
        Register::YMM11 => Some(11),
        Register::YMM12 => Some(12),
        Register::YMM13 => Some(13),
        Register::YMM14 => Some(14),
        Register::YMM15 => Some(15),
        _ => None,
    }
}

fn unicorn_ymm_register(index: usize) -> Option<RegisterX86> {
    Some(match index {
        0 => RegisterX86::YMM0,
        1 => RegisterX86::YMM1,
        2 => RegisterX86::YMM2,
        3 => RegisterX86::YMM3,
        4 => RegisterX86::YMM4,
        5 => RegisterX86::YMM5,
        6 => RegisterX86::YMM6,
        7 => RegisterX86::YMM7,
        8 => RegisterX86::YMM8,
        9 => RegisterX86::YMM9,
        10 => RegisterX86::YMM10,
        11 => RegisterX86::YMM11,
        12 => RegisterX86::YMM12,
        13 => RegisterX86::YMM13,
        14 => RegisterX86::YMM14,
        15 => RegisterX86::YMM15,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AvxStateSync {
    RegisterUpper(usize),
    AllUpper,
    AllRegisters,
}

fn discover_avx_state_sync_points(
    bytes: &[u8],
    address: u64,
) -> Result<Vec<(u64, AvxStateSync)>, GuestError> {
    // A PE executable section can contain inline data or multiple entry points,
    // so a single linear decode is not sufficient. In 64-bit mode every C4/C5
    // byte is a potential VEX prefix. Decode each candidate independently;
    // false positives only install a hook at an address that is never executed.
    let mut points = Vec::new();
    let mut info_factory = InstructionInfoFactory::new();
    for (offset, prefix) in bytes.iter().copied().enumerate() {
        if !matches!(prefix, 0xc4 | 0xc5) {
            continue;
        }
        let Some(instruction_address) = address.checked_add(offset as u64) else {
            continue;
        };
        let mut decoder = Decoder::with_ip(
            64,
            &bytes[offset..],
            instruction_address,
            DecoderOptions::NONE,
        );
        let instruction = decoder.decode();
        if instruction.is_invalid() || instruction.encoding() != EncodingKind::VEX {
            continue;
        }
        let sync = match instruction.mnemonic() {
            Mnemonic::Vzeroupper => Some(AvxStateSync::AllUpper),
            Mnemonic::Vzeroall => Some(AvxStateSync::AllRegisters),
            // The invalid-instruction fallback must observe the complete YMM
            // source before it writes an aliased XMM destination.
            Mnemonic::Vextractf128 => None,
            _ if instruction.op0_kind() == OpKind::Register
                && matches!(
                    info_factory.info(&instruction).op_access(0),
                    OpAccess::Write
                        | OpAccess::CondWrite
                        | OpAccess::ReadWrite
                        | OpAccess::ReadCondWrite
                ) =>
            {
                iced_xmm_index(instruction.op0_register()).map(AvxStateSync::RegisterUpper)
            }
            _ => None,
        };
        if let Some(sync) = sync {
            if points.len() >= MAX_AVX_STATE_SYNC_POINTS {
                return Err(GuestError::AvxStateCapacity);
            }
            points.push((instruction.ip(), sync));
        }
    }
    Ok(points)
}

fn synchronize_one_ymm(unicorn: &mut Unicorn<'_, GuestState>, index: usize, zero_all: bool) {
    let Some(register) = unicorn_ymm_register(index) else {
        return;
    };
    let mut value = aex_unicorn_buffer::YmmValue::zeroed();
    if !zero_all {
        // reg_read_long() allocates a boxed buffer.  These synchronization
        // hooks run for every native VEX.128 write (and 16 times for
        // VZEROUPPER), so read directly into the fixed-size stack buffer.
        if aex_unicorn_buffer::read_ymm(unicorn, register, &mut value).is_err() {
            return;
        }
        value.as_mut_bytes()[16..].fill(0);
    }
    // Code hooks run before the native instruction. Clear the upper half now;
    // Unicorn then computes the lower XMM result without reintroducing it.
    if unicorn.reg_write_long(register, value.as_bytes()).is_ok() {
        unicorn.get_data_mut().avx_defined_ymm[index] = true;
    }
}

fn synchronize_native_avx_state(unicorn: &mut Unicorn<'_, GuestState>, sync: AvxStateSync) {
    match sync {
        AvxStateSync::RegisterUpper(index) => synchronize_one_ymm(unicorn, index, false),
        AvxStateSync::AllUpper => {
            for index in 0..16 {
                synchronize_one_ymm(unicorn, index, false);
            }
        }
        AvxStateSync::AllRegisters => {
            for index in 0..16 {
                synchronize_one_ymm(unicorn, index, true);
            }
        }
    }
}

fn install_avx_state_sync_points(
    unicorn: &mut Unicorn<'static, GuestState>,
    points: Vec<(u64, AvxStateSync)>,
) -> Result<(), GuestError> {
    if points.len() > MAX_AVX_STATE_SYNC_POINTS {
        return Err(GuestError::AvxStateCapacity);
    }
    for (address, sync) in points {
        uc(
            "install native AVX state sync",
            unicorn.add_code_hook(address, address, move |unicorn, _, _| {
                synchronize_native_avx_state(unicorn, sync);
            }),
        )?;
    }
    Ok(())
}

fn iced_memory_address(
    unicorn: &Unicorn<'_, GuestState>,
    instruction: &iced_x86::Instruction,
) -> Option<u64> {
    if instruction.is_ip_rel_memory_operand() {
        return Some(instruction.ip_rel_memory_address());
    }
    let base = read_iced_register(unicorn, instruction.memory_base())?;
    let index = read_iced_register(unicorn, instruction.memory_index())?;
    Some(
        base.wrapping_add(index.wrapping_mul(instruction.memory_index_scale() as u64))
            .wrapping_add(instruction.memory_displacement64()),
    )
}

fn read_avx256_operand(
    unicorn: &Unicorn<'_, GuestState>,
    instruction: &iced_x86::Instruction,
    kind: OpKind,
    register: Register,
) -> Option<[u8; 32]> {
    let mut value = [0u8; 32];
    match kind {
        OpKind::Register => {
            let bytes = unicorn.reg_read_long(iced_ymm_register(register)?).ok()?;
            value.copy_from_slice(bytes.as_ref());
        }
        OpKind::Memory => {
            unicorn
                .mem_read(iced_memory_address(unicorn, instruction)?, &mut value)
                .ok()?;
        }
        _ => return None,
    }
    Some(value)
}

fn write_avx256_operand(
    unicorn: &mut Unicorn<'_, GuestState>,
    instruction: &iced_x86::Instruction,
    kind: OpKind,
    register: Register,
    value: &[u8; 32],
) -> bool {
    match kind {
        OpKind::Register => unicorn
            .reg_write_long(
                match iced_ymm_register(register) {
                    Some(register) => register,
                    None => return false,
                },
                value,
            )
            .is_ok(),
        OpKind::Memory => {
            let Some(address) = iced_memory_address(unicorn, instruction) else {
                return false;
            };
            unicorn.mem_write(address, value).is_ok()
        }
        _ => false,
    }
}

fn read_avx128_operand(
    unicorn: &Unicorn<'_, GuestState>,
    instruction: &iced_x86::Instruction,
    kind: OpKind,
    register: Register,
) -> Option<[u8; 16]> {
    let mut value = [0u8; 16];
    match kind {
        OpKind::Register => {
            let bytes = unicorn.reg_read_long(iced_xmm_register(register)?).ok()?;
            value.copy_from_slice(bytes.get(..16)?);
        }
        OpKind::Memory => {
            unicorn
                .mem_read(iced_memory_address(unicorn, instruction)?, &mut value)
                .ok()?;
        }
        _ => return None,
    }
    Some(value)
}

fn write_avx128_operand(
    unicorn: &mut Unicorn<'_, GuestState>,
    instruction: &iced_x86::Instruction,
    kind: OpKind,
    register: Register,
    value: &[u8; 16],
) -> bool {
    match kind {
        OpKind::Register => {
            let Some(index) = iced_xmm_index(register) else {
                return false;
            };
            let Some(destination) = unicorn_ymm_register(index) else {
                return false;
            };
            let mut full = [0u8; 32];
            full[..16].copy_from_slice(value);
            if unicorn.reg_write_long(destination, &full).is_err() {
                return false;
            }
            unicorn.get_data_mut().avx_defined_ymm[index] = true;
            true
        }
        OpKind::Memory => {
            let Some(address) = iced_memory_address(unicorn, instruction) else {
                return false;
            };
            unicorn.mem_write(address, value).is_ok()
        }
        _ => false,
    }
}

fn avx_fallback_budget_available(unicorn: &mut Unicorn<'_, GuestState>) -> bool {
    if unicorn.get_data().avx_fallback_instructions < MAX_AVX_FALLBACK_INSTRUCTIONS {
        return true;
    }
    unicorn.get_data_mut().callback_error = Some(format!(
        "AVX fallback instruction limit exceeded ({MAX_AVX_FALLBACK_INSTRUCTIONS})"
    ));
    let _ = unicorn.emu_stop();
    false
}

fn finish_avx_fallback(
    unicorn: &mut Unicorn<'_, GuestState>,
    instruction: &iced_x86::Instruction,
    rip: u64,
) -> bool {
    let Some(next_rip) = rip.checked_add(instruction.len() as u64) else {
        return false;
    };
    unicorn.get_data_mut().avx_fallback_instructions += 1;
    unicorn.reg_write(RegisterX86::RIP, next_rip).is_ok()
}

fn emulate_vblendv(
    unicorn: &mut Unicorn<'_, GuestState>,
    instruction: &iced_x86::Instruction,
    rip: u64,
    lane_bytes: usize,
) -> bool {
    if instruction.op_count() != 4
        || instruction.op0_kind() != OpKind::Register
        || instruction.op1_kind() != OpKind::Register
        || instruction.op3_kind() != OpKind::Register
    {
        return false;
    }
    let Some(left) = read_avx128_operand(
        unicorn,
        instruction,
        instruction.op1_kind(),
        instruction.op1_register(),
    ) else {
        return false;
    };
    let Some(right) = read_avx128_operand(
        unicorn,
        instruction,
        instruction.op2_kind(),
        instruction.op2_register(),
    ) else {
        return false;
    };
    let Some(mask) = read_avx128_operand(
        unicorn,
        instruction,
        instruction.op3_kind(),
        instruction.op3_register(),
    ) else {
        return false;
    };
    if !avx_fallback_budget_available(unicorn) {
        return true;
    }
    let mut output = [0u8; 16];
    for lane in 0..(16 / lane_bytes) {
        let start = lane * lane_bytes;
        let source = if mask[start + lane_bytes - 1] & 0x80 != 0 {
            &right
        } else {
            &left
        };
        output[start..start + lane_bytes].copy_from_slice(&source[start..start + lane_bytes]);
    }
    if !write_avx128_operand(
        unicorn,
        instruction,
        instruction.op0_kind(),
        instruction.op0_register(),
        &output,
    ) {
        return false;
    }
    finish_avx_fallback(unicorn, instruction, rip)
}

fn emulate_vextractf128(
    unicorn: &mut Unicorn<'_, GuestState>,
    instruction: &iced_x86::Instruction,
    rip: u64,
) -> bool {
    if instruction.op_count() != 3
        || instruction.op1_kind() != OpKind::Register
        || instruction.op2_kind() != OpKind::Immediate8
    {
        return false;
    }
    let Some(source_index) = iced_ymm_index(instruction.op1_register()) else {
        return false;
    };
    if !unicorn.get_data().avx_defined_ymm[source_index] {
        return false;
    }
    let Some(source) = read_avx256_operand(
        unicorn,
        instruction,
        instruction.op1_kind(),
        instruction.op1_register(),
    ) else {
        return false;
    };
    if !avx_fallback_budget_available(unicorn) {
        return true;
    }
    let start = usize::from(instruction.immediate8() & 1) * 16;
    let mut value = [0u8; 16];
    value.copy_from_slice(&source[start..start + 16]);
    if !write_avx128_operand(
        unicorn,
        instruction,
        instruction.op0_kind(),
        instruction.op0_register(),
        &value,
    ) {
        return false;
    }
    finish_avx_fallback(unicorn, instruction, rip)
}

fn emulate_avx_invalid_instruction(unicorn: &mut Unicorn<'_, GuestState>) -> bool {
    let Ok(rip) = unicorn.reg_read(RegisterX86::RIP) else {
        return false;
    };
    let Some((image_base, image_end)) = unicorn.get_data().image_region else {
        return false;
    };
    if !(image_base..image_end).contains(&rip) {
        return false;
    }
    let byte_count = usize::try_from((image_end - rip).min(15)).unwrap_or(15);
    let mut bytes = [0u8; 15];
    if unicorn.mem_read(rip, &mut bytes[..byte_count]).is_err() {
        return false;
    }
    let instruction =
        Decoder::with_ip(64, &bytes[..byte_count], rip, DecoderOptions::NONE).decode();
    if instruction.is_invalid()
        || !matches!(bytes[0], 0xc4 | 0xc5)
        || instruction.segment_prefix() != Register::None
    {
        return false;
    }
    if instruction.mnemonic() == Mnemonic::Vblendvps {
        return emulate_vblendv(unicorn, &instruction, rip, 4);
    }
    if instruction.mnemonic() == Mnemonic::Vblendvpd {
        return emulate_vblendv(unicorn, &instruction, rip, 8);
    }
    if instruction.mnemonic() == Mnemonic::Vextractf128 {
        return emulate_vextractf128(unicorn, &instruction, rip);
    }
    // Unicorn executes most VEX.128 operations natively but currently rejects
    // these 256-bit unaligned moves. VMOVUPS and VMOVDQU have identical
    // bit-copy semantics here; the mnemonic only expresses the source type.
    if !matches!(
        instruction.mnemonic(),
        Mnemonic::Vmovups | Mnemonic::Vmovdqu
    ) || instruction.op_count() != 2
    {
        return false;
    }
    if instruction.op1_kind() == OpKind::Register {
        let Some(source) = iced_ymm_index(instruction.op1_register()) else {
            return false;
        };
        if !unicorn.get_data().avx_defined_ymm[source] {
            return false;
        }
    }
    let Some(value) = read_avx256_operand(
        unicorn,
        &instruction,
        instruction.op1_kind(),
        instruction.op1_register(),
    ) else {
        return false;
    };
    if !avx_fallback_budget_available(unicorn) {
        return true;
    }
    if !write_avx256_operand(
        unicorn,
        &instruction,
        instruction.op0_kind(),
        instruction.op0_register(),
        &value,
    ) {
        return false;
    }
    if instruction.op0_kind() == OpKind::Register {
        let Some(destination) = iced_ymm_index(instruction.op0_register()) else {
            return false;
        };
        unicorn.get_data_mut().avx_defined_ymm[destination] = true;
    }
    finish_avx_fallback(unicorn, &instruction, rip)
}

fn install_avx_fallback(unicorn: &mut Unicorn<'static, GuestState>) -> Result<(), GuestError> {
    uc(
        "install bounded AVX fallback",
        unicorn.add_insn_invalid_hook(emulate_avx_invalid_instruction),
    )?;
    Ok(())
}

fn discover_image_avx_state_sync_points(
    image: &PeImage,
) -> Result<Vec<(u64, AvxStateSync)>, GuestError> {
    let mut points = Vec::new();
    for section in image
        .section_protections()
        .iter()
        .filter(|section| section.executable)
    {
        let start = section.virtual_address.min(image.mapped_bytes().len());
        let end = start
            .saturating_add(section.virtual_size)
            .min(image.mapped_bytes().len());
        if start >= end {
            continue;
        }
        let section_points = discover_avx_state_sync_points(
            &image.mapped_bytes()[start..end],
            image.image_base() + start as u64,
        )?;
        if points.len().saturating_add(section_points.len()) > MAX_AVX_STATE_SYNC_POINTS {
            return Err(GuestError::AvxStateCapacity);
        }
        points.extend(section_points);
    }
    points.sort_unstable_by_key(|(address, _)| *address);
    points.dedup_by_key(|(address, _)| *address);
    Ok(points)
}

fn classify_trace_value(raw: u64, image_base: u64, image_end: u64) -> TraceValue {
    let (classification, offset) = if (image_base..image_end).contains(&raw) {
        ("image", Some(raw - image_base))
    } else if (DATA_BASE..DATA_BASE + DATA_SIZE).contains(&raw) {
        ("guest_data", Some(raw - DATA_BASE))
    } else if (STACK_BASE..STACK_BASE + STACK_SIZE).contains(&raw) {
        ("guest_stack", Some(raw - STACK_BASE))
    } else if (STUB_BASE..STUB_BASE + STUB_SIZE).contains(&raw) {
        ("host_stub", Some(raw - STUB_BASE))
    } else if (CRT_HEAP_BASE..CRT_HEAP_END).contains(&raw) {
        ("crt_heap", Some(raw - CRT_HEAP_BASE))
    } else {
        ("scalar", None)
    };
    TraceValue {
        raw,
        classification,
        offset,
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn discover_trace_points(image: &PeImage) -> Vec<u64> {
    let mut points = Vec::new();
    for section in image
        .section_protections()
        .iter()
        .filter(|section| section.executable)
    {
        let start = section.virtual_address.min(image.mapped_bytes().len());
        let end = start
            .saturating_add(section.virtual_size)
            .min(image.mapped_bytes().len());
        if start >= end {
            continue;
        }
        let address = image.image_base() + start as u64;
        let mut decoder = Decoder::with_ip(
            64,
            &image.mapped_bytes()[start..end],
            address,
            DecoderOptions::NONE,
        );
        while decoder.can_decode() {
            let instruction = decoder.decode();
            if instruction.is_invalid() {
                continue;
            }
            if instruction.mnemonic() == Mnemonic::Call
                || instruction.mnemonic() == Mnemonic::Jmp
                || instruction.is_ip_rel_memory_operand()
                || matches!(
                    instruction.mnemonic(),
                    Mnemonic::Ret
                        | Mnemonic::Retf
                        | Mnemonic::Iret
                        | Mnemonic::Iretd
                        | Mnemonic::Iretq
                )
            {
                points.push(instruction.ip());
            }
        }
    }
    points.sort_unstable();
    points.dedup();
    points
}

fn format_trace_event(event: &TraceEvent) -> String {
    let indent = "  ".repeat(event.depth);
    let pc = event
        .pc_rva
        .map(|rva| format!(" rva={rva:#x}"))
        .unwrap_or_default();
    let target = event
        .target_rva
        .map(|rva| format!(" -> {rva:#x}"))
        .unwrap_or_default();
    let name = event
        .name
        .as_deref()
        .map(|name| format!(" {name}"))
        .unwrap_or_default();
    let count = (event.observed_count > 1)
        .then(|| format!(" ×{}", event.observed_count))
        .unwrap_or_default();
    format!(
        "{:05} {indent}{}{}{}{}{}",
        event.sequence, event.kind, pc, target, name, count
    )
}

#[derive(Default)]
struct FunctionAggregate {
    observed_calls: u64,
    observed_returns: u64,
    callees: BTreeSet<u64>,
    imports: BTreeSet<String>,
    host_callbacks: BTreeSet<String>,
}

fn aggregate_trace_functions(entry_rva: u64, events: &[TraceEvent]) -> Vec<TraceFunction> {
    let mut functions = BTreeMap::<u64, FunctionAggregate>::new();
    functions.entry(entry_rva).or_default();
    for event in events {
        let Some(function_rva) = event.function_rva else {
            continue;
        };
        let function = functions.entry(function_rva).or_default();
        match event.kind {
            "guest_call" | "tail_call" => {
                function.observed_calls += event.observed_count;
                if let Some(callee) = event.target_rva {
                    function.callees.insert(callee);
                    functions.entry(callee).or_default();
                }
            }
            "guest_return" => function.observed_returns += event.observed_count,
            "import_call" => {
                if let Some(name) = &event.name {
                    function.imports.insert(name.clone());
                }
            }
            "host_callback" => {
                if let Some(name) = &event.name {
                    function.host_callbacks.insert(name.clone());
                }
            }
            _ => {}
        }
    }
    functions
        .into_iter()
        .map(|(entry_rva, aggregate)| TraceFunction {
            entry_rva,
            entry_bytes: String::new(),
            observed_calls: aggregate.observed_calls,
            observed_returns: aggregate.observed_returns,
            callees: aggregate.callees.into_iter().collect(),
            imports: aggregate.imports.into_iter().collect(),
            host_callbacks: aggregate.host_callbacks.into_iter().collect(),
        })
        .collect()
}

impl ExecutionTrace {
    pub fn set_state_changes(&mut self, state_changes: Vec<TraceStateValue>) {
        self.state_changes = state_changes;
    }
}
