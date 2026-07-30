use aex_apple_opencl::{
    Buffer, BufferAccess, ObjectCounts, ObjectTracker, Session, MAX_BUFFER_BYTES,
};
#[cfg(test)]
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

const GPU_TOKEN_BASE: u64 = 0x0000_0040_0000_0000;
const GPU_TOKEN_GENERATION_BYTES: u64 = 0x0100_0000;
const GPU_TOKEN_GENERATION_MASK: u32 = 0x000f_ffff;
const GPU_TOKEN_GLOBAL_END: u64 =
    GPU_TOKEN_BASE + ((GPU_TOKEN_GENERATION_MASK as u64 + 1) * GPU_TOKEN_GENERATION_BYTES);
const GPU_TOKEN_KIND_STRIDE: u64 = 0x10;
const GPU_TOKEN_OBJECT_STRIDE: u64 = 0x100;
const MAX_GPU_DEVICE_ALLOCATIONS: usize = 256;
const MAX_GPU_DEVICE_BYTES: usize = 256 * 1024 * 1024;
static NEXT_GPU_TOKEN_GENERATION: AtomicU32 = AtomicU32::new(1);

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
    #[cfg(test)]
    Mock,
}

enum DeviceBufferBacking {
    OpenCl(Buffer),
    #[cfg(test)]
    Mock(RefCell<Vec<u8>>),
}

struct DeviceAllocation {
    device_index: u32,
    bytes: usize,
    backing: DeviceBufferBacking,
}

pub(crate) struct GpuRuntime {
    backend: Option<GpuBackend>,
    device_index: Option<u32>,
    device_tokens: Option<GpuDeviceTokens>,
    next_token: u64,
    token_end: u64,
    buffers: HashMap<u64, DeviceAllocation>,
    live_buffer_bytes: usize,
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
            live_buffer_bytes: 0,
        }
    }
}

impl GpuRuntime {
    pub(crate) fn begin_opencl(
        &mut self,
        device_index: u32,
    ) -> Result<GpuDeviceTokens, String> {
        if self.backend.is_some() {
            return Err("GPU runtime is already active".into());
        }
        let session =
            Session::select_gpu(device_index as usize).map_err(|error| error.to_string())?;
        let tracker = session.object_tracker();
        let tokens = self.allocate_device_tokens()?;
        self.backend = Some(GpuBackend::OpenCl { session, tracker });
        self.device_index = Some(device_index);
        self.device_tokens = Some(tokens);
        Ok(tokens)
    }

    #[cfg(test)]
    pub(crate) fn begin_mock(&mut self, device_index: u32) -> Result<GpuDeviceTokens, String> {
        if self.backend.is_some() {
            return Err("GPU runtime is already active".into());
        }
        let tokens = self.allocate_device_tokens()?;
        self.backend = Some(GpuBackend::Mock);
        self.device_index = Some(device_index);
        self.device_tokens = Some(tokens);
        Ok(tokens)
    }

    pub(crate) fn is_active(&self) -> bool {
        self.backend.is_some()
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
        if bytes > MAX_BUFFER_BYTES || bytes > MAX_GPU_DEVICE_BYTES {
            return Err(format!(
                "GPU device allocation {bytes} exceeds {} bytes",
                MAX_GPU_DEVICE_BYTES.min(MAX_BUFFER_BYTES)
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
            },
        );
        self.live_buffer_bytes += bytes;
        Ok(token)
    }

    pub(crate) fn free_device(&mut self, device_index: u32, token: u64) -> Result<(), String> {
        let allocation = self
            .buffers
            .get(&token)
            .ok_or_else(|| format!("GPU device token {token:#x} is stale or forged"))?;
        if allocation.device_index != device_index || self.device_index != Some(device_index) {
            return Err(format!(
                "GPU device token {token:#x} belongs to device {}, not {device_index}",
                allocation.device_index
            ));
        }
        let allocation = self
            .buffers
            .remove(&token)
            .expect("validated GPU allocation remains present");
        self.live_buffer_bytes -= allocation.bytes;
        drop(allocation);
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
        self.buffers.get(&token).map(|allocation| allocation.bytes)
    }

    pub(crate) fn is_buffer_token(&self, token: u64) -> bool {
        self.validates_token(token, GpuTokenKind::Buffer) && self.buffers.contains_key(&token)
    }

    pub(crate) fn looks_like_token(token: u64) -> bool {
        (GPU_TOKEN_BASE..GPU_TOKEN_GLOBAL_END).contains(&token)
            && (token - GPU_TOKEN_BASE).is_multiple_of(GPU_TOKEN_KIND_STRIDE)
            && Self::token_kind(token).is_some()
    }

    pub(crate) fn opencl_session(&self) -> Result<&Session, String> {
        match self.backend.as_ref() {
            Some(GpuBackend::OpenCl { session, .. }) => Ok(session),
            #[cfg(test)]
            Some(GpuBackend::Mock) => Err("mock GPU runtime has no OpenCL session".into()),
            None => Err("GPU runtime is not active".into()),
        }
    }

    pub(crate) fn opencl_buffer(&self, token: u64) -> Result<&Buffer, String> {
        let allocation = self.device_allocation(token)?;
        match &allocation.backing {
            DeviceBufferBacking::OpenCl(buffer) => Ok(buffer),
            #[cfg(test)]
            DeviceBufferBacking::Mock(_) => Err("mock GPU token has no OpenCL buffer".into()),
        }
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
        self.buffers.len()
    }

    pub(crate) fn live_buffer_bytes(&self) -> usize {
        self.live_buffer_bytes
    }

    pub(crate) fn object_counts(&self) -> ObjectCounts {
        match self.backend.as_ref() {
            Some(GpuBackend::OpenCl { tracker, .. }) => tracker.snapshot(),
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
            #[cfg(test)]
            Some(GpuBackend::Mock) => Ok(()),
            None => Err("GPU runtime is not active".into()),
        }
    }

    pub(crate) fn end_opencl(&mut self) -> Result<ObjectCounts, String> {
        let tracker = match self.backend.as_ref() {
            Some(GpuBackend::OpenCl { tracker, .. }) => Some(tracker.clone()),
            #[cfg(test)]
            Some(GpuBackend::Mock) => None,
            None => return Err("GPU runtime is not active".into()),
        };
        self.buffers.clear();
        self.live_buffer_bytes = 0;
        self.backend = None;
        self.device_index = None;
        self.device_tokens = None;
        Ok(tracker.map_or_else(ObjectCounts::default, |tracker| tracker.snapshot()))
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
            .ok_or_else(|| format!("GPU device token {token:#x} is stale or forged"))
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
        assert_eq!(first.end_opencl().unwrap(), ObjectCounts::default());
        assert!(!first.is_active());
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
        runtime.end_opencl().unwrap();
        assert_eq!(runtime.live_buffer_count(), 0);
        assert_eq!(runtime.live_buffer_bytes(), 0);
        assert!(!runtime.is_buffer_token(token));
    }
}
