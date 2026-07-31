#[cfg(any(target_os = "macos", test))]
use std::collections::BTreeSet;

use thiserror::Error;

pub const MAX_SHADER_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ENTRY_POINT_BYTES: usize = 256;
pub const MAX_LABEL_BYTES: usize = 256;
pub const MAX_BINDING_COUNT: usize = 8;
pub const MAX_BUFFER_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_TOTAL_BUFFER_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_WORKGROUPS_PER_AXIS: u32 = 65_535;
pub const MAX_TOTAL_WORKGROUPS: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingAccess {
    ReadOnlyStorage,
    ReadWriteStorage,
}

#[derive(Clone, Copy, Debug)]
pub struct BufferBinding<'a> {
    pub binding: u32,
    pub access: BindingAccess,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug)]
pub struct DispatchDescriptor<'a> {
    pub label: Option<&'a str>,
    pub wgsl: &'a str,
    pub entry_point: &'a str,
    pub bindings: &'a [BufferBinding<'a>],
    pub workgroups: [u32; 3],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterReport {
    pub name: String,
    pub backend: String,
    pub device_type: String,
    pub vendor: u32,
    pub device: u32,
    pub driver: String,
    pub driver_info: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ObjectCounts {
    pub buffers: usize,
    pub staging_buffers: usize,
    pub shader_modules: usize,
    pub bind_group_layouts: usize,
    pub pipeline_layouts: usize,
    pub pipelines: usize,
    pub bind_groups: usize,
    pub command_buffers: usize,
}

impl ObjectCounts {
    pub fn is_zero(self) -> bool {
        self == Self::default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BufferOutput {
    pub binding: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchReport {
    pub adapter: AdapterReport,
    pub workgroups: [u32; 3],
    pub outputs: Vec<BufferOutput>,
    pub created_resources: ObjectCounts,
    pub live_resources: ObjectCounts,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("wgpu Metal compute is supported only on macOS")]
    UnsupportedPlatform,
    #[error("no Metal adapter is available")]
    NoMetalAdapter,
    #[error("wgpu selected backend {0}, expected Metal")]
    UnexpectedBackend(String),
    #[error("requesting the wgpu device failed: {0}")]
    RequestDevice(String),
    #[error("WGSL source must not be empty")]
    EmptyShader,
    #[error("WGSL source is {actual} bytes; maximum is {maximum}")]
    ShaderTooLarge { actual: usize, maximum: usize },
    #[error("entry point must not be empty")]
    EmptyEntryPoint,
    #[error("entry point is {actual} bytes; maximum is {maximum}")]
    EntryPointTooLong { actual: usize, maximum: usize },
    #[error("debug label is {actual} bytes; maximum is {maximum}")]
    LabelTooLong { actual: usize, maximum: usize },
    #[error("binding count is {actual}; supported range is 1..={maximum}")]
    InvalidBindingCount { actual: usize, maximum: usize },
    #[error("binding index {binding} is outside the supported range 0..{maximum}")]
    BindingIndexOutOfRange { binding: u32, maximum: usize },
    #[error("binding index {0} is duplicated")]
    DuplicateBinding(u32),
    #[error("binding {binding} is empty")]
    EmptyBuffer { binding: u32 },
    #[error("binding {binding} size {actual} is not a multiple of four bytes")]
    UnalignedBuffer { binding: u32, actual: usize },
    #[error("binding {binding} is {actual} bytes; maximum is {maximum}")]
    BufferTooLarge {
        binding: u32,
        actual: usize,
        maximum: usize,
    },
    #[error("total buffer allocations are {actual} bytes; maximum is {maximum}")]
    TotalBufferBytesTooLarge { actual: usize, maximum: usize },
    #[error("workgroup axis {axis} is {actual}; supported range is 1..={maximum}")]
    InvalidWorkgroupAxis {
        axis: usize,
        actual: u32,
        maximum: u32,
    },
    #[error("dispatch contains {actual} workgroups; maximum is {maximum}")]
    DispatchTooLarge { actual: u64, maximum: u64 },
    #[error("wgpu validation failed: {0}")]
    Validation(String),
    #[error("wgpu reported out of memory: {0}")]
    OutOfMemory(String),
    #[error("wgpu reported an uncaptured error: {0}")]
    Uncaptured(String),
    #[error("polling the wgpu device failed: {0}")]
    DevicePoll(String),
    #[error("wgpu dispatch did not complete within the bounded timeout")]
    DispatchTimeout,
    #[error("wgpu dispatch completion callback disconnected")]
    CompletionChannelDisconnected,
    #[error("the wgpu session is poisoned after a fatal device or dispatch error")]
    SessionPoisoned,
    #[error("mapping readback for binding {binding} failed: {message}")]
    Map { binding: u32, message: String },
    #[error("mapping readback for binding {0} timed out")]
    MapTimeout(u32),
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn validate_dispatch(descriptor: &DispatchDescriptor<'_>) -> Result<(), Error> {
    let shader_bytes = descriptor.wgsl.len();
    if shader_bytes == 0 {
        return Err(Error::EmptyShader);
    }
    if shader_bytes > MAX_SHADER_SOURCE_BYTES {
        return Err(Error::ShaderTooLarge {
            actual: shader_bytes,
            maximum: MAX_SHADER_SOURCE_BYTES,
        });
    }

    let entry_point_bytes = descriptor.entry_point.len();
    if entry_point_bytes == 0 {
        return Err(Error::EmptyEntryPoint);
    }
    if entry_point_bytes > MAX_ENTRY_POINT_BYTES {
        return Err(Error::EntryPointTooLong {
            actual: entry_point_bytes,
            maximum: MAX_ENTRY_POINT_BYTES,
        });
    }
    if let Some(label) = descriptor.label {
        if label.len() > MAX_LABEL_BYTES {
            return Err(Error::LabelTooLong {
                actual: label.len(),
                maximum: MAX_LABEL_BYTES,
            });
        }
    }

    if descriptor.bindings.is_empty() || descriptor.bindings.len() > MAX_BINDING_COUNT {
        return Err(Error::InvalidBindingCount {
            actual: descriptor.bindings.len(),
            maximum: MAX_BINDING_COUNT,
        });
    }

    let mut seen = BTreeSet::new();
    let mut total_bytes = 0usize;
    for binding in descriptor.bindings {
        if binding.binding as usize >= MAX_BINDING_COUNT {
            return Err(Error::BindingIndexOutOfRange {
                binding: binding.binding,
                maximum: MAX_BINDING_COUNT,
            });
        }
        if !seen.insert(binding.binding) {
            return Err(Error::DuplicateBinding(binding.binding));
        }
        if binding.bytes.is_empty() {
            return Err(Error::EmptyBuffer {
                binding: binding.binding,
            });
        }
        if binding.bytes.len() % 4 != 0 {
            return Err(Error::UnalignedBuffer {
                binding: binding.binding,
                actual: binding.bytes.len(),
            });
        }
        if binding.bytes.len() > MAX_BUFFER_BYTES {
            return Err(Error::BufferTooLarge {
                binding: binding.binding,
                actual: binding.bytes.len(),
                maximum: MAX_BUFFER_BYTES,
            });
        }
        let allocation_multiplier = match binding.access {
            // queue.write_buffer creates upload staging in addition to the
            // device buffer.
            BindingAccess::ReadOnlyStorage => 2,
            // Writable data also needs a MAP_READ staging buffer and an owned
            // output Vec while that staging buffer remains mapped.
            BindingAccess::ReadWriteStorage => 3,
        };
        let allocation_bytes = binding.bytes.len().saturating_mul(allocation_multiplier);
        total_bytes = total_bytes.saturating_add(allocation_bytes);
        if total_bytes > MAX_TOTAL_BUFFER_BYTES {
            return Err(Error::TotalBufferBytesTooLarge {
                actual: total_bytes,
                maximum: MAX_TOTAL_BUFFER_BYTES,
            });
        }
    }

    let mut total_workgroups = 1u64;
    for (axis, value) in descriptor.workgroups.into_iter().enumerate() {
        if value == 0 || value > MAX_WORKGROUPS_PER_AXIS {
            return Err(Error::InvalidWorkgroupAxis {
                axis,
                actual: value,
                maximum: MAX_WORKGROUPS_PER_AXIS,
            });
        }
        total_workgroups = total_workgroups.saturating_mul(u64::from(value));
    }
    if total_workgroups > MAX_TOTAL_WORKGROUPS {
        return Err(Error::DispatchTooLarge {
            actual: total_workgroups,
            maximum: MAX_TOTAL_WORKGROUPS,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHADER: &str = "@compute @workgroup_size(1) fn main() {}";

    fn descriptor<'a>(bindings: &'a [BufferBinding<'a>]) -> DispatchDescriptor<'a> {
        DispatchDescriptor {
            label: None,
            wgsl: SHADER,
            entry_point: "main",
            bindings,
            workgroups: [1, 1, 1],
        }
    }

    #[test]
    fn accepts_a_small_bounded_dispatch() {
        let bytes = 1u32.to_le_bytes();
        let bindings = [BufferBinding {
            binding: 0,
            access: BindingAccess::ReadOnlyStorage,
            bytes: &bytes,
        }];
        assert!(validate_dispatch(&descriptor(&bindings)).is_ok());
    }

    #[test]
    fn rejects_duplicate_and_unaligned_bindings() {
        let bytes = [0u8; 4];
        let duplicate = [
            BufferBinding {
                binding: 1,
                access: BindingAccess::ReadOnlyStorage,
                bytes: &bytes,
            },
            BufferBinding {
                binding: 1,
                access: BindingAccess::ReadWriteStorage,
                bytes: &bytes,
            },
        ];
        assert!(matches!(
            validate_dispatch(&descriptor(&duplicate)),
            Err(Error::DuplicateBinding(1))
        ));

        let unaligned = [BufferBinding {
            binding: 0,
            access: BindingAccess::ReadOnlyStorage,
            bytes: &[0u8; 3],
        }];
        assert!(matches!(
            validate_dispatch(&descriptor(&unaligned)),
            Err(Error::UnalignedBuffer {
                binding: 0,
                actual: 3
            })
        ));
    }

    #[test]
    fn rejects_bindings_above_the_requested_device_limit() {
        let bytes = [0u8; 4];
        let bindings = (0..=MAX_BINDING_COUNT)
            .map(|binding| BufferBinding {
                binding: binding as u32,
                access: BindingAccess::ReadOnlyStorage,
                bytes: &bytes,
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            validate_dispatch(&descriptor(&bindings)),
            Err(Error::InvalidBindingCount {
                actual: 9,
                maximum: MAX_BINDING_COUNT
            })
        ));
    }

    #[test]
    fn rejects_zero_and_excessive_workgroups() {
        let bytes = [0u8; 4];
        let bindings = [BufferBinding {
            binding: 0,
            access: BindingAccess::ReadOnlyStorage,
            bytes: &bytes,
        }];
        let mut value = descriptor(&bindings);
        value.workgroups = [0, 1, 1];
        assert!(matches!(
            validate_dispatch(&value),
            Err(Error::InvalidWorkgroupAxis {
                axis: 0,
                actual: 0,
                ..
            })
        ));

        value.workgroups = [MAX_WORKGROUPS_PER_AXIS, MAX_WORKGROUPS_PER_AXIS, 1];
        assert!(matches!(
            validate_dispatch(&value),
            Err(Error::DispatchTooLarge { .. })
        ));
    }
}
