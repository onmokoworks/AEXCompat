use aex_abi::x86_64_windows as abi;
use iced_x86::{Decoder, DecoderOptions, Mnemonic, OpKind, Register};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use thiserror::Error;
use unicorn_engine::unicorn_const::{Arch, Mode, Prot};
use unicorn_engine::{RegisterX86, UcHookId, Unicorn};

use crate::pe::PeImage;

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
const HOST_HANDLE_SUITE: u64 = STUB_BASE + 0x81000;
const DATA_BASE: u64 = 0x0000_0000_4000_0000;
const DATA_SIZE: u64 = 0x1000_0000;
const HANDLE_DATA_BASE: u64 = DATA_BASE + 0x400_0000;
const HANDLE_DATA_END: u64 = DATA_BASE + DATA_SIZE;
// A nonzero Unicorn instruction limit enables instruction counting across the
// whole run, which is prohibitively expensive for image kernels. The wall-clock
// timeout and return-sentinel check still bound and validate guest execution.
const MAX_INSTRUCTIONS: usize = 0;
const TIMEOUT_MICROSECONDS: u64 = 600_000_000;
const MAX_TRACE_EVENTS: usize = 50_000;
const TRACE_STACK_ARGUMENTS: usize = 4;

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
    if let Some(key) = &key {
        if let Some(index) = capture.event_index.get(key).copied() {
            capture.events[index].observed_count += 1;
            return;
        }
    }
    if capture.events.len() >= MAX_TRACE_EVENTS {
        capture.truncated = true;
        return;
    }
    if let Some(key) = key {
        capture.event_index.insert(key, capture.events.len());
    }
    event.sequence = capture.events.len();
    capture.events.push(event);
}

fn trace_instruction(
    unicorn: &mut Unicorn<'_, GuestState>,
    address: u64,
    size: u32,
    image_base: u64,
    image_end: u64,
) {
    let rsp = unicorn.reg_read(RegisterX86::RSP).unwrap_or(0);
    let return_value = trace_return_value(unicorn, image_base, image_end);
    if let Some(capture) = unicorn.get_data_mut().trace.as_mut() {
        while capture
            .call_rsp_stack
            .last()
            .is_some_and(|caller_rsp| rsp >= *caller_rsp)
        {
            capture.call_rsp_stack.pop();
            let target = capture.return_stack.pop();
            let function_rva = capture.function_stack.pop().flatten();
            push_trace_event(
                capture,
                TraceEvent {
                    sequence: 0,
                    observed_count: 1,
                    depth: capture.return_stack.len(),
                    kind: "guest_return",
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
                    call_kind: None,
                    instruction_bytes: None,
                },
            );
        }
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
    if mnemonic == Mnemonic::Call {
        let target = resolve_runtime_target(unicorn, &instruction);
        let target_rva = (image_base..image_end)
            .contains(&target.unwrap_or(0))
            .then(|| target.expect("range-checked target") - image_base);
        let return_address = address.saturating_add(instruction.len() as u64);
        if let Some(capture) = unicorn.get_data_mut().trace.as_mut() {
            let depth = capture.return_stack.len();
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
                    function_rva,
                    pc_rva: (image_base..image_end)
                        .contains(&address)
                        .then(|| address - image_base),
                    target_rva,
                    name: None,
                    arguments,
                    xmm_arguments,
                    stack_arguments: call_stack_arguments,
                    return_value: None,
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
            capture.call_rsp_stack.push(rsp);
        }
    } else if matches!(
        mnemonic,
        Mnemonic::Ret | Mnemonic::Retf | Mnemonic::Iret | Mnemonic::Iretd | Mnemonic::Iretq
    ) && let Some(capture) = unicorn.get_data_mut().trace.as_mut()
    {
        let depth = capture.return_stack.len().saturating_sub(1);
        let target = capture.return_stack.pop();
        let function_rva = capture.function_stack.pop().flatten();
        capture.call_rsp_stack.pop();
        push_trace_event(
            capture,
            TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth,
                kind: "guest_return",
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

fn classify_trace_value(raw: u64, image_base: u64, image_end: u64) -> TraceValue {
    let (classification, offset) = if (image_base..image_end).contains(&raw) {
        ("image", Some(raw - image_base))
    } else if (DATA_BASE..DATA_BASE + DATA_SIZE).contains(&raw) {
        ("guest_data", Some(raw - DATA_BASE))
    } else if (STACK_BASE..STACK_BASE + STACK_SIZE).contains(&raw) {
        ("guest_stack", Some(raw - STACK_BASE))
    } else if (STUB_BASE..STUB_BASE + STUB_SIZE).contains(&raw) {
        ("host_stub", Some(raw - STUB_BASE))
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
            "guest_call" => {
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

#[derive(Default)]
struct GuestState {
    params: Vec<GuestParam>,
    callback_error: Option<String>,
    smart_input_world: u64,
    smart_output_world: u64,
    smart_width: u32,
    smart_height: u32,
    suite_requests: Vec<String>,
    pre_checkout_calls: u32,
    pre_checkout_requests: Vec<[i32; 4]>,
    checkout_pixels_calls: u32,
    checkout_output_calls: u32,
    parameter_definitions: Vec<u64>,
    next_handle_data: u64,
    handles: HashMap<u64, GuestHandle>,
    math_calls: Vec<String>,
    handle_allocations: Vec<u64>,
    census_blocks: HashMap<(u64, u32), u64>,
    trace: Option<TraceCapture>,
    trace_labels: HashMap<u64, TraceLabel>,
}

#[derive(Clone, Debug)]
struct GuestHandle {
    data: u64,
    size: u64,
    locks: u32,
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
    event_index: HashMap<TraceEventKey, usize>,
    truncated: bool,
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
pub struct TraceEvent {
    pub sequence: usize,
    pub observed_count: u64,
    pub depth: usize,
    pub kind: &'static str,
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
    pub selector: String,
    pub entry_rva: u64,
    pub return_value: u64,
    pub truncated: bool,
    pub events: Vec<TraceEvent>,
    pub functions: Vec<TraceFunction>,
    pub state_changes: Vec<TraceStateValue>,
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
        let mut unicorn = uc(
            "create x86_64 engine",
            Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()),
        )?;
        unicorn.get_data_mut().next_handle_data = HANDLE_DATA_BASE;
        let image_size =
            u64::try_from(image.mapped_bytes().len()).map_err(|_| GuestError::ImageAlignment)?;
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
                    "expf" => install_float_import(&mut unicorn, stub, "expf", f32::exp)?,
                    "floorf" => install_float_import(&mut unicorn, stub, "floorf", f32::floor)?,
                    "powf" => install_float_binary_import(&mut unicorn, stub, "powf", f32::powf)?,
                    "pow" => install_double_binary_import(&mut unicorn, stub, "pow", f64::powf)?,
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
        };
        if let Some(entry) = image.dll_entry_address() {
            let attached = engine.call_win64(entry, [image.image_base(), 1, 0, 0, 0, 0])?;
            if attached == 0 {
                return Err(GuestError::DllProcessAttach);
            }
        }
        Ok(engine)
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
        self.unicorn.get_data_mut().trace = Some(TraceCapture {
            selector: selector.to_string(),
            entry_rva: entry_address.saturating_sub(self.image_base),
            events: vec![TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth: 0,
                kind: "selector_enter",
                function_rva: Some(entry_address.saturating_sub(self.image_base)),
                pc_rva: Some(entry_address.saturating_sub(self.image_base)),
                target_rva: None,
                name: Some(selector.to_string()),
                arguments: Vec::new(),
                xmm_arguments: Vec::new(),
                stack_arguments: Vec::new(),
                return_value: None,
                call_kind: None,
                instruction_bytes: None,
            }],
            return_stack: Vec::new(),
            function_stack: vec![Some(entry_address.saturating_sub(self.image_base))],
            call_rsp_stack: Vec::new(),
            event_index: HashMap::new(),
            truncated: false,
        });
        let image_base = self.image_base;
        let image_end = self.image_end;
        let mut hook_points = self.trace_points.clone();
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
        let mut capture =
            self.unicorn.get_data_mut().trace.take().ok_or_else(|| {
                GuestError::Callback("guest execution trace is not active".into())
            })?;
        if capture.events.len() >= MAX_TRACE_EVENTS {
            capture.events.pop();
            capture.truncated = true;
        }
        let entry_rva = capture.entry_rva;
        push_trace_event(
            &mut capture,
            TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth: 0,
                kind: "selector_exit",
                function_rva: Some(entry_rva),
                pc_rva: Some(entry_rva),
                target_rva: None,
                name: Some(format!("return={return_value:#x}")),
                arguments: Vec::new(),
                xmm_arguments: Vec::new(),
                stack_arguments: Vec::new(),
                return_value: None,
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
        Ok(ExecutionTrace {
            schema: "aexcompat.aex-execution-trace",
            schema_version: 1,
            execution_backend: self.backend_name(),
            image_sha256: self.image_sha256.clone(),
            preferred_image_base: self.image_base,
            entry_export: self.entry_export.clone(),
            selector: capture.selector,
            entry_rva: capture.entry_rva,
            return_value,
            truncated: capture.truncated,
            events: capture.events,
            functions,
            state_changes: Vec::new(),
            timeline,
        })
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
        self.call_win64_with_timeout(address, args, TIMEOUT_MICROSECONDS)
    }

    fn call_win64_with_timeout(
        &mut self,
        address: u64,
        args: [u64; 6],
        timeout_microseconds: u64,
    ) -> Result<u64, GuestError> {
        let stack_top = STACK_BASE + STACK_SIZE;
        // Win64 function entry observes RSP % 16 == 8. Reserve a return address,
        // 32-byte shadow space, two stack arguments, and bounded scratch.
        let rsp = (stack_top - 0x108) | 8;
        uc(
            "write return address",
            self.unicorn.mem_write(rsp, &RETURN_ADDRESS.to_le_bytes()),
        )?;
        uc(
            "write argument 5",
            self.unicorn.mem_write(rsp + 0x28, &args[4].to_le_bytes()),
        )?;
        uc(
            "write argument 6",
            self.unicorn.mem_write(rsp + 0x30, &args[5].to_le_bytes()),
        )?;
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
            let rip = self.unicorn.reg_read(RegisterX86::RIP).unwrap_or(0);
            let rbx = self.unicorn.reg_read(RegisterX86::RBX).unwrap_or(0);
            let rcx = self.unicorn.reg_read(RegisterX86::RCX).unwrap_or(0);
            let rdx = self.unicorn.reg_read(RegisterX86::RDX).unwrap_or(0);
            let rbp = self.unicorn.reg_read(RegisterX86::RBP).unwrap_or(0);
            let r8 = self.unicorn.reg_read(RegisterX86::R8).unwrap_or(0);
            let suites = self.unicorn.get_data().suite_requests.join(", ");
            let math_calls = self.unicorn.get_data().math_calls.join(", ");
            let handle_allocations = &self.unicorn.get_data().handle_allocations;
            return Err(GuestError::Unicorn {
                operation: "execute guest function",
                detail: format!(
                    "{error} at RIP={rip:#x}, RBX={rbx:#x}, RBP={rbp:#x}, RCX={rcx:#x}, RDX={rdx:#x}, R8={r8:#x}, suite requests=[{suites}], handle allocations={handle_allocations:?}, math calls=[{math_calls}]"
                ),
            });
        }
        let rip = uc(
            "read instruction pointer",
            self.unicorn.reg_read(RegisterX86::RIP),
        )?;
        if rip != RETURN_ADDRESS {
            return Err(GuestError::Unicorn {
                operation: "execute guest function",
                detail: format!("execution stopped before the guest returned (RIP={rip:#x})"),
            });
        }
        if let Some(error) = self.unicorn.get_data_mut().callback_error.take() {
            return Err(GuestError::Callback(error));
        }
        uc("read return value", self.unicorn.reg_read(RegisterX86::RAX))
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
        if end > DATA_BASE + DATA_SIZE {
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

    pub fn configure_parameter_definitions(&mut self, definitions: Vec<u64>) {
        self.unicorn.get_data_mut().parameter_definitions = definitions;
    }

    pub fn suite_requests(&self) -> &[String] {
        &self.unicorn.get_data().suite_requests
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
    ) {
        let state = self.unicorn.get_data_mut();
        state.pre_checkout_requests.clear();
        state.smart_input_world = input_world;
        state.smart_output_world = output_world;
        state.smart_width = width;
        state.smart_height = height;
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
        for index in 0..4096u64 {
            let mut byte = [0u8; 1];
            unicorn
                .mem_read(source + index, &mut byte)
                .map_err(|error| format!("strcpy source read: {error}"))?;
            unicorn
                .mem_write(destination + index, &byte)
                .map_err(|error| format!("strcpy destination write: {error}"))?;
            if byte[0] == 0 {
                return Ok(destination);
            }
        }
        Err("strcpy source exceeds 4096 bytes".to_string())
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
    unicorn
        .get_data_mut()
        .suite_requests
        .push(format!("{name} v{version}"));
    let output = unicorn.reg_read(RegisterX86::R8).unwrap_or_default();
    if name == "PF Handle Suite" && version == 2 && output != 0 {
        if unicorn
            .mem_write(output, &HOST_HANDLE_SUITE.to_le_bytes())
            .is_ok()
        {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            return;
        }
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, u32::MAX as u64);
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

fn emulate_new_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let size = unicorn.reg_read(RegisterX86::RCX).unwrap_or(u64::MAX);
    unicorn.get_data_mut().handle_allocations.push(size);
    let allocation = (|| {
        if size > 128 * 1024 * 1024 {
            return Err(format!("handle allocation exceeds 128 MiB: {size}"));
        }
        let state = unicorn.get_data_mut();
        let handle = (state.next_handle_data + 7) & !7;
        let data = (handle + 8 + 15) & !15;
        let end = data
            .checked_add(size.max(1))
            .ok_or_else(|| "handle allocation overflow".to_string())?;
        if end > HANDLE_DATA_END {
            return Err("handle arena exhausted".to_string());
        }
        state.next_handle_data = end;
        state.handles.insert(
            handle,
            GuestHandle {
                data,
                size,
                locks: 0,
            },
        );
        Ok((handle, data))
    })();
    match allocation {
        Ok((handle, data)) => {
            let _ = unicorn.mem_write(handle, &data.to_le_bytes());
            if size != 0 {
                let _ = unicorn.mem_write(data, &vec![0u8; size as usize]);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, handle);
        }
        Err(_) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
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
    let _ = unicorn.reg_write(RegisterX86::RAX, data.unwrap_or_default());
}

fn emulate_unlock_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    if let Some(record) = unicorn.get_data_mut().handles.get_mut(&handle) {
        record.locks = record.locks.saturating_sub(1);
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_dispose_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    unicorn.get_data_mut().handles.remove(&handle);
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_handle_size(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let handle = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
    let size = unicorn
        .get_data()
        .handles
        .get(&handle)
        .map(|record| record.size)
        .unwrap_or_default();
    let _ = unicorn.reg_write(RegisterX86::RAX, size);
}

fn emulate_resize_handle(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let size = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("resize-handle size: {error}"))?;
        let handle_pointer = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("resize-handle pointer: {error}"))?;
        if size > 128 * 1024 * 1024 || handle_pointer == 0 {
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
        let data = {
            let state = unicorn.get_data_mut();
            let data = (state.next_handle_data + 15) & !15;
            let end = data
                .checked_add(size.max(1))
                .ok_or_else(|| "resize-handle overflow".to_string())?;
            if end > HANDLE_DATA_END {
                return Err("handle arena exhausted".to_string());
            }
            state.next_handle_data = end;
            data
        };
        let mut bytes = vec![0u8; size as usize];
        let copied = old.size.min(size) as usize;
        if copied != 0 {
            unicorn
                .mem_read(old.data, &mut bytes[..copied])
                .map_err(|error| format!("resize-handle old data: {error}"))?;
        }
        if size != 0 {
            unicorn
                .mem_write(data, &bytes)
                .map_err(|error| format!("resize-handle new data: {error}"))?;
        }
        unicorn
            .mem_write(handle, &data.to_le_bytes())
            .map_err(|error| format!("resize-handle record: {error}"))?;
        if let Some(record) = unicorn.get_data_mut().handles.get_mut(&handle) {
            record.data = data;
            record.size = size;
        }
        Ok(())
    })();
    let _ = unicorn.reg_write(RegisterX86::RAX, if result.is_ok() { 0 } else { 4 });
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
        let mut unicorn =
            Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()).unwrap();
        unicorn.mem_map(CODE, PAGE_SIZE, Prot::ALL).unwrap();
        unicorn.mem_map(STUB_BASE, STUB_SIZE, Prot::ALL).unwrap();
        unicorn
            .mem_map(STACK_BASE, STACK_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
        unicorn.mem_write(CODE, code).unwrap();
        unicorn.mem_write(RETURN_ADDRESS, &[0xcc]).unwrap();
        let mut trace_points = Vec::new();
        let mut decoder = Decoder::with_ip(64, code, CODE, DecoderOptions::NONE);
        while decoder.can_decode() {
            let instruction = decoder.decode();
            if instruction.mnemonic() == Mnemonic::Call
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
        }
    }

    #[test]
    fn win64_call_places_register_arguments_and_returns_rax() {
        const CODE: u64 = 0x1000_0000;
        // mov rax, rcx; add rax, rdx; ret
        let mut engine = test_engine(&[0x48, 0x89, 0xc8, 0x48, 0x01, 0xd0, 0xc3]);
        assert_eq!(engine.call_win64(CODE, [40, 2, 0, 0, 0, 0]).unwrap(), 42);
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
        assert!(trace.timeline.iter().any(|line| line.contains("×2")));
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
    fn trace_event_limit_marks_capture_as_truncated() {
        let mut capture = TraceCapture {
            selector: "GLOBAL_SETUP".into(),
            entry_rva: 0,
            events: Vec::new(),
            return_stack: Vec::new(),
            function_stack: Vec::new(),
            call_rsp_stack: Vec::new(),
            event_index: HashMap::new(),
            truncated: false,
        };
        for index in 0..=MAX_TRACE_EVENTS {
            push_trace_event(
                &mut capture,
                TraceEvent {
                    sequence: 0,
                    observed_count: 1,
                    depth: 0,
                    kind: "guest_call",
                    function_rva: None,
                    pc_rva: Some(index as u64),
                    target_rva: None,
                    name: None,
                    arguments: Vec::new(),
                    xmm_arguments: Vec::new(),
                    stack_arguments: Vec::new(),
                    return_value: None,
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
            .call_win64_with_timeout(CODE, [0; 6], 1_000)
            .unwrap_err()
            .to_string();
        assert!(error.contains("before the guest returned"), "{error}");
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
