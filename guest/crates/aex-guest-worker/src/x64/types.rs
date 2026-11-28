use super::files::GuestFiles;
use super::gpu_runtime::GpuRuntime;
use super::gpu_suite::GpuSuiteState;
use super::libraries::GuestLibrary;
use super::mutex::WindowsKernelObjects;
use super::trace::AvxStateSync;
use super::ui_session::UiSessionState;
use super::{
    BTreeMap, BTreeSet, CRT_HEAP_BASE, CRT_HEAP_END, Context, CrtHeap, EffectRegistry, HashMap,
    HashSet, MAX_CRT_HEAP_BYTES, MAX_SUITE_REQUESTS, MAX_UNSUPPORTED_SUITE_CALLS, Serialize,
    UcHookId, Unicorn, VecDeque,
};

#[derive(Clone, Debug)]
pub struct GuestParam {
    pub index: i32,
    pub param_type: i32,
    pub name: String,
    pub bytes: Vec<u8>,
}

fn parameter_disk_id(parameter: &GuestParam) -> Option<i32> {
    parameter.bytes.get(..4).and_then(|bytes| {
        let bytes: [u8; 4] = bytes.try_into().ok()?;
        Some(i32::from_le_bytes(bytes))
    })
}

/// Where a smart checkout index landed among the declared parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayerParameterResolution {
    /// Zero-based offset into the captured parameter list (slot - 1).
    pub offset: usize,
    /// `true` when the positional slot was not a layer and the offset came
    /// from matching the index against the layer parameters' disk ids. AE and
    /// the minihost's `pre_checkout_layer` resolve positionally only, so this
    /// is a host-side substitution the caller must record.
    pub disk_id_fallback: bool,
}

/// Resolves a `PF_CHECKOUT_LAYER`/pre-checkout index to a layer parameter.
///
/// Positional resolution wins outright: when parameter `index` (1-based) is a
/// `PF_Param_LAYER`, that slot is returned without consulting disk ids, no
/// matter what other parameters declare as their id. Only when the positional
/// slot is absent or not a layer are disk ids consulted, and only among layer
/// parameters, so a slider or popup that happens to carry the same id never
/// makes a valid positional checkout ambiguous.
pub(crate) fn resolve_layer_parameter_offset(
    parameters: &[GuestParam],
    checkout_index: i32,
) -> Result<LayerParameterResolution, String> {
    let positional = usize::try_from(checkout_index)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|offset| parameters.get(offset).map(|parameter| (offset, parameter)));
    if let Some((offset, parameter)) = positional
        && parameter.param_type == 0
    {
        return Ok(LayerParameterResolution {
            offset,
            disk_id_fallback: false,
        });
    }
    let mut layer_disk_matches = parameters.iter().enumerate().filter(|(_, parameter)| {
        parameter.param_type == 0 && parameter_disk_id(parameter) == Some(checkout_index)
    });
    let disk_match = layer_disk_matches.next();
    if disk_match.is_some() && layer_disk_matches.next().is_some() {
        return Err(format!(
            "smart checkout index={checkout_index} is not a positional PF_Param_LAYER and its disk_id is duplicated among layer parameters"
        ));
    }
    if let Some((offset, _)) = disk_match {
        return Ok(LayerParameterResolution {
            offset,
            disk_id_fallback: true,
        });
    }
    if positional.is_some() {
        Err(format!(
            "smart checkout index={checkout_index} is not a PF_Param_LAYER"
        ))
    } else {
        Err(format!(
            "smart checkout index={checkout_index} does not resolve to a declared parameter"
        ))
    }
}

/// A smart checkout that the host resolved through the disk-id fallback in
/// `resolve_layer_parameter_offset` instead of positionally. AE and the
/// minihost have no such fallback, so each one is a host-side substitution
/// that a sweep must be able to see next to the render result.
///
/// Records are keyed by `(requested_index, resolved_slot)` and accumulate for
/// the engine's lifetime: a repeat checkout bumps `call_count` instead of
/// adding an entry, and nothing clears the list between resident frames, so
/// a per-frame report carries the running total across every frame the
/// engine has rendered so far (the same convention as
/// `unsupported_suite_calls`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SmartCheckoutDiskIdFallback {
    pub requested_index: i32,
    pub resolved_slot: usize,
    pub call_count: u64,
}

/// Dedupes on `(requested_index, resolved_slot)` and never removes an entry;
/// see `SmartCheckoutDiskIdFallback` for the resulting cumulative semantics.
pub(crate) fn record_smart_checkout_disk_id_fallback(
    records: &mut Vec<SmartCheckoutDiskIdFallback>,
    requested_index: i32,
    resolved_slot: usize,
) {
    if let Some(record) = records.iter_mut().find(|record| {
        record.requested_index == requested_index && record.resolved_slot == resolved_slot
    }) {
        record.call_count += 1;
    } else {
        records.push(SmartCheckoutDiskIdFallback {
            requested_index,
            resolved_slot,
            call_count: 1,
        });
    }
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

pub(super) fn record_named_unsupported_suite_call(
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

#[derive(Clone, Copy)]
pub(super) struct WindowsInitOnceState {
    pub(super) owner: Option<u32>,
    pub(super) context: u64,
    pub(super) complete: bool,
}

#[derive(Default)]
pub(super) struct GuestState {
    pub(super) ui_session: UiSessionState,
    pub(super) pointer_encoding_key: Option<u64>,
    pub(super) crt_wlocale_buffer: Option<u64>,
    pub(super) crt_pctype_buffer: Option<u64>,
    pub(super) crt_locale_names_buffer: Option<u64>,
    pub(super) crt_lconv_buffer: Option<u64>,
    pub(super) crt_locales: BTreeSet<u64>,
    pub(super) next_crt_locale: u64,
    pub(super) crt_tm_buffers: BTreeMap<u32, u64>,
    pub(super) crt_strerror_buffers: BTreeMap<u32, u64>,
    pub(super) windows_hostent_buffers: BTreeMap<u32, u64>,
    pub(super) loaded_libraries: BTreeMap<String, GuestLibrary>,
    pub(super) sapphire_filebuf_fgetc_return: Option<u64>,
    pub(super) params: Vec<GuestParam>,
    pub(super) popup_choice_pages: u64,
    pub(super) custom_ui_registration: Option<CustomUiRegistration>,
    pub(super) callback_error: Option<String>,
    pub(super) unsupported_import: Option<(String, String)>,
    pub(super) smart_input_world: u64,
    pub(super) smart_output_world: u64,
    pub(super) smart_width: u32,
    pub(super) smart_height: u32,
    pub(super) smart_pixel_format: i32,
    pub(super) render_pixel_format: i32,
    pub(super) smart_current_time: i32,
    pub(super) smart_current_time_scale: u32,
    pub(super) suite_requests: Vec<String>,
    pub(super) options_button_name: Option<Vec<u8>>,
    pub(super) persistent_blob_active: bool,
    pub(super) persistent_strings: HashMap<(Vec<u8>, Vec<u8>), Vec<u8>>,
    pub(super) persistent_longs: HashMap<(Vec<u8>, Vec<u8>), i32>,
    pub(super) addrinfo_allocations: HashSet<u64>,
    pub(super) unsupported_suite_calls: Vec<UnsupportedSuiteCall>,
    pub(super) dropped_unsupported_suite_calls: u64,
    pub(super) smart_checkout_disk_id_fallbacks: Vec<SmartCheckoutDiskIdFallback>,
    pub(super) aegp_compute_cache_classes: HashMap<Vec<u8>, [u64; 4]>,
    pub(super) selector_dispatch_active: bool,
    pub(super) pending_unsupported_suite: Option<PendingUnsupportedSuite>,
    pub(super) selector_abort: Option<SelectorAbortRecord>,
    pub(super) pre_checkout_calls: u32,
    pub(super) pre_checkout_requests: Vec<[i32; 4]>,
    pub(super) smart_checkout_ids: HashMap<i32, SmartCheckout>,
    pub(super) checkout_pixels_calls: u32,
    pub(super) checkin_pixels_calls: u32,
    pub(super) checkout_output_calls: u32,
    pub(super) input_parameter_definition: u64,
    pub(super) parameter_definitions: Vec<u64>,
    pub(super) next_handle_data: u64,
    pub(super) next_pf_handle_data: u64,
    pub(super) image_region: Option<(u64, u64)>,
    pub(super) image_executable_ranges: Vec<(u64, u64)>,
    pub(super) sealed_image_read_ranges: Vec<(u64, u64)>,
    pub(super) sealed_image_reads: bool,
    pub(super) latest_runtime_target: Option<TraceRuntimeTarget>,
    pub(super) handles: HashMap<u64, GuestHandle>,
    pub(super) worlds: HashMap<u64, GuestWorld>,
    pub(super) resident_world_formats: HashMap<u64, i32>,
    pub(super) cropped_worlds: Vec<u64>,
    pub(super) aegp_memory_handles: HashMap<u64, AegpMemoryHandle>,
    pub(super) aegp_memory_free: Vec<AegpMemoryBlock>,
    pub(super) next_aegp_memory_handle: u64,
    pub(super) math_calls: Vec<String>,
    pub(super) handle_allocations: Vec<u64>,
    pub(super) handle_allocation_failures: Vec<String>,
    pub(super) census_blocks: HashMap<(u64, u32), u64>,
    pub(super) trace: Option<TraceCapture>,
    pub(super) trace_labels: HashMap<u64, TraceLabel>,
    pub(super) trace_watches: Vec<TraceWatchSpec>,
    pub(super) trace_checkpoint_only: bool,
    pub(super) pending_iterate: Option<PendingIterate>,
    pub(super) vcomp_dynamic_loop: Option<VcompDynamicLoop>,
    pub(super) vcomp_requested_threads: Option<u32>,
    pub(super) omp_dynamic_requested: Option<bool>,
    pub(super) imported_data: BTreeMap<&'static str, u64>,
    pub(super) msvcp_mutexes: HashMap<u64, MsvcpMutex>,
    pub(super) msvcp_lockit_locks: [Option<(u32, u32)>; 8],
    pub(super) msvcp_lockit_objects: HashMap<u64, (i32, u32)>,
    pub(super) pending_crt_initterm: Option<PendingCrtInitterm>,
    pub(super) crt_onexit_tables: HashMap<u64, Vec<u64>>,
    pub(super) crt_terminate_handler: u64,
    pub(super) windows_critical_sections: HashMap<u64, u32>,
    pub(super) windows_srw_locks: BTreeMap<u64, WindowsSrwLock>,
    pub(super) windows_condition_variables: HashSet<u64>,
    pub(super) windows_condition_waiters: BTreeMap<u64, VecDeque<u32>>,
    pub(super) scheduler_condition_locks: BTreeMap<u32, u64>,
    pub(super) windows_address_waiters: BTreeMap<u64, BTreeSet<u32>>,
    pub(super) windows_fls_slots: BTreeMap<u32, WindowsFlsSlot>,
    pub(super) windows_tls_slots: BTreeMap<u32, u64>,
    pub(super) pending_fls_free: Option<PendingFlsFree>,
    pub(super) windows_threads: BTreeMap<u64, WindowsThread>,
    pub(super) windows_hooks: BTreeMap<u64, (i32, u64, u64, u32)>,
    pub(super) next_windows_hook: u64,
    pub(super) windows_timers: BTreeMap<(u64, u64), (u32, u64)>,
    pub(super) windows_init_once: BTreeMap<u64, WindowsInitOnceState>,
    pub(super) next_windows_timer: u64,
    pub(super) windows_message_boxes: Vec<(String, String, u32)>,
    pub(super) windows_objects: WindowsKernelObjects,
    pub(super) windows_sids: BTreeSet<u64>,
    pub(super) windows_sid_issued: u64,
    pub(super) windows_acl_allocations: BTreeMap<u64, u64>,
    pub(super) windows_acl_issued: u64,
    pub(super) next_windows_thread_id: u32,
    pub(super) current_windows_thread_id: u32,
    pub(super) com_apartments: BTreeMap<u32, (u32, u32)>,
    // Process defaults (service count, authentication level, impersonation level).
    // No COM transport may operate without implementing these security settings.
    pub(super) com_security: Option<(i32, u32, u32)>,
    pub(super) pending_windows_thread: Option<PendingWindowsThread>,
    pub(super) windows_last_error: u32,
    pub(super) crt_errno: u32,
    pub(super) last_crt_heap_failure: Option<(u64, String)>,
    pub(super) crt_errno_buffers: BTreeMap<u32, u64>,
    pub(super) crt_random_states: BTreeMap<u32, u32>,
    pub(super) registry: crate::guest_registry::GuestRegistry,
    pub(super) guest_files: GuestFiles,
    pub(super) performance_counter_origin: Option<std::time::Instant>,
    pub(super) windows_module_refcounts: HashMap<u64, u32>,
    pub(super) windows_pinned_modules: HashSet<u64>,
    pub(super) windows_thread_error_mode: u32,
    pub(super) windows_socket_startups: u32,
    pub(super) windows_private_heaps: BTreeMap<u64, BTreeSet<u64>>,
    pub(super) scheduler_yield_reason: Option<SchedulerYieldReason>,
    pub(super) scheduler_resume_rip: u64,
    pub(super) scheduler_ready_hint: bool,
    pub(super) scheduler_woken_threads: VecDeque<u32>,
    pub(super) scheduler_virtual_tick: u64,
    pub(super) scheduler_switches_remaining: u64,
    pub(super) scheduler_wait_deadline: Option<u64>,
    pub(super) scheduler_main_wait: Option<SchedulerMainWait>,
    pub(super) scheduler_child_completed: bool,
    pub(super) scheduler_resume_active: bool,
    pub(super) scheduler_parent_context: Option<Context>,
    pub(super) windows_command_line_a: u64,
    pub(super) windows_command_line_w: u64,
    pub(super) environment_strings_base: u64,
    pub(super) environment_overrides: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    pub(super) getenv_buffer: Option<u64>,
    pub(super) wgetenv_buffers: BTreeMap<Vec<u8>, u64>,
    pub(super) wenviron_cell: Option<u64>,
    pub(super) process_prng_state: u64,
    pub(super) plugin_data_registry: EffectRegistry,
    pub(super) plugin_data_error: Option<String>,
    pub(super) crt_heap: CrtHeap,
    pub(super) crt_heap_mapped: bool,
    pub(super) extended_strings: HashMap<i32, u64>,
    pub(super) extended_empty_string: u64,
    pub(super) extended_string_table_valid: bool,
    pub(super) avx_fallback_instructions: u64,
    pub(super) avx_defined_ymm: [bool; 16],
    pub(super) avx_state_sync_points: HashMap<u64, AvxStateSync>,
    pub(super) gpu_runtime: GpuRuntime,
    pub(super) gpu_suite: GpuSuiteState,
}

impl GuestState {
    pub(super) fn seal_image_reads(&mut self) {
        let mut ranges = Vec::with_capacity(self.loaded_libraries.len() + 1);
        ranges.extend(self.image_region);
        ranges.extend(
            self.loaded_libraries
                .values()
                .map(|library| (library.base, library.end)),
        );
        ranges.sort_unstable_by_key(|range| range.0);

        let mut merged: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            if let Some(last) = merged.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        self.sealed_image_read_ranges = merged;
        self.sealed_image_reads = true;
    }

    pub(super) fn sealed_image_contains(&self, address: u64, inclusive_end: u64) -> bool {
        if !self.sealed_image_reads {
            return false;
        }
        let index = self
            .sealed_image_read_ranges
            .partition_point(|range| range.0 <= address);
        index > 0 && inclusive_end < self.sealed_image_read_ranges[index - 1].1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CustomUiRegistration {
    pub events: u32,
    pub comp_width: i32,
    pub comp_height: i32,
    pub comp_alignment: i32,
    pub layer_width: i32,
    pub layer_height: i32,
    pub layer_alignment: i32,
    pub preview_width: i32,
    pub preview_height: i32,
    pub preview_alignment: i32,
}

#[derive(Clone, Debug)]
pub(super) struct PendingUnsupportedSuite {
    pub(super) name: String,
    pub(super) version: u64,
    pub(super) acquire_error: i32,
    pub(super) caller_rsp: u64,
    pub(super) return_address: u64,
}

#[derive(Clone, Debug)]
pub(super) struct SelectorAbortRecord {
    pub(super) error: i32,
    pub(super) suite: PendingUnsupportedSuite,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SmartCheckout {
    pub(super) index: i32,
    pub(super) world: u64,
    pub(super) checked_out: bool,
}

#[derive(Clone, Debug)]
pub(super) struct VcompDynamicLoop {
    pub(super) current: i32,
    pub(super) upper: i32,
    pub(super) chunk: i32,
    pub(super) exhausted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MsvcpMutex {
    pub(super) mutex_type: u32,
    pub(super) owner_thread_id: Option<u32>,
    pub(super) lock_count: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct WindowsSrwLock {
    pub(super) owner: Option<u32>,
    pub(super) waiters: VecDeque<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct VcruntimeExceptionData {
    pub(super) what: u64,
    pub(super) do_free: bool,
}

#[derive(Clone, Debug)]
pub(super) struct PendingCrtInitterm {
    pub(super) functions: Vec<u64>,
    pub(super) next: usize,
    pub(super) return_address: u64,
    pub(super) continuation_rsp: u64,
    pub(super) stop_on_error: bool,
}

#[derive(Clone, Debug)]
pub(super) struct WindowsThread {
    pub(super) id: u32,
    pub(super) start: u64,
    pub(super) parameter: u64,
    pub(super) suspended: bool,
    pub(super) completed: bool,
    pub(super) exit_code: u32,
    pub(super) handle_open: bool,
    pub(super) stack_base: u64,
    pub(super) stack_size: u64,
    pub(super) stack_mapped: bool,
}

#[derive(Clone, Debug)]
pub(super) struct PendingWindowsThread {
    pub(super) handle: u64,
    pub(super) return_address: u64,
    pub(super) continuation_rsp: u64,
    pub(super) callback_return_rsp: u64,
    pub(super) caller_tls_values: BTreeMap<u32, u64>,
    pub(super) caller_fls_values: BTreeMap<u32, u64>,
    pub(super) caller_last_error: u32,
    pub(super) caller_crt_errno: u32,
    pub(super) caller_thread_error_mode: u32,
    pub(super) caller_thread_id: u32,
    pub(super) completion_return: u64,
    pub(super) caller_teb_stack: [u8; 16],
    pub(super) exit_code: Option<u32>,
    pub(super) fls_pass: u32,
    pub(super) fls_processed: BTreeSet<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WindowsFlsSlot {
    pub(super) callback: u64,
    pub(super) value: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PendingFlsFree {
    pub(super) index: u32,
    pub(super) return_address: u64,
    pub(super) continuation_rsp: u64,
}

#[derive(Clone, Debug)]
pub(super) struct PendingIterate {
    pub(super) caller_rsp: u64,
    pub(super) return_address: u64,
    pub(super) refcon: u64,
    pub(super) pixel_function: u64,
    pub(super) source_data: u64,
    pub(super) source_rowbytes: u64,
    pub(super) source_width: i32,
    pub(super) source_height: i32,
    pub(super) zero_outside_source: bool,
    pub(super) destination_data: u64,
    pub(super) destination_rowbytes: u64,
    pub(super) left: i32,
    pub(super) right: i32,
    pub(super) bottom: i32,
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) origin_x: i32,
    pub(super) origin_y: i32,
    pub(super) pixel_bytes: u64,
    pub(super) continuation: u64,
    pub(super) callback_name: &'static str,
    pub(super) callback_phase: IterateCallbackPhase,
    pub(super) scheduled_pixels: i32,
    pub(super) abort_function: u64,
    pub(super) progress_function: u64,
    pub(super) effect_ref: u64,
    pub(super) progress_base: i32,
    pub(super) progress_final: i32,
    pub(super) top: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum IterateCallbackPhase {
    Pixel,
    Progress,
    Abort,
}

#[derive(Clone, Debug)]
pub(super) struct GuestHandle {
    pub(super) data: u64,
    pub(super) size: u64,
    pub(super) locks: u32,
    pub(super) pending_dispose: bool,
    pub(super) handle_region: u64,
    pub(super) data_region: u64,
    pub(super) data_mapped_size: u64,
}

#[derive(Clone, Debug)]
pub(super) struct GuestWorld {
    pub(super) pixel_format: i32,
    pub(super) size: u64,
    pub(super) data_region: u64,
    pub(super) data_mapped_size: u64,
}

#[derive(Clone, Debug)]
pub(super) struct AegpMemoryHandle {
    pub(super) data: u64,
    pub(super) size: u64,
    pub(super) locks: u32,
    pub(super) end: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct AegpMemoryBlock {
    pub(super) data: u64,
    pub(super) end: u64,
}

pub struct GuestEngine<'a> {
    pub(super) unicorn: Unicorn<'a, GuestState>,
    pub(super) scheduled_windows_threads: BTreeMap<u32, ParkedWindowsThread>,
    pub(super) scheduler_ready: VecDeque<u32>,
    pub(super) scheduler_deferred_ready: VecDeque<u32>,
    pub(super) parked_main_context: Option<Context>,
    pub(super) next_data: u64,
    pub(super) next_import_stub: u64,
    pub(super) image_base: u64,
    pub(super) image_end: u64,
    pub(super) census_hook: Option<UcHookId>,
    pub(super) trace_hooks: Vec<UcHookId>,
    pub(super) trace_points: Vec<u64>,
    pub(super) image_sha256: String,
    pub(super) entry_export: String,
    pub(super) trace_modules: Vec<TraceModule>,
    pub(super) primary_attached: bool,
    pub(super) primary_poisoned: bool,
}

pub(super) struct ParkedWindowsThread {
    pub(super) crt_errno: u32,
    pub(super) context: Context,
    pub(super) pending: PendingWindowsThread,
    pub(super) tls_values: BTreeMap<u32, u64>,
    pub(super) fls_values: BTreeMap<u32, u64>,
    pub(super) last_error: u32,
    pub(super) thread_error_mode: u32,
    pub(super) teb_stack: [u8; 16],
    pub(super) wait_deadline: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SchedulerYieldReason {
    Voluntary,
    AddressWait,
    SrwLock,
    Event,
    ConditionVariable,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SchedulerMainWait {
    pub(super) address: u64,
    pub(super) deadline: Option<u64>,
    pub(super) woken: bool,
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
                .filter(|(pointer, _)| !(CRT_HEAP_BASE..CRT_HEAP_END).contains(pointer))
                .map(|(pointer, allocation)| (pointer, allocation.backing_size)),
        );
        if self.unicorn.get_data().crt_heap_mapped {
            mappings.push((CRT_HEAP_BASE, MAX_CRT_HEAP_BYTES));
        }
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
pub(super) enum TraceLabelKind {
    Import,
    HostCallback,
}

#[derive(Clone, Debug)]
pub(super) struct TraceLabel {
    pub(super) kind: TraceLabelKind,
    pub(super) name: String,
}

#[derive(Debug)]
pub(super) struct TraceCapture {
    pub(super) selector: String,
    pub(super) entry_rva: u64,
    pub(super) events: Vec<TraceEvent>,
    pub(super) return_stack: Vec<u64>,
    pub(super) function_stack: Vec<Option<u64>>,
    pub(super) call_rsp_stack: Vec<u64>,
    pub(super) call_id_stack: Vec<u64>,
    pub(super) next_call_id: u64,
    pub(super) watch_specs: Vec<TraceWatchSpec>,
    pub(super) watch_occurrence_counts: HashMap<String, u64>,
    pub(super) watch_stack: Vec<Vec<PendingTraceWatch>>,
    pub(super) selector_watches: Vec<PendingTraceWatch>,
    pub(super) checkpoint_returns: HashMap<u64, Vec<PendingTraceWatch>>,
    pub(super) unhookable_watches: Vec<UnhookableWatch>,
    pub(super) witnesses: Vec<TraceMemoryWitness>,
    pub(super) dropped_witnesses: u64,
    pub(super) reserved_watch_bytes: usize,
    pub(super) reserved_watch_count: usize,
    pub(super) basic_blocks: HashMap<(u64, u32), u64>,
    pub(super) branch_edges: HashMap<(u64, u64), u64>,
    pub(super) dropped_basic_blocks: u64,
    pub(super) dropped_branch_edges: u64,
    pub(super) previous_block: Option<u64>,
    pub(super) event_index: HashMap<TraceEventKey, usize>,
    pub(super) event_fingerprints: HashMap<usize, HashSet<u64>>,
    pub(super) known_function_entries: HashSet<u64>,
    pub(super) truncated: bool,
    pub(super) dropped_events: u64,
    pub(super) checkpoint_only: bool,
}

#[derive(Clone, Debug)]
pub(super) struct PendingTraceWatch {
    pub(super) spec_id: String,
    pub(super) register: &'static str,
    pub(super) call_id: Option<u64>,
    pub(super) function_rva: Option<u64>,
    pub(super) pc_rva: Option<u64>,
    pub(super) address: u64,
    pub(super) before: TraceMemorySnapshot,
    pub(super) image_coordinate: Option<[u32; 2]>,
    pub(super) image_row_offset: Option<u64>,
    pub(super) image_format: Option<&'static str>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dereference_offset: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dereference_offsets: Vec<u64>,
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

/// A watch the active capture mode cannot arm. Checkpoint capture hooks the
/// selector entry and the direct call sites its watches name, so a watch
/// that depends on a tail-call jump or an indirect call produces no witness;
/// listing it here keeps that absence explicit instead of silent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UnhookableWatch {
    pub id: String,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceConfiguration {
    pub capture_mode: &'static str,
    /// Watches this capture could not hook (always empty for `full_trace`).
    pub unhookable_watches: Vec<UnhookableWatch>,
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
    pub changed_ranges_truncated: bool,
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
    /// Readable 64-bit words starting at RSP, for recovering untracked native calls.
    pub stack_words: Vec<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_target: Option<TraceRuntimeTarget>,
    pub handle_allocations: Vec<u64>,
    pub handle_allocation_failures: Vec<String>,
    pub live_handle_count: usize,
    pub crt_heap_live_bytes: u64,
    pub crt_heap_allocation_count: usize,
    /// Highest-address live allocations as (pointer, requested bytes, backing bytes).
    pub crt_heap_tail_allocations: Vec<(u64, u64, u64)>,
    pub next_pf_handle_data: u64,
    pub pf_handle_data_end: u64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct TraceEventKey {
    pub(super) depth: usize,
    pub(super) kind: &'static str,
    pub(super) function_rva: Option<u64>,
    pub(super) pc_rva: Option<u64>,
    pub(super) target_rva: Option<u64>,
    pub(super) name: Option<String>,
    pub(super) call_kind: Option<&'static str>,
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
