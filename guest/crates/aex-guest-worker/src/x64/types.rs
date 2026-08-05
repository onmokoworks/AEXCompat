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
    unsupported_import: Option<(String, String)>,
    smart_input_world: u64,
    smart_output_world: u64,
    smart_width: u32,
    smart_height: u32,
    smart_pixel_format: i32,
    render_pixel_format: i32,
    smart_current_time: i32,
    smart_current_time_scale: u32,
    suite_requests: Vec<String>,
    unsupported_suite_calls: Vec<UnsupportedSuiteCall>,
    dropped_unsupported_suite_calls: u64,
    selector_dispatch_active: bool,
    pending_unsupported_suite: Option<PendingUnsupportedSuite>,
    selector_abort: Option<SelectorAbortRecord>,
    pre_checkout_calls: u32,
    pre_checkout_requests: Vec<[i32; 4]>,
    smart_checkout_ids: HashMap<i32, SmartCheckout>,
    checkout_pixels_calls: u32,
    checkin_pixels_calls: u32,
    checkout_output_calls: u32,
    input_parameter_definition: u64,
    parameter_definitions: Vec<u64>,
    next_handle_data: u64,
    next_pf_handle_data: u64,
    image_region: Option<(u64, u64)>,
    image_executable_ranges: Vec<(u64, u64)>,
    latest_runtime_target: Option<TraceRuntimeTarget>,
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
    pending_iterate: Option<PendingIterate>,
    vcomp_dynamic_loop: Option<VcompDynamicLoop>,
    vcomp_requested_threads: Option<u32>,
    msvcp_mutexes: HashMap<u64, MsvcpMutex>,
    plugin_data_registry: EffectRegistry,
    plugin_data_error: Option<String>,
    crt_heap: CrtHeap,
    extended_strings: HashMap<i32, u64>,
    extended_empty_string: u64,
    extended_string_table_valid: bool,
    avx_fallback_instructions: u64,
    avx_defined_ymm: [bool; 16],
    avx_state_sync_points: HashMap<u64, AvxStateSync>,
    gpu_runtime: GpuRuntime,
    gpu_suite: GpuSuiteState,
}

#[derive(Clone, Debug)]
struct PendingUnsupportedSuite {
    name: String,
    version: u64,
    acquire_error: i32,
    caller_rsp: u64,
    return_address: u64,
}

#[derive(Clone, Debug)]
struct SelectorAbortRecord {
    error: i32,
    suite: PendingUnsupportedSuite,
}

#[derive(Clone, Copy, Debug)]
struct SmartCheckout {
    index: i32,
    world: u64,
    checked_out: bool,
}

#[derive(Clone, Debug)]
struct VcompDynamicLoop {
    current: i32,
    upper: i32,
    chunk: i32,
    exhausted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MsvcpMutex {
    mutex_type: u32,
    lock_count: u32,
}

#[derive(Clone, Debug)]
struct PendingIterate {
    caller_rsp: u64,
    return_address: u64,
    refcon: u64,
    pixel_function: u64,
    source_data: u64,
    source_rowbytes: u64,
    source_width: i32,
    source_height: i32,
    zero_outside_source: bool,
    destination_data: u64,
    destination_rowbytes: u64,
    left: i32,
    right: i32,
    bottom: i32,
    x: i32,
    y: i32,
    origin_x: i32,
    origin_y: i32,
    pixel_bytes: u64,
    continuation: u64,
    callback_name: &'static str,
    callback_phase: IterateCallbackPhase,
    abort_function: u64,
    progress_function: u64,
    effect_ref: u64,
    progress_base: i32,
    progress_final: i32,
    top: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IterateCallbackPhase {
    Pixel,
    Progress,
    Abort,
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
        mappings.extend(self.unicorn.get_data().gpu_suite.mapped_regions());
        {
            let state = self.unicorn.get_data_mut();
            state.gpu_suite.clear_for_drop();
            if state.gpu_runtime.is_active() {
                let _ = state.gpu_runtime.end();
            }
        }
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
pub struct TraceTargetClassification {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceTargetRegister {
    pub name: String,
    pub value: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceTargetMemory {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_register: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_value: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_register: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_value: Option<u64>,
    pub scale: u32,
    pub displacement: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dereferenced_target: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceRuntimeTarget {
    pub source_address: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_rva: Option<u64>,
    pub transfer_kind: &'static str,
    pub operand_kind: &'static str,
    pub instruction_bytes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_target: Option<u64>,
    pub target: TraceTargetClassification,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub register: Option<TraceTargetRegister>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<TraceTargetMemory>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_target: Option<TraceRuntimeTarget>,
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
