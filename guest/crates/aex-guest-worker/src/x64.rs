use aex_abi::x86_64_windows as abi;
use iced_x86::{Decoder, DecoderOptions, Mnemonic, OpKind, Register};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use thiserror::Error;
use unicorn_engine::unicorn_const::{Arch, Mode, Prot};
use unicorn_engine::{RegisterX86, UcHookId, Unicorn};

use crate::crt_heap::{CrtHeap, CrtHeapError, MAX_CRT_HEAP_BYTES};
use crate::pe::PeImage;
use crate::plugin_data::{
    CALLBACK_REJECTED, EffectRegistry, RegistrationPointers, decode_registration,
};

const PAGE_SIZE: u64 = 0x1000;
const STACK_BASE: u64 = 0x0000_0000_7000_0000;
const STACK_SIZE: u64 = 0x20_0000;
const STUB_BASE: u64 = 0x0000_0000_6000_0000;
const STUB_SIZE: u64 = 0x10_0000;
const STUB_STRIDE: u64 = 16;
const RETURN_ADDRESS: u64 = STUB_BASE + STUB_SIZE - PAGE_SIZE;
const HOST_ADD_PARAM: u64 = STUB_BASE + 0x80000;
const HOST_POISON: u64 = STUB_BASE + 0x80010;
const HOST_ANSI_STRCPY: u64 = STUB_BASE + 0x80020;
const HOST_COPY: u64 = STUB_BASE + 0x80030;
const HOST_NOOP: u64 = STUB_BASE + 0x80040;
const HOST_PRE_CHECKOUT_LAYER: u64 = STUB_BASE + 0x80050;
const HOST_CHECKOUT_LAYER_PIXELS: u64 = STUB_BASE + 0x80060;
const HOST_CHECKIN_LAYER_PIXELS: u64 = STUB_BASE + 0x80070;
const HOST_CHECKOUT_OUTPUT: u64 = STUB_BASE + 0x80080;
const HOST_ACQUIRE_SUITE: u64 = STUB_BASE + 0x80090;
const HOST_CHECKOUT_PARAM: u64 = STUB_BASE + 0x800a0;
const HOST_CHECKIN_PARAM: u64 = STUB_BASE + 0x800b0;
const HOST_NEW_HANDLE: u64 = STUB_BASE + 0x800c0;
const HOST_LOCK_HANDLE: u64 = STUB_BASE + 0x800d0;
const HOST_UNLOCK_HANDLE: u64 = STUB_BASE + 0x800e0;
const HOST_DISPOSE_HANDLE: u64 = STUB_BASE + 0x800f0;
const HOST_HANDLE_SIZE: u64 = STUB_BASE + 0x80100;
const HOST_RESIZE_HANDLE: u64 = STUB_BASE + 0x80110;
const HOST_AEGP_REGISTER: u64 = STUB_BASE + 0x80120;
const HOST_AEGP_GET_MAIN_WINDOW: u64 = STUB_BASE + 0x80130;
const HOST_ITERATE8: u64 = STUB_BASE + 0x80140;
const HOST_ITERATE8_CONTINUE: u64 = STUB_BASE + 0x80150;
const HOST_COLOR_PARAM_VALUE: u64 = STUB_BASE + 0x80160;
const HOST_POINT_PARAM_VALUE: u64 = STUB_BASE + 0x80170;
const HOST_AEGP_NEW_MEM_HANDLE: u64 = STUB_BASE + 0x80180;
const HOST_AEGP_FREE_MEM_HANDLE: u64 = STUB_BASE + 0x80190;
const HOST_AEGP_LOCK_MEM_HANDLE: u64 = STUB_BASE + 0x801a0;
const HOST_AEGP_UNLOCK_MEM_HANDLE: u64 = STUB_BASE + 0x801b0;
const HOST_AEGP_MEM_HANDLE_SIZE: u64 = STUB_BASE + 0x801c0;
const HOST_AEGP_RESIZE_MEM_HANDLE: u64 = STUB_BASE + 0x801d0;
const HOST_AEGP_MEMORY_UNSUPPORTED: u64 = STUB_BASE + 0x801e0;
const HOST_PLUGIN_DATA_V2: u64 = STUB_BASE + 0x801f0;
const HOST_PLUGIN_DATA_V1: u64 = STUB_BASE + 0x80200;
const HOST_NEW_WORLD: u64 = STUB_BASE + 0x80210;
const HOST_DISPOSE_WORLD: u64 = STUB_BASE + 0x80220;
const HOST_GET_WORLD_PIXEL_FORMAT: u64 = STUB_BASE + 0x80230;
const HOST_PF_ANSI_ATAN: u64 = STUB_BASE + 0x80240;
const HOST_PF_ANSI_ATAN2: u64 = STUB_BASE + 0x80250;
const HOST_PF_ANSI_CEIL: u64 = STUB_BASE + 0x80260;
const HOST_PF_ANSI_COS: u64 = STUB_BASE + 0x80270;
const HOST_PF_ANSI_EXP: u64 = STUB_BASE + 0x80280;
const HOST_PF_ANSI_FABS: u64 = STUB_BASE + 0x80290;
const HOST_PF_ANSI_FLOOR: u64 = STUB_BASE + 0x802a0;
const HOST_PF_ANSI_FMOD: u64 = STUB_BASE + 0x802b0;
const HOST_PF_ANSI_HYPOT: u64 = STUB_BASE + 0x802c0;
const HOST_PF_ANSI_LOG: u64 = STUB_BASE + 0x802d0;
const HOST_PF_ANSI_LOG10: u64 = STUB_BASE + 0x802e0;
const HOST_PF_ANSI_POW: u64 = STUB_BASE + 0x802f0;
const HOST_PF_ANSI_SIN: u64 = STUB_BASE + 0x80300;
const HOST_PF_ANSI_SQRT: u64 = STUB_BASE + 0x80310;
const HOST_PF_ANSI_TAN: u64 = STUB_BASE + 0x80320;
const HOST_PF_ANSI_SPRINTF: u64 = STUB_BASE + 0x80330;
const HOST_PF_ANSI_STRCPY: u64 = STUB_BASE + 0x80340;
const HOST_PF_ANSI_ASIN: u64 = STUB_BASE + 0x80350;
const HOST_PF_ANSI_ACOS: u64 = STUB_BASE + 0x80360;
const HOST_PF_ANSI_STRCPY_BOUNDED: u64 = STUB_BASE + 0x80370;
const HOST_EXTENDED_ALLOC: u64 = STUB_BASE + 0x80380;
const HOST_EXTENDED_FREE: u64 = STUB_BASE + 0x80390;
const HOST_EXTENDED_LOOKUP: u64 = STUB_BASE + 0x803a0;
const HOST_HANDLE_SUITE: u64 = STUB_BASE + 0x81000;
const HOST_ITERATE8_SUITE: u64 = STUB_BASE + 0x81100;
const HOST_COLOR_PARAM_SUITE: u64 = STUB_BASE + 0x81200;
const HOST_POINT_PARAM_SUITE: u64 = STUB_BASE + 0x81300;
const HOST_AEGP_MEMORY_SUITE: u64 = STUB_BASE + 0x81400;
const HOST_WORLD_SUITE: u64 = STUB_BASE + 0x81500;
const HOST_PF_ANSI_SUITE_V2: u64 = STUB_BASE + 0x81600;
const HOST_AEGP_UTILITY_TABLES: u64 = STUB_BASE + 0x82000;
const HOST_AEGP_UNSUPPORTED_STUBS: u64 = STUB_BASE + 0x83000;
const HOST_ITERATE8_UNSUPPORTED_STUBS: u64 = STUB_BASE + 0x88000;
const DATA_BASE: u64 = 0x0000_0000_4000_0000;
const DATA_SIZE: u64 = 0x1000_0000;
const HANDLE_DATA_BASE: u64 = DATA_BASE + 0x400_0000;
const HANDLE_DATA_END: u64 = DATA_BASE + DATA_SIZE;
const AEGP_MEMORY_HANDLE_BASE: u64 = STUB_BASE + 0x90000;
const MAX_AEGP_MEMORY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_AEGP_MEMORY_HANDLES: usize = 256;
const PF_HANDLE_DATA_BASE: u64 = 0x0000_0001_0000_0000;
const PF_HANDLE_DATA_END: u64 = PF_HANDLE_DATA_BASE + 0x2_0000_0000;
const MAX_PF_HANDLE_SIZE: u64 = 0x8000_0000;
const MAX_PF_HANDLE_COUNT: usize = 16_384;
const WORLD_DATA_BASE: u64 = PF_HANDLE_DATA_END;
const WORLD_DATA_END: u64 = WORLD_DATA_BASE + 0x2_0000_0000;
const CRT_HEAP_BASE: u64 = 0x0000_0010_0000_0000;
const CRT_HEAP_END: u64 = CRT_HEAP_BASE + MAX_CRT_HEAP_BYTES;
const MAX_WORLD_SIZE: u64 = 128 * 1024 * 1024;
const MAX_WORLD_COUNT: usize = 256;
// A nonzero Unicorn instruction limit enables instruction counting across the
// whole run, which is prohibitively expensive for image kernels. The wall-clock
// timeout and return-sentinel check still bound and validate guest execution.
const MAX_INSTRUCTIONS: usize = 0;
const TIMEOUT_MICROSECONDS: u64 = 600_000_000;
const MAX_TRACE_EVENTS: usize = 50_000;
const MAX_TRACE_BASIC_BLOCKS: usize = 50_000;
const MAX_TRACE_BRANCH_EDGES: usize = 100_000;
const TRACE_STACK_ARGUMENTS: usize = 4;
const TRACE_FIRST_SAMPLES: usize = 3;
const TRACE_LAST_SAMPLES: usize = 3;
const TRACE_DISTINCT_SAMPLES: usize = 16;
const TRACE_DISTINCT_FINGERPRINTS: usize = 4096;
const MAX_TRACE_WATCH_BYTES: usize = 4096;
const MAX_TRACE_WITNESSES: usize = 256;
const MAX_UNSUPPORTED_SUITE_CALLS: usize = 256;
const MAX_SUITE_REQUESTS: usize = 256;
const MAX_AVX_FALLBACK_INSTRUCTIONS: u64 = 1_000_000;
const MAX_CRT_MEMORY_COPY_BYTES: u64 = 128 * 1024 * 1024;
const CRT_MEMORY_COPY_CHUNK: usize = 64 * 1024;

pub(crate) fn utility_suite_layout(version: u32) -> Option<(usize, usize, usize)> {
    match version {
        3 => Some((9, 7, 8)),
        7 => Some((25, 7, 8)),
        11 => Some((31, 8, 9)),
        13 => Some((33, 9, 10)),
        _ => None,
    }
}

fn utility_suite_table_address(version: u32) -> Option<u64> {
    match version {
        3 => Some(HOST_AEGP_UTILITY_TABLES),
        7 => Some(HOST_AEGP_UTILITY_TABLES + 0x100),
        11 => Some(HOST_AEGP_UTILITY_TABLES + 0x200),
        13 => Some(HOST_AEGP_UTILITY_TABLES + 0x300),
        _ => None,
    }
}

fn unsupported_suite_stub_address(version: u32, slot: usize) -> Option<u64> {
    let version_index = match version {
        3 => 0,
        7 => 1,
        11 => 2,
        13 => 3,
        _ => return None,
    };
    Some(HOST_AEGP_UNSUPPORTED_STUBS + version_index * 0x1000 + slot as u64 * STUB_STRIDE)
}

fn iterate8_suite_table_address(version: u64) -> Option<u64> {
    match version {
        1 => Some(HOST_ITERATE8_SUITE),
        2 => Some(HOST_ITERATE8_SUITE + 0x40),
        _ => None,
    }
}

#[derive(Debug, Error)]
pub enum GuestError {
    #[error("unicorn error during {operation}: {detail}")]
    Unicorn {
        operation: &'static str,
        detail: String,
    },
    #[error("mapped PE image is not page aligned")]
    ImageAlignment,
    #[error("import stub capacity exceeded")]
    StubCapacity,
    #[error("IAT entry is outside the mapped image")]
    IatRange,
    #[error("guest data arena exhausted")]
    DataCapacity,
    #[error("guest callback failed: {0}")]
    Callback(String),
    #[error("DLL process attach returned FALSE")]
    DllProcessAttach,
    #[error("guest execution failed: {reason}; crash_snapshot={snapshot_json}")]
    ExecutionCrash {
        reason: String,
        snapshot_json: String,
        snapshot: Box<TraceCrashSnapshot>,
    },
}

impl GuestError {
    pub fn diagnostic_category(&self) -> &'static str {
        match self {
            Self::Unicorn { .. } => "emulation",
            Self::ImageAlignment => "image",
            Self::StubCapacity | Self::IatRange => "import",
            Self::DataCapacity => "memory",
            Self::Callback(_) => "callback",
            Self::DllProcessAttach => "dllmain",
            Self::ExecutionCrash { .. } => "crash",
        }
    }

    pub fn diagnostic_message(&self) -> String {
        match self {
            Self::ExecutionCrash { reason, .. } => reason.clone(),
            _ => self.to_string(),
        }
    }

    pub fn crash_reason(&self) -> Option<&str> {
        match self {
            Self::ExecutionCrash { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

fn uc<T>(
    operation: &'static str,
    result: Result<T, unicorn_engine::unicorn_const::uc_error>,
) -> Result<T, GuestError> {
    result.map_err(|error| GuestError::Unicorn {
        operation,
        detail: error.to_string(),
    })
}

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
        let target = resolve_runtime_target(unicorn, &instruction);
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
                    instruction_bytes: Some(bytes_to_hex(
                        &bytes[..instruction.len().min(bytes.len())],
                    )),
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
        let target = resolve_runtime_target(unicorn, &instruction);
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
                    instruction_bytes: Some(bytes_to_hex(
                        &bytes[..instruction.len().min(bytes.len())],
                    )),
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
            let mut bytes = [0u8; 8];
            unicorn
                .mem_read(rsp.checked_add(stack_offset)?, &mut bytes)
                .ok()?;
            Some(TraceStackArgument {
                index: offset + 5,
                stack_offset,
                value: classify_trace_value(u64::from_le_bytes(bytes), image_base, image_end),
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

fn resolve_runtime_target(
    unicorn: &Unicorn<'_, GuestState>,
    instruction: &iced_x86::Instruction,
) -> Option<u64> {
    match instruction.op0_kind() {
        OpKind::NearBranch16 | OpKind::NearBranch32 | OpKind::NearBranch64 => {
            Some(instruction.near_branch_target())
        }
        OpKind::Register => read_iced_register(unicorn, instruction.op0_register()),
        OpKind::Memory => {
            let address = if instruction.is_ip_rel_memory_operand() {
                instruction.ip_rel_memory_address()
            } else {
                let base = read_iced_register(unicorn, instruction.memory_base()).unwrap_or(0);
                let index = read_iced_register(unicorn, instruction.memory_index()).unwrap_or(0);
                base.wrapping_add(index.wrapping_mul(instruction.memory_index_scale() as u64))
                    .wrapping_add(instruction.memory_displacement64())
            };
            let mut bytes = [0u8; 8];
            unicorn.mem_read(address, &mut bytes).ok()?;
            Some(u64::from_le_bytes(bytes))
        }
        _ => None,
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

fn emulate_avx_invalid_instruction(unicorn: &mut Unicorn<'_, GuestState>) -> bool {
    let Ok(rip) = unicorn.reg_read(RegisterX86::RIP) else {
        return false;
    };
    let mut bytes = [0u8; 15];
    if unicorn.mem_read(rip, &mut bytes).is_err() {
        return false;
    }
    let instruction = Decoder::with_ip(64, &bytes, rip, DecoderOptions::NONE).decode();
    if instruction.is_invalid()
        || !matches!(bytes[0], 0xc4 | 0xc5)
        || instruction.mnemonic() != Mnemonic::Vmovups
        || instruction.op_count() != 2
        || instruction.segment_prefix() != Register::None
    {
        return false;
    }
    if instruction.op1_kind() == OpKind::Register {
        let Some(source) = iced_ymm_index(instruction.op1_register()) else {
            return false;
        };
        let state = unicorn.get_data();
        if state.avx_chain_next_rip != Some(rip) || !state.avx_defined_ymm[source] {
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
    if unicorn.get_data().avx_fallback_instructions >= MAX_AVX_FALLBACK_INSTRUCTIONS {
        unicorn.get_data_mut().callback_error = Some(format!(
            "AVX fallback instruction limit exceeded ({MAX_AVX_FALLBACK_INSTRUCTIONS})"
        ));
        let _ = unicorn.emu_stop();
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
    let Some(next_rip) = rip.checked_add(instruction.len() as u64) else {
        return false;
    };
    let state = unicorn.get_data_mut();
    state.avx_fallback_instructions += 1;
    state.avx_chain_next_rip = Some(next_rip);
    if instruction.op0_kind() == OpKind::Register {
        let Some(destination) = iced_ymm_index(instruction.op0_register()) else {
            return false;
        };
        state.avx_defined_ymm[destination] = true;
    }
    unicorn.reg_write(RegisterX86::RIP, next_rip).is_ok()
}

fn install_avx_fallback(unicorn: &mut Unicorn<'static, GuestState>) -> Result<(), GuestError> {
    uc(
        "install bounded AVX fallback",
        unicorn.add_insn_invalid_hook(emulate_avx_invalid_instruction),
    )?;
    Ok(())
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

#[derive(Clone, Debug)]
pub struct GuestParam {
    pub index: i32,
    pub param_type: i32,
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UnsupportedSuiteCall {
    pub name: &'static str,
    pub version: u32,
    pub slot: usize,
    pub call_count: u64,
}

pub(crate) fn record_unsupported_suite_call(
    calls: &mut Vec<UnsupportedSuiteCall>,
    dropped: &mut u64,
    version: u32,
    slot: usize,
) {
    record_named_unsupported_suite_call(calls, dropped, "AEGP Utility Suite", version, slot);
}

pub(crate) fn record_suite_request(requests: &mut Vec<String>, request: String) {
    if requests.len() < MAX_SUITE_REQUESTS && !requests.iter().any(|seen| seen == &request) {
        requests.push(request);
    }
}

fn record_named_unsupported_suite_call(
    calls: &mut Vec<UnsupportedSuiteCall>,
    dropped: &mut u64,
    name: &'static str,
    version: u32,
    slot: usize,
) {
    if let Some(call) = calls
        .iter_mut()
        .find(|call| call.name == name && call.version == version && call.slot == slot)
    {
        call.call_count += 1;
    } else if calls.len() < MAX_UNSUPPORTED_SUITE_CALLS {
        calls.push(UnsupportedSuiteCall {
            name,
            version,
            slot,
            call_count: 1,
        });
    } else {
        *dropped += 1;
    }
}

#[derive(Default)]
struct GuestState {
    params: Vec<GuestParam>,
    callback_error: Option<String>,
    smart_input_world: u64,
    smart_output_world: u64,
    smart_width: u32,
    smart_height: u32,
    smart_pixel_format: i32,
    suite_requests: Vec<String>,
    unsupported_suite_calls: Vec<UnsupportedSuiteCall>,
    dropped_unsupported_suite_calls: u64,
    pre_checkout_calls: u32,
    pre_checkout_requests: Vec<[i32; 4]>,
    checkout_pixels_calls: u32,
    checkout_output_calls: u32,
    parameter_definitions: Vec<u64>,
    next_handle_data: u64,
    next_pf_handle_data: u64,
    image_region: Option<(u64, u64)>,
    handles: HashMap<u64, GuestHandle>,
    worlds: HashMap<u64, GuestWorld>,
    aegp_memory_handles: HashMap<u64, AegpMemoryHandle>,
    aegp_memory_free: Vec<AegpMemoryBlock>,
    next_aegp_memory_handle: u64,
    math_calls: Vec<String>,
    handle_allocations: Vec<u64>,
    handle_allocation_failures: Vec<String>,
    census_blocks: HashMap<(u64, u32), u64>,
    trace: Option<TraceCapture>,
    trace_labels: HashMap<u64, TraceLabel>,
    trace_watches: Vec<TraceWatchSpec>,
    pending_iterate8: Option<PendingIterate8>,
    vcomp_dynamic_loop: Option<VcompDynamicLoop>,
    plugin_data_registry: EffectRegistry,
    plugin_data_error: Option<String>,
    crt_heap: CrtHeap,
    extended_strings: HashMap<i32, u64>,
    extended_empty_string: u64,
    extended_string_table_valid: bool,
    avx_fallback_instructions: u64,
    avx_defined_ymm: [bool; 16],
    avx_chain_next_rip: Option<u64>,
}

#[derive(Clone, Debug)]
struct VcompDynamicLoop {
    current: i32,
    upper: i32,
    chunk: i32,
    exhausted: bool,
}

#[derive(Clone, Debug)]
struct PendingIterate8 {
    caller_rsp: u64,
    return_address: u64,
    refcon: u64,
    pixel_function: u64,
    source_data: u64,
    source_rowbytes: u64,
    destination_data: u64,
    destination_rowbytes: u64,
    left: i32,
    right: i32,
    bottom: i32,
    x: i32,
    y: i32,
}

#[derive(Clone, Debug)]
struct GuestHandle {
    data: u64,
    size: u64,
    locks: u32,
    handle_region: u64,
    data_region: u64,
    data_mapped_size: u64,
}

#[derive(Clone, Debug)]
struct GuestWorld {
    pixel_format: i32,
    size: u64,
    data_region: u64,
    data_mapped_size: u64,
}

#[derive(Clone, Debug)]
struct AegpMemoryHandle {
    data: u64,
    size: u64,
    locks: u32,
    end: u64,
}

#[derive(Clone, Copy, Debug)]
struct AegpMemoryBlock {
    data: u64,
    end: u64,
}

pub struct GuestEngine<'a> {
    unicorn: Unicorn<'a, GuestState>,
    next_data: u64,
    image_base: u64,
    image_end: u64,
    census_hook: Option<UcHookId>,
    trace_hooks: Vec<UcHookId>,
    trace_points: Vec<u64>,
    image_sha256: String,
    entry_export: String,
    trace_modules: Vec<TraceModule>,
}

impl Drop for GuestEngine<'_> {
    fn drop(&mut self) {
        let mut mappings = self
            .unicorn
            .get_data_mut()
            .worlds
            .drain()
            .map(|(_, record)| (record.data_region, record.data_mapped_size))
            .collect::<Vec<_>>();
        mappings.extend(
            self.unicorn
                .get_data()
                .crt_heap
                .allocations()
                .map(|(pointer, allocation)| (pointer, allocation.backing_size)),
        );
        for (address, size) in mappings {
            let _ = self.unicorn.mem_unmap(address, size);
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum TraceLabelKind {
    Import,
    HostCallback,
}

#[derive(Clone, Debug)]
struct TraceLabel {
    kind: TraceLabelKind,
    name: String,
}

#[derive(Debug)]
struct TraceCapture {
    selector: String,
    entry_rva: u64,
    events: Vec<TraceEvent>,
    return_stack: Vec<u64>,
    function_stack: Vec<Option<u64>>,
    call_rsp_stack: Vec<u64>,
    call_id_stack: Vec<u64>,
    next_call_id: u64,
    watch_specs: Vec<TraceWatchSpec>,
    watch_occurrence_counts: HashMap<String, u64>,
    watch_stack: Vec<Vec<PendingTraceWatch>>,
    selector_watches: Vec<PendingTraceWatch>,
    witnesses: Vec<TraceMemoryWitness>,
    dropped_witnesses: u64,
    basic_blocks: HashMap<(u64, u32), u64>,
    branch_edges: HashMap<(u64, u64), u64>,
    dropped_basic_blocks: u64,
    dropped_branch_edges: u64,
    previous_block: Option<u64>,
    event_index: HashMap<TraceEventKey, usize>,
    event_fingerprints: HashMap<usize, HashSet<u64>>,
    known_function_entries: HashSet<u64>,
    truncated: bool,
    dropped_events: u64,
}

#[derive(Clone, Debug)]
struct PendingTraceWatch {
    spec_id: String,
    register: &'static str,
    call_id: Option<u64>,
    function_rva: Option<u64>,
    pc_rva: Option<u64>,
    address: u64,
    before: TraceMemorySnapshot,
    image_coordinate: Option<[u32; 2]>,
    image_row_offset: Option<u64>,
    image_format: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceWatchSpec {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_rva: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction_rva: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub absolute_address: Option<u64>,
    pub register: &'static str,
    pub size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_coordinate: Option<[u32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_row_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_format: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceModule {
    pub name: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub symbols: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceConfiguration {
    pub max_events: usize,
    pub max_basic_blocks: usize,
    pub max_branch_edges: usize,
    pub max_witnesses: usize,
    pub max_watch_bytes: usize,
    pub max_distinct_fingerprints_per_event: usize,
    pub watches: Vec<TraceWatchSpec>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceTruncation {
    pub category: &'static str,
    pub reason: &'static str,
    pub dropped: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceMemorySnapshot {
    pub status: &'static str,
    pub address: u64,
    pub size: usize,
    pub classification: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hex: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub u8_values: Vec<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub u16_values: Vec<u16>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub u32_values: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub u64_values: Vec<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub f32_values: Vec<Option<f32>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub f64_values: Vec<Option<f64>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pointer_chain: Vec<TracePointerHop>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TracePointerHop {
    pub address: u64,
    pub classification: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceChangedRange {
    pub offset: usize,
    pub size: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceMemoryWitness {
    pub watch_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_rva: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pc_rva: Option<u64>,
    pub register: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_coordinate: Option<[u32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_row_offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_format: Option<&'static str>,
    pub before: TraceMemorySnapshot,
    pub after: TraceMemorySnapshot,
    pub changed_ranges: Vec<TraceChangedRange>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceBasicBlock {
    pub rva: u64,
    pub size: u32,
    pub observed_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceBranchEdge {
    pub from_rva: u64,
    pub to_rva: u64,
    pub observed_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceCrashFrame {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_rva: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceCrashSnapshot {
    pub reason: String,
    pub registers: BTreeMap<String, u64>,
    pub xmm_registers: Vec<TraceXmmValue>,
    pub call_stack: Vec<TraceCrashFrame>,
    pub instruction_address: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction_rva: Option<u64>,
    pub instruction_bytes: String,
    pub handle_allocations: Vec<u64>,
    pub handle_allocation_failures: Vec<String>,
    pub live_handle_count: usize,
    pub next_pf_handle_data: u64,
    pub pf_handle_data_end: u64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TraceEventKey {
    depth: usize,
    kind: &'static str,
    function_rva: Option<u64>,
    pc_rva: Option<u64>,
    target_rva: Option<u64>,
    name: Option<String>,
    call_kind: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceValue {
    pub raw: u64,
    pub classification: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceArgument {
    pub register: &'static str,
    #[serde(flatten)]
    pub value: TraceValue,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceXmmValue {
    pub register: &'static str,
    pub raw_hex: String,
    pub f32_lanes: Vec<Option<f32>>,
    pub f64_lanes: Vec<Option<f64>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceStackArgument {
    pub index: usize,
    pub stack_offset: u64,
    #[serde(flatten)]
    pub value: TraceValue,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceReturnValue {
    pub rax: TraceValue,
    pub xmm0: TraceXmmValue,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceObservation {
    pub observation: u64,
    pub fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<u64>,
    pub arguments: Vec<TraceArgument>,
    pub xmm_arguments: Vec<TraceXmmValue>,
    pub stack_arguments: Vec<TraceStackArgument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_value: Option<TraceReturnValue>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceNumericRange {
    pub field: String,
    pub minimum: f64,
    pub maximum: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TraceExemplars {
    pub distinct_fingerprints: u64,
    pub dropped_distinct_fingerprints: u64,
    pub fingerprint_tracking_truncated: bool,
    pub untracked_fingerprint_observations: u64,
    pub first: Vec<TraceObservation>,
    pub last: Vec<TraceObservation>,
    pub distinct: Vec<TraceObservation>,
    pub numeric_ranges: Vec<TraceNumericRange>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceEvent {
    pub sequence: usize,
    pub observed_count: u64,
    pub depth: usize,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_rva: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pc_rva: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_rva: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<TraceArgument>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub xmm_arguments: Vec<TraceXmmValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stack_arguments: Vec<TraceStackArgument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_value: Option<TraceReturnValue>,
    pub exemplars: TraceExemplars,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction_bytes: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceFunction {
    pub entry_rva: u64,
    pub entry_bytes: String,
    pub observed_calls: u64,
    pub observed_returns: u64,
    pub callees: Vec<u64>,
    pub imports: Vec<String>,
    pub host_callbacks: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceStateValue {
    pub name: String,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExecutionTrace {
    pub schema: &'static str,
    pub schema_version: u32,
    pub execution_backend: &'static str,
    pub image_sha256: String,
    pub preferred_image_base: u64,
    pub entry_export: String,
    pub worker_build_identity: String,
    pub modules: Vec<TraceModule>,
    pub trace_configuration: TraceConfiguration,
    pub selector: String,
    pub entry_rva: u64,
    pub return_value: u64,
    pub truncated: bool,
    pub events: Vec<TraceEvent>,
    pub functions: Vec<TraceFunction>,
    pub state_changes: Vec<TraceStateValue>,
    pub memory_witnesses: Vec<TraceMemoryWitness>,
    pub dropped_memory_witnesses: u64,
    pub basic_blocks: Vec<TraceBasicBlock>,
    pub branch_edges: Vec<TraceBranchEdge>,
    pub truncation: Vec<TraceTruncation>,
    pub timeline: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CensusBlock {
    pub address: u64,
    pub rva: u64,
    pub size_bytes: u32,
    pub executions: u64,
    pub instructions: u32,
    pub dynamic_instructions: u64,
    pub scalar_sse_fp_instructions: u32,
    pub dynamic_scalar_sse_fp_instructions: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CensusExtent {
    pub start_address: u64,
    pub end_address: u64,
    pub start_rva: u64,
    pub end_rva: u64,
    pub size_bytes: u64,
    pub block_variants: usize,
    pub dynamic_instructions: u64,
    pub dynamic_instruction_fraction: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct GuestCensus {
    pub schema_version: u32,
    pub distinct_blocks: usize,
    pub total_block_executions: u64,
    pub estimated_dynamic_instructions: u64,
    pub output_pixels: u64,
    pub estimated_dynamic_instructions_per_pixel: f64,
    pub estimated_dynamic_scalar_sse_fp_instructions: u64,
    pub scalar_sse_fp_fraction: f64,
    pub dynamic_instructions_in_scalar_sse_blocks: u64,
    pub scalar_sse_block_work_fraction: f64,
    pub top_1_dynamic_instruction_fraction: f64,
    pub top_5_dynamic_instruction_fraction: f64,
    pub top_20_dynamic_instruction_fraction: f64,
    pub blocks_for_80_percent: usize,
    pub blocks: Vec<CensusBlock>,
    pub distinct_extents: usize,
    pub top_1_extent_dynamic_instruction_fraction: f64,
    pub top_2_extent_dynamic_instruction_fraction: f64,
    pub top_20_extent_dynamic_instruction_fraction: f64,
    pub extents: Vec<CensusExtent>,
}

impl GuestEngine<'static> {
    pub fn backend_name(&self) -> &'static str {
        "unicorn-x86_64"
    }

    pub fn load(image: &PeImage) -> Result<Self, GuestError> {
        let trace_points = discover_trace_points(image);
        let image_report = image.report();
        let mut trace_modules = vec![TraceModule {
            name: image_report.entry_export.clone(),
            kind: "mapped_pe",
            sha256: Some(image_report.sha256.clone()),
            symbols: vec![image_report.entry_export.clone()],
        }];
        trace_modules.extend(image_report.imports.iter().map(|library| {
            TraceModule {
                name: library.name.clone(),
                kind: "emulated_import_stubs",
                sha256: None,
                symbols: library
                    .symbols
                    .iter()
                    .map(|symbol| symbol.name.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            }
        }));
        let mut unicorn = uc(
            "create x86_64 engine",
            Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()),
        )?;
        install_avx_fallback(&mut unicorn)?;
        unicorn.get_data_mut().next_handle_data = HANDLE_DATA_BASE;
        unicorn.get_data_mut().next_aegp_memory_handle = AEGP_MEMORY_HANDLE_BASE;
        unicorn.get_data_mut().next_pf_handle_data = PF_HANDLE_DATA_BASE;
        let image_size =
            u64::try_from(image.mapped_bytes().len()).map_err(|_| GuestError::ImageAlignment)?;
        unicorn.get_data_mut().image_region = Some((
            image.image_base(),
            image
                .image_base()
                .checked_add(image_size)
                .ok_or(GuestError::DataCapacity)?,
        ));
        if image.image_base() % PAGE_SIZE != 0 || image_size % PAGE_SIZE != 0 {
            return Err(GuestError::ImageAlignment);
        }
        uc(
            "map PE image",
            unicorn.mem_map(image.image_base(), image_size, Prot::ALL),
        )?;
        uc(
            "write PE image",
            unicorn.mem_write(image.image_base(), image.mapped_bytes()),
        )?;
        uc(
            "map import stubs",
            unicorn.mem_map(STUB_BASE, STUB_SIZE, Prot::ALL),
        )?;
        uc(
            "map stack",
            unicorn.mem_map(STACK_BASE, STACK_SIZE, Prot::READ | Prot::WRITE),
        )?;
        // MSVC's x64 __chkstk reads the Windows TEB stack limit at GS:[0x10].
        // Unicorn starts with a zero GS base, so provide only the non-executable
        // first page needed by that helper.
        uc(
            "map minimal TEB page",
            unicorn.mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE),
        )?;
        uc(
            "write TEB stack limit",
            unicorn.mem_write(0x10, &STACK_BASE.to_le_bytes()),
        )?;
        uc(
            "map guest data",
            unicorn.mem_map(DATA_BASE, DATA_SIZE, Prot::READ | Prot::WRITE),
        )?;
        let mut stub_index = 0u64;
        for library in image.imports() {
            for symbol in &library.symbols {
                let stub = STUB_BASE
                    .checked_add(stub_index * STUB_STRIDE)
                    .ok_or(GuestError::StubCapacity)?;
                if stub + STUB_STRIDE > HOST_ADD_PARAM {
                    return Err(GuestError::StubCapacity);
                }
                // Temporary import behavior for the first controlled fixture:
                // return zero inside the guest. Typed import traps replace these
                // entries before guest execution; no native host address is exposed.
                uc(
                    "write import stub",
                    unicorn.mem_write(stub, &[0x31, 0xc0, 0xc3]),
                )?;
                match symbol.name.as_str() {
                    "malloc" => {
                        uc("write malloc return", unicorn.mem_write(stub, &[0xc3]))?;
                        uc(
                            "install malloc import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_crt_malloc(unicorn, false);
                            }),
                        )?;
                    }
                    "calloc" => {
                        uc("write calloc return", unicorn.mem_write(stub, &[0xc3]))?;
                        uc(
                            "install calloc import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_crt_malloc(unicorn, true);
                            }),
                        )?;
                    }
                    "free" => {
                        uc("write free return", unicorn.mem_write(stub, &[0xc3]))?;
                        uc(
                            "install free import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_crt_free(unicorn);
                            }),
                        )?;
                    }
                    "_callnewh" => {
                        // No new-handler is installed by this bounded host.
                        // Returning zero tells the MSVC allocation path not to retry.
                    }
                    "strncpy" => {
                        uc(
                            "install strncpy import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_strncpy(unicorn);
                            }),
                        )?;
                    }
                    "memset" => {
                        uc(
                            "install memset import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_memset(unicorn);
                            }),
                        )?;
                    }
                    "memcpy" | "memmove" => {
                        uc(
                            "write CRT memory-copy return",
                            unicorn.mem_write(stub, &[0xc3]),
                        )?;
                        uc(
                            "install CRT memory-copy import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_crt_memory_copy(unicorn);
                            }),
                        )?;
                    }
                    "expf" => install_float_import(&mut unicorn, stub, "expf", f32::exp)?,
                    "floorf" => install_float_import(&mut unicorn, stub, "floorf", f32::floor)?,
                    "powf" => install_float_binary_import(&mut unicorn, stub, "powf", f32::powf)?,
                    "pow" => install_double_binary_import(&mut unicorn, stub, "pow", f64::powf)?,
                    "omp_get_max_threads" => {
                        let value = deterministic_import_i32("omp_get_max_threads")
                            .expect("known deterministic import");
                        uc(
                            "install omp_get_max_threads import",
                            unicorn.mem_write(stub, &deterministic_i32_stub(value)),
                        )?;
                    }
                    "_vcomp_fork" => {
                        // Marshal the captured arguments, then tail-jump into
                        // the outlined worker so it returns to the caller.
                        uc(
                            "write _vcomp_fork tail jump",
                            unicorn.mem_write(stub, &[0x41, 0xff, 0xe3]),
                        )?;
                        uc(
                            "install _vcomp_fork import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_vcomp_fork(unicorn);
                            }),
                        )?;
                    }
                    "_vcomp_for_dynamic_init" => {
                        uc(
                            "write _vcomp_for_dynamic_init return",
                            unicorn.mem_write(stub, &[0xc3]),
                        )?;
                        uc(
                            "install _vcomp_for_dynamic_init import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_vcomp_for_dynamic_init(unicorn);
                            }),
                        )?;
                    }
                    "_vcomp_for_dynamic_next" => {
                        uc(
                            "write _vcomp_for_dynamic_next return",
                            unicorn.mem_write(stub, &[0xc3]),
                        )?;
                        uc(
                            "install _vcomp_for_dynamic_next import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_vcomp_for_dynamic_next(unicorn);
                            }),
                        )?;
                    }
                    "_vcomp_for_static_simple_init" => {
                        uc(
                            "write _vcomp_for_static_simple_init return",
                            unicorn.mem_write(stub, &[0xc3]),
                        )?;
                        uc(
                            "install _vcomp_for_static_simple_init import",
                            unicorn.add_code_hook(stub, stub, |unicorn, _, _| {
                                emulate_vcomp_for_static_simple_init(unicorn);
                            }),
                        )?;
                    }
                    "_vcomp_enter_critsect"
                    | "_vcomp_leave_critsect"
                    | "_vcomp_barrier"
                    | "_vcomp_for_static_end" => {
                        // The worker is deliberately single-threaded, so these
                        // synchronization/end helpers are deterministic no-ops.
                    }
                    name if name.starts_with("_vcomp_") => {
                        return Err(GuestError::Callback(format!(
                            "unsupported VCOMP import: {name}"
                        )));
                    }
                    _ => {}
                }
                unicorn.get_data_mut().trace_labels.insert(
                    stub,
                    TraceLabel {
                        kind: TraceLabelKind::Import,
                        name: format!("{}!{}", library.name, symbol.name),
                    },
                );
                let iat_rva = u64::try_from(symbol.iat_rva).map_err(|_| GuestError::IatRange)?;
                let iat = image
                    .image_base()
                    .checked_add(iat_rva)
                    .ok_or(GuestError::IatRange)?;
                if iat + 8 > image.image_base() + image_size {
                    return Err(GuestError::IatRange);
                }
                uc("patch IAT", unicorn.mem_write(iat, &stub.to_le_bytes()))?;
                stub_index += 1;
            }
        }
        uc(
            "write return sentinel",
            unicorn.mem_write(RETURN_ADDRESS, &[0xcc]),
        )?;
        uc(
            "write add_param callback",
            unicorn.mem_write(HOST_ADD_PARAM, &[0xc3]),
        )?;
        uc(
            "write poison callback",
            unicorn.mem_write(HOST_POISON, &[0xb8, 0xff, 0xff, 0xff, 0xff, 0xc3]),
        )?;
        uc(
            "write unsupported AEGP Memory callback",
            unicorn.mem_write(HOST_AEGP_MEMORY_UNSUPPORTED, &[0xb8, 4, 0, 0, 0, 0xc3]),
        )?;
        uc(
            "write ANSI strcpy callback",
            unicorn.mem_write(HOST_ANSI_STRCPY, &[0xc3]),
        )?;
        uc("write copy callback", unicorn.mem_write(HOST_COPY, &[0xc3]))?;
        uc(
            "write no-op callback",
            unicorn.mem_write(HOST_NOOP, &[0x31, 0xc0, 0xc3]),
        )?;
        for (operation, address) in [
            ("write pre-checkout callback", HOST_PRE_CHECKOUT_LAYER),
            ("write checkout-pixels callback", HOST_CHECKOUT_LAYER_PIXELS),
            ("write checkin-pixels callback", HOST_CHECKIN_LAYER_PIXELS),
            ("write checkout-output callback", HOST_CHECKOUT_OUTPUT),
            ("write acquire-suite callback", HOST_ACQUIRE_SUITE),
            ("write checkout-param callback", HOST_CHECKOUT_PARAM),
            ("write checkin-param callback", HOST_CHECKIN_PARAM),
            ("write new-handle callback", HOST_NEW_HANDLE),
            ("write lock-handle callback", HOST_LOCK_HANDLE),
            ("write unlock-handle callback", HOST_UNLOCK_HANDLE),
            ("write dispose-handle callback", HOST_DISPOSE_HANDLE),
            ("write handle-size callback", HOST_HANDLE_SIZE),
            ("write resize-handle callback", HOST_RESIZE_HANDLE),
            ("write AEGP register callback", HOST_AEGP_REGISTER),
            ("write AEGP main-window callback", HOST_AEGP_GET_MAIN_WINDOW),
            ("write AEGP new-memory callback", HOST_AEGP_NEW_MEM_HANDLE),
            ("write AEGP free-memory callback", HOST_AEGP_FREE_MEM_HANDLE),
            ("write AEGP lock-memory callback", HOST_AEGP_LOCK_MEM_HANDLE),
            (
                "write AEGP unlock-memory callback",
                HOST_AEGP_UNLOCK_MEM_HANDLE,
            ),
            ("write AEGP memory-size callback", HOST_AEGP_MEM_HANDLE_SIZE),
            (
                "write AEGP resize-memory callback",
                HOST_AEGP_RESIZE_MEM_HANDLE,
            ),
            ("write new-world callback", HOST_NEW_WORLD),
            ("write dispose-world callback", HOST_DISPOSE_WORLD),
            (
                "write get-world-pixel-format callback",
                HOST_GET_WORLD_PIXEL_FORMAT,
            ),
            ("write PluginData v2 callback", HOST_PLUGIN_DATA_V2),
            ("write PluginData v1 callback", HOST_PLUGIN_DATA_V1),
            ("write Iterate8 callback", HOST_ITERATE8),
            ("write Iterate8 continuation", HOST_ITERATE8_CONTINUE),
            ("write color-param callback", HOST_COLOR_PARAM_VALUE),
            ("write point-param callback", HOST_POINT_PARAM_VALUE),
            ("write extended allocation callback", HOST_EXTENDED_ALLOC),
            ("write extended free callback", HOST_EXTENDED_FREE),
            ("write extended lookup callback", HOST_EXTENDED_LOOKUP),
        ] {
            uc(operation, unicorn.mem_write(address, &[0xc3]))?;
        }
        uc(
            "install add_param callback",
            unicorn.add_code_hook(HOST_ADD_PARAM, HOST_ADD_PARAM, |unicorn, _, _| {
                capture_add_param(unicorn);
            }),
        )?;
        uc(
            "install ANSI strcpy callback",
            unicorn.add_code_hook(HOST_ANSI_STRCPY, HOST_ANSI_STRCPY, |unicorn, _, _| {
                emulate_strcpy(unicorn);
            }),
        )?;
        uc(
            "install copy callback",
            unicorn.add_code_hook(HOST_COPY, HOST_COPY, |unicorn, _, _| {
                emulate_copy(unicorn);
            }),
        )?;
        uc(
            "install pre-checkout callback",
            unicorn.add_code_hook(
                HOST_PRE_CHECKOUT_LAYER,
                HOST_PRE_CHECKOUT_LAYER,
                emulate_pre_checkout_layer,
            ),
        )?;
        uc(
            "install checkout-pixels callback",
            unicorn.add_code_hook(
                HOST_CHECKOUT_LAYER_PIXELS,
                HOST_CHECKOUT_LAYER_PIXELS,
                emulate_checkout_layer_pixels,
            ),
        )?;
        uc(
            "install checkin-pixels callback",
            unicorn.add_code_hook(
                HOST_CHECKIN_LAYER_PIXELS,
                HOST_CHECKIN_LAYER_PIXELS,
                |unicorn, _, _| {
                    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
                },
            ),
        )?;
        uc(
            "install checkout-output callback",
            unicorn.add_code_hook(
                HOST_CHECKOUT_OUTPUT,
                HOST_CHECKOUT_OUTPUT,
                emulate_checkout_output,
            ),
        )?;
        uc(
            "install acquire-suite callback",
            unicorn.add_code_hook(
                HOST_ACQUIRE_SUITE,
                HOST_ACQUIRE_SUITE,
                emulate_acquire_suite,
            ),
        )?;
        uc(
            "install AEGP register callback",
            unicorn.add_code_hook(
                HOST_AEGP_REGISTER,
                HOST_AEGP_REGISTER,
                emulate_aegp_register,
            ),
        )?;
        uc(
            "install AEGP main-window callback",
            unicorn.add_code_hook(
                HOST_AEGP_GET_MAIN_WINDOW,
                HOST_AEGP_GET_MAIN_WINDOW,
                emulate_aegp_get_main_window,
            ),
        )?;
        uc(
            "install Iterate8 callback",
            unicorn.add_code_hook(HOST_ITERATE8, HOST_ITERATE8, emulate_iterate8),
        )?;
        uc(
            "install Iterate8 continuation",
            unicorn.add_code_hook(
                HOST_ITERATE8_CONTINUE,
                HOST_ITERATE8_CONTINUE,
                continue_iterate8,
            ),
        )?;
        uc(
            "install color-param callback",
            unicorn.add_code_hook(
                HOST_COLOR_PARAM_VALUE,
                HOST_COLOR_PARAM_VALUE,
                emulate_color_param_value,
            ),
        )?;
        uc(
            "install point-param callback",
            unicorn.add_code_hook(
                HOST_POINT_PARAM_VALUE,
                HOST_POINT_PARAM_VALUE,
                emulate_point_param_value,
            ),
        )?;
        uc(
            "install PluginData v2 callback",
            unicorn.add_code_hook(HOST_PLUGIN_DATA_V2, HOST_PLUGIN_DATA_V2, |unicorn, _, _| {
                capture_plugin_data_registration(unicorn, true)
            }),
        )?;
        uc(
            "install PluginData v1 callback",
            unicorn.add_code_hook(HOST_PLUGIN_DATA_V1, HOST_PLUGIN_DATA_V1, |unicorn, _, _| {
                capture_plugin_data_registration(unicorn, false)
            }),
        )?;
        uc(
            "install checkout-param callback",
            unicorn.add_code_hook(
                HOST_CHECKOUT_PARAM,
                HOST_CHECKOUT_PARAM,
                emulate_checkout_param,
            ),
        )?;
        uc(
            "install checkin-param callback",
            unicorn.add_code_hook(HOST_CHECKIN_PARAM, HOST_CHECKIN_PARAM, |unicorn, _, _| {
                let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            }),
        )?;
        uc(
            "install extended allocation callback",
            unicorn.add_code_hook(
                HOST_EXTENDED_ALLOC,
                HOST_EXTENDED_ALLOC,
                emulate_extended_alloc,
            ),
        )?;
        uc(
            "install extended free callback",
            unicorn.add_code_hook(
                HOST_EXTENDED_FREE,
                HOST_EXTENDED_FREE,
                emulate_extended_free,
            ),
        )?;
        uc(
            "install extended lookup callback",
            unicorn.add_code_hook(
                HOST_EXTENDED_LOOKUP,
                HOST_EXTENDED_LOOKUP,
                emulate_extended_lookup,
            ),
        )?;
        for (operation, address, callback) in [
            (
                "install new-handle callback",
                HOST_NEW_HANDLE,
                emulate_new_handle as fn(&mut Unicorn<'_, GuestState>, u64, u32),
            ),
            (
                "install lock-handle callback",
                HOST_LOCK_HANDLE,
                emulate_lock_handle,
            ),
            (
                "install unlock-handle callback",
                HOST_UNLOCK_HANDLE,
                emulate_unlock_handle,
            ),
            (
                "install dispose-handle callback",
                HOST_DISPOSE_HANDLE,
                emulate_dispose_handle,
            ),
            (
                "install handle-size callback",
                HOST_HANDLE_SIZE,
                emulate_handle_size,
            ),
            (
                "install resize-handle callback",
                HOST_RESIZE_HANDLE,
                emulate_resize_handle,
            ),
            (
                "install AEGP new-memory callback",
                HOST_AEGP_NEW_MEM_HANDLE,
                emulate_aegp_new_mem_handle,
            ),
            (
                "install AEGP free-memory callback",
                HOST_AEGP_FREE_MEM_HANDLE,
                emulate_aegp_free_mem_handle,
            ),
            (
                "install AEGP lock-memory callback",
                HOST_AEGP_LOCK_MEM_HANDLE,
                emulate_aegp_lock_mem_handle,
            ),
            (
                "install AEGP unlock-memory callback",
                HOST_AEGP_UNLOCK_MEM_HANDLE,
                emulate_aegp_unlock_mem_handle,
            ),
            (
                "install AEGP memory-size callback",
                HOST_AEGP_MEM_HANDLE_SIZE,
                emulate_aegp_mem_handle_size,
            ),
            (
                "install AEGP resize-memory callback",
                HOST_AEGP_RESIZE_MEM_HANDLE,
                emulate_aegp_resize_mem_handle,
            ),
            (
                "install new-world callback",
                HOST_NEW_WORLD,
                emulate_new_world,
            ),
            (
                "install dispose-world callback",
                HOST_DISPOSE_WORLD,
                emulate_dispose_world,
            ),
            (
                "install get-world-pixel-format callback",
                HOST_GET_WORLD_PIXEL_FORMAT,
                emulate_get_world_pixel_format,
            ),
        ] {
            uc(operation, unicorn.add_code_hook(address, address, callback))?;
        }
        let mut handle_suite = [0u8; 48];
        for (offset, address) in [
            HOST_NEW_HANDLE,
            HOST_LOCK_HANDLE,
            HOST_UNLOCK_HANDLE,
            HOST_DISPOSE_HANDLE,
            HOST_HANDLE_SIZE,
            HOST_RESIZE_HANDLE,
        ]
        .into_iter()
        .enumerate()
        {
            handle_suite[offset * 8..offset * 8 + 8].copy_from_slice(&address.to_le_bytes());
        }
        uc(
            "write PF Handle Suite",
            unicorn.mem_write(HOST_HANDLE_SUITE, &handle_suite),
        )?;
        let mut aegp_memory_suite = [0u8; 64];
        for (slot, address) in [
            HOST_AEGP_NEW_MEM_HANDLE,
            HOST_AEGP_FREE_MEM_HANDLE,
            HOST_AEGP_LOCK_MEM_HANDLE,
            HOST_AEGP_UNLOCK_MEM_HANDLE,
            HOST_AEGP_MEM_HANDLE_SIZE,
            HOST_AEGP_RESIZE_MEM_HANDLE,
            HOST_AEGP_MEMORY_UNSUPPORTED,
            HOST_AEGP_MEMORY_UNSUPPORTED,
        ]
        .into_iter()
        .enumerate()
        {
            aegp_memory_suite[slot * 8..slot * 8 + 8].copy_from_slice(&address.to_le_bytes());
        }
        uc(
            "write AEGP Memory Suite",
            unicorn.mem_write(HOST_AEGP_MEMORY_SUITE, &aegp_memory_suite),
        )?;
        let mut world_suite = [0u8; 24];
        for (slot, address) in [
            HOST_NEW_WORLD,
            HOST_DISPOSE_WORLD,
            HOST_GET_WORLD_PIXEL_FORMAT,
        ]
        .into_iter()
        .enumerate()
        {
            world_suite[slot * 8..slot * 8 + 8].copy_from_slice(&address.to_le_bytes());
        }
        uc(
            "write PF World Suite",
            unicorn.mem_write(HOST_WORLD_SUITE, &world_suite),
        )?;
        install_iterate8_suites(&mut unicorn)?;
        install_pf_ansi_suite_v2(&mut unicorn)?;
        uc(
            "write PF ColorParamSuite",
            unicorn.mem_write(
                HOST_COLOR_PARAM_SUITE,
                &HOST_COLOR_PARAM_VALUE.to_le_bytes(),
            ),
        )?;
        uc(
            "write PF PointParamSuite",
            unicorn.mem_write(
                HOST_POINT_PARAM_SUITE,
                &HOST_POINT_PARAM_VALUE.to_le_bytes(),
            ),
        )?;
        install_aegp_utility_suites(&mut unicorn)?;
        for (address, name) in [
            (HOST_ADD_PARAM, "add_param"),
            (HOST_POISON, "unsupported_callback"),
            (HOST_ANSI_STRCPY, "ansi_strcpy"),
            (HOST_COPY, "copy"),
            (HOST_NOOP, "noop"),
            (HOST_PRE_CHECKOUT_LAYER, "pre_checkout_layer"),
            (HOST_CHECKOUT_LAYER_PIXELS, "checkout_layer_pixels"),
            (HOST_CHECKIN_LAYER_PIXELS, "checkin_layer_pixels"),
            (HOST_CHECKOUT_OUTPUT, "checkout_output"),
            (HOST_ACQUIRE_SUITE, "acquire_suite"),
            (HOST_CHECKOUT_PARAM, "checkout_param"),
            (HOST_CHECKIN_PARAM, "checkin_param"),
            (HOST_NEW_HANDLE, "new_handle"),
            (HOST_LOCK_HANDLE, "lock_handle"),
            (HOST_UNLOCK_HANDLE, "unlock_handle"),
            (HOST_DISPOSE_HANDLE, "dispose_handle"),
            (HOST_HANDLE_SIZE, "handle_size"),
            (HOST_RESIZE_HANDLE, "resize_handle"),
            (HOST_AEGP_REGISTER, "aegp_register_with_aegp"),
            (HOST_AEGP_GET_MAIN_WINDOW, "aegp_get_main_window"),
            (HOST_AEGP_NEW_MEM_HANDLE, "aegp_new_mem_handle"),
            (HOST_AEGP_FREE_MEM_HANDLE, "aegp_free_mem_handle"),
            (HOST_AEGP_LOCK_MEM_HANDLE, "aegp_lock_mem_handle"),
            (HOST_AEGP_UNLOCK_MEM_HANDLE, "aegp_unlock_mem_handle"),
            (HOST_AEGP_MEM_HANDLE_SIZE, "aegp_mem_handle_size"),
            (HOST_AEGP_RESIZE_MEM_HANDLE, "aegp_resize_mem_handle"),
            (HOST_NEW_WORLD, "new_world"),
            (HOST_DISPOSE_WORLD, "dispose_world"),
            (HOST_GET_WORLD_PIXEL_FORMAT, "get_world_pixel_format"),
            (HOST_PF_ANSI_ATAN, "pf_ansi_atan"),
            (HOST_PF_ANSI_ATAN2, "pf_ansi_atan2"),
            (HOST_PF_ANSI_CEIL, "pf_ansi_ceil"),
            (HOST_PF_ANSI_COS, "pf_ansi_cos"),
            (HOST_PF_ANSI_EXP, "pf_ansi_exp"),
            (HOST_PF_ANSI_FABS, "pf_ansi_fabs"),
            (HOST_PF_ANSI_FLOOR, "pf_ansi_floor"),
            (HOST_PF_ANSI_FMOD, "pf_ansi_fmod"),
            (HOST_PF_ANSI_HYPOT, "pf_ansi_hypot"),
            (HOST_PF_ANSI_LOG, "pf_ansi_log"),
            (HOST_PF_ANSI_LOG10, "pf_ansi_log10"),
            (HOST_PF_ANSI_POW, "pf_ansi_pow"),
            (HOST_PF_ANSI_SIN, "pf_ansi_sin"),
            (HOST_PF_ANSI_SQRT, "pf_ansi_sqrt"),
            (HOST_PF_ANSI_TAN, "pf_ansi_tan"),
            (HOST_PF_ANSI_SPRINTF, "pf_ansi_sprintf"),
            (HOST_PF_ANSI_STRCPY, "pf_ansi_strcpy"),
            (HOST_PF_ANSI_ASIN, "pf_ansi_asin"),
            (HOST_PF_ANSI_ACOS, "pf_ansi_acos"),
            (HOST_PF_ANSI_STRCPY_BOUNDED, "pf_ansi_strcpy_bounded"),
            (HOST_PLUGIN_DATA_V2, "plugin_data_v2"),
            (HOST_PLUGIN_DATA_V1, "plugin_data_v1"),
            (HOST_ITERATE8, "iterate8"),
            (HOST_ITERATE8_CONTINUE, "iterate8_continue"),
            (HOST_COLOR_PARAM_VALUE, "color_param_value"),
            (HOST_POINT_PARAM_VALUE, "point_param_value"),
            (HOST_EXTENDED_ALLOC, "extended_alloc"),
            (HOST_EXTENDED_FREE, "extended_free"),
            (HOST_EXTENDED_LOOKUP, "extended_lookup"),
        ] {
            unicorn.get_data_mut().trace_labels.insert(
                address,
                TraceLabel {
                    kind: TraceLabelKind::HostCallback,
                    name: name.to_string(),
                },
            );
        }
        let mut engine = Self {
            unicorn,
            next_data: DATA_BASE,
            image_base: image.image_base(),
            image_end: image.image_base() + image_size,
            census_hook: None,
            trace_hooks: Vec::new(),
            trace_points,
            image_sha256: image_report.sha256,
            entry_export: image_report.entry_export,
            trace_modules,
        };
        if let Some(table) = image.string_table() {
            let empty = engine.allocate(1, 1)?;
            engine.write(empty, &[0])?;
            let mut strings = HashMap::with_capacity(table.len());
            for (id, value) in table {
                let address = engine.allocate(value.len() + 1, 1)?;
                engine.write(address, value)?;
                engine.write(address + value.len() as u64, &[0])?;
                strings.insert(*id, address);
            }
            let state = engine.unicorn.get_data_mut();
            state.extended_strings = strings;
            state.extended_empty_string = empty;
            state.extended_string_table_valid = true;
        }
        if let Some(entry) = image.dll_entry_address() {
            let attached = engine.call_win64(entry, [image.image_base(), 1, 0, 0, 0, 0])?;
            if attached == 0 {
                return Err(GuestError::DllProcessAttach);
            }
        }
        Ok(engine)
    }

    pub fn resolve_effect_entry(
        &mut self,
        image: &PeImage,
        selector: Option<&str>,
        basic_suite: u64,
    ) -> Result<u64, GuestError> {
        if let Some(entry) = image.entry_address() {
            if selector.is_some() {
                return Err(GuestError::Callback(
                    "effect selection is unavailable when a direct effect entrypoint exists".into(),
                ));
            }
            return Ok(entry);
        }
        let (registration_entry, callback) = if let Some(entry) =
            image.export_address("PluginDataEntryFunction2")
        {
            (entry, HOST_PLUGIN_DATA_V2)
        } else if let Some(entry) = image.export_address("PluginDataEntryFunction") {
            (entry, HOST_PLUGIN_DATA_V1)
        } else {
            return Err(GuestError::Callback(
                    "effect selector requires PluginData registration, but no registration export exists"
                        .into(),
                ));
        };
        self.unicorn.get_data_mut().plugin_data_registry = EffectRegistry::default();
        self.unicorn.get_data_mut().plugin_data_error = None;
        let host_name = self.allocate(10, 1)?;
        self.write(host_name, b"AEXCompat\0")?;
        let host_version = self.allocate(5, 1)?;
        self.write(host_version, b"2025\0")?;
        let returned = self.call_win64(
            registration_entry,
            [1, callback, basic_suite, host_name, host_version, 0],
        )? as i32;
        if let Some(error) = self.unicorn.get_data_mut().plugin_data_error.take() {
            return Err(GuestError::Callback(format!(
                "PluginData registration rejected: {error}"
            )));
        }
        if returned != 0 {
            return Err(GuestError::Callback(format!(
                "PluginData entrypoint returned {returned}"
            )));
        }
        let registration = self
            .unicorn
            .get_data()
            .plugin_data_registry
            .select(selector)
            .map_err(|error| GuestError::Callback(error.to_string()))?
            .clone();
        let entry = image
            .export_address(&registration.entrypoint)
            .ok_or_else(|| {
                GuestError::Callback(format!(
                    "registered effect entrypoint {} is not an executable export",
                    registration.entrypoint
                ))
            })?;
        self.entry_export.clone_from(&registration.entrypoint);
        if let Some(module) = self.trace_modules.first_mut() {
            module.name.clone_from(&registration.entrypoint);
            module.symbols = vec![registration.entrypoint];
        }
        Ok(entry)
    }

    pub fn begin_execution_trace(
        &mut self,
        selector: &str,
        entry_address: u64,
    ) -> Result<(), GuestError> {
        if self.unicorn.get_data().trace.is_some() {
            return Err(GuestError::Callback(
                "guest execution trace is already active".into(),
            ));
        }
        let watch_specs = self.unicorn.get_data().trace_watches.clone();
        let selector_watches = watch_specs
            .iter()
            .filter_map(|spec| {
                let address = spec.absolute_address?;
                Some(PendingTraceWatch {
                    spec_id: spec.id.clone(),
                    register: spec.register,
                    call_id: None,
                    function_rva: None,
                    pc_rva: None,
                    address,
                    before: trace_memory_snapshot(
                        &self.unicorn,
                        address,
                        spec.size,
                        self.image_base,
                        self.image_end,
                    ),
                    image_coordinate: spec.image_coordinate,
                    image_row_offset: spec.image_row_offset,
                    image_format: spec.image_format,
                })
            })
            .collect();
        self.unicorn.get_data_mut().trace = Some(TraceCapture {
            selector: selector.to_string(),
            entry_rva: entry_address.saturating_sub(self.image_base),
            events: vec![TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth: 0,
                kind: "selector_enter",
                call_id: None,
                function_rva: Some(entry_address.saturating_sub(self.image_base)),
                pc_rva: Some(entry_address.saturating_sub(self.image_base)),
                target_rva: None,
                name: Some(selector.to_string()),
                arguments: Vec::new(),
                xmm_arguments: Vec::new(),
                stack_arguments: Vec::new(),
                return_value: None,
                exemplars: TraceExemplars::default(),
                call_kind: None,
                instruction_bytes: None,
            }],
            return_stack: Vec::new(),
            function_stack: vec![Some(entry_address.saturating_sub(self.image_base))],
            call_rsp_stack: Vec::new(),
            call_id_stack: Vec::new(),
            next_call_id: 1,
            watch_specs,
            watch_occurrence_counts: HashMap::new(),
            watch_stack: Vec::new(),
            selector_watches,
            witnesses: Vec::new(),
            dropped_witnesses: 0,
            basic_blocks: HashMap::new(),
            branch_edges: HashMap::new(),
            dropped_basic_blocks: 0,
            dropped_branch_edges: 0,
            previous_block: None,
            event_index: HashMap::new(),
            event_fingerprints: HashMap::new(),
            known_function_entries: HashSet::from([entry_address.saturating_sub(self.image_base)]),
            truncated: false,
            dropped_events: 0,
        });
        let image_base = self.image_base;
        let image_end = self.image_end;
        let mut hook_points = self.trace_points.clone();
        hook_points.push(entry_address);
        let label_points = self
            .unicorn
            .get_data()
            .trace_labels
            .keys()
            .copied()
            .collect::<Vec<_>>();
        hook_points.extend(label_points.iter().copied());
        for point in label_points {
            let mut bytes = [0u8; STUB_STRIDE as usize];
            if self.unicorn.mem_read(point, &mut bytes).is_err() {
                continue;
            }
            let mut decoder = Decoder::with_ip(64, &bytes, point, DecoderOptions::NONE);
            while decoder.can_decode() {
                let instruction = decoder.decode();
                if matches!(instruction.mnemonic(), Mnemonic::Ret | Mnemonic::Retf) {
                    hook_points.push(instruction.ip());
                    break;
                }
            }
        }
        hook_points.sort_unstable();
        hook_points.dedup();
        let block_hook = uc(
            "install guest trace block hook",
            self.unicorn.add_block_hook(
                image_base,
                image_end - 1,
                move |unicorn, address, size| {
                    if let Some(capture) = unicorn.get_data_mut().trace.as_mut() {
                        let block_key = (address, size);
                        if let Some(observed) = capture.basic_blocks.get_mut(&block_key) {
                            *observed += 1;
                        } else if capture.basic_blocks.len() < MAX_TRACE_BASIC_BLOCKS {
                            capture.basic_blocks.insert(block_key, 1);
                        } else {
                            capture.dropped_basic_blocks += 1;
                        }
                        if let Some(previous) = capture.previous_block.replace(address) {
                            let edge_key = (previous, address);
                            if let Some(observed) = capture.branch_edges.get_mut(&edge_key) {
                                *observed += 1;
                            } else if capture.branch_edges.len() < MAX_TRACE_BRANCH_EDGES {
                                capture.branch_edges.insert(edge_key, 1);
                            } else {
                                capture.dropped_branch_edges += 1;
                            }
                        }
                    }
                },
            ),
        )?;
        self.trace_hooks.push(block_hook);
        for point in hook_points {
            let hook = uc(
                "install guest execution trace point",
                self.unicorn
                    .add_code_hook(point, point, move |unicorn, address, size| {
                        trace_instruction(unicorn, address, size, image_base, image_end);
                    }),
            )?;
            self.trace_hooks.push(hook);
        }
        Ok(())
    }

    pub fn finish_execution_trace(
        &mut self,
        return_value: u64,
    ) -> Result<ExecutionTrace, GuestError> {
        for hook in self.trace_hooks.drain(..) {
            uc(
                "remove guest execution trace hook",
                self.unicorn.remove_hook(hook),
            )?;
        }
        let selector_watches = self
            .unicorn
            .get_data()
            .trace
            .as_ref()
            .map(|capture| capture.selector_watches.clone())
            .unwrap_or_default();
        let selector_witnesses = selector_watches
            .into_iter()
            .map(|pending| {
                let after = trace_memory_snapshot(
                    &self.unicorn,
                    pending.address,
                    pending.before.size,
                    self.image_base,
                    self.image_end,
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
            .collect::<Vec<_>>();
        let mut capture =
            self.unicorn.get_data_mut().trace.take().ok_or_else(|| {
                GuestError::Callback("guest execution trace is not active".into())
            })?;
        for witness in selector_witnesses {
            if capture.witnesses.len() >= MAX_TRACE_WITNESSES {
                capture.witnesses.pop();
                capture.dropped_witnesses += 1;
            }
            capture.witnesses.push(witness);
        }
        if capture.events.len() >= MAX_TRACE_EVENTS {
            capture.events.pop();
            capture.truncated = true;
            capture.dropped_events += 1;
        }
        let entry_rva = capture.entry_rva;
        push_trace_event(
            &mut capture,
            TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth: 0,
                kind: "selector_exit",
                call_id: None,
                function_rva: Some(entry_rva),
                pc_rva: Some(entry_rva),
                target_rva: None,
                name: Some(format!("return={return_value:#x}")),
                arguments: Vec::new(),
                xmm_arguments: Vec::new(),
                stack_arguments: Vec::new(),
                return_value: None,
                exemplars: TraceExemplars::default(),
                call_kind: None,
                instruction_bytes: None,
            },
        );
        let mut functions = aggregate_trace_functions(entry_rva, &capture.events);
        for function in &mut functions {
            let mut bytes = [0u8; 16];
            if self
                .unicorn
                .mem_read(self.image_base + function.entry_rva, &mut bytes)
                .is_ok()
            {
                function.entry_bytes = bytes_to_hex(&bytes);
            }
        }
        let timeline = capture.events.iter().map(format_trace_event).collect();
        let mut truncation = Vec::new();
        if capture.truncated {
            truncation.push(TraceTruncation {
                category: "events",
                reason: "event_budget",
                dropped: capture.dropped_events,
            });
        }
        if capture.dropped_witnesses > 0 {
            truncation.push(TraceTruncation {
                category: "memory_witnesses",
                reason: "witness_budget",
                dropped: capture.dropped_witnesses,
            });
        }
        if capture.dropped_basic_blocks > 0 {
            truncation.push(TraceTruncation {
                category: "basic_blocks",
                reason: "distinct_block_budget",
                dropped: capture.dropped_basic_blocks,
            });
        }
        if capture.dropped_branch_edges > 0 {
            truncation.push(TraceTruncation {
                category: "branch_edges",
                reason: "distinct_edge_budget",
                dropped: capture.dropped_branch_edges,
            });
        }
        let untracked_fingerprints = capture
            .events
            .iter()
            .map(|event| event.exemplars.untracked_fingerprint_observations)
            .sum();
        if untracked_fingerprints > 0 {
            truncation.push(TraceTruncation {
                category: "exemplar_fingerprints",
                reason: "fingerprint_budget",
                dropped: untracked_fingerprints,
            });
        }
        let trace_truncated = !truncation.is_empty();
        let trace_configuration = TraceConfiguration {
            max_events: MAX_TRACE_EVENTS,
            max_basic_blocks: MAX_TRACE_BASIC_BLOCKS,
            max_branch_edges: MAX_TRACE_BRANCH_EDGES,
            max_witnesses: MAX_TRACE_WITNESSES,
            max_watch_bytes: MAX_TRACE_WATCH_BYTES,
            max_distinct_fingerprints_per_event: TRACE_DISTINCT_FINGERPRINTS,
            watches: capture.watch_specs.clone(),
        };
        let mut basic_blocks = capture
            .basic_blocks
            .into_iter()
            .map(|((address, size), observed_count)| TraceBasicBlock {
                rva: address.saturating_sub(self.image_base),
                size,
                observed_count,
            })
            .collect::<Vec<_>>();
        basic_blocks.sort_by_key(|block| (block.rva, block.size));
        let mut branch_edges = capture
            .branch_edges
            .into_iter()
            .map(|((from, to), observed_count)| TraceBranchEdge {
                from_rva: from.saturating_sub(self.image_base),
                to_rva: to.saturating_sub(self.image_base),
                observed_count,
            })
            .collect::<Vec<_>>();
        branch_edges.sort_by_key(|edge| (edge.from_rva, edge.to_rva));
        Ok(ExecutionTrace {
            schema: "aexcompat.aex-execution-trace",
            schema_version: 1,
            execution_backend: self.backend_name(),
            image_sha256: self.image_sha256.clone(),
            preferred_image_base: self.image_base,
            entry_export: self.entry_export.clone(),
            worker_build_identity: format!(
                "aex-guest-worker/{} rev={} ({}/{})",
                env!("CARGO_PKG_VERSION"),
                env!("AEXCOMPAT_BUILD_REVISION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            modules: self.trace_modules.clone(),
            trace_configuration,
            selector: capture.selector,
            entry_rva: capture.entry_rva,
            return_value,
            truncated: trace_truncated,
            events: capture.events,
            functions,
            state_changes: Vec::new(),
            memory_witnesses: capture.witnesses,
            dropped_memory_witnesses: capture.dropped_witnesses,
            basic_blocks,
            branch_edges,
            truncation,
            timeline,
        })
    }

    pub fn configure_trace_watches(&mut self, watches: Vec<TraceWatchSpec>) {
        self.unicorn.get_data_mut().trace_watches = watches;
    }

    pub fn add_trace_watch(&mut self, watch: TraceWatchSpec) {
        self.unicorn.get_data_mut().trace_watches.push(watch);
    }

    pub fn begin_block_census(&mut self) -> Result<(), GuestError> {
        if self.census_hook.is_some() {
            return Err(GuestError::Callback(
                "guest block census is already active".into(),
            ));
        }
        self.unicorn.get_data_mut().census_blocks.clear();
        let hook = uc(
            "install guest block census",
            self.unicorn.add_block_hook(
                self.image_base,
                self.image_end - 1,
                |unicorn, address, size| {
                    *unicorn
                        .get_data_mut()
                        .census_blocks
                        .entry((address, size))
                        .or_default() += 1;
                },
            ),
        )?;
        self.census_hook = Some(hook);
        Ok(())
    }

    pub fn finish_block_census(&mut self, output_pixels: u64) -> Result<GuestCensus, GuestError> {
        let hook = self
            .census_hook
            .take()
            .ok_or_else(|| GuestError::Callback("guest block census is not active".into()))?;
        uc("remove guest block census", self.unicorn.remove_hook(hook))?;

        let counts = std::mem::take(&mut self.unicorn.get_data_mut().census_blocks);
        let mut blocks = Vec::with_capacity(counts.len());
        for ((address, size), executions) in counts {
            let mut bytes = vec![0u8; size as usize];
            uc(
                "read census block",
                self.unicorn.mem_read(address, &mut bytes),
            )?;
            let mut decoder = Decoder::with_ip(64, &bytes, address, DecoderOptions::NONE);
            let mut instructions = 0u32;
            let mut scalar_sse_fp_instructions = 0u32;
            while decoder.can_decode() {
                let instruction = decoder.decode();
                if instruction.is_invalid() {
                    break;
                }
                instructions += 1;
                if is_scalar_sse_fp(instruction.mnemonic()) {
                    scalar_sse_fp_instructions += 1;
                }
            }
            blocks.push(CensusBlock {
                address,
                rva: address - self.image_base,
                size_bytes: size,
                executions,
                instructions,
                dynamic_instructions: executions.saturating_mul(instructions as u64),
                scalar_sse_fp_instructions,
                dynamic_scalar_sse_fp_instructions: executions
                    .saturating_mul(scalar_sse_fp_instructions as u64),
            });
        }
        blocks.sort_by_key(|block| std::cmp::Reverse(block.dynamic_instructions));
        let estimated_dynamic_instructions = blocks
            .iter()
            .map(|block| block.dynamic_instructions)
            .sum::<u64>();
        let estimated_dynamic_scalar_sse_fp_instructions = blocks
            .iter()
            .map(|block| block.dynamic_scalar_sse_fp_instructions)
            .sum::<u64>();
        let dynamic_instructions_in_scalar_sse_blocks = blocks
            .iter()
            .filter(|block| block.scalar_sse_fp_instructions != 0)
            .map(|block| block.dynamic_instructions)
            .sum::<u64>();
        let fraction = |count: usize| {
            if estimated_dynamic_instructions == 0 {
                0.0
            } else {
                blocks
                    .iter()
                    .take(count)
                    .map(|block| block.dynamic_instructions)
                    .sum::<u64>() as f64
                    / estimated_dynamic_instructions as f64
            }
        };
        let blocks_for_80_percent = if estimated_dynamic_instructions == 0 {
            0
        } else {
            let mut cumulative = 0u64;
            blocks
                .iter()
                .position(|block| {
                    cumulative = cumulative.saturating_add(block.dynamic_instructions);
                    cumulative as f64 / estimated_dynamic_instructions as f64 >= 0.8
                })
                .map_or(blocks.len(), |index| index + 1)
        };
        let extents =
            coalesce_census_extents(&blocks, self.image_base, estimated_dynamic_instructions);
        let extent_fraction = |count: usize| {
            extents
                .iter()
                .take(count)
                .map(|extent| extent.dynamic_instruction_fraction)
                .sum()
        };
        Ok(GuestCensus {
            schema_version: 1,
            distinct_blocks: blocks.len(),
            total_block_executions: blocks.iter().map(|block| block.executions).sum(),
            estimated_dynamic_instructions,
            output_pixels,
            estimated_dynamic_instructions_per_pixel: if output_pixels == 0 {
                0.0
            } else {
                estimated_dynamic_instructions as f64 / output_pixels as f64
            },
            estimated_dynamic_scalar_sse_fp_instructions,
            scalar_sse_fp_fraction: if estimated_dynamic_instructions == 0 {
                0.0
            } else {
                estimated_dynamic_scalar_sse_fp_instructions as f64
                    / estimated_dynamic_instructions as f64
            },
            dynamic_instructions_in_scalar_sse_blocks,
            scalar_sse_block_work_fraction: if estimated_dynamic_instructions == 0 {
                0.0
            } else {
                dynamic_instructions_in_scalar_sse_blocks as f64
                    / estimated_dynamic_instructions as f64
            },
            top_1_dynamic_instruction_fraction: fraction(1),
            top_5_dynamic_instruction_fraction: fraction(5),
            top_20_dynamic_instruction_fraction: fraction(20),
            blocks_for_80_percent,
            blocks,
            distinct_extents: extents.len(),
            top_1_extent_dynamic_instruction_fraction: extent_fraction(1),
            top_2_extent_dynamic_instruction_fraction: extent_fraction(2),
            top_20_extent_dynamic_instruction_fraction: extent_fraction(20),
            extents,
        })
    }

    pub fn call_win64(&mut self, address: u64, args: [u64; 6]) -> Result<u64, GuestError> {
        self.call_win64_with_timeout(address, &args, TIMEOUT_MICROSECONDS)
    }

    fn call_win64_with_timeout(
        &mut self,
        address: u64,
        args: &[u64],
        timeout_microseconds: u64,
    ) -> Result<u64, GuestError> {
        if args.len() < 4 || args.len() > 16 {
            return Err(GuestError::Callback(format!(
                "Win64 call requires 4..=16 arguments, got {}",
                args.len()
            )));
        }
        self.unicorn.get_data_mut().avx_fallback_instructions = 0;
        self.unicorn.get_data_mut().avx_defined_ymm = [false; 16];
        self.unicorn.get_data_mut().avx_chain_next_rip = None;
        let stack_top = STACK_BASE + STACK_SIZE;
        // Win64 function entry observes RSP % 16 == 8. Reserve a return
        // address, 32-byte shadow space, bounded stack arguments, and scratch.
        let rsp = (stack_top - 0x108) | 8;
        uc(
            "write return address",
            self.unicorn.mem_write(rsp, &RETURN_ADDRESS.to_le_bytes()),
        )?;
        for (index, value) in args.iter().copied().enumerate().skip(4) {
            uc(
                "write stack argument",
                self.unicorn
                    .mem_write(rsp + 0x28 + ((index - 4) * 8) as u64, &value.to_le_bytes()),
            )?;
        }
        for (register, value) in [
            (RegisterX86::RSP, rsp),
            (RegisterX86::RCX, args[0]),
            (RegisterX86::RDX, args[1]),
            (RegisterX86::R8, args[2]),
            (RegisterX86::R9, args[3]),
        ] {
            uc(
                "write argument register",
                self.unicorn.reg_write(register, value),
            )?;
        }
        if let Err(error) = self.unicorn.emu_start(
            address,
            RETURN_ADDRESS,
            timeout_microseconds,
            MAX_INSTRUCTIONS,
        ) {
            return Err(self.execution_crash(format!("emulation error: {error}")));
        }
        let rip = uc(
            "read instruction pointer",
            self.unicorn.reg_read(RegisterX86::RIP),
        )?;
        if let Some(error) = self.unicorn.get_data_mut().callback_error.take() {
            return Err(GuestError::Callback(error));
        }
        if rip != RETURN_ADDRESS {
            return Err(self.execution_crash(format!(
                "execution stopped before the guest returned (RIP={rip:#x})"
            )));
        }
        uc("read return value", self.unicorn.reg_read(RegisterX86::RAX))
    }

    fn execution_crash(&self, reason: String) -> GuestError {
        let registers = [
            ("rax", RegisterX86::RAX),
            ("rbx", RegisterX86::RBX),
            ("rcx", RegisterX86::RCX),
            ("rdx", RegisterX86::RDX),
            ("rsi", RegisterX86::RSI),
            ("rdi", RegisterX86::RDI),
            ("rbp", RegisterX86::RBP),
            ("rsp", RegisterX86::RSP),
            ("r8", RegisterX86::R8),
            ("r9", RegisterX86::R9),
            ("r10", RegisterX86::R10),
            ("r11", RegisterX86::R11),
            ("r12", RegisterX86::R12),
            ("r13", RegisterX86::R13),
            ("r14", RegisterX86::R14),
            ("r15", RegisterX86::R15),
            ("rip", RegisterX86::RIP),
            ("rflags", RegisterX86::EFLAGS),
        ]
        .into_iter()
        .map(|(name, register)| {
            (
                name.to_string(),
                self.unicorn.reg_read(register).unwrap_or(0),
            )
        })
        .collect::<BTreeMap<_, _>>();
        let xmm_registers = [
            ("xmm0", RegisterX86::XMM0),
            ("xmm1", RegisterX86::XMM1),
            ("xmm2", RegisterX86::XMM2),
            ("xmm3", RegisterX86::XMM3),
            ("xmm4", RegisterX86::XMM4),
            ("xmm5", RegisterX86::XMM5),
            ("xmm6", RegisterX86::XMM6),
            ("xmm7", RegisterX86::XMM7),
            ("xmm8", RegisterX86::XMM8),
            ("xmm9", RegisterX86::XMM9),
            ("xmm10", RegisterX86::XMM10),
            ("xmm11", RegisterX86::XMM11),
            ("xmm12", RegisterX86::XMM12),
            ("xmm13", RegisterX86::XMM13),
            ("xmm14", RegisterX86::XMM14),
            ("xmm15", RegisterX86::XMM15),
        ]
        .into_iter()
        .filter_map(|(name, register)| read_xmm(&self.unicorn, name, register))
        .collect();
        let call_stack = self
            .unicorn
            .get_data()
            .trace
            .as_ref()
            .map(|capture| {
                capture
                    .function_stack
                    .iter()
                    .enumerate()
                    .map(|(index, function_rva)| TraceCrashFrame {
                        call_id: index
                            .checked_sub(1)
                            .and_then(|index| capture.call_id_stack.get(index).copied()),
                        function_rva: *function_rva,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let rip = *registers.get("rip").unwrap_or(&0);
        let mut instruction = [0; 32];
        let instruction_bytes = if self.unicorn.mem_read(rip, &mut instruction).is_ok() {
            bytes_to_hex(&instruction)
        } else {
            String::new()
        };
        let snapshot = TraceCrashSnapshot {
            reason: reason.clone(),
            registers,
            xmm_registers,
            call_stack,
            instruction_address: rip,
            instruction_rva: (self.image_base..self.image_end)
                .contains(&rip)
                .then(|| rip - self.image_base),
            instruction_bytes,
            handle_allocations: self.unicorn.get_data().handle_allocations.clone(),
            handle_allocation_failures: self.unicorn.get_data().handle_allocation_failures.clone(),
            live_handle_count: self.unicorn.get_data().handles.len(),
            next_pf_handle_data: self.unicorn.get_data().next_pf_handle_data,
            pf_handle_data_end: PF_HANDLE_DATA_END,
        };
        GuestError::ExecutionCrash {
            reason,
            snapshot_json: serde_json::to_string(&snapshot)
                .unwrap_or_else(|error| format!("{{\"serialization_error\":\"{error}\"}}")),
            snapshot: Box::new(snapshot),
        }
    }

    pub fn allocate(&mut self, size: usize, alignment: u64) -> Result<u64, GuestError> {
        let alignment = alignment.max(1).next_power_of_two();
        let start = self
            .next_data
            .checked_add(alignment - 1)
            .map(|value| value & !(alignment - 1))
            .ok_or(GuestError::DataCapacity)?;
        let end = start
            .checked_add(u64::try_from(size).map_err(|_| GuestError::DataCapacity)?)
            .ok_or(GuestError::DataCapacity)?;
        if end > HANDLE_DATA_BASE {
            return Err(GuestError::DataCapacity);
        }
        self.next_data = end;
        Ok(start)
    }

    pub fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), GuestError> {
        uc("write guest data", self.unicorn.mem_write(address, bytes))
    }

    pub fn read(&self, address: u64, bytes: &mut [u8]) -> Result<(), GuestError> {
        uc("read guest data", self.unicorn.mem_read(address, bytes))
    }

    pub fn write_u64(&mut self, address: u64, value: u64) -> Result<(), GuestError> {
        self.write(address, &value.to_le_bytes())
    }

    pub fn add_param_callback_address(&self) -> u64 {
        HOST_ADD_PARAM
    }

    pub fn poison_callback_address(&self) -> u64 {
        HOST_POISON
    }

    pub fn ansi_strcpy_callback_address(&self) -> u64 {
        HOST_ANSI_STRCPY
    }

    pub fn copy_callback_address(&self) -> u64 {
        HOST_COPY
    }

    pub fn noop_callback_address(&self) -> u64 {
        HOST_NOOP
    }

    pub fn acquire_suite_callback_address(&self) -> u64 {
        HOST_ACQUIRE_SUITE
    }

    pub fn checkout_param_callback_address(&self) -> u64 {
        HOST_CHECKOUT_PARAM
    }

    pub fn checkin_param_callback_address(&self) -> u64 {
        HOST_CHECKIN_PARAM
    }

    pub fn extended_alloc_callback_address(&self) -> u64 {
        HOST_EXTENDED_ALLOC
    }

    pub fn extended_free_callback_address(&self) -> u64 {
        HOST_EXTENDED_FREE
    }

    pub fn extended_lookup_callback_address(&self) -> u64 {
        HOST_EXTENDED_LOOKUP
    }

    pub fn configure_parameter_definitions(
        &mut self,
        definitions: Vec<u64>,
    ) -> Result<(), GuestError> {
        if definitions.len() != self.unicorn.get_data().params.len() {
            return Err(GuestError::Callback(
                "active parameter definition count differs from setup".into(),
            ));
        }
        let mut active_colors = Vec::with_capacity(definitions.len());
        for (definition, parameter) in definitions
            .iter()
            .copied()
            .zip(self.unicorn.get_data().params.iter())
        {
            let mut color = [0u8; abi::PF_PIXEL_SIZE];
            if parameter.param_type == 5 {
                self.unicorn
                    .mem_read(definition + abi::PARAM_U_OFFSET as u64, &mut color)
                    .map_err(|error| GuestError::Unicorn {
                        operation: "read active color parameter",
                        detail: error.to_string(),
                    })?;
                active_colors.push(Some(color));
            } else {
                active_colors.push(None);
            }
        }
        let state = self.unicorn.get_data_mut();
        for (parameter, active) in state.params.iter_mut().zip(active_colors) {
            if let Some(color) = active {
                parameter.bytes[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_PIXEL_SIZE]
                    .copy_from_slice(&color);
            }
        }
        state.parameter_definitions = definitions;
        Ok(())
    }

    pub fn suite_requests(&self) -> &[String] {
        &self.unicorn.get_data().suite_requests
    }

    pub fn unsupported_suite_calls(&self) -> &[UnsupportedSuiteCall] {
        &self.unicorn.get_data().unsupported_suite_calls
    }

    pub fn dropped_unsupported_suite_calls(&self) -> u64 {
        self.unicorn.get_data().dropped_unsupported_suite_calls
    }

    pub fn smart_callback_counts(&self) -> (u32, u32, u32) {
        let state = self.unicorn.get_data();
        (
            state.pre_checkout_calls,
            state.checkout_pixels_calls,
            state.checkout_output_calls,
        )
    }

    pub fn pre_checkout_requests(&self) -> &[[i32; 4]] {
        &self.unicorn.get_data().pre_checkout_requests
    }

    pub fn handle_allocations(&self) -> &[u64] {
        &self.unicorn.get_data().handle_allocations
    }

    pub fn pre_checkout_layer_callback_address(&self) -> u64 {
        HOST_PRE_CHECKOUT_LAYER
    }

    pub fn checkout_layer_pixels_callback_address(&self) -> u64 {
        HOST_CHECKOUT_LAYER_PIXELS
    }

    pub fn checkin_layer_pixels_callback_address(&self) -> u64 {
        HOST_CHECKIN_LAYER_PIXELS
    }

    pub fn checkout_output_callback_address(&self) -> u64 {
        HOST_CHECKOUT_OUTPUT
    }

    pub fn new_handle_callback_address(&self) -> u64 {
        HOST_NEW_HANDLE
    }

    pub fn lock_handle_callback_address(&self) -> u64 {
        HOST_LOCK_HANDLE
    }

    pub fn unlock_handle_callback_address(&self) -> u64 {
        HOST_UNLOCK_HANDLE
    }

    pub fn dispose_handle_callback_address(&self) -> u64 {
        HOST_DISPOSE_HANDLE
    }

    pub fn handle_size_callback_address(&self) -> u64 {
        HOST_HANDLE_SIZE
    }

    pub fn resize_handle_callback_address(&self) -> u64 {
        HOST_RESIZE_HANDLE
    }

    pub fn configure_smart_render(
        &mut self,
        input_world: u64,
        output_world: u64,
        width: u32,
        height: u32,
        pixel_format: i32,
    ) {
        let state = self.unicorn.get_data_mut();
        state.pre_checkout_requests.clear();
        state.smart_input_world = input_world;
        state.smart_output_world = output_world;
        state.smart_width = width;
        state.smart_height = height;
        state.smart_pixel_format = pixel_format;
    }

    pub fn parameters(&self) -> &[GuestParam] {
        &self.unicorn.get_data().params
    }
}

fn capture_add_param(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let index = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("read add_param index: {error}"))? as i32;
        let pointer = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("read add_param pointer: {error}"))?;
        let mut bytes = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        unicorn
            .mem_read(pointer, &mut bytes)
            .map_err(|error| format!("read PF_ParamDef at {pointer:#x}: {error}"))?;
        let param_type = i32::from_le_bytes(
            bytes[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
                .try_into()
                .expect("generated PF_ParamDef field is four bytes"),
        );
        let name_bytes =
            &bytes[abi::PARAM_NAME_OFFSET..abi::PARAM_NAME_OFFSET + abi::PARAM_NAME_SIZE];
        let name_end = name_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name_bytes.len());
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();
        Ok(GuestParam {
            index,
            param_type,
            name,
            bytes,
        })
    })();
    match result {
        Ok(param) => {
            unicorn.get_data_mut().params.push(param);
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn capture_plugin_data_registration(
    unicorn: &mut Unicorn<'_, GuestState>,
    includes_support_url: bool,
) {
    let result = (|| -> Result<(), String> {
        let context = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("PluginData context read failed: {error}"))?;
        if context != 1 {
            return Err(format!(
                "PluginData callback context {context:#x} is invalid"
            ));
        }
        let register = |unicorn: &Unicorn<'_, GuestState>, register| {
            unicorn
                .reg_read(register)
                .map_err(|error| format!("PluginData register read failed: {error}"))
        };
        let rsp = register(unicorn, RegisterX86::RSP)?;
        let stack_u64 = |unicorn: &Unicorn<'_, GuestState>, offset: u64| {
            let address = rsp
                .checked_add(offset)
                .ok_or_else(|| "PluginData stack address overflow".to_string())?;
            let bytes = unicorn.mem_read_as_vec(address, 8).map_err(|error| {
                format!("PluginData stack read at {address:#x} failed: {error}")
            })?;
            Ok::<u64, String>(u64::from_le_bytes(
                bytes
                    .try_into()
                    .map_err(|_| "PluginData stack read returned wrong size".to_string())?,
            ))
        };
        let pointers = RegistrationPointers {
            name: register(unicorn, RegisterX86::RDX)?,
            match_name: register(unicorn, RegisterX86::R8)?,
            category: register(unicorn, RegisterX86::R9)?,
            entrypoint: stack_u64(unicorn, 0x28)?,
            kind: stack_u64(unicorn, 0x30)? as u32 as i32,
            api_major: stack_u64(unicorn, 0x38)? as u32 as i32,
            api_minor: stack_u64(unicorn, 0x40)? as u32 as i32,
            reserved_info: stack_u64(unicorn, 0x48)? as u32 as i32,
            support_url: includes_support_url
                .then(|| stack_u64(unicorn, 0x50))
                .transpose()?,
        };
        let registration = decode_registration(pointers, |address| {
            unicorn
                .mem_read_as_vec(address, 1)
                .ok()
                .and_then(|bytes| bytes.first().copied())
        })
        .map_err(|error| error.to_string())?;
        unicorn
            .get_data_mut()
            .plugin_data_registry
            .push(registration)
            .map_err(|error| error.to_string())
    })();
    let returned = match result {
        Ok(()) => 0,
        Err(error) => {
            if unicorn.get_data().plugin_data_error.is_none() {
                unicorn.get_data_mut().plugin_data_error = Some(error);
            }
            CALLBACK_REJECTED
        }
    };
    let _ = unicorn.reg_write(RegisterX86::RAX, returned as u32 as u64);
}

fn vcomp_callback_error(unicorn: &mut Unicorn<'_, GuestState>, message: String) {
    if unicorn.get_data().callback_error.is_none() {
        unicorn.get_data_mut().callback_error = Some(message);
    }
}

fn read_vcomp_register(
    unicorn: &Unicorn<'_, GuestState>,
    register: RegisterX86,
) -> Result<u64, String> {
    unicorn
        .reg_read(register)
        .map_err(|error| format!("VCOMP register read failed: {error}"))
}

fn read_vcomp_u64(unicorn: &Unicorn<'_, GuestState>, address: u64) -> Result<u64, String> {
    let bytes = unicorn
        .mem_read_as_vec(address, 8)
        .map_err(|error| format!("VCOMP memory read at {address:#x} failed: {error}"))?;
    Ok(u64::from_le_bytes(bytes.try_into().map_err(|_| {
        "VCOMP memory read returned the wrong size".to_string()
    })?))
}

fn write_vcomp_i32(
    unicorn: &mut Unicorn<'_, GuestState>,
    address: u64,
    value: i32,
) -> Result<(), String> {
    if address == 0 {
        return Err("VCOMP output pointer is null".to_string());
    }
    unicorn
        .mem_write(address, &value.to_le_bytes())
        .map_err(|error| format!("VCOMP memory write at {address:#x} failed: {error}"))
}

fn emulate_vcomp_fork(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let worker_count = read_vcomp_register(unicorn, RegisterX86::RCX)?;
        if worker_count != 1 {
            return Err(format!(
                "VCOMP worker count {worker_count} is unsupported; expected 1"
            ));
        }
        let argument_count = usize::try_from(read_vcomp_register(unicorn, RegisterX86::RDX)?)
            .map_err(|_| "VCOMP argument count does not fit usize".to_string())?;
        if argument_count > 64 {
            return Err(format!(
                "VCOMP outlined worker argument count {argument_count} exceeds 64"
            ));
        }
        let worker = read_vcomp_register(unicorn, RegisterX86::R8)?;
        if worker == 0 {
            return Err("VCOMP outlined worker pointer is null".to_string());
        }
        unicorn
            .mem_read_as_vec(worker, 1)
            .map_err(|error| format!("VCOMP outlined worker {worker:#x} is unmapped: {error}"))?;

        let rsp = read_vcomp_register(unicorn, RegisterX86::RSP)?;
        let mut arguments = Vec::with_capacity(argument_count);
        if argument_count != 0 {
            arguments.push(read_vcomp_register(unicorn, RegisterX86::R9)?);
        }
        for index in 1..argument_count {
            let address = rsp
                .checked_add(0x20)
                .and_then(|base| base.checked_add((index as u64) * 8))
                .ok_or_else(|| "VCOMP captured argument address overflow".to_string())?;
            arguments.push(read_vcomp_u64(unicorn, address)?);
        }

        for (index, register) in [
            RegisterX86::RCX,
            RegisterX86::RDX,
            RegisterX86::R8,
            RegisterX86::R9,
        ]
        .into_iter()
        .enumerate()
        {
            unicorn
                .reg_write(register, arguments.get(index).copied().unwrap_or(0))
                .map_err(|error| format!("VCOMP worker register write failed: {error}"))?;
        }
        for (index, value) in arguments.iter().copied().enumerate().skip(4) {
            let address = rsp
                .checked_add(0x28)
                .and_then(|base| base.checked_add(((index - 4) as u64) * 8))
                .ok_or_else(|| "VCOMP worker stack argument address overflow".to_string())?;
            unicorn
                .mem_write(address, &value.to_le_bytes())
                .map_err(|error| format!("VCOMP worker stack write failed: {error}"))?;
        }
        unicorn
            .reg_write(RegisterX86::R11, worker)
            .map_err(|error| format!("VCOMP worker target write failed: {error}"))?;
        Ok(())
    })();
    if let Err(error) = result {
        vcomp_callback_error(unicorn, error);
        let _ = unicorn.reg_write(RegisterX86::R11, RETURN_ADDRESS);
    }
}

fn emulate_vcomp_for_dynamic_init(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let schedule = read_vcomp_register(unicorn, RegisterX86::RCX)?;
        let lower = read_vcomp_register(unicorn, RegisterX86::RDX)? as u32 as i32;
        let upper = read_vcomp_register(unicorn, RegisterX86::R8)? as u32 as i32;
        let step = read_vcomp_register(unicorn, RegisterX86::R9)? as u32 as i32;
        let rsp = read_vcomp_register(unicorn, RegisterX86::RSP)?;
        let chunk = read_vcomp_u64(unicorn, rsp + 0x28)? as u32 as i32;
        if schedule != 0x62 {
            return Err(format!(
                "VCOMP dynamic schedule {schedule:#x} is unsupported; expected 0x62"
            ));
        }
        if step != 1 {
            return Err(format!(
                "VCOMP dynamic loop step {step} is unsupported; expected 1"
            ));
        }
        if chunk <= 0 {
            return Err(format!(
                "VCOMP dynamic loop chunk {chunk} is invalid; expected a positive value"
            ));
        }
        unicorn.get_data_mut().vcomp_dynamic_loop = Some(VcompDynamicLoop {
            current: lower,
            upper,
            chunk,
            exhausted: false,
        });
        Ok(())
    })();
    if let Err(error) = result {
        vcomp_callback_error(unicorn, error);
    }
}

fn emulate_vcomp_for_dynamic_next(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let lower_output = read_vcomp_register(unicorn, RegisterX86::RCX)?;
        let upper_output = read_vcomp_register(unicorn, RegisterX86::RDX)?;
        let loop_state = unicorn
            .get_data()
            .vcomp_dynamic_loop
            .clone()
            .ok_or_else(|| "VCOMP dynamic next called before dynamic init".to_string())?;
        if loop_state.exhausted || loop_state.current > loop_state.upper {
            unicorn
                .reg_write(RegisterX86::RAX, 0)
                .map_err(|error| format!("VCOMP return write failed: {error}"))?;
            return Ok(());
        }
        let chunk_end = loop_state
            .current
            .checked_add(loop_state.chunk - 1)
            .unwrap_or(i32::MAX)
            .min(loop_state.upper);
        write_vcomp_i32(unicorn, lower_output, loop_state.current)?;
        write_vcomp_i32(unicorn, upper_output, chunk_end)?;
        if let Some(loop_state) = unicorn.get_data_mut().vcomp_dynamic_loop.as_mut() {
            if chunk_end == loop_state.upper {
                loop_state.exhausted = true;
            } else {
                loop_state.current = chunk_end + 1;
            }
        }
        unicorn
            .reg_write(RegisterX86::RAX, 1)
            .map_err(|error| format!("VCOMP return write failed: {error}"))?;
        Ok(())
    })();
    if let Err(error) = result {
        vcomp_callback_error(unicorn, error);
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    }
}

fn emulate_vcomp_for_static_simple_init(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let lower = read_vcomp_register(unicorn, RegisterX86::RCX)? as u32 as i32;
        let upper = read_vcomp_register(unicorn, RegisterX86::RDX)? as u32 as i32;
        let step = read_vcomp_register(unicorn, RegisterX86::R8)? as u32 as i32;
        let increment = read_vcomp_register(unicorn, RegisterX86::R9)? as u32 as i32;
        if step != 1 || increment != 1 {
            return Err(format!(
                "VCOMP static loop step/increment {step}/{increment} is unsupported; expected 1/1"
            ));
        }
        let rsp = read_vcomp_register(unicorn, RegisterX86::RSP)?;
        let lower_output = read_vcomp_u64(unicorn, rsp + 0x28)?;
        let upper_output = read_vcomp_u64(unicorn, rsp + 0x30)?;
        write_vcomp_i32(unicorn, lower_output, lower)?;
        write_vcomp_i32(unicorn, upper_output, upper)?;
        Ok(())
    })();
    if let Err(error) = result {
        vcomp_callback_error(unicorn, error);
    }
}

fn install_float_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    address: u64,
    name: &'static str,
    operation: fn(f32) -> f32,
) -> Result<(), GuestError> {
    uc(
        "install unary float import",
        unicorn.add_code_hook(address, address, move |unicorn, _, _| {
            if let Ok(bits) = unicorn.reg_read(RegisterX86::XMM0) {
                let value = f32::from_bits(bits as u32);
                let output = operation(value);
                if unicorn.get_data().math_calls.len() < 32 {
                    unicorn
                        .get_data_mut()
                        .math_calls
                        .push(format!("{name}({value})={output}"));
                }
                let _ = unicorn.reg_write(RegisterX86::XMM0, output.to_bits() as u64);
            }
        }),
    )
    .map(|_| ())
}

fn deterministic_import_i32(name: &str) -> Option<i32> {
    match name {
        // The emulator is deliberately single-threaded. Returning one keeps
        // OpenMP-aware kernels deterministic while preserving the API's
        // required positive thread-count contract.
        "omp_get_max_threads" => Some(1),
        _ => None,
    }
}

fn deterministic_i32_stub(value: i32) -> [u8; 6] {
    let bytes = value.to_le_bytes();
    [0xb8, bytes[0], bytes[1], bytes[2], bytes[3], 0xc3]
}

fn install_float_binary_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    address: u64,
    name: &'static str,
    operation: fn(f32, f32) -> f32,
) -> Result<(), GuestError> {
    uc(
        "install binary float import",
        unicorn.add_code_hook(address, address, move |unicorn, _, _| {
            if let (Ok(left), Ok(right)) = (
                unicorn.reg_read(RegisterX86::XMM0),
                unicorn.reg_read(RegisterX86::XMM1),
            ) {
                let value = operation(f32::from_bits(left as u32), f32::from_bits(right as u32));
                if unicorn.get_data().math_calls.len() < 32 {
                    unicorn.get_data_mut().math_calls.push(format!(
                        "{name}({},{})={value}",
                        f32::from_bits(left as u32),
                        f32::from_bits(right as u32)
                    ));
                }
                let _ = unicorn.reg_write(RegisterX86::XMM0, value.to_bits() as u64);
            }
        }),
    )
    .map(|_| ())
}

fn install_double_binary_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    address: u64,
    name: &'static str,
    operation: fn(f64, f64) -> f64,
) -> Result<(), GuestError> {
    uc(
        "install binary double import",
        unicorn.add_code_hook(address, address, move |unicorn, _, _| {
            if let (Ok(left), Ok(right)) = (
                unicorn.reg_read(RegisterX86::XMM0),
                unicorn.reg_read(RegisterX86::XMM1),
            ) {
                let value = operation(f64::from_bits(left), f64::from_bits(right));
                if unicorn.get_data().math_calls.len() < 32 {
                    unicorn.get_data_mut().math_calls.push(format!(
                        "{name}({},{})={value}",
                        f64::from_bits(left),
                        f64::from_bits(right)
                    ));
                }
                let _ = unicorn.reg_write(RegisterX86::XMM0, value.to_bits());
            }
        }),
    )
    .map(|_| ())
}

fn emulate_crt_malloc(unicorn: &mut Unicorn<'_, GuestState>, calloc: bool) {
    let requested_size = if calloc {
        let count = unicorn.reg_read(RegisterX86::RCX);
        let element_size = unicorn.reg_read(RegisterX86::RDX);
        match (count, element_size) {
            (Ok(count), Ok(element_size)) => CrtHeap::checked_calloc_size(count, element_size),
            _ => Err(CrtHeapError::SizeOverflow),
        }
    } else {
        unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|_| CrtHeapError::SizeOverflow)
    };
    let pointer = requested_size.and_then(|size| allocate_crt_region(unicorn, size));
    // malloc/calloc report normal bounded allocation failure as NULL. This is
    // distinct from free ownership violations, which stop at the callback boundary.
    let _ = unicorn.reg_write(RegisterX86::RAX, pointer.unwrap_or(0));
}

fn allocate_crt_region(
    unicorn: &mut Unicorn<'_, GuestState>,
    size: u64,
) -> Result<u64, CrtHeapError> {
    let allocation = unicorn.get_data().crt_heap.prepare_allocation(size)?;
    let pointer = unicorn
        .get_data()
        .crt_heap
        .first_fit(CRT_HEAP_BASE, CRT_HEAP_END, allocation)?;
    unicorn
        .mem_map(pointer, allocation.backing_size, Prot::READ | Prot::WRITE)
        .map_err(|_| CrtHeapError::AddressSpaceExhausted)?;
    if let Err(error) = unicorn.get_data_mut().crt_heap.insert(pointer, allocation) {
        let _ = unicorn.mem_unmap(pointer, allocation.backing_size);
        return Err(error);
    }
    Ok(pointer)
}

fn free_crt_region(unicorn: &mut Unicorn<'_, GuestState>, pointer: u64) -> Result<(), String> {
    let allocation = unicorn
        .get_data_mut()
        .crt_heap
        .remove(pointer)
        .map_err(|error| error.to_string())?;
    unicorn
        .mem_unmap(pointer, allocation.backing_size)
        .map_err(|error| format!("unmap CRT allocation {pointer:#x}: {error}"))
}

fn emulate_extended_alloc(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let (output, size) = match (
        unicorn.reg_read(RegisterX86::RCX),
        unicorn.reg_read(RegisterX86::RDX),
    ) {
        (Ok(output), Ok(size)) => (output, size),
        (output, size) => {
            unicorn.get_data_mut().callback_error = Some(format!(
                "read extended allocation arguments: output={output:?}, size={size:?}"
            ));
            let _ = unicorn.emu_stop();
            return;
        }
    };
    if output == 0 || size == 0 || size > 1 << 24 {
        let _ = unicorn.reg_write(RegisterX86::RAX, 4);
        return;
    }
    let pointer = match allocate_crt_region(unicorn, size) {
        Ok(pointer) => pointer,
        Err(_) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 4);
            return;
        }
    };
    if let Err(error) = unicorn.mem_write(output, &pointer.to_le_bytes()) {
        let _ = free_crt_region(unicorn, pointer);
        unicorn.get_data_mut().callback_error =
            Some(format!("extended allocation output write: {error}"));
        let _ = unicorn.emu_stop();
        return;
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_extended_free(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let pointer_address = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("extended free pointer address: {error}"))?;
        if pointer_address == 0 {
            return Ok(());
        }
        let mut pointer = [0u8; 8];
        unicorn
            .mem_read(pointer_address, &mut pointer)
            .map_err(|error| format!("extended free pointer read: {error}"))?;
        let pointer = u64::from_le_bytes(pointer);
        if pointer != 0 {
            free_crt_region(unicorn, pointer)?;
        }
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn emulate_extended_lookup(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let id = match unicorn.reg_read(RegisterX86::RDX) {
        Ok(id) => id as i32,
        Err(error) => {
            unicorn.get_data_mut().callback_error =
                Some(format!("read extended lookup id: {error}"));
            let _ = unicorn.emu_stop();
            return;
        }
    };
    let state = unicorn.get_data();
    let pointer = state
        .extended_strings
        .get(&id)
        .copied()
        .or_else(|| {
            state
                .extended_string_table_valid
                .then_some(state.extended_empty_string)
        })
        .unwrap_or(0);
    let _ = unicorn.reg_write(RegisterX86::RAX, pointer);
}

fn emulate_crt_free(unicorn: &mut Unicorn<'_, GuestState>) {
    let pointer = match unicorn.reg_read(RegisterX86::RCX) {
        Ok(pointer) => pointer,
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(format!("read CRT free pointer: {error}"));
            let _ = unicorn.emu_stop();
            return;
        }
    };
    if pointer == 0 {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    if let Err(error) = free_crt_region(unicorn, pointer) {
        unicorn.get_data_mut().callback_error = Some(error);
        let _ = unicorn.emu_stop();
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_memset(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("memset destination: {error}"))?;
        let value = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("memset value: {error}"))? as u8;
        let length = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("memset length: {error}"))?;
        if length > DATA_SIZE {
            return Err(format!("memset length exceeds guest data bound: {length}"));
        }
        let bytes = vec![value; length as usize];
        unicorn
            .mem_write(destination, &bytes)
            .map_err(|error| format!("memset write: {error}"))?;
        Ok(destination)
    })();
    match result {
        Ok(destination) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, destination);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_crt_memory_copy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("memory-copy destination: {error}"))?;
        let source = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("memory-copy source: {error}"))?;
        let length = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("memory-copy length: {error}"))?;
        if length > MAX_CRT_MEMORY_COPY_BYTES {
            return Err(format!(
                "memory-copy length {length} exceeds {MAX_CRT_MEMORY_COPY_BYTES}"
            ));
        }
        if length == 0 {
            return Ok(destination);
        }
        let source_end = source
            .checked_add(length)
            .ok_or_else(|| "memory-copy source range overflow".to_string())?;
        destination
            .checked_add(length)
            .ok_or_else(|| "memory-copy destination range overflow".to_string())?;
        let copy_backward = destination > source && destination < source_end;
        let mut copied = 0u64;
        while copied < length {
            let chunk = (length - copied).min(CRT_MEMORY_COPY_CHUNK as u64);
            let offset = if copy_backward {
                length - copied - chunk
            } else {
                copied
            };
            let source_address = source
                .checked_add(offset)
                .ok_or_else(|| "memory-copy source address overflow".to_string())?;
            let destination_address = destination
                .checked_add(offset)
                .ok_or_else(|| "memory-copy destination address overflow".to_string())?;
            let mut bytes = vec![0u8; chunk as usize];
            unicorn
                .mem_read(source_address, &mut bytes)
                .map_err(|error| format!("memory-copy source read: {error}"))?;
            unicorn
                .mem_write(destination_address, &bytes)
                .map_err(|error| format!("memory-copy destination write: {error}"))?;
            copied += chunk;
        }
        Ok(destination)
    })();
    match result {
        Ok(destination) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, destination);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_strncpy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("strncpy destination: {error}"))?;
        let source = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("strncpy source: {error}"))?;
        let count = usize::try_from(
            unicorn
                .reg_read(RegisterX86::R8)
                .map_err(|error| format!("strncpy count: {error}"))?,
        )
        .map_err(|_| "strncpy count does not fit usize".to_string())?;
        if count > 4096 {
            return Err(format!("strncpy count {count} exceeds 4096"));
        }
        let mut output = vec![0u8; count];
        let mut terminated = false;
        for (index, byte) in output.iter_mut().enumerate() {
            if terminated {
                *byte = 0;
                continue;
            }
            let mut source_byte = [0u8; 1];
            unicorn
                .mem_read(source + index as u64, &mut source_byte)
                .map_err(|error| format!("strncpy source read: {error}"))?;
            *byte = source_byte[0];
            terminated = source_byte[0] == 0;
        }
        unicorn
            .mem_write(destination, &output)
            .map_err(|error| format!("strncpy destination write: {error}"))?;
        Ok(destination)
    })();
    match result {
        Ok(destination) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, destination);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_strcpy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("strcpy destination: {error}"))?;
        let source = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("strcpy source: {error}"))?;
        if destination == 0 || source == 0 {
            return Ok(0);
        }
        let mut output = Vec::new();
        for index in 0..4096u64 {
            let mut byte = [0u8; 1];
            let source_address = source
                .checked_add(index)
                .ok_or_else(|| "strcpy source range overflow".to_string())?;
            unicorn
                .mem_read(source_address, &mut byte)
                .map_err(|error| format!("strcpy source read: {error}"))?;
            output.push(byte[0]);
            if byte[0] == 0 {
                unicorn
                    .mem_write(destination, &output)
                    .map_err(|error| format!("strcpy destination write: {error}"))?;
                return Ok(destination);
            }
        }
        Ok(0)
    })();
    match result {
        Ok(destination) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, destination);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_ansi_strcpy_bounded(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("PF ANSI bounded strcpy destination: {error}"))?;
        let destination_size = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("PF ANSI bounded strcpy size: {error}"))?;
        let source = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("PF ANSI bounded strcpy source: {error}"))?;
        if destination == 0 || source == 0 || destination_size == 0 {
            return Ok(4);
        }
        let mut source_bytes = Vec::new();
        for index in 0..4096u64 {
            let mut byte = [0u8; 1];
            let source_address = source
                .checked_add(index)
                .ok_or_else(|| "PF ANSI bounded strcpy source range overflow".to_string())?;
            unicorn
                .mem_read(source_address, &mut byte)
                .map_err(|error| format!("PF ANSI bounded strcpy source read: {error}"))?;
            if byte[0] == 0 {
                let copied = (source_bytes.len() as u64).min(destination_size - 1) as usize;
                source_bytes.truncate(copied);
                source_bytes.push(0);
                unicorn
                    .mem_write(destination, &source_bytes)
                    .map_err(|error| {
                        format!("PF ANSI bounded strcpy destination write: {error}")
                    })?;
                return Ok(0);
            }
            source_bytes.push(byte[0]);
        }
        Ok(4)
    })();
    match result {
        Ok(error) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, error);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 4);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_copy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let source = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("copy source world: {error}"))?;
        let destination = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("copy destination world: {error}"))?;
        let read_u64 = |unicorn: &Unicorn<'_, GuestState>, address| {
            let mut bytes = [0u8; 8];
            unicorn
                .mem_read(address, &mut bytes)
                .map_err(|error| format!("copy world pointer read: {error}"))?;
            Ok::<u64, String>(u64::from_le_bytes(bytes))
        };
        let read_i32 = |unicorn: &Unicorn<'_, GuestState>, address| {
            let mut bytes = [0u8; 4];
            unicorn
                .mem_read(address, &mut bytes)
                .map_err(|error| format!("copy world field read: {error}"))?;
            Ok::<i32, String>(i32::from_le_bytes(bytes))
        };
        let source_data = read_u64(unicorn, source + abi::LAYER_DATA_OFFSET as u64)?;
        let destination_data = read_u64(unicorn, destination + abi::LAYER_DATA_OFFSET as u64)?;
        let source_rowbytes =
            read_i32(unicorn, source + abi::LAYER_ROWBYTES_OFFSET as u64)?.max(0) as usize;
        let destination_rowbytes =
            read_i32(unicorn, destination + abi::LAYER_ROWBYTES_OFFSET as u64)?.max(0) as usize;
        let height = read_i32(unicorn, source + abi::LAYER_HEIGHT_OFFSET as u64)?
            .min(read_i32(
                unicorn,
                destination + abi::LAYER_HEIGHT_OFFSET as u64,
            )?)
            .max(0) as usize;
        let row_size = source_rowbytes.min(destination_rowbytes);
        for row in 0..height {
            let mut bytes = vec![0u8; row_size];
            unicorn
                .mem_read(source_data + (row * source_rowbytes) as u64, &mut bytes)
                .map_err(|error| format!("copy source pixels: {error}"))?;
            unicorn
                .mem_write(
                    destination_data + (row * destination_rowbytes) as u64,
                    &bytes,
                )
                .map_err(|error| format!("copy destination pixels: {error}"))?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_pre_checkout_layer(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    unicorn.get_data_mut().pre_checkout_calls += 1;
    let result = (|| {
        let index = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("pre-checkout index: {error}"))? as i32;
        let checkout_id = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("pre-checkout id: {error}"))? as i32;
        if index != 0 || checkout_id != 0 {
            return Err(format!(
                "unsupported smart checkout index={index} id={checkout_id}"
            ));
        }
        let request_pointer = unicorn
            .reg_read(RegisterX86::R9)
            .map_err(|error| format!("pre-checkout request: {error}"))?;
        if request_pointer == 0 {
            return Err("pre-checkout request is null".to_string());
        }
        let mut request_rect_bytes = [0u8; 16];
        unicorn
            .mem_read(request_pointer, &mut request_rect_bytes)
            .map_err(|error| format!("pre-checkout request rect: {error}"))?;
        let mut request_rect = [0i32; 4];
        for (index, value) in request_rect.iter_mut().enumerate() {
            let offset = index * 4;
            *value = i32::from_le_bytes(
                request_rect_bytes[offset..offset + 4]
                    .try_into()
                    .expect("render request rectangle element is four bytes"),
            );
        }
        unicorn
            .get_data_mut()
            .pre_checkout_requests
            .push(request_rect);
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("pre-checkout stack: {error}"))?;
        let mut result_pointer = [0u8; 8];
        unicorn
            .mem_read(rsp + 0x40, &mut result_pointer)
            .map_err(|error| format!("pre-checkout result pointer: {error}"))?;
        let result_pointer = u64::from_le_bytes(result_pointer);
        if result_pointer == 0 {
            return Err("pre-checkout result is null".to_string());
        }
        let state = unicorn.get_data();
        let width = state.smart_width as i32;
        let height = state.smart_height as i32;
        let mut bytes = [0u8; 76];
        for (offset, value) in [
            (0, 0),
            (4, 0),
            (8, width),
            (12, height),
            (16, 0),
            (20, 0),
            (24, width),
            (28, height),
            (32, 1),
            (36, 1),
            (44, width),
            (48, height),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        unicorn
            .mem_write(result_pointer, &bytes)
            .map_err(|error| format!("pre-checkout result write: {error}"))?;
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn emulate_checkout_layer_pixels(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    unicorn.get_data_mut().checkout_pixels_calls += 1;
    let result = (|| {
        let checkout_id = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("checkout-pixels id: {error}"))?
            as i32;
        let output = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("checkout-pixels output: {error}"))?;
        let input_world = unicorn.get_data().smart_input_world;
        if checkout_id != 0 || output == 0 || input_world == 0 {
            return Err(format!(
                "invalid checkout-pixels id={checkout_id} output={output:#x}"
            ));
        }
        unicorn
            .mem_write(output, &input_world.to_le_bytes())
            .map_err(|error| format!("checkout-pixels world write: {error}"))?;
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn emulate_checkout_output(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    unicorn.get_data_mut().checkout_output_calls += 1;
    let result = (|| {
        let output = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("checkout-output pointer: {error}"))?;
        let output_world = unicorn.get_data().smart_output_world;
        if output == 0 || output_world == 0 {
            return Err(format!("invalid checkout-output pointer={output:#x}"));
        }
        unicorn
            .mem_write(output, &output_world.to_le_bytes())
            .map_err(|error| format!("checkout-output world write: {error}"))?;
        Ok(())
    })();
    finish_callback(unicorn, result);
}

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

fn schedule_iterate8_pixel(unicorn: &mut Unicorn<'_, GuestState>) -> Result<(), String> {
    let pending = unicorn
        .get_data()
        .pending_iterate8
        .as_ref()
        .cloned()
        .ok_or_else(|| "Iterate8 continuation has no pending call".to_string())?;
    let input = if pending.source_data == 0 {
        0
    } else {
        pending.source_data + pending.y as u64 * pending.source_rowbytes + pending.x as u64 * 4
    };
    let output = pending.destination_data
        + pending.y as u64 * pending.destination_rowbytes
        + pending.x as u64 * 4;
    let callback_rsp = pending
        .caller_rsp
        .checked_sub(0x30)
        .ok_or_else(|| "Iterate8 callback stack underflow".to_string())?;
    unicorn
        .mem_write(callback_rsp, &HOST_ITERATE8_CONTINUE.to_le_bytes())
        .map_err(|error| format!("Iterate8 callback return address: {error}"))?;
    unicorn
        .mem_write(callback_rsp + 0x28, &output.to_le_bytes())
        .map_err(|error| format!("Iterate8 callback output argument: {error}"))?;
    for (register, value) in [
        (RegisterX86::RSP, callback_rsp),
        (RegisterX86::RCX, pending.refcon),
        (RegisterX86::RDX, pending.x as u32 as u64),
        (RegisterX86::R8, pending.y as u32 as u64),
        (RegisterX86::R9, input),
        (RegisterX86::RIP, pending.pixel_function),
    ] {
        unicorn
            .reg_write(register, value)
            .map_err(|error| format!("Iterate8 callback register: {error}"))?;
    }
    Ok(())
}

fn finish_iterate8(unicorn: &mut Unicorn<'_, GuestState>, result: u64) -> Result<(), String> {
    let pending = unicorn
        .get_data_mut()
        .pending_iterate8
        .take()
        .ok_or_else(|| "Iterate8 completion has no pending call".to_string())?;
    for (register, value) in [
        (RegisterX86::RSP, pending.caller_rsp + 8),
        (RegisterX86::RIP, pending.return_address),
        (RegisterX86::RAX, result),
    ] {
        unicorn
            .reg_write(register, value)
            .map_err(|error| format!("Iterate8 completion register: {error}"))?;
    }
    Ok(())
}

fn emulate_iterate8(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        if unicorn.get_data().pending_iterate8.is_some() {
            return Err("nested PF Iterate8 calls are unsupported".to_string());
        }
        let caller_rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("Iterate8 stack: {error}"))?;
        let return_address = read_guest_u64(unicorn, caller_rsp, "Iterate8 return address")?;
        let source_world = unicorn
            .reg_read(RegisterX86::R9)
            .map_err(|error| format!("Iterate8 source world: {error}"))?;
        let area = read_guest_u64(unicorn, caller_rsp + 0x28, "Iterate8 area")?;
        let refcon = read_guest_u64(unicorn, caller_rsp + 0x30, "Iterate8 refcon")?;
        let pixel_function = read_guest_u64(unicorn, caller_rsp + 0x38, "Iterate8 pixel callback")?;
        let destination_world =
            read_guest_u64(unicorn, caller_rsp + 0x40, "Iterate8 destination world")?;
        if pixel_function == 0 || destination_world == 0 {
            unicorn
                .reg_write(RegisterX86::RAX, 4)
                .map_err(|error| format!("Iterate8 invalid-call return: {error}"))?;
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
        let (source_data, source_rowbytes, width, height) = if source_world == 0 {
            (0, 0, destination_width, destination_height)
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
            (
                data,
                rowbytes,
                source_width.min(destination_width),
                source_height.min(destination_height),
            )
        };
        if destination_data == 0
            || source_world != 0 && source_data == 0
            || source_world != 0 && source_rowbytes < width.saturating_mul(4)
            || destination_rowbytes < width.saturating_mul(4)
            || width <= 0
            || height <= 0
        {
            unicorn
                .reg_write(RegisterX86::RAX, 4)
                .map_err(|error| format!("Iterate8 invalid-world return: {error}"))?;
            return Ok(());
        }
        let mut bounds = [0, 0, width, height];
        if area != 0 {
            for (index, value) in bounds.iter_mut().enumerate() {
                *value = read_guest_i32(unicorn, area + (index * 4) as u64, "Iterate8 area field")?;
            }
            bounds[0] = bounds[0].clamp(0, width);
            bounds[1] = bounds[1].clamp(0, height);
            bounds[2] = bounds[2].clamp(bounds[0], width);
            bounds[3] = bounds[3].clamp(bounds[1], height);
        }
        if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
            unicorn
                .reg_write(RegisterX86::RAX, 4)
                .map_err(|error| format!("Iterate8 empty-area return: {error}"))?;
            return Ok(());
        }
        unicorn.get_data_mut().pending_iterate8 = Some(PendingIterate8 {
            caller_rsp,
            return_address,
            refcon,
            pixel_function,
            source_data,
            source_rowbytes: source_rowbytes as u64,
            destination_data,
            destination_rowbytes: destination_rowbytes as u64,
            left: bounds[0],
            right: bounds[2],
            bottom: bounds[3],
            x: bounds[0],
            y: bounds[1],
        });
        schedule_iterate8_pixel(unicorn)
    })();
    if let Err(error) = result {
        unicorn.get_data_mut().callback_error = Some(error);
        let _ = unicorn.emu_stop();
    }
}

fn continue_iterate8(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let callback_error = unicorn
            .reg_read(RegisterX86::RAX)
            .map_err(|error| format!("Iterate8 callback return: {error}"))?;
        if callback_error as u32 != 0 {
            return finish_iterate8(unicorn, callback_error as u32 as u64);
        }
        let pending = unicorn
            .get_data_mut()
            .pending_iterate8
            .as_mut()
            .ok_or_else(|| "Iterate8 continuation has no pending call".to_string())?;
        pending.x += 1;
        if pending.x >= pending.right {
            pending.x = pending.left;
            pending.y += 1;
        }
        if pending.y >= pending.bottom {
            finish_iterate8(unicorn, 0)
        } else {
            schedule_iterate8_pixel(unicorn)
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
            |unicorn, _, _| {
                if unicorn.get_data().callback_error.is_none() {
                    unicorn.get_data_mut().callback_error =
                        Some("PF ANSI Suite v2 sprintf is unsupported".into());
                }
                let _ = unicorn.reg_write(RegisterX86::RAX, u32::MAX as u64);
                let _ = unicorn.emu_stop();
            },
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
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    if name == "AEGP Memory Suite"
        && version == 1
        && output != 0
        && unicorn
            .mem_write(output, &HOST_AEGP_MEMORY_SUITE.to_le_bytes())
            .is_ok()
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    if name == "PF World Suite"
        && version == 2
        && output != 0
        && unicorn
            .mem_write(output, &HOST_WORLD_SUITE.to_le_bytes())
            .is_ok()
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    if name == "PF ANSI Suite"
        && version == 2
        && output != 0
        && unicorn
            .mem_write(output, &HOST_PF_ANSI_SUITE_V2.to_le_bytes())
            .is_ok()
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    if name == "PF Iterate8 Suite"
        && output != 0
        && let Some(table) = iterate8_suite_table_address(version)
        && unicorn.mem_write(output, &table.to_le_bytes()).is_ok()
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    if name == "PF ColorParamSuite"
        && version == 1
        && output != 0
        && unicorn
            .mem_write(output, &HOST_COLOR_PARAM_SUITE.to_le_bytes())
            .is_ok()
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    if name == "PF PointParamSuite"
        && version == 1
        && output != 0
        && unicorn
            .mem_write(output, &HOST_POINT_PARAM_SUITE.to_le_bytes())
            .is_ok()
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    if name == "AEGP Utility Suite"
        && output != 0
        && let Some(table) = u32::try_from(version)
            .ok()
            .and_then(utility_suite_table_address)
        && unicorn.mem_write(output, &table.to_le_bytes()).is_ok()
    {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, u32::MAX as u64);
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
        let source = index
            .checked_sub(1)
            .and_then(|offset| unicorn.get_data().parameter_definitions.get(offset))
            .copied()
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
    let valid = unicorn
        .get_data_mut()
        .handles
        .get_mut(&handle)
        .is_some_and(|record| {
            if record.locks == 0 {
                false
            } else {
                record.locks -= 1;
                true
            }
        });
    if !valid {
        unicorn.get_data_mut().callback_error = Some(format!(
            "PF Handle unlock received stale or unlocked handle {handle:#x}"
        ));
        let _ = unicorn.emu_stop();
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_dispose_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let record = unicorn.get_data().handles.get(&handle).cloned();
    match record {
        Some(record) if record.locks == 0 => {
            unicorn.get_data_mut().handles.remove(&handle);
            if let Err(error) = unicorn.mem_unmap(record.data_region, record.data_mapped_size) {
                unicorn.get_data_mut().callback_error =
                    Some(format!("PF Handle data unmap failed: {error}"));
                let _ = unicorn.emu_stop();
            }
            if let Err(error) = unicorn.mem_unmap(record.handle_region, PAGE_SIZE) {
                unicorn.get_data_mut().callback_error =
                    Some(format!("PF Handle header unmap failed: {error}"));
                let _ = unicorn.emu_stop();
            }
        }
        _ => {
            unicorn.get_data_mut().callback_error = Some(format!(
                "PF Handle dispose received stale or locked handle {handle:#x}"
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
    let result = (|| {
        let width = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("new-world width: {error}"))? as u32
            as i32;
        let height = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("new-world height: {error}"))? as u32
            as i32;
        let clear = unicorn
            .reg_read(RegisterX86::R9)
            .map_err(|error| format!("new-world clear flag: {error}"))? as u8
            != 0;
        let pixel_format = aegp_stack_arg(unicorn, 0x28)? as u32 as i32;
        let world = aegp_stack_arg(unicorn, 0x30)?;
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
        .worlds
        .get(&world)
        .map(|record| record.pixel_format)
        .or_else(|| {
            (world != 0
                && (world == unicorn.get_data().smart_input_world
                    || world == unicorn.get_data().smart_output_world))
                .then_some(unicorn.get_data().smart_pixel_format)
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

fn finish_callback(unicorn: &mut Unicorn<'_, GuestState>, result: Result<(), String>) {
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.reg_write(RegisterX86::RAX, 4);
            let _ = unicorn.emu_stop();
        }
    }
}

fn is_scalar_sse_fp(mnemonic: Mnemonic) -> bool {
    matches!(
        mnemonic,
        Mnemonic::Addss
            | Mnemonic::Subss
            | Mnemonic::Mulss
            | Mnemonic::Divss
            | Mnemonic::Sqrtss
            | Mnemonic::Minss
            | Mnemonic::Maxss
            | Mnemonic::Comiss
            | Mnemonic::Ucomiss
            | Mnemonic::Cvtsi2ss
            | Mnemonic::Cvtss2si
            | Mnemonic::Cvttss2si
            | Mnemonic::Addsd
            | Mnemonic::Subsd
            | Mnemonic::Mulsd
            | Mnemonic::Divsd
            | Mnemonic::Sqrtsd
            | Mnemonic::Minsd
            | Mnemonic::Maxsd
            | Mnemonic::Comisd
            | Mnemonic::Ucomisd
            | Mnemonic::Cvtsi2sd
            | Mnemonic::Cvtsd2si
            | Mnemonic::Cvttsd2si
    )
}

fn coalesce_census_extents(
    blocks: &[CensusBlock],
    image_base: u64,
    total_dynamic_instructions: u64,
) -> Vec<CensusExtent> {
    let mut by_address = blocks.iter().collect::<Vec<_>>();
    by_address.sort_by_key(|block| (block.address, block.size_bytes));
    let mut extents: Vec<CensusExtent> = Vec::new();
    for block in by_address {
        let block_end = block.address + u64::from(block.size_bytes);
        if let Some(extent) = extents.last_mut().filter(|extent| {
            // Adjacent or overlapping translated blocks belong to one
            // promotable guest-code region. QEMU may split the same bytes into
            // several block variants depending on the incoming branch.
            block.address <= extent.end_address
        }) {
            extent.end_address = extent.end_address.max(block_end);
            extent.end_rva = extent.end_address - image_base;
            extent.size_bytes = extent.end_address - extent.start_address;
            extent.block_variants += 1;
            extent.dynamic_instructions = extent
                .dynamic_instructions
                .saturating_add(block.dynamic_instructions);
        } else {
            extents.push(CensusExtent {
                start_address: block.address,
                end_address: block_end,
                start_rva: block.address - image_base,
                end_rva: block_end - image_base,
                size_bytes: block_end - block.address,
                block_variants: 1,
                dynamic_instructions: block.dynamic_instructions,
                dynamic_instruction_fraction: 0.0,
            });
        }
    }
    for extent in &mut extents {
        extent.dynamic_instruction_fraction = if total_dynamic_instructions == 0 {
            0.0
        } else {
            extent.dynamic_instructions as f64 / total_dynamic_instructions as f64
        };
    }
    extents.sort_by_key(|extent| std::cmp::Reverse(extent.dynamic_instructions));
    extents
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_engine(code: &[u8]) -> GuestEngine<'static> {
        const CODE: u64 = 0x1000_0000;
        let mut unicorn = Unicorn::new_with_data(
            Arch::X86,
            Mode::MODE_64,
            GuestState {
                next_handle_data: HANDLE_DATA_BASE,
                next_aegp_memory_handle: AEGP_MEMORY_HANDLE_BASE,
                ..GuestState::default()
            },
        )
        .unwrap();
        install_avx_fallback(&mut unicorn).unwrap();
        unicorn.mem_map(CODE, PAGE_SIZE, Prot::ALL).unwrap();
        unicorn
            .mem_map(DATA_BASE, PAGE_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
        unicorn
            .mem_map(HANDLE_DATA_BASE, PAGE_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
        unicorn.mem_map(STUB_BASE, STUB_SIZE, Prot::ALL).unwrap();
        unicorn
            .mem_map(STACK_BASE, STACK_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
        unicorn.mem_write(CODE, code).unwrap();
        unicorn.mem_write(RETURN_ADDRESS, &[0xcc]).unwrap();
        for address in [
            HOST_ACQUIRE_SUITE,
            HOST_NEW_HANDLE,
            HOST_LOCK_HANDLE,
            HOST_UNLOCK_HANDLE,
            HOST_DISPOSE_HANDLE,
            HOST_HANDLE_SIZE,
            HOST_RESIZE_HANDLE,
            HOST_AEGP_REGISTER,
            HOST_AEGP_GET_MAIN_WINDOW,
            HOST_AEGP_NEW_MEM_HANDLE,
            HOST_AEGP_FREE_MEM_HANDLE,
            HOST_AEGP_LOCK_MEM_HANDLE,
            HOST_AEGP_UNLOCK_MEM_HANDLE,
            HOST_AEGP_MEM_HANDLE_SIZE,
            HOST_AEGP_RESIZE_MEM_HANDLE,
            HOST_NEW_WORLD,
            HOST_DISPOSE_WORLD,
            HOST_GET_WORLD_PIXEL_FORMAT,
            HOST_ITERATE8,
            HOST_ITERATE8_CONTINUE,
            HOST_COLOR_PARAM_VALUE,
            HOST_POINT_PARAM_VALUE,
            HOST_EXTENDED_ALLOC,
            HOST_EXTENDED_FREE,
            HOST_EXTENDED_LOOKUP,
        ] {
            unicorn.mem_write(address, &[0xc3]).unwrap();
        }
        unicorn
            .mem_write(HOST_POISON, &[0xb8, 0xff, 0xff, 0xff, 0xff, 0xc3])
            .unwrap();
        unicorn
            .mem_write(HOST_AEGP_MEMORY_UNSUPPORTED, &[0xb8, 4, 0, 0, 0, 0xc3])
            .unwrap();
        unicorn
            .add_code_hook(
                HOST_ACQUIRE_SUITE,
                HOST_ACQUIRE_SUITE,
                emulate_acquire_suite,
            )
            .unwrap();
        for (address, callback) in [
            (
                HOST_NEW_HANDLE,
                emulate_new_handle as fn(&mut Unicorn<'_, GuestState>, u64, u32),
            ),
            (HOST_LOCK_HANDLE, emulate_lock_handle),
            (HOST_UNLOCK_HANDLE, emulate_unlock_handle),
            (HOST_DISPOSE_HANDLE, emulate_dispose_handle),
            (HOST_HANDLE_SIZE, emulate_handle_size),
            (HOST_RESIZE_HANDLE, emulate_resize_handle),
        ] {
            unicorn.add_code_hook(address, address, callback).unwrap();
        }
        unicorn
            .add_code_hook(
                HOST_AEGP_REGISTER,
                HOST_AEGP_REGISTER,
                emulate_aegp_register,
            )
            .unwrap();
        unicorn
            .add_code_hook(
                HOST_AEGP_GET_MAIN_WINDOW,
                HOST_AEGP_GET_MAIN_WINDOW,
                emulate_aegp_get_main_window,
            )
            .unwrap();
        for (address, callback) in [
            (
                HOST_AEGP_NEW_MEM_HANDLE,
                emulate_aegp_new_mem_handle as fn(&mut Unicorn<'_, GuestState>, u64, u32),
            ),
            (HOST_AEGP_FREE_MEM_HANDLE, emulate_aegp_free_mem_handle),
            (HOST_AEGP_LOCK_MEM_HANDLE, emulate_aegp_lock_mem_handle),
            (HOST_AEGP_UNLOCK_MEM_HANDLE, emulate_aegp_unlock_mem_handle),
            (HOST_AEGP_MEM_HANDLE_SIZE, emulate_aegp_mem_handle_size),
            (HOST_AEGP_RESIZE_MEM_HANDLE, emulate_aegp_resize_mem_handle),
            (HOST_NEW_WORLD, emulate_new_world),
            (HOST_DISPOSE_WORLD, emulate_dispose_world),
            (HOST_GET_WORLD_PIXEL_FORMAT, emulate_get_world_pixel_format),
        ] {
            unicorn.add_code_hook(address, address, callback).unwrap();
        }
        let mut aegp_memory_suite = [0u8; 64];
        for (slot, callback) in [
            HOST_AEGP_NEW_MEM_HANDLE,
            HOST_AEGP_FREE_MEM_HANDLE,
            HOST_AEGP_LOCK_MEM_HANDLE,
            HOST_AEGP_UNLOCK_MEM_HANDLE,
            HOST_AEGP_MEM_HANDLE_SIZE,
            HOST_AEGP_RESIZE_MEM_HANDLE,
            HOST_AEGP_MEMORY_UNSUPPORTED,
            HOST_AEGP_MEMORY_UNSUPPORTED,
        ]
        .into_iter()
        .enumerate()
        {
            aegp_memory_suite[slot * 8..slot * 8 + 8].copy_from_slice(&callback.to_le_bytes());
        }
        unicorn
            .mem_write(HOST_AEGP_MEMORY_SUITE, &aegp_memory_suite)
            .unwrap();
        let mut world_suite = [0u8; 24];
        for (slot, callback) in [
            HOST_NEW_WORLD,
            HOST_DISPOSE_WORLD,
            HOST_GET_WORLD_PIXEL_FORMAT,
        ]
        .into_iter()
        .enumerate()
        {
            world_suite[slot * 8..slot * 8 + 8].copy_from_slice(&callback.to_le_bytes());
        }
        unicorn.mem_write(HOST_WORLD_SUITE, &world_suite).unwrap();
        unicorn
            .add_code_hook(HOST_ITERATE8, HOST_ITERATE8, emulate_iterate8)
            .unwrap();
        unicorn
            .add_code_hook(
                HOST_ITERATE8_CONTINUE,
                HOST_ITERATE8_CONTINUE,
                continue_iterate8,
            )
            .unwrap();
        unicorn
            .add_code_hook(
                HOST_COLOR_PARAM_VALUE,
                HOST_COLOR_PARAM_VALUE,
                emulate_color_param_value,
            )
            .unwrap();
        unicorn.get_data_mut().next_pf_handle_data = PF_HANDLE_DATA_BASE;
        unicorn
            .add_code_hook(
                HOST_POINT_PARAM_VALUE,
                HOST_POINT_PARAM_VALUE,
                emulate_point_param_value,
            )
            .unwrap();
        unicorn
            .add_code_hook(
                HOST_EXTENDED_ALLOC,
                HOST_EXTENDED_ALLOC,
                emulate_extended_alloc,
            )
            .unwrap();
        unicorn
            .add_code_hook(
                HOST_EXTENDED_FREE,
                HOST_EXTENDED_FREE,
                emulate_extended_free,
            )
            .unwrap();
        unicorn
            .add_code_hook(
                HOST_EXTENDED_LOOKUP,
                HOST_EXTENDED_LOOKUP,
                emulate_extended_lookup,
            )
            .unwrap();
        install_iterate8_suites(&mut unicorn).unwrap();
        install_pf_ansi_suite_v2(&mut unicorn).unwrap();
        unicorn
            .mem_write(
                HOST_COLOR_PARAM_SUITE,
                &HOST_COLOR_PARAM_VALUE.to_le_bytes(),
            )
            .unwrap();
        unicorn
            .mem_write(
                HOST_POINT_PARAM_SUITE,
                &HOST_POINT_PARAM_VALUE.to_le_bytes(),
            )
            .unwrap();
        install_aegp_utility_suites(&mut unicorn).unwrap();
        let mut trace_points = Vec::new();
        let mut decoder = Decoder::with_ip(64, code, CODE, DecoderOptions::NONE);
        while decoder.can_decode() {
            let instruction = decoder.decode();
            if instruction.mnemonic() == Mnemonic::Call
                || instruction.mnemonic() == Mnemonic::Jmp
                || instruction.is_ip_rel_memory_operand()
                || matches!(instruction.mnemonic(), Mnemonic::Ret | Mnemonic::Retf)
            {
                trace_points.push(instruction.ip());
            }
        }
        GuestEngine {
            unicorn,
            next_data: DATA_BASE,
            image_base: CODE,
            image_end: CODE + PAGE_SIZE,
            census_hook: None,
            trace_hooks: Vec::new(),
            trace_points,
            image_sha256: "synthetic".into(),
            entry_export: "fixture_entry".into(),
            trace_modules: vec![TraceModule {
                name: "fixture".into(),
                kind: "mapped_pe",
                sha256: None,
                symbols: vec!["fixture_entry".into()],
            }],
        }
    }

    #[test]
    fn crt_heap_imports_allocate_zero_reuse_and_reject_invalid_free() {
        let mut engine = test_engine(&[0xc3]);
        engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
        emulate_crt_malloc(&mut engine.unicorn, false);
        let first = engine.unicorn.reg_read(RegisterX86::RAX).unwrap();
        assert_ne!(first, 0);
        assert_eq!(first % crate::crt_heap::CRT_HEAP_ALIGNMENT, 0);

        engine.unicorn.reg_write(RegisterX86::RCX, 8).unwrap();
        engine.unicorn.reg_write(RegisterX86::RDX, 4).unwrap();
        emulate_crt_malloc(&mut engine.unicorn, true);
        let calloc_pointer = engine.unicorn.reg_read(RegisterX86::RAX).unwrap();
        let mut bytes = [0xff; 32];
        engine.unicorn.mem_read(calloc_pointer, &mut bytes).unwrap();
        assert_eq!(bytes, [0; 32]);

        engine.unicorn.reg_write(RegisterX86::RCX, first).unwrap();
        emulate_crt_free(&mut engine.unicorn);
        assert!(engine.unicorn.get_data().callback_error.is_none());
        engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
        emulate_crt_malloc(&mut engine.unicorn, false);
        assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), first);

        engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
        emulate_crt_free(&mut engine.unicorn);
        assert!(engine.unicorn.get_data().callback_error.is_none());
        engine
            .unicorn
            .reg_write(RegisterX86::RCX, 0xdead_beef)
            .unwrap();
        emulate_crt_free(&mut engine.unicorn);
        assert!(
            engine
                .unicorn
                .get_data()
                .callback_error
                .as_deref()
                .is_some_and(|message| message.contains("foreign or already-freed"))
        );
    }

    #[test]
    fn extended_inter_allocation_is_zeroed_bounded_and_owned() {
        let mut engine = test_engine(&[0xc3]);
        let output = engine.allocate(8, 8).unwrap();
        engine.write(output, &[0xff; 8]).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_EXTENDED_ALLOC, [output, 4000, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut pointer = [0u8; 8];
        engine.read(output, &mut pointer).unwrap();
        let pointer = u64::from_le_bytes(pointer);
        assert_ne!(pointer, 0);
        let mut bytes = [0xff; 32];
        engine.read(pointer, &mut bytes).unwrap();
        assert_eq!(bytes, [0; 32]);

        assert_eq!(
            engine
                .call_win64(HOST_EXTENDED_FREE, [output, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert!(
            engine
                .unicorn
                .get_data()
                .crt_heap
                .allocations()
                .next()
                .is_none()
        );

        assert_eq!(
            engine
                .call_win64(HOST_EXTENDED_ALLOC, [0, 4000, 0, 0, 0, 0])
                .unwrap(),
            4
        );
    }

    #[test]
    fn extended_inter_lookup_serves_values_empty_and_absent_tables() {
        let mut engine = test_engine(&[0xc3]);
        let value = engine.allocate(6, 1).unwrap();
        let empty = engine.allocate(1, 1).unwrap();
        engine.write(value, b"Label\0").unwrap();
        engine.write(empty, &[0]).unwrap();
        {
            let state = engine.unicorn.get_data_mut();
            state.extended_strings.insert(27, value);
            state.extended_empty_string = empty;
            state.extended_string_table_valid = true;
        }
        assert_eq!(
            engine
                .call_win64(HOST_EXTENDED_LOOKUP, [0, 27, 0, 0, 0, 0])
                .unwrap(),
            value
        );
        assert_eq!(
            engine
                .call_win64(HOST_EXTENDED_LOOKUP, [0, 999, 0, 0, 0, 0])
                .unwrap(),
            empty
        );
        engine.unicorn.get_data_mut().extended_string_table_valid = false;
        assert_eq!(
            engine
                .call_win64(HOST_EXTENDED_LOOKUP, [0, 999, 0, 0, 0, 0])
                .unwrap(),
            0
        );
    }

    #[test]
    fn crt_memory_copy_copies_bytes_and_returns_destination() {
        let mut engine = test_engine(&[0xc3]);
        let source = DATA_BASE + 0x100;
        let destination = DATA_BASE + 0x200;
        let bytes = b"generic CRT memory copy";
        engine.unicorn.mem_write(source, bytes).unwrap();
        engine
            .unicorn
            .reg_write(RegisterX86::RCX, destination)
            .unwrap();
        engine.unicorn.reg_write(RegisterX86::RDX, source).unwrap();
        engine
            .unicorn
            .reg_write(RegisterX86::R8, bytes.len() as u64)
            .unwrap();

        emulate_crt_memory_copy(&mut engine.unicorn);

        assert_eq!(
            engine
                .unicorn
                .mem_read_as_vec(destination, bytes.len())
                .unwrap(),
            bytes
        );
        assert_eq!(
            engine.unicorn.reg_read(RegisterX86::RAX).unwrap(),
            destination
        );
        assert_eq!(engine.unicorn.get_data().callback_error, None);
    }

    #[test]
    fn crt_memory_copy_preserves_overlapping_memmove_semantics() {
        let mut engine = test_engine(&[0xc3]);
        let buffer = DATA_BASE + 0x100;
        engine
            .unicorn
            .mem_write(buffer, b"0123456789abcdef")
            .unwrap();
        engine
            .unicorn
            .reg_write(RegisterX86::RCX, buffer + 4)
            .unwrap();
        engine.unicorn.reg_write(RegisterX86::RDX, buffer).unwrap();
        engine.unicorn.reg_write(RegisterX86::R8, 12).unwrap();

        emulate_crt_memory_copy(&mut engine.unicorn);

        assert_eq!(
            engine.unicorn.mem_read_as_vec(buffer, 16).unwrap(),
            b"01230123456789ab"
        );
    }

    #[test]
    fn zero_length_crt_memory_copy_accepts_unmapped_pointers() {
        let mut engine = test_engine(&[0xc3]);
        let destination = 0xdead_beef_dead_beef;
        engine
            .unicorn
            .reg_write(RegisterX86::RCX, destination)
            .unwrap();
        engine.unicorn.reg_write(RegisterX86::RDX, 0).unwrap();
        engine.unicorn.reg_write(RegisterX86::R8, 0).unwrap();

        emulate_crt_memory_copy(&mut engine.unicorn);

        assert_eq!(
            engine.unicorn.reg_read(RegisterX86::RAX).unwrap(),
            destination
        );
        assert_eq!(engine.unicorn.get_data().callback_error, None);
    }

    #[test]
    fn crt_memory_copy_fails_closed_without_masking_an_earlier_error() {
        let mut engine = test_engine(&[0xc3]);
        engine
            .unicorn
            .reg_write(RegisterX86::RCX, DATA_BASE)
            .unwrap();
        engine
            .unicorn
            .reg_write(RegisterX86::RDX, u64::MAX)
            .unwrap();
        engine.unicorn.reg_write(RegisterX86::R8, 2).unwrap();
        emulate_crt_memory_copy(&mut engine.unicorn);
        assert_eq!(
            engine.unicorn.get_data().callback_error.as_deref(),
            Some("memory-copy source range overflow")
        );

        engine.unicorn.get_data_mut().callback_error = Some("earlier failure".into());
        engine
            .unicorn
            .reg_write(RegisterX86::R8, MAX_CRT_MEMORY_COPY_BYTES + 1)
            .unwrap();
        emulate_crt_memory_copy(&mut engine.unicorn);
        assert_eq!(
            engine.unicorn.get_data().callback_error.as_deref(),
            Some("earlier failure")
        );
    }

    #[test]
    fn crt_heap_imports_return_null_for_overflow_and_budget_failure() {
        let mut engine = test_engine(&[0xc3]);
        engine
            .unicorn
            .reg_write(RegisterX86::RCX, u64::MAX)
            .unwrap();
        engine.unicorn.reg_write(RegisterX86::RDX, 2).unwrap();
        emulate_crt_malloc(&mut engine.unicorn, true);
        assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);

        engine
            .unicorn
            .reg_write(
                RegisterX86::RCX,
                crate::crt_heap::MAX_CRT_ALLOCATION_BYTES + 1,
            )
            .unwrap();
        emulate_crt_malloc(&mut engine.unicorn, false);
        assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
        assert!(engine.unicorn.get_data().callback_error.is_none());
    }

    fn read_u64(engine: &GuestEngine<'static>, address: u64) -> u64 {
        let mut bytes = [0u8; 8];
        engine.read(address, &mut bytes).unwrap();
        u64::from_le_bytes(bytes)
    }

    fn call_aegp_new_mem_handle(
        engine: &mut GuestEngine<'static>,
        callback: u64,
        label: u64,
        output: u64,
        size: u64,
    ) -> (u64, u64) {
        engine.write(output, &u64::MAX.to_le_bytes()).unwrap();
        let result = engine
            .call_win64(callback, [1, label, size, 0, output, 0])
            .unwrap();
        (result, read_u64(engine, output))
    }

    fn aegp_memory_state_snapshot(
        engine: &GuestEngine<'static>,
    ) -> (u64, u64, Vec<(u64, u64, u64, u32, u64)>, Vec<(u64, u64)>) {
        let state = engine.unicorn.get_data();
        let mut handles = state
            .aegp_memory_handles
            .iter()
            .map(|(&handle, record)| (handle, record.data, record.size, record.locks, record.end))
            .collect::<Vec<_>>();
        handles.sort_by_key(|record| record.0);
        let free = state
            .aegp_memory_free
            .iter()
            .map(|block| (block.data, block.end))
            .collect();
        (
            state.next_handle_data,
            state.next_aegp_memory_handle,
            handles,
            free,
        )
    }

    #[test]
    fn win64_call_places_register_arguments_and_returns_rax() {
        const CODE: u64 = 0x1000_0000;
        // mov rax, rcx; add rax, rdx; ret
        let mut engine = test_engine(&[0x48, 0x89, 0xc8, 0x48, 0x01, 0xd0, 0xc3]);
        assert_eq!(engine.call_win64(CODE, [40, 2, 0, 0, 0, 0]).unwrap(), 42);
    }

    #[test]
    fn vcomp_fork_tail_calls_outlined_worker_with_captured_arguments() {
        const CODE: u64 = 0x1000_0000;
        const VCOMP_FORK: u64 = STUB_BASE + 0x100;
        let mut engine = test_engine(&[
            0x48, 0x89, 0xc8, // mov rax, rcx
            0x48, 0x01, 0xd0, // add rax, rdx
            0x4c, 0x01, 0xc0, // add rax, r8
            0xc3, // ret
        ]);
        engine
            .unicorn
            .mem_write(VCOMP_FORK, &[0x41, 0xff, 0xe3])
            .unwrap();
        engine
            .unicorn
            .add_code_hook(VCOMP_FORK, VCOMP_FORK, |unicorn, _, _| {
                emulate_vcomp_fork(unicorn);
            })
            .unwrap();

        assert_eq!(
            engine
                .call_win64(VCOMP_FORK, [1, 3, CODE, 11, 22, 33])
                .unwrap(),
            66
        );
    }

    #[test]
    fn plugin_data_v2_and_v1_callbacks_decode_distinct_win64_stack_arguments() {
        const CALLBACK_V2: u64 = STUB_BASE + 0x180;
        const CALLBACK_V1: u64 = STUB_BASE + 0x190;
        let mut engine = test_engine(&[0xc3]);
        engine.unicorn.mem_write(CALLBACK_V2, &[0xc3]).unwrap();
        engine
            .unicorn
            .add_code_hook(CALLBACK_V2, CALLBACK_V2, |unicorn, _, _| {
                capture_plugin_data_registration(unicorn, true);
            })
            .unwrap();
        engine.unicorn.mem_write(CALLBACK_V1, &[0xc3]).unwrap();
        engine
            .unicorn
            .add_code_hook(CALLBACK_V1, CALLBACK_V1, |unicorn, _, _| {
                capture_plugin_data_registration(unicorn, false);
            })
            .unwrap();
        let mut cursor = DATA_BASE;
        let mut write_text = |text: &[u8]| {
            let address = cursor;
            engine.unicorn.mem_write(address, text).unwrap();
            cursor += text.len() as u64;
            address
        };
        let name = write_text(b"Fixture\0");
        let match_name = write_text(b"fixture.match\0");
        let category = write_text(b"Tests\0");
        let entrypoint = write_text(b"FilterMain\0");
        let support_url = write_text(b"https://example.invalid\0");

        assert_eq!(
            engine
                .call_win64_with_timeout(
                    CALLBACK_V2,
                    &[
                        1,
                        name,
                        match_name,
                        category,
                        entrypoint,
                        crate::plugin_data::EFFECT_KIND as u32 as u64,
                        13,
                        29,
                        9,
                        support_url,
                    ],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            0
        );
        let registration = engine
            .unicorn
            .get_data()
            .plugin_data_registry
            .select(Some("fixture.match"))
            .unwrap();
        assert_eq!(registration.entrypoint, "FilterMain");
        assert_eq!(registration.reserved_info, 9);
        assert_eq!(
            registration.support_url.as_deref(),
            Some(b"https://example.invalid".as_slice())
        );

        engine.unicorn.get_data_mut().plugin_data_registry = EffectRegistry::default();
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    CALLBACK_V1,
                    &[
                        1,
                        name,
                        match_name,
                        category,
                        entrypoint,
                        crate::plugin_data::EFFECT_KIND as u32 as u64,
                        13,
                        29,
                        11,
                    ],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            0
        );
        let registration = engine
            .unicorn
            .get_data()
            .plugin_data_registry
            .select(Some("fixture.match"))
            .unwrap();
        assert_eq!(registration.reserved_info, 11);
        assert_eq!(registration.support_url, None);
    }

    #[test]
    fn vcomp_dynamic_loop_returns_serial_chunks_until_exhausted() {
        const VCOMP_INIT: u64 = STUB_BASE + 0x110;
        const VCOMP_NEXT: u64 = STUB_BASE + 0x120;
        let mut engine = test_engine(&[0xc3]);
        engine.unicorn.mem_write(VCOMP_INIT, &[0xc3]).unwrap();
        engine.unicorn.mem_write(VCOMP_NEXT, &[0xc3]).unwrap();
        engine
            .unicorn
            .add_code_hook(VCOMP_INIT, VCOMP_INIT, |unicorn, _, _| {
                emulate_vcomp_for_dynamic_init(unicorn);
            })
            .unwrap();
        engine
            .unicorn
            .add_code_hook(VCOMP_NEXT, VCOMP_NEXT, |unicorn, _, _| {
                emulate_vcomp_for_dynamic_next(unicorn);
            })
            .unwrap();
        let lower_output = DATA_BASE;
        let upper_output = DATA_BASE + 4;

        engine
            .call_win64(VCOMP_INIT, [0x62, 2, 10, 1, 8, 0])
            .unwrap();
        assert_eq!(
            engine
                .call_win64(VCOMP_NEXT, [lower_output, upper_output, 0, 0, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(lower_output, 4).unwrap(),
            2i32.to_le_bytes()
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(upper_output, 4).unwrap(),
            9i32.to_le_bytes()
        );
        assert_eq!(
            engine
                .call_win64(VCOMP_NEXT, [lower_output, upper_output, 0, 0, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(lower_output, 4).unwrap(),
            10i32.to_le_bytes()
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(upper_output, 4).unwrap(),
            10i32.to_le_bytes()
        );
        assert_eq!(
            engine
                .call_win64(VCOMP_NEXT, [lower_output, upper_output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
    }

    #[test]
    fn vcomp_static_loop_writes_single_thread_bounds() {
        const VCOMP_STATIC_INIT: u64 = STUB_BASE + 0x130;
        let mut engine = test_engine(&[0xc3]);
        engine
            .unicorn
            .mem_write(VCOMP_STATIC_INIT, &[0xc3])
            .unwrap();
        engine
            .unicorn
            .add_code_hook(VCOMP_STATIC_INIT, VCOMP_STATIC_INIT, |unicorn, _, _| {
                emulate_vcomp_for_static_simple_init(unicorn);
            })
            .unwrap();
        let lower_output = DATA_BASE;
        let upper_output = DATA_BASE + 4;

        engine
            .call_win64(VCOMP_STATIC_INIT, [2, 9, 1, 1, lower_output, upper_output])
            .unwrap();
        assert_eq!(
            engine.unicorn.mem_read_as_vec(lower_output, 4).unwrap(),
            2i32.to_le_bytes()
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(upper_output, 4).unwrap(),
            9i32.to_le_bytes()
        );
    }

    #[test]
    fn vcomp_rejects_unsupported_worker_count_and_dynamic_schedule() {
        const CODE: u64 = 0x1000_0000;
        const VCOMP_FORK: u64 = STUB_BASE + 0x140;
        let mut fork_engine = test_engine(&[0xc3]);
        fork_engine
            .unicorn
            .mem_write(VCOMP_FORK, &[0x41, 0xff, 0xe3])
            .unwrap();
        fork_engine
            .unicorn
            .add_code_hook(VCOMP_FORK, VCOMP_FORK, |unicorn, _, _| {
                emulate_vcomp_fork(unicorn);
            })
            .unwrap();
        assert!(
            fork_engine
                .call_win64(VCOMP_FORK, [2, 1, CODE, 7, 0, 0])
                .unwrap_err()
                .to_string()
                .contains("worker count 2 is unsupported")
        );

        const VCOMP_INIT: u64 = STUB_BASE + 0x150;
        let mut init_engine = test_engine(&[0xc3]);
        init_engine.unicorn.mem_write(VCOMP_INIT, &[0xc3]).unwrap();
        init_engine
            .unicorn
            .add_code_hook(VCOMP_INIT, VCOMP_INIT, |unicorn, _, _| {
                emulate_vcomp_for_dynamic_init(unicorn);
            })
            .unwrap();
        assert!(
            init_engine
                .call_win64(VCOMP_INIT, [0x61, 0, 7, 1, 8, 0])
                .unwrap_err()
                .to_string()
                .contains("dynamic schedule 0x61 is unsupported")
        );
    }

    #[test]
    fn openmp_thread_count_is_positive_and_deterministic() {
        const CODE: u64 = 0x1000_0000;
        assert_eq!(deterministic_import_i32("omp_get_max_threads"), Some(1));
        assert_eq!(deterministic_import_i32("unknown_import"), None);
        assert_eq!(deterministic_i32_stub(1), [0xb8, 1, 0, 0, 0, 0xc3]);
        let mut engine = test_engine(&deterministic_i32_stub(1));
        assert_eq!(engine.call_win64(CODE, [0; 6]).unwrap(), 1);
    }

    #[test]
    fn pf_handle_suite_maps_large_allocations_outside_guest_data() {
        let mut engine = test_engine(&[0xc3]);
        let size = 333_294_848;
        let handle = engine
            .call_win64(HOST_NEW_HANDLE, [size, 0, 0, 0, 0, 0])
            .unwrap();
        assert!(handle >= PF_HANDLE_DATA_BASE);
        let data = engine
            .call_win64(HOST_LOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
            .unwrap();
        assert!(data > handle);
        assert_eq!(
            engine
                .call_win64(HOST_HANDLE_SIZE, [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            size
        );
        engine.write(data + size - 1, &[0x5a]).unwrap();
        let mut last = [0u8; 1];
        engine.read(data + size - 1, &mut last).unwrap();
        assert_eq!(last, [0x5a]);
        assert_eq!(
            engine
                .call_win64(HOST_UNLOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .call_win64(HOST_DISPOSE_HANDLE, [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        let reused = engine
            .call_win64(HOST_NEW_HANDLE, [size, 0, 0, 0, 0, 0])
            .unwrap();
        assert_eq!(reused, handle, "disposed address space must be reusable");
    }

    #[test]
    fn pf_handle_region_finder_skips_the_mapped_pe_image() {
        let state = GuestState {
            image_region: Some((
                PF_HANDLE_DATA_BASE + PAGE_SIZE,
                PF_HANDLE_DATA_BASE + 3 * PAGE_SIZE,
            )),
            ..GuestState::default()
        };
        assert_eq!(
            find_pf_region(&state, 2 * PAGE_SIZE, None).unwrap(),
            PF_HANDLE_DATA_BASE + 3 * PAGE_SIZE
        );
    }

    #[test]
    fn pf_handle_budget_bounds_live_bytes_and_handle_count() {
        let observed_size = 333_294_848;
        let mut state = GuestState::default();
        for index in 0..6u64 {
            state.handles.insert(
                PF_HANDLE_DATA_BASE + index * PAGE_SIZE,
                GuestHandle {
                    data: 0,
                    size: observed_size,
                    locks: 0,
                    handle_region: 0,
                    data_region: 0,
                    data_mapped_size: PAGE_SIZE,
                },
            );
        }
        assert!(validate_pf_handle_budget(&state, observed_size, None).is_err());
        let existing = state.handles.values().next().unwrap().clone();
        assert!(validate_pf_handle_budget(&state, observed_size, Some(&existing)).is_ok());

        state.handles.clear();
        for index in 0..1025u64 {
            state.handles.insert(
                PF_HANDLE_DATA_BASE + index * PAGE_SIZE,
                GuestHandle {
                    data: 0,
                    size: 0,
                    locks: 0,
                    handle_region: 0,
                    data_region: 0,
                    data_mapped_size: PAGE_SIZE,
                },
            );
        }
        assert!(
            validate_pf_handle_budget(&state, 0, None).is_ok(),
            "real AEX workloads must be allowed to exceed the old 1024-handle cap"
        );
        for index in 1025..MAX_PF_HANDLE_COUNT as u64 {
            state.handles.insert(
                PF_HANDLE_DATA_BASE + index * PAGE_SIZE,
                GuestHandle {
                    data: 0,
                    size: 0,
                    locks: 0,
                    handle_region: 0,
                    data_region: 0,
                    data_mapped_size: PAGE_SIZE,
                },
            );
        }
        assert!(validate_pf_handle_budget(&state, 0, None).is_err());
        let existing = state.handles.values().next().unwrap().clone();
        assert!(validate_pf_handle_budget(&state, 1, Some(&existing)).is_ok());
    }

    #[test]
    fn pf_world_suite_v2_owns_formats_and_fails_closed() {
        let mut engine = test_engine(&[0xc3]);
        let suite_name = engine.allocate(15, 1).unwrap();
        engine.write(suite_name, b"PF World Suite\0").unwrap();
        let suite_output = engine.allocate(8, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_ACQUIRE_SUITE, [suite_name, 2, suite_output, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut suite = [0u8; 8];
        engine.read(suite_output, &mut suite).unwrap();
        assert_eq!(u64::from_le_bytes(suite), HOST_WORLD_SUITE);
        let world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_NEW_WORLD, [1, 3, 2, 1, 0xdead_beef, world])
                .unwrap(),
            4
        );
        assert!(engine.unicorn.get_data().worlds.is_empty());
        assert_eq!(
            engine
                .call_win64(HOST_NEW_WORLD, [1, 3, 2, 1, 0x3631_6561, world])
                .unwrap(),
            0
        );
        let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        engine.unicorn.mem_read(world, &mut definition).unwrap();
        let data = u64::from_le_bytes(
            definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
                .try_into()
                .unwrap(),
        );
        assert_eq!(
            i32::from_le_bytes(
                definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
                    .try_into()
                    .unwrap()
            ),
            24
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(data, 48).unwrap(),
            vec![0; 48]
        );
        assert_eq!(
            engine
                .call_win64(HOST_NEW_WORLD, [1, 3, 2, 1, 0x3631_6561, world])
                .unwrap(),
            4
        );
        assert_eq!(engine.unicorn.get_data().worlds.len(), 1);

        let format_output = engine.allocate(4, 4).unwrap();
        engine
            .unicorn
            .mem_write(format_output, &0xfeed_beefu32.to_le_bytes())
            .unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_GET_WORLD_PIXEL_FORMAT, [0, format_output, 0, 0, 0, 0])
                .unwrap(),
            4
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(format_output, 4).unwrap(),
            0xfeed_beefu32.to_le_bytes()
        );
        assert_eq!(
            engine
                .call_win64(
                    HOST_GET_WORLD_PIXEL_FORMAT,
                    [world, format_output, 0, 0, 0, 0]
                )
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(format_output, 4).unwrap(),
            0x3631_6561u32.to_le_bytes()
        );
        let smart_input = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        let smart_output = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        engine.configure_smart_render(
            smart_input,
            smart_output,
            3,
            2,
            crate::pixel::PF_PIXEL_FORMAT_ARGB128,
        );
        for smart_world in [smart_input, smart_output] {
            assert_eq!(
                engine
                    .call_win64(
                        HOST_GET_WORLD_PIXEL_FORMAT,
                        [smart_world, format_output, 0, 0, 0, 0]
                    )
                    .unwrap(),
                0
            );
            assert_eq!(
                engine.unicorn.mem_read_as_vec(format_output, 4).unwrap(),
                crate::pixel::PF_PIXEL_FORMAT_ARGB128.to_le_bytes()
            );
        }
        assert_eq!(
            engine
                .call_win64(HOST_DISPOSE_WORLD, [1, world, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert!(engine.unicorn.get_data().worlds.is_empty());
        assert_eq!(
            engine
                .unicorn
                .mem_read_as_vec(world, abi::PF_LAYER_DEF_SIZE)
                .unwrap(),
            vec![0; abi::PF_LAYER_DEF_SIZE]
        );
        assert_eq!(
            engine
                .call_win64(HOST_DISPOSE_WORLD, [1, world, 0, 0, 0, 0])
                .unwrap(),
            4
        );
        let classic_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        let mut classic_definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        classic_definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&2i32.to_le_bytes());
        classic_definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&8i32.to_le_bytes());
        engine.write(classic_world, &classic_definition).unwrap();
        assert_eq!(
            engine
                .call_win64(
                    HOST_GET_WORLD_PIXEL_FORMAT,
                    [classic_world, format_output, 0, 0, 0, 0]
                )
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(format_output, 4).unwrap(),
            0x6267_7261u32.to_le_bytes()
        );

        let float_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_NEW_WORLD, [1, 1, 1, 0x100, 0x3233_6561, float_world],)
                .unwrap(),
            0
        );
        engine
            .unicorn
            .mem_read(float_world, &mut definition)
            .unwrap();
        let float_data = u64::from_le_bytes(
            definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
                .try_into()
                .unwrap(),
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(float_data, 16).unwrap(),
            vec![0xcd; 16]
        );
        assert_eq!(
            engine
                .call_win64(HOST_DISPOSE_WORLD, [1, float_world, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .call_win64(
                    HOST_NEW_WORLD,
                    [1, i32::MAX as u64, i32::MAX as u64, 1, 0x6267_7261, world,]
                )
                .unwrap(),
            4
        );
        assert!(engine.unicorn.get_data().worlds.is_empty());
    }

    #[test]
    fn pf_handle_suite_fails_closed_on_unknown_lock() {
        let mut engine = test_engine(&[0xc3]);
        let error = engine
            .call_win64(HOST_LOCK_HANDLE, [0xdead_beef, 0, 0, 0, 0, 0])
            .unwrap_err();
        assert!(error.to_string().contains("unknown handle"));
    }

    #[test]
    fn pf_handle_resize_preserves_bytes_and_stable_handle() {
        let mut engine = test_engine(&[0xc3]);
        let handle = engine
            .call_win64(HOST_NEW_HANDLE, [16, 0, 0, 0, 0, 0])
            .unwrap();
        let old_data = engine
            .call_win64(HOST_LOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
            .unwrap();
        engine.write(old_data, &[1, 2, 3, 4]).unwrap();
        engine
            .call_win64(HOST_UNLOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
            .unwrap();
        let handle_pointer = engine.allocate(8, 8).unwrap();
        engine.write(handle_pointer, &handle.to_le_bytes()).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_RESIZE_HANDLE, [8192, handle_pointer, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut stable = [0u8; 8];
        engine.read(handle_pointer, &mut stable).unwrap();
        assert_eq!(u64::from_le_bytes(stable), handle);
        let new_data = engine
            .call_win64(HOST_LOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
            .unwrap();
        assert_ne!(new_data, old_data);
        let mut preserved = [0u8; 4];
        engine.read(new_data, &mut preserved).unwrap();
        assert_eq!(preserved, [1, 2, 3, 4]);
    }

    #[test]
    fn color_param_suite_is_stateful_and_fails_closed() {
        let mut engine = test_engine(&[0xc3]);
        let name = engine.allocate(32, 1).unwrap();
        engine.write(name, b"PF ColorParamSuite\0").unwrap();
        let suite_output = engine.allocate(8, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_ACQUIRE_SUITE, [name, 1, suite_output, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut pointer = [0u8; 8];
        engine.read(suite_output, &mut pointer).unwrap();
        assert_eq!(u64::from_le_bytes(pointer), HOST_COLOR_PARAM_SUITE);

        let mut captured = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        captured[..4].copy_from_slice(&101i32.to_le_bytes());
        captured[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
            .copy_from_slice(&5i32.to_le_bytes());
        captured[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 8]
            .copy_from_slice(&[255, 10, 20, 30, 255, 1, 2, 3]);
        engine.unicorn.get_data_mut().params.push(GuestParam {
            index: 1,
            param_type: 5,
            name: "Key Color".into(),
            bytes: captured.clone(),
        });
        let definition = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
        engine.write(definition, &captured).unwrap();
        engine
            .configure_parameter_definitions(vec![definition])
            .unwrap();
        let definition_copy = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
        engine.write(definition_copy, &captured).unwrap();
        let definition = definition_copy;
        let output = engine.allocate(abi::PF_PIXEL_FLOAT_SIZE, 4).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_COLOR_PARAM_VALUE, [1, definition, output, 0, 0, 0],)
                .unwrap(),
            0
        );
        let mut values = [0u8; abi::PF_PIXEL_FLOAT_SIZE];
        engine.read(output, &mut values).unwrap();
        let channels = (0..4)
            .map(|index| f32::from_le_bytes(values[index * 4..index * 4 + 4].try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(channels, [1.0, 10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0]);

        engine
            .write(definition + abi::PARAM_U_OFFSET as u64, &[255, 1, 2, 3])
            .unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_COLOR_PARAM_VALUE, [1, definition, output, 0, 0, 0],)
                .unwrap(),
            0
        );
        engine
            .write(definition + abi::PARAM_U_OFFSET as u64, &[9, 9, 9, 9])
            .unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_COLOR_PARAM_VALUE, [1, definition, output, 0, 0, 0],)
                .unwrap(),
            516
        );
        engine.write(definition, &999i32.to_le_bytes()).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_COLOR_PARAM_VALUE, [1, definition, output, 0, 0, 0],)
                .unwrap(),
            513
        );
        engine.write(definition, &101i32.to_le_bytes()).unwrap();
        engine
            .write(
                definition + abi::PARAM_PARAM_TYPE_OFFSET as u64,
                &6i32.to_le_bytes(),
            )
            .unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_COLOR_PARAM_VALUE, [1, definition, output, 0, 0, 0],)
                .unwrap(),
            514
        );
        assert_eq!(
            engine
                .call_win64(HOST_COLOR_PARAM_VALUE, [0, definition, output, 0, 0, 0])
                .unwrap(),
            516
        );
    }

    #[test]
    fn point_param_suite_returns_signed_fixed_values_as_doubles() {
        let mut engine = test_engine(&[0xc3]);
        let name = engine.allocate(32, 1).unwrap();
        engine.write(name, b"PF PointParamSuite\0").unwrap();
        let suite_output = engine.allocate(8, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_ACQUIRE_SUITE, [name, 1, suite_output, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut pointer = [0u8; 8];
        engine.read(suite_output, &mut pointer).unwrap();
        assert_eq!(u64::from_le_bytes(pointer), HOST_POINT_PARAM_SUITE);
        engine.read(HOST_POINT_PARAM_SUITE, &mut pointer).unwrap();
        assert_eq!(u64::from_le_bytes(pointer), HOST_POINT_PARAM_VALUE);

        let definition = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
        engine
            .write(
                definition + abi::PARAM_U_OFFSET as u64,
                &[98304i32.to_le_bytes(), (-147456i32).to_le_bytes()].concat(),
            )
            .unwrap();
        let output = engine.allocate(16, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_POINT_PARAM_VALUE, [1, definition, output, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut values = [0u8; 16];
        engine.read(output, &mut values).unwrap();
        assert_eq!(f64::from_le_bytes(values[..8].try_into().unwrap()), 1.5);
        assert_eq!(f64::from_le_bytes(values[8..].try_into().unwrap()), -2.25);
        assert_eq!(
            engine
                .call_win64(HOST_POINT_PARAM_VALUE, [1, 0, output, 0, 0, 0])
                .unwrap(),
            4
        );
        assert_eq!(
            engine
                .call_win64(HOST_POINT_PARAM_VALUE, [1, definition, 0, 0, 0, 0])
                .unwrap(),
            4
        );
    }

    #[test]
    fn iterate8_calls_guest_pixel_callback_for_each_argb8_pixel() {
        const CODE: u64 = 0x1000_0000;
        // mov rax,[rsp+0x28]; mov edx,[r9]; mov [rax],edx; xor eax,eax; ret
        let mut engine = test_engine(&[
            0x48, 0x8b, 0x44, 0x24, 0x28, 0x41, 0x8b, 0x11, 0x89, 0x10, 0x31, 0xc0, 0xc3,
        ]);
        let source_pixels = engine.allocate(8, 4).unwrap();
        let destination_pixels = engine.allocate(8, 4).unwrap();
        let source_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        engine
            .write(source_pixels, &[1, 2, 3, 4, 5, 6, 7, 8])
            .unwrap();
        for (world, pixels) in [
            (source_world, source_pixels),
            (destination_world, destination_pixels),
        ] {
            let mut bytes = vec![0u8; abi::PF_LAYER_DEF_SIZE];
            bytes[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
                .copy_from_slice(&pixels.to_le_bytes());
            bytes[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
                .copy_from_slice(&8i32.to_le_bytes());
            bytes[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
                .copy_from_slice(&2i32.to_le_bytes());
            bytes[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
                .copy_from_slice(&1i32.to_le_bytes());
            engine.write(world, &bytes).unwrap();
        }
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    HOST_ITERATE8,
                    &[0, 0, 1, source_world, 0, 0, CODE, destination_world],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            0
        );
        let mut output = [0u8; 8];
        engine.read(destination_pixels, &mut output).unwrap();
        assert_eq!(output, [1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn iterate8_preserves_null_source_pixel_for_generators() {
        const CODE: u64 = 0x1000_0000;
        // mov rax,[rsp+0x28]; xor edx,edx; test r9,r9; setne dl;
        // mov [rax],edx; xor eax,eax; ret
        let mut engine = test_engine(&[
            0x48, 0x8b, 0x44, 0x24, 0x28, 0x31, 0xd2, 0x4d, 0x85, 0xc9, 0x0f, 0x95, 0xc2, 0x89,
            0x10, 0x31, 0xc0, 0xc3,
        ]);
        let destination_pixels = engine.allocate(4, 4).unwrap();
        let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        let mut world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        world[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&destination_pixels.to_le_bytes());
        world[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&4i32.to_le_bytes());
        world[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&1i32.to_le_bytes());
        world[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&1i32.to_le_bytes());
        engine.write(destination_world, &world).unwrap();
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    HOST_ITERATE8,
                    &[0, 0, 1, 0, 0, 0, CODE, destination_world],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            0
        );
        let mut output = [0xffu8; 4];
        engine.read(destination_pixels, &mut output).unwrap();
        assert_eq!(output, [0; 4]);
    }

    #[test]
    fn iterate8_unsupported_slots_fail_closed_with_suite_diagnostics() {
        let mut engine = test_engine(&[0xc3]);
        let mut callback = [0u8; 8];
        engine.read(HOST_ITERATE8_SUITE + 8, &mut callback).unwrap();
        assert_eq!(
            engine
                .call_win64(u64::from_le_bytes(callback), [0; 6])
                .unwrap(),
            4
        );
        assert_eq!(
            engine.unsupported_suite_calls(),
            [UnsupportedSuiteCall {
                name: "PF Iterate8 Suite",
                version: 1,
                slot: 1,
                call_count: 1,
            }]
        );
    }

    #[test]
    fn pf_ansi_suite_v2_matches_the_windows_slot_layout() {
        let mut engine = test_engine(&[0xc3]);
        let name = DATA_BASE + 0x100;
        let output = DATA_BASE + 0x200;
        engine.unicorn.mem_write(name, b"PF ANSI Suite\0").unwrap();

        assert_eq!(
            engine
                .call_win64(HOST_ACQUIRE_SUITE, [name, 2, output, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut suite_pointer = [0u8; 8];
        engine.unicorn.mem_read(output, &mut suite_pointer).unwrap();
        assert_eq!(u64::from_le_bytes(suite_pointer), HOST_PF_ANSI_SUITE_V2);

        let mut table = [0u8; 21 * 8];
        engine
            .unicorn
            .mem_read(HOST_PF_ANSI_SUITE_V2, &mut table)
            .unwrap();
        let callbacks = table
            .chunks_exact(8)
            .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            callbacks,
            [
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
            ]
        );
        assert_eq!(engine.suite_requests(), ["PF ANSI Suite v2"]);
    }

    #[test]
    fn pf_ansi_suite_v2_executes_double_and_bounded_string_callbacks() {
        let mut engine = test_engine(&[0xc3]);
        let name = DATA_BASE + 0x20;
        let output = DATA_BASE + 0x40;
        engine.unicorn.mem_write(name, b"PF ANSI Suite\0").unwrap();
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [name, 2, output, 0, 0, 0])
            .unwrap();
        let mut suite_bytes = [0u8; 8];
        engine.unicorn.mem_read(output, &mut suite_bytes).unwrap();
        let suite = u64::from_le_bytes(suite_bytes);
        let callback = |engine: &GuestEngine<'_>, slot: u64| {
            let mut bytes = [0u8; 8];
            engine
                .unicorn
                .mem_read(suite + slot * 8, &mut bytes)
                .unwrap();
            u64::from_le_bytes(bytes)
        };

        let mut xmm0 = [0u8; 16];
        xmm0[..8].copy_from_slice(&(0.5f64).to_le_bytes());
        engine
            .unicorn
            .reg_write_long(RegisterX86::XMM0, &xmm0)
            .unwrap();
        engine.call_win64(callback(&engine, 12), [0; 6]).unwrap();
        let xmm0 = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
        let sine = f64::from_le_bytes(xmm0[..8].try_into().unwrap());
        assert!((sine - 0.5f64.sin()).abs() < f64::EPSILON);

        let source = DATA_BASE + 0x100;
        let destination = DATA_BASE + 0x200;
        engine
            .unicorn
            .mem_write(source, b"bounded metadata\0")
            .unwrap();
        engine.unicorn.mem_write(destination, b"XXXXXXXX").unwrap();
        assert_eq!(
            engine
                .call_win64(callback(&engine, 16), [destination, source, 0, 0, 0, 0])
                .unwrap(),
            destination
        );
        assert_eq!(
            engine
                .unicorn
                .mem_read_as_vec(destination, b"bounded metadata\0".len())
                .unwrap(),
            b"bounded metadata\0"
        );
        assert_eq!(
            engine
                .call_win64(callback(&engine, 20), [destination, 8, source, 0, 0, 0],)
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(destination, 8).unwrap(),
            b"bounded\0"
        );
    }

    #[test]
    fn pf_ansi_numeric_policy_matches_the_windows_finite_contract() {
        assert_eq!(ansi_sqrt(-1.0), 0.0);
        assert_eq!(ansi_log(0.0), 0.0);
        assert_eq!(ansi_asin(2.0), 0.0);
        assert_eq!(ansi_fmod(4.0, 0.0), 0.0);
        assert_eq!(ansi_pow(f64::NAN, 2.0), 0.0);
        assert_eq!(ansi_exp(1000.0), 0.0);
        assert_eq!(ansi_pow(2.0, 3.0), 8.0);
        assert_eq!(ansi_hypot(3.0, 4.0), 5.0);
    }

    #[test]
    fn pf_ansi_bounded_copy_rejects_malformed_calls_and_unmapped_memory() {
        let mut engine = test_engine(&[0xc3]);
        assert_eq!(
            engine
                .call_win64(HOST_PF_ANSI_STRCPY, [0, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .call_win64(HOST_PF_ANSI_STRCPY_BOUNDED, [0, 0, 0, 0, 0, 0])
                .unwrap(),
            4
        );
        let error = engine
            .call_win64(
                HOST_PF_ANSI_STRCPY_BOUNDED,
                [DATA_BASE, 32, 0xdead_beef, 0, 0, 0],
            )
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("PF ANSI bounded strcpy source read"),
            "{error}"
        );

        let mut engine = test_engine(&[0xc3]);
        engine
            .unicorn
            .mem_map(DATA_BASE + PAGE_SIZE, PAGE_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
        engine
            .unicorn
            .mem_write(DATA_BASE, &vec![b'x'; 4096])
            .unwrap();
        let destination = DATA_BASE + PAGE_SIZE;
        engine
            .unicorn
            .mem_write(destination, b"unchanged\0")
            .unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_PF_ANSI_STRCPY, [destination, DATA_BASE, 0, 0, 0, 0],)
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .unicorn
                .mem_read_as_vec(destination, b"unchanged\0".len())
                .unwrap(),
            b"unchanged\0"
        );
    }

    #[test]
    fn aegp_utility_v7_v13_supported_and_unsupported_slots_execute() {
        let mut engine = test_engine(&[0xc3]);
        let suite_name = engine.allocate(19, 1).unwrap();
        engine.write(suite_name, b"AEGP Utility Suite\0").unwrap();

        for version in [7u64, 13] {
            let output = engine.allocate(8, 8).unwrap();
            assert_eq!(
                engine
                    .call_win64(HOST_ACQUIRE_SUITE, [suite_name, version, output, 0, 0, 0])
                    .unwrap(),
                0
            );
            let mut table_bytes = [0u8; 8];
            engine.read(output, &mut table_bytes).unwrap();
            let table = u64::from_le_bytes(table_bytes);
            let (_, register_slot, window_slot) = utility_suite_layout(version as u32).unwrap();

            let mut callback_bytes = [0u8; 8];
            engine
                .read(table + (register_slot * 8) as u64, &mut callback_bytes)
                .unwrap();
            let register = u64::from_le_bytes(callback_bytes);
            let plugin_id = engine.allocate(4, 4).unwrap();
            assert_eq!(
                engine
                    .call_win64(register, [0, suite_name, plugin_id, 0, 0, 0])
                    .unwrap(),
                0
            );
            let mut plugin_id_bytes = [0u8; 4];
            engine.read(plugin_id, &mut plugin_id_bytes).unwrap();
            assert_eq!(i32::from_le_bytes(plugin_id_bytes), 1);

            engine
                .read(table + (window_slot * 8) as u64, &mut callback_bytes)
                .unwrap();
            let get_window = u64::from_le_bytes(callback_bytes);
            let window = engine.allocate(8, 8).unwrap();
            engine.write(window, &u64::MAX.to_le_bytes()).unwrap();
            assert_eq!(
                engine
                    .call_win64(get_window, [window, 0, 0, 0, 0, 0])
                    .unwrap(),
                0
            );
            let mut window_bytes = [0u8; 8];
            engine.read(window, &mut window_bytes).unwrap();
            assert_eq!(u64::from_le_bytes(window_bytes), 0);

            engine.read(table, &mut callback_bytes).unwrap();
            let unsupported = u64::from_le_bytes(callback_bytes);
            assert_eq!(engine.call_win64(unsupported, [0; 6]).unwrap(), 4);
            assert_eq!(engine.call_win64(unsupported, [0; 6]).unwrap(), 4);
        }

        let unsupported_output = engine.allocate(8, 8).unwrap();
        engine
            .write(unsupported_output, &u64::MAX.to_le_bytes())
            .unwrap();
        assert_eq!(
            engine
                .call_win64(
                    HOST_ACQUIRE_SUITE,
                    [suite_name, 12, unsupported_output, 0, 0, 0],
                )
                .unwrap(),
            u32::MAX as u64
        );
        let mut unsupported_output_bytes = [0u8; 8];
        engine
            .read(unsupported_output, &mut unsupported_output_bytes)
            .unwrap();
        assert_eq!(u64::from_le_bytes(unsupported_output_bytes), 0);

        assert_eq!(
            engine.unsupported_suite_calls(),
            [
                UnsupportedSuiteCall {
                    name: "AEGP Utility Suite",
                    version: 7,
                    slot: 0,
                    call_count: 2,
                },
                UnsupportedSuiteCall {
                    name: "AEGP Utility Suite",
                    version: 13,
                    slot: 0,
                    call_count: 2,
                },
            ]
        );
        assert_eq!(engine.dropped_unsupported_suite_calls(), 0);
        assert_eq!(
            engine.suite_requests(),
            [
                "AEGP Utility Suite v7",
                "AEGP Utility Suite v13",
                "AEGP Utility Suite v12",
            ]
        );
    }

    #[test]
    fn aegp_memory_v1_slots_zero_through_five_are_bounded_and_fail_closed() {
        let mut engine = test_engine(&[0xc3]);
        let suite_name = engine.allocate(18, 1).unwrap();
        engine.write(suite_name, b"AEGP Memory Suite\0").unwrap();
        let suite_output = engine.allocate(8, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_ACQUIRE_SUITE, [suite_name, 1, suite_output, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut table_bytes = [0u8; 8];
        engine.read(suite_output, &mut table_bytes).unwrap();
        let table = u64::from_le_bytes(table_bytes);
        let mut callbacks = [0u64; 8];
        for (slot, callback) in callbacks.iter_mut().enumerate() {
            engine
                .read(table + (slot * 8) as u64, &mut table_bytes)
                .unwrap();
            *callback = u64::from_le_bytes(table_bytes);
        }

        let label = engine.allocate(4, 1).unwrap();
        engine.write(label, b"olm\0").unwrap();
        let handle_output = engine.allocate(8, 8).unwrap();
        engine
            .write(handle_output, &u64::MAX.to_le_bytes())
            .unwrap();
        assert_eq!(
            engine
                .call_win64(
                    callbacks[0],
                    [1, label, i32::MAX as u64 + 1, 0, handle_output, 0]
                )
                .unwrap(),
            4
        );
        engine.read(handle_output, &mut table_bytes).unwrap();
        assert_eq!(u64::from_le_bytes(table_bytes), 0);

        assert_eq!(
            engine
                .call_win64(callbacks[0], [1, label, 32, 1, handle_output, 0])
                .unwrap(),
            0
        );
        engine.read(handle_output, &mut table_bytes).unwrap();
        let handle = u64::from_le_bytes(table_bytes);
        assert_ne!(handle, 0);
        assert_eq!(handle % 8, 0);

        let data_output = engine.allocate(8, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(callbacks[2], [handle, data_output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        engine.read(data_output, &mut table_bytes).unwrap();
        let data = u64::from_le_bytes(table_bytes);
        assert_eq!(data % 16, 0);
        let mut bytes = [0xffu8; 32];
        engine.read(data, &mut bytes).unwrap();
        assert_eq!(bytes, [0; 32]);
        engine.write(data, &[0x11, 0x22, 0x33, 0x44]).unwrap();
        assert_eq!(
            engine
                .call_win64(callbacks[2], [handle, data_output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .call_win64(callbacks[5], [label, 64, handle, 0, 0, 0])
                .unwrap(),
            4
        );
        assert_eq!(
            engine
                .call_win64(callbacks[3], [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .call_win64(callbacks[3], [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .call_win64(callbacks[5], [label, 64, handle, 0, 0, 0])
                .unwrap(),
            0
        );

        let size_output = engine.allocate(4, 4).unwrap();
        assert_eq!(
            engine
                .call_win64(callbacks[4], [handle, size_output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut size_bytes = [0u8; 4];
        engine.read(size_output, &mut size_bytes).unwrap();
        assert_eq!(u32::from_le_bytes(size_bytes), 64);
        assert_eq!(
            engine
                .call_win64(callbacks[2], [handle, data_output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        engine.read(data_output, &mut table_bytes).unwrap();
        let resized_data = u64::from_le_bytes(table_bytes);
        let mut resized = [0xffu8; 64];
        engine.read(resized_data, &mut resized).unwrap();
        assert_eq!(&resized[..4], &[0x11, 0x22, 0x33, 0x44]);
        assert_eq!(&resized[4..], &[0; 60]);
        assert_eq!(
            engine
                .call_win64(callbacks[3], [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .call_win64(callbacks[1], [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .call_win64(callbacks[1], [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            4
        );
        assert_eq!(
            engine
                .call_win64(callbacks[2], [handle, data_output, 0, 0, 0, 0])
                .unwrap(),
            4
        );
        assert_eq!(engine.call_win64(callbacks[6], [0; 6]).unwrap(), 4);
        assert_eq!(engine.call_win64(callbacks[7], [0; 6]).unwrap(), 4);
    }

    #[test]
    fn aegp_memory_v1_callbacks_enforce_aggregate_live_byte_budget_atomically() {
        const MIB: u64 = 1024 * 1024;

        let mut engine = test_engine(&[0xc3]);
        engine
            .unicorn
            .mem_unmap(HANDLE_DATA_BASE, PAGE_SIZE)
            .unwrap();
        engine
            .unicorn
            .mem_map(
                HANDLE_DATA_BASE,
                MAX_AEGP_MEMORY_BYTES * 2,
                Prot::READ | Prot::WRITE,
            )
            .unwrap();

        let suite_name = engine.allocate(18, 1).unwrap();
        engine.write(suite_name, b"AEGP Memory Suite\0").unwrap();
        let suite_output = engine.allocate(8, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_ACQUIRE_SUITE, [suite_name, 1, suite_output, 0, 0, 0])
                .unwrap(),
            0
        );
        let table = read_u64(&engine, suite_output);
        let mut callbacks = [0u64; 6];
        for (slot, callback) in callbacks.iter_mut().enumerate() {
            *callback = read_u64(&engine, table + (slot * 8) as u64);
        }
        let label = engine.allocate(7, 1).unwrap();
        engine.write(label, b"budget\0").unwrap();
        let handle_output = engine.allocate(8, 8).unwrap();
        let data_output = engine.allocate(8, 8).unwrap();

        let (result, nine_mib_handle) =
            call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 9 * MIB);
        assert_eq!(result, 0);
        assert_ne!(nine_mib_handle, 0);
        assert_eq!(
            engine
                .call_win64(callbacks[2], [nine_mib_handle, data_output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        let nine_mib_data = read_u64(&engine, data_output);
        engine
            .write(nine_mib_data, &[0x51, 0x42, 0x33, 0x24])
            .unwrap();
        assert_eq!(
            engine
                .call_win64(callbacks[3], [nine_mib_handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        let before_rejected_new = aegp_memory_state_snapshot(&engine);
        let (result, rejected_handle) =
            call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 9 * MIB);
        assert_eq!(result, 4);
        assert_eq!(rejected_handle, 0);
        assert_eq!(aegp_memory_state_snapshot(&engine), before_rejected_new);
        let mut marker = [0u8; 4];
        engine.read(nine_mib_data, &mut marker).unwrap();
        assert_eq!(marker, [0x51, 0x42, 0x33, 0x24]);
        assert_eq!(
            engine
                .call_win64(callbacks[1], [nine_mib_handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );

        let (result, eight_mib_a) =
            call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 8 * MIB);
        assert_eq!(result, 0);
        let (result, eight_mib_b) =
            call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 8 * MIB);
        assert_eq!(result, 0);
        assert_eq!(
            engine
                .call_win64(callbacks[2], [eight_mib_a, data_output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        let eight_mib_a_data = read_u64(&engine, data_output);
        engine
            .write(eight_mib_a_data, &[0xa1, 0xb2, 0xc3, 0xd4])
            .unwrap();
        assert_eq!(
            engine
                .call_win64(callbacks[3], [eight_mib_a, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );

        let before_one_byte_rejection = aegp_memory_state_snapshot(&engine);
        let (result, rejected_handle) =
            call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 1);
        assert_eq!(result, 4);
        assert_eq!(rejected_handle, 0);
        assert_eq!(
            aegp_memory_state_snapshot(&engine),
            before_one_byte_rejection
        );

        assert_eq!(
            engine
                .call_win64(callbacks[1], [eight_mib_b, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        let (result, replacement_eight_mib) =
            call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 8 * MIB);
        assert_eq!(result, 0);
        assert_eq!(
            engine
                .call_win64(callbacks[5], [label, 8 * MIB, eight_mib_a, 0, 0, 0])
                .unwrap(),
            0
        );

        let before_rejected_resize = aegp_memory_state_snapshot(&engine);
        assert_eq!(
            engine
                .call_win64(callbacks[5], [label, 9 * MIB, eight_mib_a, 0, 0, 0])
                .unwrap(),
            4
        );
        assert_eq!(aegp_memory_state_snapshot(&engine), before_rejected_resize);
        assert_eq!(
            engine
                .call_win64(callbacks[2], [eight_mib_a, data_output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(read_u64(&engine, data_output), eight_mib_a_data);
        engine.read(eight_mib_a_data, &mut marker).unwrap();
        assert_eq!(marker, [0xa1, 0xb2, 0xc3, 0xd4]);
        assert_eq!(
            engine
                .call_win64(callbacks[3], [eight_mib_a, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );

        assert_eq!(
            engine
                .call_win64(callbacks[5], [label, 7 * MIB, eight_mib_a, 0, 0, 0])
                .unwrap(),
            0
        );
        let (result, recovered_one_mib) =
            call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, MIB);
        assert_eq!(result, 0);

        for handle in [eight_mib_a, replacement_eight_mib, recovered_one_mib] {
            assert_eq!(
                engine
                    .call_win64(callbacks[1], [handle, 0, 0, 0, 0, 0])
                    .unwrap(),
                0
            );
        }
        assert!(engine.unicorn.get_data().aegp_memory_handles.is_empty());
    }

    #[test]
    fn aegp_memory_v1_reclaims_backing_storage_for_alloc_free_and_resize_cycles() {
        let mut engine = test_engine(&[0xc3]);
        let suite_name = engine.allocate(18, 1).unwrap();
        engine.write(suite_name, b"AEGP Memory Suite\0").unwrap();
        let suite_output = engine.allocate(8, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_ACQUIRE_SUITE, [suite_name, 1, suite_output, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut bytes = [0u8; 8];
        engine.read(suite_output, &mut bytes).unwrap();
        let table = u64::from_le_bytes(bytes);
        let mut callbacks = [0u64; 6];
        for (slot, callback) in callbacks.iter_mut().enumerate() {
            engine.read(table + (slot * 8) as u64, &mut bytes).unwrap();
            *callback = u64::from_le_bytes(bytes);
        }
        let label = engine.allocate(4, 1).unwrap();
        engine.write(label, b"olm\0").unwrap();
        let handle_output = engine.allocate(8, 8).unwrap();
        let data_output = engine.allocate(8, 8).unwrap();

        let mut first_handle = 0;
        let mut first_data = 0;
        for cycle in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
            assert_eq!(
                engine
                    .call_win64(callbacks[0], [1, label, 32, 0, handle_output, 0])
                    .unwrap(),
                0
            );
            engine.read(handle_output, &mut bytes).unwrap();
            let handle = u64::from_le_bytes(bytes);
            assert_ne!(handle, first_handle);
            assert_eq!(
                engine
                    .call_win64(callbacks[2], [handle, data_output, 0, 0, 0, 0])
                    .unwrap(),
                0
            );
            engine.read(data_output, &mut bytes).unwrap();
            let data = u64::from_le_bytes(bytes);
            if cycle == 0 {
                first_handle = handle;
                first_data = data;
            } else {
                assert_eq!(data, first_data);
            }
            assert_eq!(
                engine
                    .call_win64(callbacks[3], [handle, 0, 0, 0, 0, 0])
                    .unwrap(),
                0
            );
            assert_eq!(
                engine
                    .call_win64(callbacks[1], [handle, 0, 0, 0, 0, 0])
                    .unwrap(),
                0
            );
        }

        assert_eq!(
            engine
                .call_win64(callbacks[0], [1, label, 32, 0, handle_output, 0])
                .unwrap(),
            0
        );
        engine.read(handle_output, &mut bytes).unwrap();
        let handle = u64::from_le_bytes(bytes);
        for _ in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
            assert_eq!(
                engine
                    .call_win64(callbacks[5], [label, 128, handle, 0, 0, 0])
                    .unwrap(),
                0
            );
            assert_eq!(
                engine
                    .call_win64(callbacks[5], [label, 32, handle, 0, 0, 0])
                    .unwrap(),
                0
            );
        }
        assert_eq!(
            engine
                .call_win64(callbacks[1], [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .call_win64(callbacks[2], [first_handle, data_output, 0, 0, 0, 0])
                .unwrap(),
            4
        );
    }

    #[test]
    fn execution_trace_records_nested_calls_and_returns_with_rvas() {
        const CODE: u64 = 0x1000_0000;
        // call +1; ret; call +1; ret; ret
        let mut engine = test_engine(&[
            0xe8, 0x01, 0x00, 0x00, 0x00, 0xc3, 0xe8, 0x01, 0x00, 0x00, 0x00, 0xc3, 0xc3,
        ]);
        engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
        let result = engine.call_win64(CODE, [0; 6]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        assert_eq!(trace.schema, "aexcompat.aex-execution-trace");
        assert_eq!(trace.events.first().unwrap().kind, "selector_enter");
        assert_eq!(trace.events.last().unwrap().kind, "selector_exit");
        let calls = trace
            .events
            .iter()
            .filter(|event| event.kind == "guest_call")
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].pc_rva, Some(0));
        assert_eq!(calls[0].target_rva, Some(6));
        assert_eq!(calls[1].depth, 1);
        assert!(
            trace
                .events
                .iter()
                .filter(|event| event.kind == "guest_return")
                .count()
                >= 3
        );
        assert!(
            trace
                .events
                .iter()
                .enumerate()
                .all(|(index, event)| event.sequence == index)
        );
    }

    #[test]
    fn execution_trace_labels_indirect_import_stub_calls() {
        const CODE: u64 = 0x1000_0000;
        let mut code = vec![0x48, 0xb8];
        code.extend_from_slice(&STUB_BASE.to_le_bytes());
        code.extend_from_slice(&[0xff, 0xd0, 0xc3]);
        let mut engine = test_engine(&code);
        engine
            .unicorn
            .mem_write(STUB_BASE, &[0x31, 0xc0, 0xc3])
            .unwrap();
        engine.unicorn.get_data_mut().trace_labels.insert(
            STUB_BASE,
            TraceLabel {
                kind: TraceLabelKind::Import,
                name: "fixture.dll!fixture_import".into(),
            },
        );
        engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
        let result = engine.call_win64(CODE, [0; 6]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        let import = trace
            .events
            .iter()
            .find(|event| event.kind == "import_call")
            .unwrap();
        assert_eq!(import.name.as_deref(), Some("fixture.dll!fixture_import"));
        assert!(!trace.timeline.is_empty());
    }

    #[test]
    fn execution_trace_resolves_register_indirect_guest_target() {
        const CODE: u64 = 0x1000_0000;
        let target = CODE + 13;
        let mut code = vec![0x48, 0xb8];
        code.extend_from_slice(&target.to_le_bytes());
        code.extend_from_slice(&[0xff, 0xd0, 0xc3, 0xc3]);
        let mut engine = test_engine(&code);
        engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
        let result = engine.call_win64(CODE, [0; 6]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        let call = trace
            .events
            .iter()
            .find(|event| event.kind == "guest_call")
            .unwrap();
        assert_eq!(call.call_kind, Some("indirect"));
        assert_eq!(call.target_rva, Some(13));
    }

    #[test]
    fn execution_trace_resolves_rip_relative_indirect_guest_target() {
        const CODE: u64 = 0x1000_0000;
        let target = CODE + 16;
        // call qword ptr [rip+2]; ret; nop; dq target; ret
        let mut code = vec![0xff, 0x15, 0x02, 0, 0, 0, 0xc3, 0x90];
        code.extend_from_slice(&target.to_le_bytes());
        code.push(0xc3);
        let mut engine = test_engine(&code);
        engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
        let result = engine.call_win64(CODE, [0; 6]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        let call = trace
            .events
            .iter()
            .find(|event| event.kind == "guest_call")
            .unwrap();
        assert_eq!(call.call_kind, Some("indirect"));
        assert_eq!(call.target_rva, Some(16));
    }

    #[test]
    fn execution_trace_keeps_ordinary_jump_as_taken_block_edge() {
        const CODE: u64 = 0x1000_0000;
        // jmp +1; int3; mov eax, 42; ret
        let mut engine = test_engine(&[0xeb, 0x01, 0xcc, 0xb8, 42, 0, 0, 0, 0xc3]);
        engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
        let result = engine.call_win64(CODE, [0; 6]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        assert_eq!(result, 42);
        assert!(!trace.events.iter().any(|event| event.kind == "tail_call"));
        assert!(
            trace
                .branch_edges
                .iter()
                .any(|edge| edge.from_rva == 0 && edge.to_rva == 3)
        );
        assert!(trace.basic_blocks.iter().any(|block| block.rva == 3));
    }

    #[test]
    fn execution_trace_records_jump_to_known_function_as_tail_call() {
        const CODE: u64 = 0x1000_0000;
        // call target; jmp target; padding; target: inc byte ptr [rcx]; mov eax,42; ret
        let mut engine = test_engine(&[
            0xe8, 0x06, 0, 0, 0, 0xeb, 0x04, 0x90, 0x90, 0x90, 0x90, 0xfe, 0x01, 0xb8, 42, 0, 0, 0,
            0xc3,
        ]);
        let buffer = engine.allocate(1, 1).unwrap();
        engine.write(buffer, &[1]).unwrap();
        engine.configure_trace_watches(vec![TraceWatchSpec {
            id: "tail-target".into(),
            function_rva: Some(11),
            instruction_rva: None,
            absolute_address: None,
            register: "rcx",
            size: 1,
            occurrence: None,
            image_coordinate: None,
            image_row_offset: None,
            image_format: None,
        }]);
        engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
        let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        assert_eq!(result, 42);
        let tail = trace
            .events
            .iter()
            .find(|event| event.kind == "tail_call")
            .unwrap();
        assert_eq!(tail.pc_rva, Some(5));
        assert_eq!(tail.target_rva, Some(11));
        assert_eq!(tail.call_kind, Some("runtime_jmp"));
        assert!(trace.events.iter().any(|event| {
            event.kind == "guest_return"
                && event.pc_rva == Some(18)
                && event.target_rva.is_none()
                && event.function_rva == Some(11)
        }));
        assert!(!trace.events.iter().any(|event| {
            event.kind == "guest_return"
                && event.pc_rva == Some(18)
                && event.function_rva == Some(0)
        }));
        let entry_function = trace
            .functions
            .iter()
            .find(|function| function.entry_rva == 0)
            .unwrap();
        assert_eq!(entry_function.observed_calls, 2);
        assert_eq!(entry_function.callees, vec![11]);
        assert_eq!(trace.memory_witnesses.len(), 2);
        assert_eq!(trace.memory_witnesses[1].watch_id, "tail-target");
        assert_eq!(trace.memory_witnesses[1].before.u8_values, [2]);
        assert_eq!(trace.memory_witnesses[1].after.u8_values, [3]);
    }

    #[test]
    fn execution_trace_treats_explicitly_watched_first_jump_as_tail_call() {
        const CODE: u64 = 0x1000_0000;
        // jmp target; padding; target: mov rax,[rsp+0x28]; inc byte ptr [rax]; ret
        let mut engine = test_engine(&[
            0xeb, 0x02, 0x90, 0x90, 0x48, 0x8b, 0x44, 0x24, 0x28, 0xfe, 0x00, 0xc3,
        ]);
        let buffer = engine.allocate(1, 1).unwrap();
        engine.write(buffer, &[1]).unwrap();
        engine.configure_trace_watches(vec![TraceWatchSpec {
            id: "first-tail-stack5".into(),
            function_rva: Some(4),
            instruction_rva: None,
            absolute_address: None,
            register: "stack5",
            size: 1,
            occurrence: None,
            image_coordinate: None,
            image_row_offset: None,
            image_format: None,
        }]);
        engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
        let result = engine.call_win64(CODE, [0, 0, 0, 0, buffer, 0]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        let tail = trace
            .events
            .iter()
            .find(|event| event.kind == "tail_call")
            .unwrap();
        assert_eq!(tail.target_rva, Some(4));
        assert_eq!(tail.stack_arguments[0].value.raw, buffer);
        let witness = trace.memory_witnesses.first().unwrap();
        assert_eq!(witness.watch_id, "first-tail-stack5");
        assert_eq!(witness.before.u8_values, [1]);
        assert_eq!(witness.after.u8_values, [2]);
    }

    #[test]
    fn execution_trace_applies_watch_occurrence_to_tail_calls() {
        const CODE: u64 = 0x1000_0000;
        // Call target once, then tail-call it. The second match must be the tail call.
        let mut engine = test_engine(&[
            0xe8, 0x06, 0, 0, 0, 0xeb, 0x04, 0x90, 0x90, 0x90, 0x90, 0xfe, 0x01, 0xb8, 42, 0, 0, 0,
            0xc3,
        ]);
        let buffer = engine.allocate(1, 1).unwrap();
        engine.write(buffer, &[1]).unwrap();
        engine.configure_trace_watches(vec![TraceWatchSpec {
            id: "second-tail-target".into(),
            function_rva: Some(11),
            instruction_rva: None,
            absolute_address: None,
            register: "rcx",
            size: 1,
            occurrence: Some(2),
            image_coordinate: None,
            image_row_offset: None,
            image_format: None,
        }]);
        engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
        let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        assert_eq!(result, 42);
        assert_eq!(trace.memory_witnesses.len(), 1);
        let witness = &trace.memory_witnesses[0];
        assert_eq!(witness.watch_id, "second-tail-target");
        assert_eq!(witness.before.u8_values, [2]);
        assert_eq!(witness.after.u8_values, [3]);
        assert!(
            trace
                .events
                .iter()
                .any(|event| { event.kind == "tail_call" && event.target_rva == Some(11) })
        );
    }

    #[test]
    fn execution_trace_activates_function_watch_at_selector_entry() {
        const CODE: u64 = 0x1000_0000;
        // inc byte ptr [rcx]; ret
        let mut engine = test_engine(&[0xfe, 0x01, 0xc3]);
        let buffer = engine.allocate(1, 1).unwrap();
        engine.write(buffer, &[1]).unwrap();
        engine.configure_trace_watches(vec![TraceWatchSpec {
            id: "selector-entry".into(),
            function_rva: Some(0),
            instruction_rva: None,
            absolute_address: None,
            register: "rcx",
            size: 1,
            occurrence: None,
            image_coordinate: None,
            image_row_offset: None,
            image_format: None,
        }]);
        engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
        let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        let witness = trace.memory_witnesses.first().unwrap();
        assert_eq!(witness.watch_id, "selector-entry");
        assert_eq!(witness.function_rva, Some(0));
        assert_eq!(witness.before.u8_values, [1]);
        assert_eq!(witness.after.u8_values, [2]);
    }

    #[test]
    fn execution_trace_records_rip_relative_constant_access() {
        const CODE: u64 = 0x1000_0000;
        let value = 0x1122_3344_5566_7788u64;
        // mov rax,[rip+1]; ret; dq value
        let mut code = vec![0x48, 0x8b, 0x05, 0x01, 0, 0, 0, 0xc3];
        code.extend_from_slice(&value.to_le_bytes());
        let mut engine = test_engine(&code);
        engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
        let result = engine.call_win64(CODE, [0; 6]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        assert_eq!(result, value);
        let access = trace
            .events
            .iter()
            .find(|event| event.kind == "rip_constant")
            .unwrap();
        assert_eq!(access.pc_rva, Some(0));
        assert_eq!(access.target_rva, Some(8));
        assert!(access.name.as_deref().unwrap().contains("1122334455667788"));
    }

    #[test]
    fn execution_trace_folds_repeated_call_sites_without_losing_count() {
        const CODE: u64 = 0x1000_0000;
        // mov ecx,2; loop: call return; dec ecx; jnz loop; return: ret
        let mut engine = test_engine(&[
            0xb9, 0x02, 0x00, 0x00, 0x00, 0xe8, 0x04, 0x00, 0x00, 0x00, 0xff, 0xc9, 0x75, 0xf7,
            0xc3,
        ]);
        engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
        let result = engine.call_win64(CODE, [0; 6]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        let call = trace
            .events
            .iter()
            .find(|event| event.kind == "guest_call")
            .unwrap();
        assert_eq!(call.pc_rva, Some(5));
        assert_eq!(call.observed_count, 2);
        assert_eq!(call.exemplars.distinct_fingerprints, 2);
        assert_eq!(call.exemplars.first.len(), 2);
        assert_eq!(call.exemplars.last.len(), 2);
        let rcx = call
            .exemplars
            .numeric_ranges
            .iter()
            .find(|range| range.field == "rcx")
            .unwrap();
        assert_eq!((rcx.minimum, rcx.maximum), (1.0, 2.0));
        assert!(trace.timeline.iter().any(|line| line.contains("×2")));
    }

    #[test]
    fn exemplar_fingerprint_tracking_is_bounded_and_explicitly_truncated() {
        let mut exemplars = TraceExemplars::default();
        let mut fingerprints = HashSet::new();
        for index in 0..TRACE_DISTINCT_FINGERPRINTS + 3 {
            update_exemplars(
                &mut exemplars,
                &mut fingerprints,
                TraceObservation {
                    observation: index as u64 + 1,
                    fingerprint: format!("{index:016x}"),
                    call_id: None,
                    arguments: Vec::new(),
                    xmm_arguments: Vec::new(),
                    stack_arguments: Vec::new(),
                    return_value: None,
                },
            );
        }

        assert_eq!(fingerprints.len(), TRACE_DISTINCT_FINGERPRINTS);
        assert_eq!(
            exemplars.distinct_fingerprints,
            TRACE_DISTINCT_FINGERPRINTS as u64
        );
        assert!(exemplars.fingerprint_tracking_truncated);
        assert_eq!(exemplars.untracked_fingerprint_observations, 3);
    }

    #[test]
    fn execution_trace_reports_fingerprint_budget_truncation() {
        const CODE: u64 = 0x1000_0000;
        let iterations = TRACE_DISTINCT_FINGERPRINTS as u32 + 4;
        let mut code = vec![0xb9];
        code.extend_from_slice(&iterations.to_le_bytes());
        code.extend_from_slice(&[0xe8, 0x04, 0, 0, 0, 0xff, 0xc9, 0x75, 0xf7, 0xc3]);
        let mut engine = test_engine(&code);
        engine.begin_execution_trace("RENDER", CODE).unwrap();
        let result = engine.call_win64(CODE, [0; 6]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        let call = trace
            .events
            .iter()
            .find(|event| event.kind == "guest_call")
            .unwrap();
        assert_eq!(call.observed_count, iterations as u64);
        assert!(call.exemplars.fingerprint_tracking_truncated);
        assert_eq!(call.exemplars.untracked_fingerprint_observations, 4);
        assert!(trace.truncation.iter().any(|item| {
            item.category == "exemplar_fingerprints"
                && item.reason == "fingerprint_budget"
                && item.dropped == 4
        }));
        assert!(trace.truncated);
    }

    #[test]
    fn execution_trace_witnesses_memory_before_and_after_a_call() {
        const CODE: u64 = 0x1000_0000;
        // call +1; ret; mov byte ptr [rcx], 0x2a; ret
        let mut engine = test_engine(&[0xe8, 0x01, 0, 0, 0, 0xc3, 0xc6, 0x01, 0x2a, 0xc3]);
        let buffer = engine.allocate(4, 1).unwrap();
        engine.write(buffer, &[1, 2, 3, 4]).unwrap();
        engine.configure_trace_watches(vec![TraceWatchSpec {
            id: "target-buffer".into(),
            function_rva: Some(6),
            instruction_rva: None,
            absolute_address: None,
            register: "rcx",
            size: 4,
            occurrence: None,
            image_coordinate: None,
            image_row_offset: None,
            image_format: None,
        }]);
        engine.begin_execution_trace("RENDER", CODE).unwrap();
        let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        let call = trace
            .events
            .iter()
            .find(|event| event.kind == "guest_call")
            .unwrap();
        let returned = trace
            .events
            .iter()
            .find(|event| event.kind == "guest_return" && event.function_rva == Some(6))
            .unwrap();
        assert_eq!(call.call_id, returned.call_id);
        let witness = trace.memory_witnesses.first().unwrap();
        assert_eq!(witness.call_id, call.call_id);
        assert_eq!(witness.before.u8_values, [1, 2, 3, 4]);
        assert_eq!(witness.after.u8_values, [42, 2, 3, 4]);
        assert_eq!(witness.changed_ranges.len(), 1);
        assert_eq!(witness.changed_ranges[0].offset, 0);
        assert_eq!(witness.changed_ranges[0].size, 1);
    }

    #[test]
    fn execution_trace_selects_one_based_watch_occurrence_without_spending_witness_budget() {
        const CODE: u64 = 0x1000_0000;
        // Call the same target three times, then return 42. The target increments [rcx].
        let mut engine = test_engine(&[
            0xe8, 0x10, 0, 0, 0, 0xe8, 0x0b, 0, 0, 0, 0xe8, 0x06, 0, 0, 0, 0xb8, 42, 0, 0, 0, 0xc3,
            0xfe, 0x01, 0xc3,
        ]);
        let buffer = engine.allocate(1, 1).unwrap();
        engine.write(buffer, &[1]).unwrap();
        engine.configure_trace_watches(vec![TraceWatchSpec {
            id: "second-target-call".into(),
            function_rva: Some(21),
            instruction_rva: None,
            absolute_address: None,
            register: "rcx",
            size: 1,
            occurrence: Some(2),
            image_coordinate: None,
            image_row_offset: None,
            image_format: None,
        }]);
        engine.begin_execution_trace("RENDER", CODE).unwrap();
        let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        assert_eq!(result, 42);
        let mut final_value = [0u8; 1];
        engine.read(buffer, &mut final_value).unwrap();
        assert_eq!(final_value, [4]);
        assert_eq!(trace.memory_witnesses.len(), 1);
        assert_eq!(trace.dropped_memory_witnesses, 0);
        let witness = &trace.memory_witnesses[0];
        assert_eq!(witness.watch_id, "second-target-call");
        assert_eq!(witness.before.u8_values, [2]);
        assert_eq!(witness.after.u8_values, [3]);
    }

    #[test]
    fn inferred_return_path_completes_pending_memory_witness() {
        const CODE: u64 = 0x1000_0000;
        // call target_a; call target_b; ret; nop;
        // target_a: mov byte ptr [rcx],0x2a; ret; target_b: ret
        let mut engine = test_engine(&[
            0xe8, 0x07, 0, 0, 0, 0xe8, 0x06, 0, 0, 0, 0xc3, 0x90, 0xc6, 0x01, 0x2a, 0xc3, 0xc3,
        ]);
        engine.trace_points = vec![CODE, CODE + 5];
        let buffer = engine.allocate(4, 1).unwrap();
        engine.write(buffer, &[1, 2, 3, 4]).unwrap();
        engine.configure_trace_watches(vec![TraceWatchSpec {
            id: "inferred-return".into(),
            function_rva: Some(12),
            instruction_rva: None,
            absolute_address: None,
            register: "rcx",
            size: 4,
            occurrence: None,
            image_coordinate: None,
            image_row_offset: None,
            image_format: None,
        }]);
        engine.begin_execution_trace("RENDER", CODE).unwrap();
        let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
        let trace = engine.finish_execution_trace(result).unwrap();

        let witness = trace
            .memory_witnesses
            .iter()
            .find(|witness| witness.watch_id == "inferred-return")
            .unwrap();
        assert_eq!(witness.before.u8_values, [1, 2, 3, 4]);
        assert_eq!(witness.after.u8_values, [42, 2, 3, 4]);
        assert!(trace.events.iter().any(|event| {
            event.kind == "guest_return" && event.name.as_deref() == Some("inferred_from_stack")
        }));
    }

    #[test]
    fn unicorn_rejects_vex_l256_without_the_bounded_fallback() {
        const CODE: u64 = 0x1000_0000;
        let mut unicorn =
            Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()).unwrap();
        unicorn.mem_map(CODE, PAGE_SIZE, Prot::ALL).unwrap();
        unicorn
            .mem_write(CODE, &[0xc5, 0xfc, 0x10, 0x00]) // vmovups ymm0,[rax]
            .unwrap();
        unicorn.reg_write(RegisterX86::RAX, CODE).unwrap();

        let error = unicorn.emu_start(CODE, CODE + 4, 0, 1).unwrap_err();

        assert_eq!(error, unicorn_engine::unicorn_const::uc_error::INSN_INVALID);
    }

    #[test]
    fn avx_fallback_moves_all_256_bits_between_memory_and_ymm() {
        const CODE: u64 = 0x1000_0000;
        let source = DATA_BASE;
        let destination = DATA_BASE + 32;
        let mut engine = test_engine(&[
            0xc5, 0xfc, 0x10, 0x09, // vmovups ymm1,[rcx]
            0xc5, 0xfc, 0x11, 0x0a, // vmovups [rdx],ymm1
            0xc3,
        ]);
        let expected = std::array::from_fn::<_, 32, _>(|index| (index as u8) ^ 0xa5);
        engine.write(source, &expected).unwrap();

        engine
            .call_win64(CODE, [source, destination, 0, 0, 0, 0])
            .unwrap();

        let mut actual = [0u8; 32];
        engine.unicorn.mem_read(destination, &mut actual).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 2);
    }

    #[test]
    fn avx_fallback_keeps_unimplemented_instructions_fail_closed() {
        const CODE: u64 = 0x1000_0000;
        let mut engine = test_engine(&[
            0xc5, 0xfc, 0x58, 0xc0, // vaddps ymm0,ymm0,ymm0
            0xc3,
        ]);

        let error = engine.call_win64(CODE, [0; 6]).unwrap_err();

        assert!(
            error
                .to_string()
                .to_ascii_lowercase()
                .contains("invalid instruction"),
            "{error}"
        );
        assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 0);
    }

    #[test]
    fn avx_fallback_limit_stops_before_mutating_the_destination() {
        const CODE: u64 = 0x1000_0000;
        let mut engine = test_engine(&[
            0xc5, 0xfc, 0x10, 0x00, // vmovups ymm0,[rax]
            0xc3,
        ]);
        engine
            .unicorn
            .reg_write(RegisterX86::RAX, DATA_BASE)
            .unwrap();
        engine.unicorn.get_data_mut().avx_fallback_instructions = MAX_AVX_FALLBACK_INSTRUCTIONS;
        let before = engine.unicorn.reg_read_long(RegisterX86::YMM0).unwrap();

        engine.unicorn.emu_start(CODE, CODE + 4, 0, 1).unwrap();

        assert!(
            engine
                .unicorn
                .get_data()
                .callback_error
                .as_deref()
                .unwrap()
                .contains("AVX fallback instruction limit exceeded"),
        );
        assert_eq!(
            engine
                .unicorn
                .reg_read_long(RegisterX86::YMM0)
                .unwrap()
                .as_ref(),
            before.as_ref()
        );
    }

    #[test]
    fn avx_fallback_budget_and_defined_registers_reset_per_dispatch() {
        const CODE: u64 = 0x1000_0000;
        let mut engine = test_engine(&[0xc3]);
        engine.unicorn.get_data_mut().avx_fallback_instructions = MAX_AVX_FALLBACK_INSTRUCTIONS;
        engine.unicorn.get_data_mut().avx_defined_ymm[0] = true;
        engine.unicorn.get_data_mut().avx_chain_next_rip = Some(CODE);

        engine.call_win64(CODE, [0; 6]).unwrap();

        assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 0);
        assert_eq!(engine.unicorn.get_data().avx_defined_ymm, [false; 16]);
        assert_eq!(engine.unicorn.get_data().avx_chain_next_rip, None);
    }

    #[test]
    fn avx_fallback_rejects_register_source_after_unicorn_instruction_gap() {
        const CODE: u64 = 0x1000_0000;
        let mut engine = test_engine(&[
            0xc5, 0xf8, 0x57, 0xc0, // vxorps xmm0,xmm0,xmm0
            0xc5, 0xfc, 0x11, 0x01, // vmovups [rcx],ymm0
            0xc3,
        ]);
        engine
            .unicorn
            .reg_write_long(RegisterX86::YMM0, &[0x5a; 32])
            .unwrap();

        let error = engine
            .call_win64(CODE, [DATA_BASE, 0, 0, 0, 0, 0])
            .unwrap_err();

        assert!(
            error
                .to_string()
                .to_ascii_lowercase()
                .contains("invalid instruction"),
            "{error}"
        );
        let mut destination = [0u8; 32];
        engine
            .unicorn
            .mem_read(DATA_BASE, &mut destination)
            .unwrap();
        assert_eq!(destination, [0; 32]);
    }

    #[test]
    fn memory_witness_reports_unmapped_and_oversized_reads() {
        const CODE: u64 = 0x1000_0000;
        let engine = test_engine(&[0xc3]);
        let unmapped =
            trace_memory_snapshot(&engine.unicorn, 0xdead_beef, 16, CODE, CODE + PAGE_SIZE);
        assert_eq!(unmapped.status, "unmapped");
        assert!(unmapped.sha256.is_none());
        let oversized = trace_memory_snapshot(
            &engine.unicorn,
            DATA_BASE,
            MAX_TRACE_WATCH_BYTES + 1,
            CODE,
            CODE + PAGE_SIZE,
        );
        assert_eq!(oversized.status, "oversized");
        assert!(oversized.hex.is_none());
    }

    #[test]
    fn call_envelope_decodes_xmm_stack_and_return_values() {
        const CODE: u64 = 0x1000_0000;
        let mut engine = test_engine(&[0xc3]);
        let xmm_bytes = [
            1.5f32.to_le_bytes(),
            (-2.25f32).to_le_bytes(),
            3.0f32.to_le_bytes(),
            4.5f32.to_le_bytes(),
        ]
        .concat();
        engine
            .unicorn
            .reg_write_long(RegisterX86::XMM0, &xmm_bytes)
            .unwrap();
        let rsp = STACK_BASE + 0x1000;
        engine.unicorn.reg_write(RegisterX86::RSP, rsp).unwrap();
        engine
            .unicorn
            .mem_write(rsp + 0x20, &0x1122_3344_5566_7788u64.to_le_bytes())
            .unwrap();
        engine
            .unicorn
            .reg_write(RegisterX86::RAX, 0xaabb_ccdd)
            .unwrap();

        let xmm = trace_xmm_arguments(&engine.unicorn);
        assert_eq!(xmm[0].register, "xmm0");
        assert_eq!(xmm[0].f32_lanes[0], Some(1.5));
        assert_eq!(xmm[0].f32_lanes[1], Some(-2.25));
        let stack = trace_stack_arguments(&engine.unicorn, rsp, 0x20, CODE, CODE + PAGE_SIZE);
        assert_eq!(stack[0].index, 5);
        assert_eq!(stack[0].value.raw, 0x1122_3344_5566_7788);
        let returned = trace_return_value(&engine.unicorn, CODE, CODE + PAGE_SIZE);
        assert_eq!(returned.rax.raw, 0xaabb_ccdd);
        assert_eq!(returned.xmm0.f32_lanes[2], Some(3.0));
    }

    #[test]
    fn numeric_ranges_include_f64_xmm_lanes() {
        let xmm = TraceXmmValue {
            register: "xmm1",
            raw_hex: String::new(),
            f32_lanes: Vec::new(),
            f64_lanes: vec![Some(1.25), Some(-3.5)],
        };
        let returned = TraceReturnValue {
            rax: TraceValue {
                raw: 0,
                classification: "integer",
                offset: None,
            },
            xmm0: TraceXmmValue {
                register: "xmm0",
                raw_hex: String::new(),
                f32_lanes: Vec::new(),
                f64_lanes: vec![Some(9.75)],
            },
        };
        let observation = TraceObservation {
            observation: 1,
            fingerprint: "0000000000000001".into(),
            call_id: None,
            arguments: Vec::new(),
            xmm_arguments: vec![xmm],
            stack_arguments: Vec::new(),
            return_value: Some(returned),
        };
        let mut exemplars = TraceExemplars::default();

        update_numeric_ranges(&mut exemplars, &observation);

        assert!(exemplars.numeric_ranges.iter().any(|range| {
            range.field == "xmm1.f64[1]" && range.minimum == -3.5 && range.maximum == -3.5
        }));
        assert!(exemplars.numeric_ranges.iter().any(|range| {
            range.field == "xmm0.f64[0]" && range.minimum == 9.75 && range.maximum == 9.75
        }));
    }

    #[test]
    fn trace_event_limit_marks_capture_as_truncated() {
        let mut capture = TraceCapture {
            selector: "GLOBAL_SETUP".into(),
            entry_rva: 0,
            events: Vec::new(),
            return_stack: Vec::new(),
            function_stack: Vec::new(),
            call_rsp_stack: Vec::new(),
            call_id_stack: Vec::new(),
            next_call_id: 1,
            watch_specs: Vec::new(),
            watch_occurrence_counts: HashMap::new(),
            watch_stack: Vec::new(),
            selector_watches: Vec::new(),
            witnesses: Vec::new(),
            dropped_witnesses: 0,
            basic_blocks: HashMap::new(),
            branch_edges: HashMap::new(),
            dropped_basic_blocks: 0,
            dropped_branch_edges: 0,
            previous_block: None,
            event_index: HashMap::new(),
            event_fingerprints: HashMap::new(),
            known_function_entries: HashSet::new(),
            truncated: false,
            dropped_events: 0,
        };
        for index in 0..=MAX_TRACE_EVENTS {
            push_trace_event(
                &mut capture,
                TraceEvent {
                    sequence: 0,
                    observed_count: 1,
                    depth: 0,
                    kind: "guest_call",
                    call_id: Some(index as u64 + 1),
                    function_rva: None,
                    pc_rva: Some(index as u64),
                    target_rva: None,
                    name: None,
                    arguments: Vec::new(),
                    xmm_arguments: Vec::new(),
                    stack_arguments: Vec::new(),
                    return_value: None,
                    exemplars: TraceExemplars::default(),
                    call_kind: None,
                    instruction_bytes: None,
                },
            );
        }
        assert_eq!(capture.events.len(), MAX_TRACE_EVENTS);
        assert!(capture.truncated);
    }

    #[test]
    fn win64_call_rejects_execution_that_does_not_reach_return_sentinel() {
        const CODE: u64 = 0x1000_0000;
        let mut engine = test_engine(&[0xf4]); // hlt
        let error = engine.call_win64(CODE, [0; 6]).unwrap_err().to_string();
        assert!(error.contains("before the guest returned"), "{error}");
    }

    #[test]
    fn win64_call_timeout_still_fails_closed() {
        const CODE: u64 = 0x1000_0000;
        let mut engine = test_engine(&[0xeb, 0xfe]); // jmp $
        let error = engine
            .call_win64_with_timeout(CODE, &[0; 6], 1_000)
            .unwrap_err();
        assert!(
            error.to_string().contains("before the guest returned"),
            "{error}"
        );
        let GuestError::ExecutionCrash { snapshot, .. } = error else {
            panic!("expected structured crash snapshot");
        };
        assert_eq!(snapshot.registers.len(), 18);
        assert_eq!(snapshot.xmm_registers.len(), 16);
        assert_eq!(snapshot.instruction_rva, Some(0));
        assert!(!snapshot.instruction_bytes.is_empty());
    }

    #[test]
    fn block_census_is_opt_in_and_counts_repeated_guest_work() {
        const CODE: u64 = 0x1000_0000;
        // mov ecx,10; dec ecx; jne -4; ret
        let mut engine = test_engine(&[0xb9, 10, 0, 0, 0, 0xff, 0xc9, 0x75, 0xfc, 0xc3]);
        engine.begin_block_census().unwrap();
        engine.call_win64(CODE, [0; 6]).unwrap();
        let census = engine.finish_block_census(1).unwrap();
        assert!(census.total_block_executions >= 10);
        assert!(census.estimated_dynamic_instructions >= 20);
        assert_eq!(census.output_pixels, 1);
        assert_eq!(
            census.estimated_dynamic_instructions_per_pixel,
            census.estimated_dynamic_instructions as f64
        );
        assert!(!census.extents.is_empty());
        assert!(census.top_1_extent_dynamic_instruction_fraction > 0.0);
    }

    #[test]
    fn census_extents_merge_overlapping_translation_block_variants() {
        let block = |address, size_bytes, dynamic_instructions| CensusBlock {
            address,
            rva: address - 0x1000,
            size_bytes,
            executions: 1,
            instructions: dynamic_instructions as u32,
            dynamic_instructions,
            scalar_sse_fp_instructions: 0,
            dynamic_scalar_sse_fp_instructions: 0,
        };
        let extents = coalesce_census_extents(
            &[
                block(0x1010, 8, 20),
                block(0x1014, 8, 30),
                block(0x1020, 4, 60),
            ],
            0x1000,
            110,
        );
        assert_eq!(extents.len(), 2);
        assert_eq!(extents[0].start_rva, 0x20);
        assert_eq!(extents[0].dynamic_instruction_fraction, 60.0 / 110.0);
        assert_eq!(extents[1].start_rva, 0x10);
        assert_eq!(extents[1].end_rva, 0x1c);
        assert_eq!(extents[1].block_variants, 2);
    }
}
