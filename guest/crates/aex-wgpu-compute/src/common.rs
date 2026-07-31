use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

pub const MAX_SHADER_SOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ENTRY_POINT_BYTES: usize = 256;
pub const MAX_LABEL_BYTES: usize = 256;
pub const MAX_BINDING_COUNT: usize = 20;
pub const MAX_BINDING_INDEX_EXCLUSIVE: usize = 32;
pub const MAX_STORAGE_BINDING_COUNT: usize = 8;
pub const MAX_UNIFORM_BINDING_COUNT: usize = 12;
pub const MAX_BUFFER_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_UNIFORM_BUFFER_BYTES: usize = 64 * 1024;
pub const MAX_TOTAL_BUFFER_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_WORKGROUPS_PER_AXIS: u32 = 65_535;
pub const MAX_TOTAL_WORKGROUPS: u64 = 16 * 1024 * 1024;
pub const MAX_PIPELINE_CONSTANTS: usize = 32;
pub const MAX_PIPELINE_CONSTANT_KEY_BYTES: usize = 128;
pub const MAX_METAL_ADAPTER_COUNT: usize = 16;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const UNIFORM_BUFFER_SIZE_ALIGNMENT: usize = 16;
const MAX_NAGA_ENTRY_POINTS: usize = 16;
const MAX_NAGA_GLOBALS: usize = 256;
const MAX_NAGA_OVERRIDES: usize = 64;
const MAX_NAGA_IR_ITEMS: usize = 262_144;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingAccess {
    ReadOnlyStorage,
    ReadWriteStorage,
    Uniform,
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

#[derive(Clone, Copy, Debug)]
pub struct PipelineConstant<'a> {
    pub key: &'a str,
    pub value: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct NagaDispatchDescriptor<'a> {
    pub label: Option<&'a str>,
    pub entry_point: &'a str,
    pub bindings: &'a [BufferBinding<'a>],
    pub constants: &'a [PipelineConstant<'a>],
    pub workgroups: [u32; 3],
}

#[derive(Clone, Debug)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct ValidatedNagaModule {
    module: naga::Module,
    compute_entry_points: BTreeSet<String>,
    bindings_by_entry: BTreeMap<String, BTreeMap<u32, BindingAccess>>,
    override_keys: BTreeSet<String>,
    required_override_keys: BTreeSet<String>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl ValidatedNagaModule {
    pub fn new(module: naga::Module) -> Result<Self, Error> {
        if module.entry_points.len() > MAX_NAGA_ENTRY_POINTS
            || module.global_variables.len() > MAX_NAGA_GLOBALS
            || module.overrides.len() > MAX_NAGA_OVERRIDES
            || !naga_ir_items_within_limit(&module, MAX_NAGA_IR_ITEMS)
        {
            return Err(Error::NagaModuleTooComplex);
        }
        for entry in &module.entry_points {
            if entry.name.is_empty() {
                return Err(Error::EmptyEntryPoint);
            }
            if entry.name.len() > MAX_ENTRY_POINT_BYTES {
                return Err(Error::EntryPointTooLong {
                    actual: entry.name.len(),
                    maximum: MAX_ENTRY_POINT_BYTES,
                });
            }
        }
        let module_info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .map_err(|error| Error::InvalidNagaModule(bounded_message(error.to_string())))?;

        let compute_entry_points = module
            .entry_points
            .iter()
            .filter(|entry| entry.stage == naga::ShaderStage::Compute)
            .map(|entry| entry.name.clone())
            .collect::<BTreeSet<_>>();
        if compute_entry_points.is_empty() {
            return Err(Error::NagaModuleHasNoComputeEntryPoint);
        }

        let mut global_bindings = BTreeMap::new();
        for (handle, variable) in module.global_variables.iter() {
            if variable.space == naga::AddressSpace::PushConstant {
                return Err(Error::UnsupportedNagaAddressSpace("PushConstant"));
            }
            let Some(resource) = &variable.binding else {
                continue;
            };
            if resource.group != 0 {
                return Err(Error::UnsupportedBindGroup(resource.group));
            }
            if resource.binding as usize >= MAX_BINDING_INDEX_EXCLUSIVE {
                return Err(Error::BindingIndexOutOfRange {
                    binding: resource.binding,
                    maximum: MAX_BINDING_INDEX_EXCLUSIVE,
                });
            }
            let access = match variable.space {
                naga::AddressSpace::Uniform => BindingAccess::Uniform,
                naga::AddressSpace::Storage { access }
                    if access.contains(naga::StorageAccess::STORE) =>
                {
                    BindingAccess::ReadWriteStorage
                }
                naga::AddressSpace::Storage { .. } => BindingAccess::ReadOnlyStorage,
                _ => return Err(Error::UnsupportedNagaAddressSpace("bound resource")),
            };
            if global_bindings
                .insert(handle, (resource.binding, access))
                .is_some()
            {
                return Err(Error::DuplicateBinding(resource.binding));
            }
        }
        let mut bindings_by_entry = BTreeMap::new();
        for (entry_index, entry) in module.entry_points.iter().enumerate() {
            if entry.stage != naga::ShaderStage::Compute {
                continue;
            }
            let entry_info = module_info.get_entry_point(entry_index);
            let mut bindings = BTreeMap::new();
            for (handle, (binding, access)) in &global_bindings {
                if entry_info[*handle] == naga::valid::GlobalUse::empty() {
                    continue;
                }
                if bindings.insert(*binding, *access).is_some() {
                    return Err(Error::DuplicateBinding(*binding));
                }
            }
            bindings_by_entry.insert(entry.name.clone(), bindings);
        }

        let mut override_keys = BTreeSet::new();
        let mut required_override_keys = BTreeSet::new();
        for (_, value) in module.overrides.iter() {
            let key = if let Some(id) = value.id {
                id.to_string()
            } else if let Some(name) = &value.name {
                name.clone()
            } else {
                return Err(Error::InvalidNagaOverride);
            };
            if !valid_pipeline_constant_key(&key) {
                return Err(Error::InvalidPipelineConstantKey(key));
            }
            if value.init.is_none() {
                required_override_keys.insert(key.clone());
            }
            override_keys.insert(key);
        }

        Ok(Self {
            module,
            compute_entry_points,
            bindings_by_entry,
            override_keys,
            required_override_keys,
        })
    }

    pub fn compute_entry_points(&self) -> impl Iterator<Item = &str> {
        self.compute_entry_points.iter().map(String::as_str)
    }

    pub(crate) fn module(&self) -> &naga::Module {
        &self.module
    }

    pub(crate) fn validate_descriptor(
        &self,
        descriptor: &NagaDispatchDescriptor<'_>,
    ) -> Result<(), Error> {
        validate_dispatch_parts(
            descriptor.label,
            descriptor.entry_point,
            descriptor.bindings,
            descriptor.workgroups,
        )?;
        let Some(expected_bindings) = self.bindings_by_entry.get(descriptor.entry_point) else {
            return Err(Error::UnknownComputeEntryPoint(
                descriptor.entry_point.to_owned(),
            ));
        };
        let supplied = descriptor
            .bindings
            .iter()
            .map(|binding| (binding.binding, binding.access))
            .collect::<BTreeMap<_, _>>();
        if &supplied != expected_bindings {
            return Err(Error::NagaBindingMismatch);
        }
        validate_pipeline_constants(
            descriptor.constants,
            &self.override_keys,
            &self.required_override_keys,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterReport {
    pub index: usize,
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
    #[error("Metal adapter count {actual} exceeds bounded maximum {maximum}")]
    TooManyMetalAdapters { actual: usize, maximum: usize },
    #[error("Metal adapter index {requested} is out of range; available adapters: {available}")]
    MetalAdapterIndexOutOfRange { requested: usize, available: usize },
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
    #[error("uniform binding {binding} is {actual} bytes; maximum is {maximum}")]
    UniformBufferTooLarge {
        binding: u32,
        actual: usize,
        maximum: usize,
    },
    #[error(
        "uniform binding {binding} size {actual} must be nonzero and aligned to {alignment} bytes"
    )]
    InvalidUniformBufferSize {
        binding: u32,
        actual: usize,
        alignment: usize,
    },
    #[error("storage binding count {actual} exceeds device limit {maximum}")]
    StorageBindingLimit { actual: usize, maximum: usize },
    #[error("uniform binding count {actual} exceeds device limit {maximum}")]
    UniformBindingLimit { actual: usize, maximum: usize },
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
    #[error("Naga module exceeds bounded facade complexity limits")]
    NagaModuleTooComplex,
    #[error("Naga module validation failed: {0}")]
    InvalidNagaModule(String),
    #[error("Naga module has no compute entry point")]
    NagaModuleHasNoComputeEntryPoint,
    #[error("Naga override has neither an id nor a name")]
    InvalidNagaOverride,
    #[error("Naga module uses unsupported bind group {0}; only group 0 is supported")]
    UnsupportedBindGroup(u32),
    #[error("Naga module uses unsupported address space {0}")]
    UnsupportedNagaAddressSpace(&'static str),
    #[error("Naga module does not contain compute entry point {0}")]
    UnknownComputeEntryPoint(String),
    #[error("Naga module bindings do not exactly match the supplied reflection bindings")]
    NagaBindingMismatch,
    #[error("pipeline constant count {actual} exceeds maximum {maximum}")]
    PipelineConstantCount { actual: usize, maximum: usize },
    #[error("pipeline constant key {0:?} is invalid or exceeds the bounded length")]
    InvalidPipelineConstantKey(String),
    #[error("pipeline constant key {0:?} is duplicated")]
    DuplicatePipelineConstant(String),
    #[error("pipeline constant {0:?} is not declared by the Naga module")]
    UnknownPipelineConstant(String),
    #[error("required pipeline constant {0:?} is missing")]
    MissingPipelineConstant(String),
    #[error("pipeline constant {0:?} must be finite")]
    NonFinitePipelineConstant(String),
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

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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

    validate_dispatch_parts(
        descriptor.label,
        descriptor.entry_point,
        descriptor.bindings,
        descriptor.workgroups,
    )
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn validate_dispatch_parts(
    label: Option<&str>,
    entry_point: &str,
    bindings: &[BufferBinding<'_>],
    workgroups: [u32; 3],
) -> Result<(), Error> {
    let entry_point_bytes = entry_point.len();
    if entry_point_bytes == 0 {
        return Err(Error::EmptyEntryPoint);
    }
    if entry_point_bytes > MAX_ENTRY_POINT_BYTES {
        return Err(Error::EntryPointTooLong {
            actual: entry_point_bytes,
            maximum: MAX_ENTRY_POINT_BYTES,
        });
    }
    if let Some(label) = label
        && label.len() > MAX_LABEL_BYTES
    {
        return Err(Error::LabelTooLong {
            actual: label.len(),
            maximum: MAX_LABEL_BYTES,
        });
    }

    if bindings.is_empty() || bindings.len() > MAX_BINDING_COUNT {
        return Err(Error::InvalidBindingCount {
            actual: bindings.len(),
            maximum: MAX_BINDING_COUNT,
        });
    }

    let mut seen = BTreeSet::new();
    let mut total_bytes = 0usize;
    let mut storage_count = 0usize;
    let mut uniform_count = 0usize;
    for binding in bindings {
        if binding.binding as usize >= MAX_BINDING_INDEX_EXCLUSIVE {
            return Err(Error::BindingIndexOutOfRange {
                binding: binding.binding,
                maximum: MAX_BINDING_INDEX_EXCLUSIVE,
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
        if binding.access == BindingAccess::Uniform {
            uniform_count += 1;
            if binding.bytes.len() > MAX_UNIFORM_BUFFER_BYTES {
                return Err(Error::UniformBufferTooLarge {
                    binding: binding.binding,
                    actual: binding.bytes.len(),
                    maximum: MAX_UNIFORM_BUFFER_BYTES,
                });
            }
            if binding.bytes.len() % UNIFORM_BUFFER_SIZE_ALIGNMENT != 0 {
                return Err(Error::InvalidUniformBufferSize {
                    binding: binding.binding,
                    actual: binding.bytes.len(),
                    alignment: UNIFORM_BUFFER_SIZE_ALIGNMENT,
                });
            }
        } else {
            storage_count += 1;
        }
        let allocation_multiplier = match binding.access {
            // queue.write_buffer creates upload staging in addition to the
            // device buffer.
            BindingAccess::ReadOnlyStorage => 2,
            // Writable data also needs a MAP_READ staging buffer and an owned
            // output Vec while that staging buffer remains mapped.
            BindingAccess::ReadWriteStorage => 3,
            BindingAccess::Uniform => 2,
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
    if storage_count > MAX_STORAGE_BINDING_COUNT {
        return Err(Error::StorageBindingLimit {
            actual: storage_count,
            maximum: MAX_STORAGE_BINDING_COUNT,
        });
    }
    if uniform_count > MAX_UNIFORM_BINDING_COUNT {
        return Err(Error::UniformBindingLimit {
            actual: uniform_count,
            maximum: MAX_UNIFORM_BINDING_COUNT,
        });
    }

    let mut total_workgroups = 1u64;
    for (axis, value) in workgroups.into_iter().enumerate() {
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

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn validate_pipeline_constants(
    constants: &[PipelineConstant<'_>],
    known: &BTreeSet<String>,
    required: &BTreeSet<String>,
) -> Result<(), Error> {
    if constants.len() > MAX_PIPELINE_CONSTANTS {
        return Err(Error::PipelineConstantCount {
            actual: constants.len(),
            maximum: MAX_PIPELINE_CONSTANTS,
        });
    }
    let mut supplied = BTreeSet::new();
    for constant in constants {
        if !valid_pipeline_constant_key(constant.key) {
            return Err(Error::InvalidPipelineConstantKey(constant.key.to_owned()));
        }
        if !constant.value.is_finite() {
            return Err(Error::NonFinitePipelineConstant(constant.key.to_owned()));
        }
        if !supplied.insert(constant.key) {
            return Err(Error::DuplicatePipelineConstant(constant.key.to_owned()));
        }
        if !known.contains(constant.key) {
            return Err(Error::UnknownPipelineConstant(constant.key.to_owned()));
        }
    }
    if let Some(missing) = required.iter().find(|key| !supplied.contains(key.as_str())) {
        return Err(Error::MissingPipelineConstant(missing.clone()));
    }
    Ok(())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn valid_pipeline_constant_key(key: &str) -> bool {
    if key.is_empty() || key.len() > MAX_PIPELINE_CONSTANT_KEY_BYTES || !key.is_ascii() {
        return false;
    }
    if key.bytes().all(|byte| byte.is_ascii_digit()) {
        return key
            .parse::<u32>()
            .is_ok_and(|value| value.to_string() == key);
    }
    let mut bytes = key.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn bounded_message(message: String) -> String {
    message.chars().take(4_096).collect()
}

fn naga_ir_items_within_limit(module: &naga::Module, maximum: usize) -> bool {
    let mut items = module
        .types
        .len()
        .saturating_add(module.constants.len())
        .saturating_add(module.overrides.len())
        .saturating_add(module.global_variables.len())
        .saturating_add(module.global_expressions.len())
        .saturating_add(module.functions.len())
        .saturating_add(module.entry_points.len())
        .saturating_add(module.diagnostic_filters.len());
    if items > maximum {
        return false;
    }
    for (_, ty) in module.types.iter() {
        if let naga::TypeInner::Struct { members, .. } = &ty.inner {
            items = items.saturating_add(members.len());
            if items > maximum {
                return false;
            }
        }
    }
    for (_, function) in module.functions.iter() {
        items = items
            .saturating_add(function.arguments.len())
            .saturating_add(function.local_variables.len())
            .saturating_add(function.expressions.len());
        if items > maximum || !naga_statements_within_limit(&function.body, &mut items, maximum) {
            return false;
        }
    }
    for entry in &module.entry_points {
        items = items
            .saturating_add(entry.function.arguments.len())
            .saturating_add(entry.function.local_variables.len())
            .saturating_add(entry.function.expressions.len());
        if items > maximum
            || !naga_statements_within_limit(&entry.function.body, &mut items, maximum)
        {
            return false;
        }
    }
    true
}

fn naga_statements_within_limit(root: &naga::Block, items: &mut usize, maximum: usize) -> bool {
    let mut pending = vec![root];
    while let Some(block) = pending.pop() {
        *items = items.saturating_add(block.len());
        if *items > maximum {
            return false;
        }
        for statement in block.iter() {
            match statement {
                naga::Statement::Block(block) => pending.push(block),
                naga::Statement::If { accept, reject, .. } => {
                    pending.push(accept);
                    pending.push(reject);
                }
                naga::Statement::Switch { cases, .. } => {
                    *items = items.saturating_add(cases.len());
                    if *items > maximum {
                        return false;
                    }
                    pending.extend(cases.iter().map(|case| &case.body));
                }
                naga::Statement::Loop {
                    body, continuing, ..
                } => {
                    pending.push(body);
                    pending.push(continuing);
                }
                _ => {}
            }
        }
    }
    true
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
    fn naga_complexity_limit_counts_nested_statements() {
        let nested =
            naga::Block::from_vec(vec![naga::Statement::Block(naga::Block::from_vec(vec![
                naga::Statement::Block(naga::Block::from_vec(vec![naga::Statement::Break])),
            ]))]);
        let mut items = 0;
        assert!(naga_statements_within_limit(&nested, &mut items, 3));
        assert_eq!(items, 3);

        let mut items = 0;
        assert!(!naga_statements_within_limit(&nested, &mut items, 2));
    }

    #[test]
    fn naga_complexity_limit_counts_struct_members() {
        let mut module = naga::Module::default();
        let scalar = module.types.insert(
            naga::Type {
                name: None,
                inner: naga::TypeInner::Scalar(naga::Scalar {
                    kind: naga::ScalarKind::Uint,
                    width: 4,
                }),
            },
            naga::Span::UNDEFINED,
        );
        let members = (0..3)
            .map(|index| naga::StructMember {
                name: None,
                ty: scalar,
                binding: None,
                offset: index * 4,
            })
            .collect();
        module.types.insert(
            naga::Type {
                name: None,
                inner: naga::TypeInner::Struct { members, span: 12 },
            },
            naga::Span::UNDEFINED,
        );

        assert!(naga_ir_items_within_limit(&module, 5));
        assert!(!naga_ir_items_within_limit(&module, 4));
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
        match validate_dispatch(&descriptor(&bindings)) {
            Err(Error::InvalidBindingCount { actual, maximum }) => {
                assert_eq!(actual, MAX_BINDING_COUNT + 1);
                assert_eq!(maximum, MAX_BINDING_COUNT);
            }
            other => panic!("unexpected validation result: {other:?}"),
        }
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
