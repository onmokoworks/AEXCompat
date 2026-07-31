use aex_apple_opencl::{
    Buffer, BufferAccess, Error as AppleOpenClError, Kernel, ObjectCounts, ObjectTracker, Program,
    Session, MAX_BUFFER_BYTES,
};
use aex_wgpu_compute::MAX_BUFFER_BYTES as MAX_WGPU_BUFFER_BYTES;
use crate::gpu_lifecycle::{
    GpuRuntimeBackendKind, OpenClBridgeEvidence, OpenClErrorEvidence, WgpuRuntimeEvidence,
};
use std::cell::RefCell;
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicU32, Ordering},
};

const GPU_TOKEN_BASE: u64 = 0x0000_0040_0000_0000;
const GPU_TOKEN_GENERATION_BYTES: u64 = 0x0100_0000;
const GPU_TOKEN_GENERATION_MASK: u32 = 0x000f_ffff;
const GPU_TOKEN_GLOBAL_END: u64 =
    GPU_TOKEN_BASE + ((GPU_TOKEN_GENERATION_MASK as u64 + 1) * GPU_TOKEN_GENERATION_BYTES);
const GPU_TOKEN_KIND_STRIDE: u64 = 0x10;
const GPU_TOKEN_OBJECT_STRIDE: u64 = 0x100;
const MAX_GPU_DEVICE_ALLOCATIONS: usize = 256;
const MAX_GPU_DEVICE_BYTES: usize = 256 * 1024 * 1024;
const MAX_OPENCL_PROGRAMS: usize = 256;
const MAX_OPENCL_KERNELS: usize = 1_024;
const MAX_OPENCL_LIVE_SOURCE_BYTES: usize = 64 * 1024 * 1024;
const MAX_OPENCL_EVIDENCE_ERROR_BYTES: usize = 4_096;
pub(crate) const CL_SUCCESS: i32 = 0;
pub(crate) const CL_BUILD_PROGRAM_FAILURE: i32 = -11;
pub(crate) const CL_OUT_OF_RESOURCES: i32 = -5;
pub(crate) const CL_OUT_OF_HOST_MEMORY: i32 = -6;
pub(crate) const CL_INVALID_VALUE: i32 = -30;
pub(crate) const CL_INVALID_DEVICE: i32 = -33;
pub(crate) const CL_INVALID_CONTEXT: i32 = -34;
pub(crate) const CL_INVALID_COMMAND_QUEUE: i32 = -36;
pub(crate) const CL_INVALID_MEM_OBJECT: i32 = -38;
pub(crate) const CL_INVALID_PROGRAM: i32 = -44;
pub(crate) const CL_INVALID_PROGRAM_EXECUTABLE: i32 = -45;
pub(crate) const CL_INVALID_KERNEL: i32 = -48;
pub(crate) const CL_INVALID_ARG_INDEX: i32 = -49;
pub(crate) const CL_INVALID_ARG_VALUE: i32 = -50;
pub(crate) const CL_INVALID_ARG_SIZE: i32 = -51;
pub(crate) const CL_INVALID_KERNEL_ARGS: i32 = -52;
pub(crate) const CL_INVALID_WORK_DIMENSION: i32 = -53;
pub(crate) const CL_INVALID_WORK_GROUP_SIZE: i32 = -54;
pub(crate) const CL_INVALID_GLOBAL_OFFSET: i32 = -56;
pub(crate) const CL_INVALID_EVENT_WAIT_LIST: i32 = -57;
pub(crate) const CL_INVALID_GLOBAL_WORK_SIZE: i32 = -63;
static NEXT_GPU_TOKEN_GENERATION: AtomicU32 = AtomicU32::new(1);
static ISSUED_GPU_TOKENS: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum GpuTokenKind {
    Platform = 1,
    Device = 2,
    Context = 3,
    Queue = 4,
    Buffer = 5,
    Program = 6,
    Kernel = 7,
}

impl GpuTokenKind {
    fn decode(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::Platform),
            2 => Some(Self::Device),
            3 => Some(Self::Context),
            4 => Some(Self::Queue),
            5 => Some(Self::Buffer),
            6 => Some(Self::Program),
            7 => Some(Self::Kernel),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GpuDeviceTokens {
    pub(crate) platform: u64,
    pub(crate) device: u64,
    pub(crate) context: u64,
    pub(crate) queue: u64,
}

enum GpuBackend {
    OpenCl {
        session: Session,
        tracker: ObjectTracker,
    },
    WgpuMetal(WgpuExecutor),
    #[cfg(test)]
    Mock,
}

enum DeviceBufferBacking {
    OpenCl(Buffer),
    WgpuMetal(RefCell<Vec<u8>>),
    #[cfg(test)]
    Mock(RefCell<Vec<u8>>),
}

struct DeviceAllocation {
    device_index: u32,
    bytes: usize,
    backing: DeviceBufferBacking,
    // Freeing invalidates the guest token immediately. The bounded registry
    // retains this record only while a kernel still owns the native buffer.
    token_live: bool,
    kernel_bindings: usize,
}

enum ProgramBacking {
    OpenCl(Program),
    WgpuMetal(aex_clspv::ValidatedArtifact),
    #[cfg(test)]
    Mock,
}

struct ProgramRecord {
    source: String,
    backing: Option<ProgramBacking>,
}

enum KernelBacking {
    OpenCl(Kernel),
    WgpuMetal(WgpuKernel),
    #[cfg(test)]
    Mock(BTreeMap<u32, Vec<u8>>),
}

struct KernelRecord {
    backing: KernelBacking,
    bound_buffers: BTreeMap<u32, u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OpenClRuntimeError {
    pub(crate) status: i32,
    pub(crate) detail: String,
}

impl OpenClRuntimeError {
    fn new(status: i32, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
        }
    }

    fn from_apple(error: AppleOpenClError, fallback_status: i32) -> Self {
        let status = match &error {
            AppleOpenClError::Api { code, .. }
            | AppleOpenClError::ApiWithDetail { code, .. }
            | AppleOpenClError::ProgramBuild { code, .. } => *code,
            AppleOpenClError::InvalidWorkDimensions => CL_INVALID_WORK_DIMENSION,
            AppleOpenClError::GlobalOffsetDimensionMismatch => CL_INVALID_GLOBAL_OFFSET,
            AppleOpenClError::LocalWorkDimensionMismatch
            | AppleOpenClError::ZeroLocalWorkSize { .. }
            | AppleOpenClError::NonDivisibleLocalWorkSize { .. } => CL_INVALID_WORK_GROUP_SIZE,
            AppleOpenClError::ZeroGlobalWorkSize { .. } => CL_INVALID_GLOBAL_WORK_SIZE,
            _ => fallback_status,
        };
        Self::new(status, error.to_string())
    }
}

pub(crate) struct GpuRuntime {
    backend: Option<GpuBackend>,
    device_index: Option<u32>,
    device_tokens: Option<GpuDeviceTokens>,
    next_token: u64,
    token_end: u64,
    buffers: HashMap<u64, DeviceAllocation>,
    programs: HashMap<u64, ProgramRecord>,
    kernels: HashMap<u64, KernelRecord>,
    live_buffer_bytes: usize,
    live_program_source_bytes: usize,
    issued_tokens: HashSet<u64>,
    opencl_evidence: OpenClBridgeEvidence,
    wgpu_evidence: RefCell<Option<WgpuRuntimeEvidence>>,
    wgpu_sticky_error: RefCell<Option<String>>,
}

impl Default for GpuRuntime {
    fn default() -> Self {
        let generation = next_gpu_token_generation();
        let token_start =
            GPU_TOKEN_BASE + u64::from(generation) * GPU_TOKEN_GENERATION_BYTES;
        Self {
            backend: None,
            device_index: None,
            device_tokens: None,
            next_token: token_start,
            token_end: token_start + GPU_TOKEN_GENERATION_BYTES,
            buffers: HashMap::new(),
            programs: HashMap::new(),
            kernels: HashMap::new(),
            live_buffer_bytes: 0,
            live_program_source_bytes: 0,
            issued_tokens: HashSet::new(),
            opencl_evidence: OpenClBridgeEvidence::default(),
            wgpu_evidence: RefCell::new(None),
            wgpu_sticky_error: RefCell::new(None),
        }
    }
}

impl Drop for GpuRuntime {
    fn drop(&mut self) {
        if self.is_active() {
            let _ = self.end();
        }
        let mut issued = issued_gpu_tokens()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for token in &self.issued_tokens {
            issued.remove(token);
        }
    }
}

impl GpuRuntime {
    pub(crate) fn begin(
        &mut self,
        backend_kind: GpuRuntimeBackendKind,
        device_index: u32,
    ) -> Result<GpuDeviceTokens, String> {
        self.begin_with_wgpu_config_loader(
            backend_kind,
            device_index,
            WgpuCompilerConfig::from_process_env,
        )
    }

    fn begin_with_wgpu_config_loader(
        &mut self,
        backend_kind: GpuRuntimeBackendKind,
        device_index: u32,
        load_wgpu_config: impl FnOnce() -> Result<WgpuCompilerConfig, String>,
    ) -> Result<GpuDeviceTokens, String> {
        self.prepare_begin_attempt()?;
        let (backend, opencl_evidence, wgpu_evidence) = match backend_kind {
            GpuRuntimeBackendKind::AppleOpenCl => {
                let session = Session::select_gpu(device_index as usize)
                    .map_err(|error| error.to_string())?;
                let tracker = session.object_tracker();
                let device = session.device().clone();
                (
                    GpuBackend::OpenCl { session, tracker },
                    OpenClBridgeEvidence {
                        device_index: Some(device_index),
                        platform: Some(device.platform_name().to_string()),
                        device: Some(device.name().to_string()),
                        vendor: Some(device.vendor().to_string()),
                        compute_units: Some(device.compute_units()),
                        ..OpenClBridgeEvidence::default()
                    },
                    None,
                )
            }
            GpuRuntimeBackendKind::WgpuMetal => {
                let config = load_wgpu_config()?;
                let executor = WgpuExecutor::new(device_index, config)?;
                let evidence = executor.initial_evidence();
                (
                    GpuBackend::WgpuMetal(executor),
                    OpenClBridgeEvidence {
                        device_index: Some(device_index),
                        ..OpenClBridgeEvidence::default()
                    },
                    Some(evidence),
                )
            }
        };
        let tokens = self.allocate_device_tokens()?;
        self.opencl_evidence = opencl_evidence;
        *self.wgpu_evidence.borrow_mut() = wgpu_evidence;
        self.backend = Some(backend);
        self.device_index = Some(device_index);
        self.device_tokens = Some(tokens);
        Ok(tokens)
    }

    fn prepare_begin_attempt(&mut self) -> Result<(), String> {
        if self.backend.is_some() {
            return Err("GPU runtime is already active".into());
        }
        self.opencl_evidence = OpenClBridgeEvidence::default();
        *self.wgpu_evidence.borrow_mut() = None;
        *self.wgpu_sticky_error.borrow_mut() = None;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn begin_mock(&mut self, device_index: u32) -> Result<GpuDeviceTokens, String> {
        self.prepare_begin_attempt()?;
        let tokens = self.allocate_device_tokens()?;
        self.opencl_evidence = OpenClBridgeEvidence {
            device_index: Some(device_index),
            platform: Some("mock".into()),
            device: Some("mock".into()),
            vendor: Some("mock".into()),
            compute_units: Some(1),
            ..OpenClBridgeEvidence::default()
        };
        self.backend = Some(GpuBackend::Mock);
        self.device_index = Some(device_index);
        self.device_tokens = Some(tokens);
        Ok(tokens)
    }

    pub(crate) fn is_active(&self) -> bool {
        self.backend.is_some()
    }

    pub(crate) fn backend_kind(&self) -> Option<GpuRuntimeBackendKind> {
        match self.backend.as_ref() {
            Some(GpuBackend::OpenCl { .. }) => Some(GpuRuntimeBackendKind::AppleOpenCl),
            Some(GpuBackend::WgpuMetal(_)) => Some(GpuRuntimeBackendKind::WgpuMetal),
            #[cfg(test)]
            Some(GpuBackend::Mock) => Some(GpuRuntimeBackendKind::AppleOpenCl),
            None => None,
        }
    }

    pub(crate) fn device_index(&self) -> Option<u32> {
        self.device_index
    }

    pub(crate) fn device_tokens(&self) -> Option<GpuDeviceTokens> {
        self.device_tokens
    }

    pub(crate) fn validates_platform(&self, token: u64) -> bool {
        self.device_tokens.is_some_and(|tokens| tokens.platform == token)
    }

    pub(crate) fn validates_device(&self, token: u64) -> bool {
        self.device_tokens.is_some_and(|tokens| tokens.device == token)
    }

    pub(crate) fn validates_context(&self, token: u64) -> bool {
        self.device_tokens.is_some_and(|tokens| tokens.context == token)
    }

    pub(crate) fn validates_queue(&self, token: u64) -> bool {
        self.device_tokens.is_some_and(|tokens| tokens.queue == token)
    }

    pub(crate) fn allocate_device(
        &mut self,
        device_index: u32,
        bytes: usize,
        access: BufferAccess,
    ) -> Result<u64, String> {
        if self.device_index != Some(device_index) {
            return Err(format!(
                "GPU device index {device_index} does not match active device {:?}",
                self.device_index
            ));
        }
        if bytes == 0 {
            return Err("GPU device allocation must contain at least one byte".into());
        }
        let backend_buffer_limit = match self.backend.as_ref() {
            Some(GpuBackend::WgpuMetal(_)) => MAX_WGPU_BUFFER_BYTES,
            _ => MAX_BUFFER_BYTES,
        };
        if bytes > backend_buffer_limit || bytes > MAX_GPU_DEVICE_BYTES {
            return Err(format!(
                "GPU device allocation {bytes} exceeds {} bytes",
                MAX_GPU_DEVICE_BYTES.min(backend_buffer_limit)
            ));
        }
        if self.buffers.len() >= MAX_GPU_DEVICE_ALLOCATIONS {
            return Err(format!(
                "GPU device allocation count exceeds {MAX_GPU_DEVICE_ALLOCATIONS}"
            ));
        }
        if self
            .live_buffer_bytes
            .checked_add(bytes)
            .is_none_or(|total| total > MAX_GPU_DEVICE_BYTES)
        {
            return Err(format!(
                "GPU device live bytes exceed {MAX_GPU_DEVICE_BYTES}"
            ));
        }

        let backing = match self.backend.as_ref() {
            Some(GpuBackend::OpenCl { session, .. }) => DeviceBufferBacking::OpenCl(
                session
                    .create_buffer(bytes, access)
                    .map_err(|error| error.to_string())?,
            ),
            Some(GpuBackend::WgpuMetal(_)) => {
                DeviceBufferBacking::WgpuMetal(RefCell::new(vec![0; bytes]))
            }
            #[cfg(test)]
            Some(GpuBackend::Mock) => {
                DeviceBufferBacking::Mock(RefCell::new(vec![0; bytes]))
            }
            None => return Err("GPU runtime is not active".into()),
        };
        let token = self.allocate_token(GpuTokenKind::Buffer)?;
        self.buffers.insert(
            token,
            DeviceAllocation {
                device_index,
                bytes,
                backing,
                token_live: true,
                kernel_bindings: 0,
            },
        );
        self.live_buffer_bytes += bytes;
        Ok(token)
    }

    pub(crate) fn free_device(&mut self, device_index: u32, token: u64) -> Result<(), String> {
        let allocation = self
            .buffers
            .get_mut(&token)
            .ok_or_else(|| format!("GPU device token {token:#x} is stale or forged"))?;
        if !allocation.token_live {
            return Err(format!(
                "GPU device token {token:#x} is stale or forged"
            ));
        }
        if allocation.device_index != device_index || self.device_index != Some(device_index) {
            return Err(format!(
                "GPU device token {token:#x} belongs to device {}, not {device_index}",
                allocation.device_index
            ));
        }
        allocation.token_live = false;
        let is_unbound = allocation.kernel_bindings == 0;
        if is_unbound {
            self.drop_released_buffer(token);
        }
        Ok(())
    }

    pub(crate) fn write_device(
        &self,
        token: u64,
        offset: usize,
        source: &[u8],
    ) -> Result<(), String> {
        let allocation = self.device_allocation(token)?;
        match (&self.backend, &allocation.backing) {
            (
                Some(GpuBackend::OpenCl { session, .. }),
                DeviceBufferBacking::OpenCl(buffer),
            ) => session
                .write_buffer(buffer, offset, source)
                .map_err(|error| error.to_string()),
            (
                Some(GpuBackend::WgpuMetal(_)),
                DeviceBufferBacking::WgpuMetal(bytes),
            ) => {
                let mut bytes = bytes.borrow_mut();
                let range = checked_buffer_range(
                    "wgpu Metal buffer write",
                    offset,
                    source.len(),
                    bytes.len(),
                )?;
                bytes[range].copy_from_slice(source);
                Ok(())
            }
            #[cfg(test)]
            (Some(GpuBackend::Mock), DeviceBufferBacking::Mock(bytes)) => {
                let mut bytes = bytes.borrow_mut();
                let range = checked_buffer_range(
                    "mock GPU buffer write",
                    offset,
                    source.len(),
                    bytes.len(),
                )?;
                bytes[range].copy_from_slice(source);
                Ok(())
            }
            _ => Err("GPU device token belongs to another runtime backend".into()),
        }
    }

    pub(crate) fn read_device(
        &self,
        token: u64,
        offset: usize,
        destination: &mut [u8],
    ) -> Result<(), String> {
        let allocation = self.device_allocation(token)?;
        match (&self.backend, &allocation.backing) {
            (
                Some(GpuBackend::OpenCl { session, .. }),
                DeviceBufferBacking::OpenCl(buffer),
            ) => session
                .read_buffer(buffer, offset, destination)
                .map_err(|error| error.to_string()),
            (
                Some(GpuBackend::WgpuMetal(_)),
                DeviceBufferBacking::WgpuMetal(bytes),
            ) => {
                let bytes = bytes.borrow();
                let range = checked_buffer_range(
                    "wgpu Metal buffer read",
                    offset,
                    destination.len(),
                    bytes.len(),
                )?;
                destination.copy_from_slice(&bytes[range]);
                Ok(())
            }
            #[cfg(test)]
            (Some(GpuBackend::Mock), DeviceBufferBacking::Mock(bytes)) => {
                let bytes = bytes.borrow();
                let range = checked_buffer_range(
                    "mock GPU buffer read",
                    offset,
                    destination.len(),
                    bytes.len(),
                )?;
                destination.copy_from_slice(&bytes[range]);
                Ok(())
            }
            _ => Err("GPU device token belongs to another runtime backend".into()),
        }
    }

    pub(crate) fn buffer_len(&self, token: u64) -> Option<usize> {
        self.buffers
            .get(&token)
            .filter(|allocation| allocation.token_live)
            .map(|allocation| allocation.bytes)
    }

    pub(crate) fn is_buffer_token(&self, token: u64) -> bool {
        self.validates_token(token, GpuTokenKind::Buffer)
            && self
                .buffers
                .get(&token)
                .is_some_and(|allocation| allocation.token_live)
    }

    pub(crate) fn looks_like_token(token: u64) -> bool {
        (GPU_TOKEN_BASE..GPU_TOKEN_GLOBAL_END).contains(&token)
            && (token - GPU_TOKEN_BASE).is_multiple_of(GPU_TOKEN_KIND_STRIDE)
            && Self::token_kind(token).is_some()
    }

    pub(crate) fn opencl_session(&self) -> Result<&Session, String> {
        match self.backend.as_ref() {
            Some(GpuBackend::OpenCl { session, .. }) => Ok(session),
            Some(GpuBackend::WgpuMetal(_)) => {
                Err("wgpu Metal runtime does not expose an Apple OpenCL session".into())
            }
            #[cfg(test)]
            Some(GpuBackend::Mock) => Err("mock GPU runtime has no OpenCL session".into()),
            None => Err("GPU runtime is not active".into()),
        }
    }

    pub(crate) fn opencl_buffer(&self, token: u64) -> Result<&Buffer, String> {
        if matches!(self.backend.as_ref(), Some(GpuBackend::WgpuMetal(_))) {
            return Err("wgpu Metal buffers are host-mirrored and have no Apple OpenCL object".into());
        }
        let allocation = self.device_allocation(token)?;
        match &allocation.backing {
            DeviceBufferBacking::OpenCl(buffer) => Ok(buffer),
            DeviceBufferBacking::WgpuMetal(_) => {
                Err("wgpu Metal buffer has no Apple OpenCL object".into())
            }
            #[cfg(test)]
            DeviceBufferBacking::Mock(_) => Err("mock GPU token has no OpenCL buffer".into()),
        }
    }

    pub(crate) fn stage_opencl_program(
        &mut self,
        context: u64,
        source: String,
        source_strings: usize,
    ) -> Result<u64, OpenClRuntimeError> {
        if !self.validates_context(context) {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_CONTEXT,
                format!("OpenCL context token {context:#x} is stale, forged, or cross-engine"),
            ));
        }
        if source.is_empty() {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_VALUE,
                "OpenCL program source is empty",
            ));
        }
        if self.programs.len() >= MAX_OPENCL_PROGRAMS {
            return Err(OpenClRuntimeError::new(
                CL_OUT_OF_HOST_MEMORY,
                format!("OpenCL program count exceeds {MAX_OPENCL_PROGRAMS}"),
            ));
        }
        let next_source_bytes = self
            .live_program_source_bytes
            .checked_add(source.len())
            .filter(|bytes| *bytes <= MAX_OPENCL_LIVE_SOURCE_BYTES)
            .ok_or_else(|| {
                OpenClRuntimeError::new(
                    CL_OUT_OF_HOST_MEMORY,
                    format!(
                        "OpenCL retained program sources exceed {MAX_OPENCL_LIVE_SOURCE_BYTES} bytes"
                    ),
                )
            })?;
        let token = self
            .allocate_token(GpuTokenKind::Program)
            .map_err(|detail| OpenClRuntimeError::new(CL_OUT_OF_HOST_MEMORY, detail))?;
        let source_bytes = source.len();
        self.programs.insert(
            token,
            ProgramRecord {
                source,
                backing: None,
            },
        );
        self.live_program_source_bytes = next_source_bytes;
        self.opencl_evidence.source_strings = self
            .opencl_evidence
            .source_strings
            .saturating_add(source_strings as u64);
        self.opencl_evidence.source_bytes = self
            .opencl_evidence
            .source_bytes
            .saturating_add(source_bytes as u64);
        Ok(token)
    }

    pub(crate) fn build_opencl_program(
        &mut self,
        program: u64,
        devices: &[u64],
        options: Option<&str>,
    ) -> Result<(), OpenClRuntimeError> {
        if !self.validates_token(program, GpuTokenKind::Program)
            || !self.programs.contains_key(&program)
        {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_PROGRAM,
                format!("OpenCL program token {program:#x} is stale, forged, or cross-engine"),
            ));
        }
        if devices.iter().any(|token| !self.validates_device(*token)) {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_DEVICE,
                "OpenCL build device list contains a stale, forged, or cross-engine token",
            ));
        }
        let source = self
            .programs
            .get(&program)
            .expect("validated OpenCL program remains present")
            .source
            .clone();
        let backing = match self.backend.as_ref() {
            Some(GpuBackend::OpenCl { session, .. }) => ProgramBacking::OpenCl(
                session
                    .build_program(&source, options)
                    .map_err(|error| {
                        OpenClRuntimeError::from_apple(error, CL_INVALID_PROGRAM)
                    })?,
            ),
            Some(GpuBackend::WgpuMetal(executor)) => {
                if let Some(evidence) = self.wgpu_evidence.borrow_mut().as_mut() {
                    evidence.backend_operations_attempted =
                        evidence.backend_operations_attempted.saturating_add(1);
                }
                let build = match executor.build_program(&source, options) {
                    Ok(build) => build,
                    Err(detail) => {
                        let detail = format!("wgpu OpenCL program build failed: {detail}");
                        self.record_wgpu_sticky_error(&detail);
                        return Err(OpenClRuntimeError::new(CL_BUILD_PROGRAM_FAILURE, detail));
                    }
                };
                if let Some(evidence) = self.wgpu_evidence.borrow_mut().as_mut() {
                    add_artifact_evidence(evidence, build.evidence);
                }
                ProgramBacking::WgpuMetal(build.artifact)
            }
            #[cfg(test)]
            Some(GpuBackend::Mock) => ProgramBacking::Mock,
            None => {
                return Err(OpenClRuntimeError::new(
                    CL_INVALID_CONTEXT,
                    "OpenCL runtime is not active",
                ));
            }
        };
        self.programs
            .get_mut(&program)
            .expect("validated OpenCL program remains present")
            .backing = Some(backing);
        self.opencl_evidence.programs_built =
            self.opencl_evidence.programs_built.saturating_add(1);
        Ok(())
    }

    pub(crate) fn create_opencl_kernel(
        &mut self,
        program: u64,
        name: &str,
    ) -> Result<u64, OpenClRuntimeError> {
        if !self.validates_token(program, GpuTokenKind::Program) {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_PROGRAM,
                format!("OpenCL program token {program:#x} is stale, forged, or cross-engine"),
            ));
        }
        let record = self.programs.get(&program).ok_or_else(|| {
            OpenClRuntimeError::new(
                CL_INVALID_PROGRAM,
                format!("OpenCL program token {program:#x} is stale or released"),
            )
        })?;
        let backing = match record.backing.as_ref() {
            Some(ProgramBacking::OpenCl(program)) => KernelBacking::OpenCl(
                program
                    .create_kernel(name)
                    .map_err(|error| {
                        OpenClRuntimeError::from_apple(error, CL_INVALID_PROGRAM_EXECUTABLE)
                    })?,
            ),
            Some(ProgramBacking::WgpuMetal(artifact)) => {
                match WgpuKernel::new(artifact.clone(), name) {
                    Ok(kernel) => KernelBacking::WgpuMetal(kernel),
                    Err(error) => {
                        self.record_wgpu_sticky_error(&format!(
                            "wgpu kernel creation failed: {}",
                            error.detail
                        ));
                        return Err(error);
                    }
                }
            }
            #[cfg(test)]
            Some(ProgramBacking::Mock) => KernelBacking::Mock(BTreeMap::new()),
            None => {
                return Err(OpenClRuntimeError::new(
                    CL_INVALID_PROGRAM_EXECUTABLE,
                    "OpenCL program has not been built successfully",
                ));
            }
        };
        if self.kernels.len() >= MAX_OPENCL_KERNELS {
            return Err(OpenClRuntimeError::new(
                CL_OUT_OF_HOST_MEMORY,
                format!("OpenCL kernel count exceeds {MAX_OPENCL_KERNELS}"),
            ));
        }
        let token = self
            .allocate_token(GpuTokenKind::Kernel)
            .map_err(|detail| OpenClRuntimeError::new(CL_OUT_OF_HOST_MEMORY, detail))?;
        self.kernels.insert(
            token,
            KernelRecord {
                backing,
                bound_buffers: BTreeMap::new(),
            },
        );
        self.opencl_evidence.kernels_created =
            self.opencl_evidence.kernels_created.saturating_add(1);
        Ok(token)
    }

    pub(crate) fn set_opencl_kernel_raw_arg(
        &mut self,
        kernel: u64,
        index: u32,
        bytes: &[u8],
    ) -> Result<(), OpenClRuntimeError> {
        if !self.validates_token(kernel, GpuTokenKind::Kernel) {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_KERNEL,
                format!("OpenCL kernel token {kernel:#x} is stale, forged, or cross-engine"),
            ));
        }
        let previous_buffer = {
            let record = self.kernels.get_mut(&kernel).ok_or_else(|| {
                OpenClRuntimeError::new(
                    CL_INVALID_KERNEL,
                    format!("OpenCL kernel token {kernel:#x} is stale or released"),
                )
            })?;
            match &mut record.backing {
                KernelBacking::OpenCl(kernel) => kernel
                    .set_raw_arg(index, bytes)
                    .map_err(|error| OpenClRuntimeError::from_apple(error, CL_INVALID_ARG_SIZE))?,
                KernelBacking::WgpuMetal(kernel) => {
                    kernel.set_raw_argument(index, bytes)?;
                }
                #[cfg(test)]
                KernelBacking::Mock(arguments) => {
                    if bytes.is_empty() {
                        return Err(OpenClRuntimeError::new(
                            CL_INVALID_ARG_SIZE,
                            "OpenCL kernel argument is empty",
                        ));
                    }
                    arguments.insert(index, bytes.to_vec());
                }
            }
            record.bound_buffers.remove(&index)
        };
        if let Some(buffer) = previous_buffer {
            self.drop_kernel_buffer_binding(buffer);
        }
        self.opencl_evidence.scalar_arguments =
            self.opencl_evidence.scalar_arguments.saturating_add(1);
        self.opencl_evidence.scalar_argument_bytes = self
            .opencl_evidence
            .scalar_argument_bytes
            .saturating_add(bytes.len() as u64);
        Ok(())
    }

    pub(crate) fn set_opencl_kernel_arg(
        &mut self,
        kernel: u64,
        index: u32,
        bytes: &[u8],
    ) -> Result<(), OpenClRuntimeError> {
        let expectation = self
            .kernels
            .get(&kernel)
            .and_then(|record| match &record.backing {
                KernelBacking::WgpuMetal(kernel) => Some(kernel.argument_expectation(index)),
                _ => None,
            })
            .transpose()?;
        match expectation {
            Some(WgpuArgumentExpectation::StorageBuffer) => {
                if bytes.len() != size_of::<u64>() {
                    return Err(OpenClRuntimeError::new(
                        CL_INVALID_ARG_SIZE,
                        format!(
                            "wgpu storage argument {index} requires an eight-byte cl_mem token"
                        ),
                    ));
                }
                let token = u64::from_le_bytes(
                    bytes
                        .try_into()
                        .expect("validated storage token has eight bytes"),
                );
                self.set_opencl_kernel_buffer_arg(kernel, index, token)
            }
            Some(WgpuArgumentExpectation::PodUniform { .. }) => {
                self.set_opencl_kernel_raw_arg(kernel, index, bytes)
            }
            None => {
                if bytes.len() == size_of::<u64>() {
                    let token = u64::from_le_bytes(
                        bytes
                            .try_into()
                            .expect("eight-byte OpenCL argument has exact length"),
                    );
                    if self.is_buffer_token(token) {
                        return self.set_opencl_kernel_buffer_arg(kernel, index, token);
                    }
                    if Self::is_issued_token(token) {
                        return Err(OpenClRuntimeError::new(
                            CL_INVALID_MEM_OBJECT,
                            format!(
                                "OpenCL kernel argument contains stale, forged, cross-kind, or cross-engine token {token:#x}"
                            ),
                        ));
                    }
                }
                self.set_opencl_kernel_raw_arg(kernel, index, bytes)
            }
        }
    }

    pub(crate) fn set_opencl_kernel_buffer_arg(
        &mut self,
        kernel: u64,
        index: u32,
        buffer: u64,
    ) -> Result<(), OpenClRuntimeError> {
        if !self.validates_token(kernel, GpuTokenKind::Kernel) {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_KERNEL,
                format!("OpenCL kernel token {kernel:#x} is stale, forged, or cross-engine"),
            ));
        }
        if !self.validates_token(buffer, GpuTokenKind::Buffer) {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_MEM_OBJECT,
                format!("OpenCL buffer token {buffer:#x} is stale, forged, or cross-engine"),
            ));
        }
        let previous_buffer = {
            let allocation = self
                .buffers
                .get(&buffer)
                .filter(|allocation| allocation.token_live)
                .ok_or_else(|| {
                    OpenClRuntimeError::new(
                        CL_INVALID_MEM_OBJECT,
                        format!("OpenCL buffer token {buffer:#x} is stale or released"),
                    )
                })?;
            let record = self.kernels.get_mut(&kernel).ok_or_else(|| {
                OpenClRuntimeError::new(
                    CL_INVALID_KERNEL,
                    format!("OpenCL kernel token {kernel:#x} is stale or released"),
                )
            })?;
            match (&mut record.backing, &allocation.backing) {
                (KernelBacking::OpenCl(kernel), DeviceBufferBacking::OpenCl(buffer)) => kernel
                    .set_buffer_arg(index, buffer)
                    .map_err(|error| {
                        OpenClRuntimeError::from_apple(error, CL_INVALID_ARG_VALUE)
                    })?,
                (
                    KernelBacking::WgpuMetal(kernel),
                    DeviceBufferBacking::WgpuMetal(_),
                ) => {
                    kernel.set_buffer_argument(index, buffer)?;
                }
                (KernelBacking::OpenCl(_), DeviceBufferBacking::WgpuMetal(_))
                | (KernelBacking::WgpuMetal(_), DeviceBufferBacking::OpenCl(_)) => {
                    return Err(OpenClRuntimeError::new(
                        CL_INVALID_MEM_OBJECT,
                        "OpenCL kernel and buffer belong to different runtime backends",
                    ));
                }
                #[cfg(test)]
                (KernelBacking::Mock(arguments), DeviceBufferBacking::Mock(_)) => {
                    arguments.insert(index, buffer.to_le_bytes().to_vec());
                }
                #[cfg(test)]
                _ => {
                    return Err(OpenClRuntimeError::new(
                        CL_INVALID_MEM_OBJECT,
                        "OpenCL kernel and buffer belong to different runtime backends",
                    ));
                }
            }
            record.bound_buffers.insert(index, buffer)
        };
        if previous_buffer != Some(buffer) {
            self.add_kernel_buffer_binding(buffer);
            if let Some(previous) = previous_buffer {
                self.drop_kernel_buffer_binding(previous);
            }
        }
        self.opencl_evidence.buffer_arguments =
            self.opencl_evidence.buffer_arguments.saturating_add(1);
        Ok(())
    }

    pub(crate) fn enqueue_opencl_kernel(
        &mut self,
        queue: u64,
        kernel: u64,
        global_offset: Option<&[usize]>,
        global: &[usize],
        local: Option<&[usize]>,
    ) -> Result<(), OpenClRuntimeError> {
        if !self.validates_queue(queue) {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_COMMAND_QUEUE,
                format!("OpenCL queue token {queue:#x} is stale, forged, or cross-engine"),
            ));
        }
        if !self.validates_token(kernel, GpuTokenKind::Kernel) {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_KERNEL,
                format!("OpenCL kernel token {kernel:#x} is stale, forged, or cross-engine"),
            ));
        }
        let record = self.kernels.get(&kernel).ok_or_else(|| {
            OpenClRuntimeError::new(
                CL_INVALID_KERNEL,
                format!("OpenCL kernel token {kernel:#x} is stale or released"),
            )
        })?;
        match (&self.backend, &record.backing) {
            (
                Some(GpuBackend::OpenCl { session, .. }),
                KernelBacking::OpenCl(kernel),
            ) => session
                .enqueue_nd_range_with_offset(kernel, global_offset, global, local)
                .map_err(|error| {
                    OpenClRuntimeError::from_apple(error, CL_INVALID_WORK_DIMENSION)
                })?,
            #[cfg(test)]
            (Some(GpuBackend::Mock), KernelBacking::Mock(_)) => {}
            (
                Some(GpuBackend::WgpuMetal(executor)),
                KernelBacking::WgpuMetal(kernel),
            ) => {
                let plan = match kernel.prepare_dispatch(
                    global_offset,
                    global,
                    local,
                    |token| {
                        let allocation = self
                            .buffers
                            .get(&token)
                            .ok_or_else(|| format!("bound buffer token {token:#x} was lost"))?;
                        match &allocation.backing {
                            DeviceBufferBacking::WgpuMetal(bytes) => Ok(bytes.borrow().clone()),
                            _ => Err(format!(
                                "bound buffer token {token:#x} belongs to another backend"
                            )),
                        }
                    },
                ) {
                    Ok(plan) => plan,
                    Err(error) => {
                        self.record_wgpu_sticky_error(&format!(
                            "wgpu dispatch preparation failed: {}",
                            error.detail
                        ));
                        return Err(error);
                    }
                };
                if let Some(evidence) = self.wgpu_evidence.borrow_mut().as_mut() {
                    evidence.backend_operations_attempted =
                        evidence.backend_operations_attempted.saturating_add(1);
                }
                let report = match executor.dispatch(&plan) {
                    Ok(report) => report,
                    Err(error) => {
                        let detail = format!("wgpu kernel dispatch failed: {error}");
                        if let Some(evidence) = self.wgpu_evidence.borrow_mut().as_mut() {
                            replace_resource_counts(
                                &mut evidence.live_resources,
                                executor.live_objects(),
                            );
                        }
                        self.record_wgpu_sticky_error(&detail);
                        return Err(OpenClRuntimeError::new(CL_OUT_OF_RESOURCES, detail));
                    }
                };
                let mut seen_outputs = BTreeSet::new();
                let mut download_bytes = 0u64;
                for output in &report.outputs {
                    let Some(token) = plan.output_tokens.get(&output.binding) else {
                        let detail = format!(
                            "wgpu returned unexpected writable binding {}",
                            output.binding
                        );
                        self.record_wgpu_sticky_error(&detail);
                        return Err(OpenClRuntimeError::new(CL_OUT_OF_RESOURCES, detail));
                    };
                    if !seen_outputs.insert(output.binding) {
                        let detail =
                            format!("wgpu returned duplicate writable binding {}", output.binding);
                        self.record_wgpu_sticky_error(&detail);
                        return Err(OpenClRuntimeError::new(CL_OUT_OF_RESOURCES, detail));
                    }
                    let Some(allocation) = self.buffers.get(token) else {
                        let detail = format!("wgpu output buffer token {token:#x} was lost");
                        self.record_wgpu_sticky_error(&detail);
                        return Err(OpenClRuntimeError::new(CL_OUT_OF_RESOURCES, detail));
                    };
                    let DeviceBufferBacking::WgpuMetal(bytes) = &allocation.backing else {
                        let detail =
                            format!("wgpu output buffer token {token:#x} changed backend");
                        self.record_wgpu_sticky_error(&detail);
                        return Err(OpenClRuntimeError::new(CL_OUT_OF_RESOURCES, detail));
                    };
                    let mut bytes = bytes.borrow_mut();
                    if bytes.len() != output.bytes.len() {
                        let detail = format!(
                            "wgpu output binding {} changed size from {} to {}",
                            output.binding,
                            bytes.len(),
                            output.bytes.len()
                        );
                        self.record_wgpu_sticky_error(&detail);
                        return Err(OpenClRuntimeError::new(CL_OUT_OF_RESOURCES, detail));
                    }
                    bytes.copy_from_slice(&output.bytes);
                    download_bytes = download_bytes.saturating_add(output.bytes.len() as u64);
                }
                if seen_outputs.len() != plan.output_tokens.len() {
                    let detail = "wgpu omitted a reflected writable binding".to_string();
                    self.record_wgpu_sticky_error(&detail);
                    return Err(OpenClRuntimeError::new(CL_OUT_OF_RESOURCES, detail));
                }
                if let Some(evidence) = self.wgpu_evidence.borrow_mut().as_mut() {
                    add_resource_counts(&mut evidence.created_resources, report.created_resources);
                    replace_resource_counts(&mut evidence.live_resources, report.live_resources);
                    add_dispatch_evidence(evidence, plan.evidence(download_bytes));
                }
            }
            _ => {
                return Err(OpenClRuntimeError::new(
                    CL_INVALID_KERNEL,
                    "OpenCL kernel belongs to another runtime backend",
                ));
            }
        }
        self.opencl_evidence.kernel_dispatches =
            self.opencl_evidence.kernel_dispatches.saturating_add(1);
        let work_items = global
            .iter()
            .copied()
            .try_fold(1usize, usize::checked_mul)
            .unwrap_or(usize::MAX) as u64;
        self.opencl_evidence.dispatched_work_items = self
            .opencl_evidence
            .dispatched_work_items
            .saturating_add(work_items);
        Ok(())
    }

    pub(crate) fn release_opencl_kernel(
        &mut self,
        kernel: u64,
    ) -> Result<(), OpenClRuntimeError> {
        if !self.validates_token(kernel, GpuTokenKind::Kernel)
            || !self.kernels.contains_key(&kernel)
        {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_KERNEL,
                format!("OpenCL kernel token {kernel:#x} is stale, forged, or cross-engine"),
            ));
        }
        let record = self
            .kernels
            .remove(&kernel)
            .expect("validated OpenCL kernel remains present");
        let bound_buffers = record.bound_buffers.values().copied().collect::<Vec<_>>();
        drop(record);
        for buffer in bound_buffers {
            self.drop_kernel_buffer_binding(buffer);
        }
        self.opencl_evidence.kernels_released =
            self.opencl_evidence.kernels_released.saturating_add(1);
        Ok(())
    }

    pub(crate) fn is_program_token(&self, token: u64) -> bool {
        self.validates_token(token, GpuTokenKind::Program) && self.programs.contains_key(&token)
    }

    pub(crate) fn is_kernel_token(&self, token: u64) -> bool {
        self.validates_token(token, GpuTokenKind::Kernel) && self.kernels.contains_key(&token)
    }

    pub(crate) fn is_issued_token(token: u64) -> bool {
        issued_gpu_tokens()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&token)
    }

    pub(crate) fn record_opencl_api_call(&mut self, operation: &'static str) {
        let count = self
            .opencl_evidence
            .api_calls
            .entry(operation.to_string())
            .or_default();
        *count = count.saturating_add(1);
    }

    pub(crate) fn record_opencl_error(
        &mut self,
        operation: &'static str,
        status: i32,
        detail: &str,
    ) {
        self.opencl_evidence.errors = self.opencl_evidence.errors.saturating_add(1);
        self.opencl_evidence.last_error = Some(OpenClErrorEvidence {
            operation: operation.to_string(),
            status,
            detail: bounded_opencl_evidence_detail(detail),
        });
    }

    pub(crate) fn opencl_evidence(&self) -> OpenClBridgeEvidence {
        let mut evidence = self.opencl_evidence.clone();
        evidence.live_buffers = self.buffers.len();
        evidence.live_programs = self.programs.len();
        evidence.live_kernels = self.kernels.len();
        apply_object_counts(&mut evidence, self.object_counts());
        evidence
    }

    pub(crate) fn wgpu_evidence(&self) -> Option<WgpuRuntimeEvidence> {
        self.wgpu_evidence.borrow().clone()
    }

    pub(crate) fn allocate_token(&mut self, kind: GpuTokenKind) -> Result<u64, String> {
        let token = self
            .next_token
            .checked_add(u64::from(kind as u8) * GPU_TOKEN_KIND_STRIDE)
            .ok_or_else(|| "GPU synthetic token overflow".to_string())?;
        let next = self
            .next_token
            .checked_add(GPU_TOKEN_OBJECT_STRIDE)
            .filter(|next| *next < self.token_end)
            .ok_or_else(|| "GPU synthetic token space exhausted".to_string())?;
        self.next_token = next;
        self.issued_tokens.insert(token);
        issued_gpu_tokens()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(token);
        Ok(token)
    }

    pub(crate) fn validates_token(&self, token: u64, kind: GpuTokenKind) -> bool {
        (self.token_start()..self.token_end).contains(&token)
            && Self::token_kind(token) == Some(kind)
    }

    pub(crate) fn token_kind(token: u64) -> Option<GpuTokenKind> {
        if !(GPU_TOKEN_BASE..GPU_TOKEN_GLOBAL_END).contains(&token)
            || !(token - GPU_TOKEN_BASE).is_multiple_of(GPU_TOKEN_KIND_STRIDE)
        {
            return None;
        }
        let generation_offset = (token - GPU_TOKEN_BASE) % GPU_TOKEN_GENERATION_BYTES;
        let kind = (generation_offset % GPU_TOKEN_OBJECT_STRIDE) / GPU_TOKEN_KIND_STRIDE;
        GpuTokenKind::decode(kind)
    }

    pub(crate) fn live_buffer_count(&self) -> usize {
        // Token-dead buffers remain charged while kernel bindings retain them.
        self.buffers.len()
    }

    pub(crate) fn live_buffer_bytes(&self) -> usize {
        // Kept in sync with the charged records above, not just live tokens.
        self.live_buffer_bytes
    }

    pub(crate) fn object_counts(&self) -> ObjectCounts {
        match self.backend.as_ref() {
            Some(GpuBackend::OpenCl { tracker, .. }) => tracker.snapshot(),
            Some(GpuBackend::WgpuMetal(_)) => ObjectCounts::default(),
            #[cfg(test)]
            Some(GpuBackend::Mock) | None => ObjectCounts::default(),
            #[cfg(not(test))]
            None => ObjectCounts::default(),
        }
    }

    pub(crate) fn finish(&self) -> Result<(), String> {
        match self.backend.as_ref() {
            Some(GpuBackend::OpenCl { session, .. }) => {
                session.finish().map_err(|error| error.to_string())
            }
            Some(GpuBackend::WgpuMetal(executor)) => {
                if let Some(detail) = self.wgpu_sticky_error.borrow().clone() {
                    return Err(detail);
                }
                let live = executor.live_objects();
                if !live.is_zero() {
                    return Err("wgpu Metal session retained live dispatch resources".into());
                }
                Ok(())
            }
            #[cfg(test)]
            Some(GpuBackend::Mock) => Ok(()),
            None => Err("GPU runtime is not active".into()),
        }
    }

    pub(crate) fn end(&mut self) -> Result<ObjectCounts, String> {
        let tracker = match self.backend.as_ref() {
            Some(GpuBackend::OpenCl { tracker, .. }) => Some(tracker.clone()),
            Some(GpuBackend::WgpuMetal(_)) => None,
            #[cfg(test)]
            Some(GpuBackend::Mock) => None,
            None => return Err("GPU runtime is not active".into()),
        };
        let wgpu_live = match self.backend.as_ref() {
            Some(GpuBackend::WgpuMetal(executor)) => Some(executor.live_objects()),
            _ => None,
        };
        let wgpu_sticky_error = self.wgpu_sticky_error.borrow().clone();
        // OpenCL kernels retain their bound buffers in the safe facade. Drop
        // kernels before programs and the runtime's owning buffer registry,
        // then finally release the session/context.
        self.kernels.clear();
        self.programs.clear();
        self.buffers.clear();
        self.live_buffer_bytes = 0;
        self.live_program_source_bytes = 0;
        self.backend = None;
        self.device_index = None;
        self.device_tokens = None;
        let counts = tracker.map_or_else(ObjectCounts::default, |tracker| tracker.snapshot());
        apply_object_counts(&mut self.opencl_evidence, counts);
        self.opencl_evidence.live_buffers = 0;
        self.opencl_evidence.live_programs = 0;
        self.opencl_evidence.live_kernels = 0;
        self.opencl_evidence.cleanup_balanced =
            counts.live_total() == 0 && counts.release_errors == 0;
        if let Some(evidence) = self.wgpu_evidence.borrow_mut().as_mut() {
            if let Some(live) = wgpu_live {
                replace_resource_counts(&mut evidence.live_resources, live);
            }
            evidence.cleanup_balanced =
                evidence.live_resources.is_zero() && wgpu_sticky_error.is_none();
        }
        if let Some(detail) = wgpu_sticky_error {
            return Err(detail);
        }
        if wgpu_live.is_some_and(|counts| !counts.is_zero()) {
            return Err("wgpu Metal session retained live dispatch resources at shutdown".into());
        }
        Ok(counts)
    }

    fn record_wgpu_sticky_error(&self, detail: &str) {
        let detail = bounded_opencl_evidence_detail(detail);
        let mut sticky = self.wgpu_sticky_error.borrow_mut();
        if sticky.is_none() {
            *sticky = Some(detail.clone());
        }
        if let Some(evidence) = self.wgpu_evidence.borrow_mut().as_mut() {
            evidence.last_error = Some(detail);
            evidence.cleanup_balanced = false;
        }
    }

    fn allocate_device_tokens(&mut self) -> Result<GpuDeviceTokens, String> {
        Ok(GpuDeviceTokens {
            platform: self.allocate_token(GpuTokenKind::Platform)?,
            device: self.allocate_token(GpuTokenKind::Device)?,
            context: self.allocate_token(GpuTokenKind::Context)?,
            queue: self.allocate_token(GpuTokenKind::Queue)?,
        })
    }

    fn token_start(&self) -> u64 {
        self.token_end - GPU_TOKEN_GENERATION_BYTES
    }

    fn device_allocation(&self, token: u64) -> Result<&DeviceAllocation, String> {
        self.buffers
            .get(&token)
            .filter(|allocation| allocation.token_live)
            .ok_or_else(|| format!("GPU device token {token:#x} is stale or forged"))
    }

    fn add_kernel_buffer_binding(&mut self, token: u64) {
        let allocation = self
            .buffers
            .get_mut(&token)
            .expect("successfully bound GPU allocation remains present");
        allocation.kernel_bindings = allocation
            .kernel_bindings
            .checked_add(1)
            .expect("GPU kernel buffer binding count overflow");
    }

    fn drop_kernel_buffer_binding(&mut self, token: u64) {
        let allocation = self
            .buffers
            .get_mut(&token)
            .expect("bound GPU allocation remains present");
        allocation.kernel_bindings = allocation
            .kernel_bindings
            .checked_sub(1)
            .expect("GPU kernel buffer binding count underflow");
        let is_released_and_unbound = !allocation.token_live && allocation.kernel_bindings == 0;
        if is_released_and_unbound {
            self.drop_released_buffer(token);
        }
    }

    fn drop_released_buffer(&mut self, token: u64) {
        let allocation = self
            .buffers
            .remove(&token)
            .expect("released GPU allocation remains present");
        debug_assert!(!allocation.token_live);
        debug_assert_eq!(allocation.kernel_bindings, 0);
        self.live_buffer_bytes = self
            .live_buffer_bytes
            .checked_sub(allocation.bytes)
            .expect("GPU live buffer byte accounting underflow");
        drop(allocation);
    }
}

fn next_gpu_token_generation() -> u32 {
    loop {
        let raw = NEXT_GPU_TOKEN_GENERATION.fetch_add(1, Ordering::Relaxed);
        let generation = raw & GPU_TOKEN_GENERATION_MASK;
        if generation != 0 {
            return generation;
        }
    }
}

fn issued_gpu_tokens() -> &'static Mutex<HashSet<u64>> {
    ISSUED_GPU_TOKENS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn apply_object_counts(evidence: &mut OpenClBridgeEvidence, counts: ObjectCounts) {
    evidence.native_contexts = counts.contexts;
    evidence.native_command_queues = counts.command_queues;
    evidence.native_buffers = counts.buffers;
    evidence.native_programs = counts.programs;
    evidence.native_kernels = counts.kernels;
    evidence.native_release_errors = counts.release_errors;
}

fn bounded_opencl_evidence_detail(detail: &str) -> String {
    if detail.len() <= MAX_OPENCL_EVIDENCE_ERROR_BYTES {
        return detail.to_string();
    }
    let mut end = MAX_OPENCL_EVIDENCE_ERROR_BYTES;
    while !detail.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &detail[..end])
}

fn checked_buffer_range(
    operation: &str,
    offset: usize,
    length: usize,
    buffer_len: usize,
) -> Result<std::ops::Range<usize>, String> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| format!("{operation} range overflow"))?;
    if end > buffer_len {
        return Err(format!(
            "{operation} range {offset}..{end} exceeds buffer length {buffer_len}"
        ));
    }
    Ok(offset..end)
}

#[cfg(test)]
mod gpu_runtime_tests {
    use super::*;

    #[test]
    fn mock_runtime_uses_per_engine_tokens_and_bounded_buffers() {
        let mut first = GpuRuntime::default();
        let mut second = GpuRuntime::default();
        let first_tokens = first.begin_mock(0).unwrap();
        let second_tokens = second.begin_mock(0).unwrap();
        assert_ne!(first_tokens, second_tokens);
        assert!(first.validates_platform(first_tokens.platform));
        assert!(first.validates_device(first_tokens.device));
        assert!(first.validates_context(first_tokens.context));
        assert!(first.validates_queue(first_tokens.queue));
        assert!(!second.validates_context(first_tokens.context));
        assert!(GpuRuntime::looks_like_token(first_tokens.context));

        let buffer = first
            .allocate_device(0, 16, BufferAccess::ReadWrite)
            .unwrap();
        assert!(first.is_buffer_token(buffer));
        assert_eq!(first.buffer_len(buffer), Some(16));
        first.write_device(buffer, 4, &[1, 2, 3, 4]).unwrap();
        let mut bytes = [0; 4];
        first.read_device(buffer, 4, &mut bytes).unwrap();
        assert_eq!(bytes, [1, 2, 3, 4]);
        assert!(first.write_device(buffer, 15, &[1, 2]).is_err());
        assert!(first.allocate_device(1, 16, BufferAccess::ReadWrite).is_err());

        first.free_device(0, buffer).unwrap();
        assert!(!first.is_buffer_token(buffer));
        assert!(first.free_device(0, buffer).is_err());
        assert_eq!(first.live_buffer_count(), 0);
        assert_eq!(first.live_buffer_bytes(), 0);
        assert_eq!(first.object_counts(), ObjectCounts::default());
        first.finish().unwrap();
        assert_eq!(first.end().unwrap(), ObjectCounts::default());
        assert!(!first.is_active());
    }

    #[test]
    fn failed_second_begin_clears_prior_evidence_before_config_loading() {
        let mut runtime = GpuRuntime::default();
        runtime.begin_mock(3).unwrap();
        runtime.record_opencl_api_call("priorCall");
        runtime.end().unwrap();
        *runtime.wgpu_evidence.borrow_mut() = Some(WgpuRuntimeEvidence {
            executor_available: true,
            ..WgpuRuntimeEvidence::default()
        });
        *runtime.wgpu_sticky_error.borrow_mut() = Some("prior sticky error".into());
        assert!(!runtime.opencl_evidence.api_calls.is_empty());

        let error = runtime
            .begin_with_wgpu_config_loader(GpuRuntimeBackendKind::WgpuMetal, 0, || {
                Err("new configuration failure".into())
            })
            .unwrap_err();

        assert_eq!(error, "new configuration failure");
        assert_eq!(runtime.opencl_evidence, OpenClBridgeEvidence::default());
        assert!(runtime.wgpu_evidence.borrow().is_none());
        assert!(runtime.wgpu_sticky_error.borrow().is_none());
        assert!(!runtime.is_active());
    }

    #[test]
    fn sticky_wgpu_error_cannot_serialize_cleanup_as_balanced() {
        let mut runtime = GpuRuntime::default();
        runtime.begin_mock(0).unwrap();
        *runtime.wgpu_evidence.borrow_mut() = Some(WgpuRuntimeEvidence {
            executor_available: true,
            cleanup_balanced: true,
            ..WgpuRuntimeEvidence::default()
        });
        runtime.record_wgpu_sticky_error("proof dispatch failed");

        assert_eq!(runtime.end().unwrap_err(), "proof dispatch failed");
        let evidence = runtime.wgpu_evidence().unwrap();
        assert!(!evidence.cleanup_balanced);
        assert_eq!(evidence.last_error.as_deref(), Some("proof dispatch failed"));
        let json = serde_json::to_value(evidence).unwrap();
        assert_eq!(json["cleanup_balanced"], false);
    }

    #[test]
    fn runtime_shutdown_drops_live_buffers_before_the_session() {
        let mut runtime = GpuRuntime::default();
        runtime.begin_mock(7).unwrap();
        let token = runtime
            .allocate_device(7, 32, BufferAccess::ReadOnly)
            .unwrap();
        assert_eq!(runtime.device_index(), Some(7));
        assert_eq!(runtime.device_tokens().is_some(), true);
        assert_eq!(
            runtime.opencl_session().err().unwrap(),
            "mock GPU runtime has no OpenCL session"
        );
        assert_eq!(
            runtime.opencl_buffer(token).err().unwrap(),
            "mock GPU token has no OpenCL buffer"
        );
        runtime.end().unwrap();
        assert_eq!(runtime.live_buffer_count(), 0);
        assert_eq!(runtime.live_buffer_bytes(), 0);
        assert!(!runtime.is_buffer_token(token));
    }

    #[test]
    fn retained_program_sources_have_an_aggregate_bound() {
        let mut runtime = GpuRuntime::default();
        let tokens = runtime.begin_mock(0).unwrap();
        runtime.live_program_source_bytes = MAX_OPENCL_LIVE_SOURCE_BYTES;
        let error = runtime
            .stage_opencl_program(tokens.context, "x".into(), 1)
            .unwrap_err();
        assert_eq!(error.status, CL_OUT_OF_HOST_MEMORY);
        assert!(runtime.programs.is_empty());
        runtime.end().unwrap();
        assert_eq!(runtime.live_program_source_bytes, 0);
    }

    #[test]
    fn freed_kernel_bound_buffers_cannot_bypass_allocation_count() {
        let (mut runtime, kernel) = mock_runtime_with_kernel();
        for index in 0..MAX_GPU_DEVICE_ALLOCATIONS as u32 {
            let buffer = runtime
                .allocate_device(0, 1, BufferAccess::ReadWrite)
                .unwrap();
            runtime
                .set_opencl_kernel_buffer_arg(kernel, index, buffer)
                .unwrap();
            runtime.free_device(0, buffer).unwrap();
            assert!(!runtime.is_buffer_token(buffer));
            assert_eq!(runtime.buffer_len(buffer), None);
        }

        assert_eq!(runtime.live_buffer_count(), MAX_GPU_DEVICE_ALLOCATIONS);
        assert_eq!(runtime.live_buffer_bytes(), MAX_GPU_DEVICE_ALLOCATIONS);
        assert_eq!(
            runtime.opencl_evidence().live_buffers,
            MAX_GPU_DEVICE_ALLOCATIONS
        );
        assert!(
            runtime
                .allocate_device(0, 1, BufferAccess::ReadWrite)
                .unwrap_err()
                .contains("allocation count")
        );

        runtime.end().unwrap();
        assert_eq!(runtime.live_buffer_count(), 0);
        assert_eq!(runtime.live_buffer_bytes(), 0);
        assert!(runtime.buffers.is_empty());
        assert!(runtime.opencl_evidence().cleanup_balanced);
    }

    #[test]
    fn freed_buffer_charge_tracks_byte_limit_replacement_and_kernel_release() {
        let (mut runtime, kernel) = mock_runtime_with_kernel();
        // Model one maximum-sized mock allocation without reserving 256 MiB of
        // host memory in the unit test. The production allocation path applies
        // the same bytes field and live byte counter before a token can bind.
        let maximum_buffer = runtime.allocate_token(GpuTokenKind::Buffer).unwrap();
        assert!(
            runtime
                .buffers
                .insert(
                    maximum_buffer,
                    DeviceAllocation {
                        device_index: 0,
                        bytes: MAX_GPU_DEVICE_BYTES,
                        backing: DeviceBufferBacking::Mock(RefCell::new(Vec::new())),
                        token_live: true,
                        kernel_bindings: 0,
                    },
                )
                .is_none()
        );
        runtime.live_buffer_bytes = MAX_GPU_DEVICE_BYTES;

        runtime
            .set_opencl_kernel_buffer_arg(kernel, 0, maximum_buffer)
            .unwrap();
        runtime.free_device(0, maximum_buffer).unwrap();
        assert!(!runtime.is_buffer_token(maximum_buffer));
        assert!(runtime.free_device(0, maximum_buffer).is_err());
        assert!(runtime.read_device(maximum_buffer, 0, &mut []).is_err());
        assert_eq!(
            runtime
                .set_opencl_kernel_buffer_arg(kernel, 0, maximum_buffer)
                .unwrap_err()
                .status,
            CL_INVALID_MEM_OBJECT
        );
        assert_eq!(runtime.live_buffer_count(), 1);
        assert_eq!(runtime.live_buffer_bytes(), MAX_GPU_DEVICE_BYTES);
        assert!(
            runtime
                .allocate_device(0, 1, BufferAccess::ReadWrite)
                .unwrap_err()
                .contains("live bytes")
        );

        runtime
            .set_opencl_kernel_raw_arg(kernel, 0, &[0])
            .unwrap();
        assert_eq!(runtime.live_buffer_count(), 0);
        assert_eq!(runtime.live_buffer_bytes(), 0);
        assert!(runtime.buffers.is_empty());

        let shared = runtime
            .allocate_device(0, 8, BufferAccess::ReadWrite)
            .unwrap();
        runtime
            .set_opencl_kernel_buffer_arg(kernel, 1, shared)
            .unwrap();
        runtime
            .set_opencl_kernel_buffer_arg(kernel, 2, shared)
            .unwrap();
        runtime.free_device(0, shared).unwrap();
        runtime
            .set_opencl_kernel_raw_arg(kernel, 1, &[0])
            .unwrap();
        assert_eq!(runtime.live_buffer_count(), 1);
        assert_eq!(runtime.live_buffer_bytes(), 8);
        runtime
            .set_opencl_kernel_raw_arg(kernel, 2, &[0])
            .unwrap();
        assert_eq!(runtime.live_buffer_count(), 0);
        assert_eq!(runtime.live_buffer_bytes(), 0);

        let first = runtime
            .allocate_device(0, 8, BufferAccess::ReadWrite)
            .unwrap();
        let replacement = runtime
            .allocate_device(0, 16, BufferAccess::ReadWrite)
            .unwrap();
        runtime
            .set_opencl_kernel_buffer_arg(kernel, 3, first)
            .unwrap();
        runtime.free_device(0, first).unwrap();
        assert_eq!(runtime.live_buffer_count(), 2);
        assert_eq!(runtime.live_buffer_bytes(), 24);

        runtime
            .set_opencl_kernel_buffer_arg(kernel, 3, replacement)
            .unwrap();
        assert_eq!(runtime.live_buffer_count(), 1);
        assert_eq!(runtime.live_buffer_bytes(), 16);
        assert!(!runtime.buffers.contains_key(&first));

        runtime.free_device(0, replacement).unwrap();
        assert_eq!(runtime.live_buffer_count(), 1);
        assert_eq!(runtime.live_buffer_bytes(), 16);
        runtime.release_opencl_kernel(kernel).unwrap();
        assert_eq!(runtime.live_buffer_count(), 0);
        assert_eq!(runtime.live_buffer_bytes(), 0);
        assert!(runtime.buffers.is_empty());
        runtime.end().unwrap();
    }

    fn mock_runtime_with_kernel() -> (GpuRuntime, u64) {
        let mut runtime = GpuRuntime::default();
        let tokens = runtime.begin_mock(0).unwrap();
        let program = runtime
            .stage_opencl_program(tokens.context, "__kernel void test() {}".into(), 1)
            .unwrap();
        runtime
            .build_opencl_program(program, &[tokens.device], None)
            .unwrap();
        let kernel = runtime.create_opencl_kernel(program, "test").unwrap();
        (runtime, kernel)
    }
}
