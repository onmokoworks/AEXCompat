use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use naga::back::PipelineConstants;
use naga::back::pipeline_constants::process_overrides;
use naga::front::spv;
use naga::valid::{Capabilities, ModuleInfo, ValidationFlags, Validator};
use naga::{AddressSpace, ScalarKind, ShaderStage, TypeInner};

use crate::{KernelArgumentKind, KernelReflection, SpirvModule, SpirvReflection};

const MAX_LOCAL_SIZE_PER_DIMENSION: u32 = 1_024;
const MAX_LOCAL_INVOCATIONS: u32 = 1_024;

/// A single, fully validated compute entry point ready for `ShaderSource::Naga`.
#[derive(Clone, Debug)]
pub struct PreparedEntryPoint {
    pub module: naga::Module,
    pub info: ModuleInfo,
    pub entry_point: String,
    pub local_size: [u32; 3],
    /// Validated byte span for each selected POD uniform descriptor.
    pub pod_uniform_spans: BTreeMap<(u32, u32), u32>,
}

#[derive(Debug)]
pub enum NagaError {
    EmptyEntryPoint,
    InvalidLocalSize {
        local_size: [u32; 3],
        reason: &'static str,
    },
    MissingReflectionKernel {
        entry_point: String,
    },
    RequiredWorkgroupSizeMismatch {
        entry_point: String,
        required: [u32; 3],
        requested: [u32; 3],
    },
    Parse(String),
    MissingEntryPoint {
        entry_point: String,
    },
    AmbiguousEntryPoint {
        entry_point: String,
        count: usize,
    },
    NonComputeEntryPoint {
        entry_point: String,
        stage: ShaderStage,
    },
    WorkgroupSizeOverridesUnsupported {
        entry_point: String,
    },
    ExistingWorkgroupSizeMismatch {
        entry_point: String,
        existing: [u32; 3],
        requested: [u32; 3],
    },
    MissingWorkgroupSizeEvidence {
        entry_point: String,
    },
    SpecOverrideCount {
        spec_id: u32,
        count: usize,
    },
    DuplicateReflectedSpecId {
        spec_id: u32,
    },
    UnreflectedOverride {
        id: Option<u16>,
        name: Option<String>,
    },
    UnsupportedOverrideType {
        spec_id: u32,
    },
    WorkDimRequired {
        spec_id: u32,
    },
    InvalidWorkDim {
        work_dim: u32,
    },
    OverrideProcessing(String),
    OverridesRemain {
        count: usize,
    },
    UnsupportedReflectedArgument {
        kernel: String,
        ordinal: u32,
    },
    DuplicateReflectedBinding {
        kernel: String,
        group: u32,
        binding: u32,
    },
    ConflictingReflectedBindingKinds {
        group: u32,
        binding: u32,
    },
    InvalidPodRange {
        kernel: String,
        ordinal: u32,
        offset: u32,
        size: u32,
    },
    OverlappingPodRanges {
        kernel: String,
        group: u32,
        binding: u32,
    },
    UnexpectedBoundGlobal {
        group: u32,
        binding: u32,
    },
    UnsupportedBoundGlobalSpace {
        group: u32,
        binding: u32,
        space: AddressSpace,
    },
    ReflectedBindingKindMismatch {
        group: u32,
        binding: u32,
        expected: &'static str,
        actual: &'static str,
    },
    UnboundResource {
        space: AddressSpace,
    },
    UnexpectedEntryPointBinding {
        entry_point: String,
        group: u32,
        binding: u32,
    },
    MissingEntryPointBinding {
        entry_point: String,
        group: u32,
        binding: u32,
    },
    DuplicateActiveBinding {
        entry_point: String,
        group: u32,
        binding: u32,
    },
    UniformBindingNotStruct {
        entry_point: String,
        group: u32,
        binding: u32,
    },
    PodRangeExceedsUniformSpan {
        entry_point: String,
        group: u32,
        binding: u32,
        required: u32,
        span: u32,
    },
    UnsupportedPodMemberType {
        entry_point: String,
        group: u32,
        binding: u32,
        offset: u32,
    },
    PodLayoutMismatch {
        entry_point: String,
        group: u32,
        binding: u32,
    },
    Validation(String),
}

impl fmt::Display for NagaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyEntryPoint => write!(formatter, "entry point name is empty"),
            Self::InvalidLocalSize { local_size, reason } => {
                write!(formatter, "invalid local size {local_size:?}: {reason}")
            }
            Self::MissingReflectionKernel { entry_point } => {
                write!(formatter, "reflection has no kernel named {entry_point:?}")
            }
            Self::RequiredWorkgroupSizeMismatch {
                entry_point,
                required,
                requested,
            } => write!(
                formatter,
                "kernel {entry_point:?} requires local size {required:?}, not {requested:?}"
            ),
            Self::Parse(error) => write!(formatter, "strict Naga SPIR-V parse failed: {error}"),
            Self::MissingEntryPoint { entry_point } => {
                write!(formatter, "SPIR-V has no entry point named {entry_point:?}")
            }
            Self::AmbiguousEntryPoint { entry_point, count } => write!(
                formatter,
                "SPIR-V has {count} entry points named {entry_point:?}"
            ),
            Self::NonComputeEntryPoint { entry_point, stage } => write!(
                formatter,
                "entry point {entry_point:?} is {stage:?}, not compute"
            ),
            Self::WorkgroupSizeOverridesUnsupported { entry_point } => write!(
                formatter,
                "entry point {entry_point:?} has unsupported Naga workgroup-size overrides"
            ),
            Self::ExistingWorkgroupSizeMismatch {
                entry_point,
                existing,
                requested,
            } => write!(
                formatter,
                "entry point {entry_point:?} declares local size {existing:?}, not {requested:?}"
            ),
            Self::MissingWorkgroupSizeEvidence { entry_point } => write!(
                formatter,
                "entry point {entry_point:?} has no reflected workgroup-size evidence"
            ),
            Self::SpecOverrideCount { spec_id, count } => write!(
                formatter,
                "clspv SpecId {spec_id} resolves to {count} Naga overrides instead of exactly one"
            ),
            Self::DuplicateReflectedSpecId { spec_id } => {
                write!(formatter, "reflection reuses clspv SpecId {spec_id}")
            }
            Self::UnreflectedOverride { id, name } => write!(
                formatter,
                "Naga module contains unreflected override id={id:?} name={name:?}"
            ),
            Self::UnsupportedOverrideType { spec_id } => write!(
                formatter,
                "clspv SpecId {spec_id} is not a 32-bit unsigned Naga scalar"
            ),
            Self::WorkDimRequired { spec_id } => write!(
                formatter,
                "module requires an observed OpenCL work_dim for SpecId {spec_id}"
            ),
            Self::InvalidWorkDim { work_dim } => {
                write!(formatter, "OpenCL work_dim {work_dim} is outside 1..=3")
            }
            Self::OverrideProcessing(error) => {
                write!(formatter, "Naga override freezing failed: {error}")
            }
            Self::OverridesRemain { count } => write!(
                formatter,
                "Naga override freezing left {count} pipeline overrides"
            ),
            Self::UnsupportedReflectedArgument { kernel, ordinal } => write!(
                formatter,
                "kernel {kernel:?} argument {ordinal} is outside the supported reflection ABI"
            ),
            Self::DuplicateReflectedBinding {
                kernel,
                group,
                binding,
            } => write!(
                formatter,
                "kernel {kernel:?} reuses descriptor {group}:{binding} outside a clustered POD UBO"
            ),
            Self::ConflictingReflectedBindingKinds { group, binding } => write!(
                formatter,
                "reflection assigns incompatible resource kinds to descriptor {group}:{binding}"
            ),
            Self::InvalidPodRange {
                kernel,
                ordinal,
                offset,
                size,
            } => write!(
                formatter,
                "kernel {kernel:?} POD argument {ordinal} has invalid range {offset}+{size}"
            ),
            Self::OverlappingPodRanges {
                kernel,
                group,
                binding,
            } => write!(
                formatter,
                "kernel {kernel:?} has overlapping POD ranges at descriptor {group}:{binding}"
            ),
            Self::UnexpectedBoundGlobal { group, binding } => write!(
                formatter,
                "Naga module contains unreflected descriptor {group}:{binding}"
            ),
            Self::UnsupportedBoundGlobalSpace {
                group,
                binding,
                space,
            } => write!(
                formatter,
                "descriptor {group}:{binding} uses unsupported address space {space:?}"
            ),
            Self::ReflectedBindingKindMismatch {
                group,
                binding,
                expected,
                actual,
            } => write!(
                formatter,
                "descriptor {group}:{binding} is reflected as {expected}, but Naga reports {actual}"
            ),
            Self::UnboundResource { space } => {
                write!(
                    formatter,
                    "Naga module contains an unbound {space:?} resource"
                )
            }
            Self::UnexpectedEntryPointBinding {
                entry_point,
                group,
                binding,
            } => write!(
                formatter,
                "entry point {entry_point:?} uses descriptor {group}:{binding}, which belongs to another kernel"
            ),
            Self::MissingEntryPointBinding {
                entry_point,
                group,
                binding,
            } => write!(
                formatter,
                "entry point {entry_point:?} does not use reflected descriptor {group}:{binding}"
            ),
            Self::DuplicateActiveBinding {
                entry_point,
                group,
                binding,
            } => write!(
                formatter,
                "entry point {entry_point:?} resolves descriptor {group}:{binding} to multiple globals"
            ),
            Self::UniformBindingNotStruct {
                entry_point,
                group,
                binding,
            } => write!(
                formatter,
                "entry point {entry_point:?} POD descriptor {group}:{binding} is not a Naga struct"
            ),
            Self::PodRangeExceedsUniformSpan {
                entry_point,
                group,
                binding,
                required,
                span,
            } => write!(
                formatter,
                "entry point {entry_point:?} POD descriptor {group}:{binding} needs {required} bytes but its Naga struct spans {span}"
            ),
            Self::UnsupportedPodMemberType {
                entry_point,
                group,
                binding,
                offset,
            } => write!(
                formatter,
                "entry point {entry_point:?} POD descriptor {group}:{binding} has a non-scalar member at byte {offset}"
            ),
            Self::PodLayoutMismatch {
                entry_point,
                group,
                binding,
            } => write!(
                formatter,
                "entry point {entry_point:?} POD descriptor {group}:{binding} does not exactly match reflected offset/size ranges"
            ),
            Self::Validation(error) => {
                write!(formatter, "patched Naga module validation failed: {error}")
            }
        }
    }
}

impl Error for NagaError {}

/// Strictly parses a reflection-stripped clspv module and prepares one compute
/// entry point for an observed OpenCL local size.
pub fn prepare_entry_point(
    spirv: &SpirvModule,
    entry_point: &str,
    local_size: [u32; 3],
) -> Result<PreparedEntryPoint, NagaError> {
    prepare_entry_point_inner(spirv, entry_point, None, local_size)
}

/// Prepares an entry point whose reachable clspv ABI includes `get_work_dim`.
pub fn prepare_entry_point_for_dispatch(
    spirv: &SpirvModule,
    entry_point: &str,
    work_dim: u32,
    local_size: [u32; 3],
) -> Result<PreparedEntryPoint, NagaError> {
    if !(1..=3).contains(&work_dim) {
        return Err(NagaError::InvalidWorkDim { work_dim });
    }
    if local_size[work_dim as usize..]
        .iter()
        .any(|&size| size != 1)
    {
        return Err(NagaError::InvalidLocalSize {
            local_size,
            reason: "dimensions above work_dim must be one",
        });
    }
    prepare_entry_point_inner(spirv, entry_point, Some(work_dim), local_size)
}

fn prepare_entry_point_inner(
    spirv: &SpirvModule,
    entry_point: &str,
    work_dim: Option<u32>,
    local_size: [u32; 3],
) -> Result<PreparedEntryPoint, NagaError> {
    if entry_point.is_empty() {
        return Err(NagaError::EmptyEntryPoint);
    }
    validate_local_size(local_size)?;

    let reflection = spirv.reflection();
    let reflected_kernel =
        reflection
            .kernel(entry_point)
            .ok_or_else(|| NagaError::MissingReflectionKernel {
                entry_point: entry_point.to_owned(),
            })?;
    if let Some(required) = reflected_kernel.required_workgroup_size
        && required != local_size
    {
        return Err(NagaError::RequiredWorkgroupSizeMismatch {
            entry_point: entry_point.to_owned(),
            required,
            requested: local_size,
        });
    }
    if let Some(spec_id) = reflection.work_dim_spec_id()
        && work_dim.is_none()
    {
        return Err(NagaError::WorkDimRequired { spec_id });
    }

    let options = spv::Options {
        adjust_coordinate_space: false,
        strict_capabilities: true,
        block_ctx_dump_prefix: None,
    };
    let mut module = spv::Frontend::new(spirv.stripped_words().iter().copied(), &options)
        .parse()
        .map_err(|error| NagaError::Parse(error.to_string()))?;

    let has_reflected_workgroup_size = reflected_kernel.required_workgroup_size.is_some()
        || reflection.workgroup_spec_ids().is_some();
    retain_and_patch_entry_point(
        &mut module,
        entry_point,
        local_size,
        has_reflected_workgroup_size,
    )?;

    // Naga 24 represents clspv's WorkgroupSize OpSpecConstantComposite as a
    // Constant whose initializer refers to overrides. It becomes an ordinary
    // constant after freezing, but cannot pass CONSTANTS validation before
    // that point. Validate every other IR class first, then perform full
    // validation on the frozen, override-free module below.
    let pipeline_constants = clspv_pipeline_constants(reflection, &module, work_dim, local_size)?;
    let pre_freeze_flags = ValidationFlags::all().difference(ValidationFlags::CONSTANTS);
    let mut validator = Validator::new(pre_freeze_flags, Capabilities::empty());
    let initial_info = validator
        .validate(&module)
        .map_err(|error| NagaError::Validation(format!("{error:?}")))?;

    let (frozen_module, _) = process_overrides(&module, &initial_info, &pipeline_constants)
        .map_err(|error| NagaError::OverrideProcessing(error.to_string()))?;
    let module = frozen_module.into_owned();
    if !module.overrides.is_empty() {
        return Err(NagaError::OverridesRemain {
            count: module.overrides.len(),
        });
    }

    let mut validator = Validator::new(ValidationFlags::all(), Capabilities::empty());
    let info = validator
        .validate_no_overrides(&module)
        .map_err(|error| NagaError::Validation(format!("{error:?}")))?;

    let pod_uniform_spans =
        validate_reflected_resources(&module, &info, reflection, reflected_kernel)?;

    Ok(PreparedEntryPoint {
        module,
        info,
        entry_point: entry_point.to_owned(),
        local_size,
        pod_uniform_spans,
    })
}

fn clspv_pipeline_constants(
    reflection: &SpirvReflection,
    module: &naga::Module,
    work_dim: Option<u32>,
    local_size: [u32; 3],
) -> Result<PipelineConstants, NagaError> {
    let mut expected = BTreeMap::new();
    if let Some(spec_ids) = reflection.workgroup_spec_ids() {
        for (spec_id, value) in [
            (spec_ids.x, local_size[0]),
            (spec_ids.y, local_size[1]),
            (spec_ids.z, local_size[2]),
        ] {
            if expected.insert(spec_id, value).is_some() {
                return Err(NagaError::DuplicateReflectedSpecId { spec_id });
            }
        }
    }
    if let Some(spec_id) = reflection.work_dim_spec_id() {
        let value = work_dim.ok_or(NagaError::WorkDimRequired { spec_id })?;
        if expected.insert(spec_id, value).is_some() {
            return Err(NagaError::DuplicateReflectedSpecId { spec_id });
        }
    }

    let mut actual_counts = BTreeMap::<u32, usize>::new();
    for (_, override_) in module.overrides.iter() {
        let Some(id) = override_.id else {
            return Err(NagaError::UnreflectedOverride {
                id: None,
                name: override_.name.clone(),
            });
        };
        let spec_id = u32::from(id);
        if !expected.contains_key(&spec_id) {
            return Err(NagaError::UnreflectedOverride {
                id: Some(id),
                name: override_.name.clone(),
            });
        }
        match &module.types[override_.ty].inner {
            TypeInner::Scalar(scalar) if scalar.kind == ScalarKind::Uint && scalar.width == 4 => {}
            _ => return Err(NagaError::UnsupportedOverrideType { spec_id }),
        }
        *actual_counts.entry(spec_id).or_default() += 1;
    }

    let mut constants = PipelineConstants::new();
    for (spec_id, value) in expected {
        let count = actual_counts.get(&spec_id).copied().unwrap_or(0);
        if count != 1 {
            return Err(NagaError::SpecOverrideCount { spec_id, count });
        }
        constants.insert(spec_id.to_string(), f64::from(value));
    }
    Ok(constants)
}

fn validate_local_size(local_size: [u32; 3]) -> Result<(), NagaError> {
    if local_size.contains(&0) {
        return Err(NagaError::InvalidLocalSize {
            local_size,
            reason: "all dimensions must be nonzero",
        });
    }
    if local_size
        .iter()
        .any(|&size| size > MAX_LOCAL_SIZE_PER_DIMENSION)
    {
        return Err(NagaError::InvalidLocalSize {
            local_size,
            reason: "a dimension exceeds the preparation bound",
        });
    }
    let product = local_size.into_iter().try_fold(1_u32, u32::checked_mul);
    if product.is_none_or(|product| product > MAX_LOCAL_INVOCATIONS) {
        return Err(NagaError::InvalidLocalSize {
            local_size,
            reason: "the invocation product exceeds the preparation bound",
        });
    }
    Ok(())
}

fn retain_and_patch_entry_point(
    module: &mut naga::Module,
    entry_point: &str,
    local_size: [u32; 3],
    has_reflected_workgroup_size: bool,
) -> Result<(), NagaError> {
    let matching: Vec<_> = module
        .entry_points
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| (entry.name == entry_point).then_some(index))
        .collect();
    let index = match matching.as_slice() {
        [] => {
            return Err(NagaError::MissingEntryPoint {
                entry_point: entry_point.to_owned(),
            });
        }
        [index] => *index,
        _ => {
            return Err(NagaError::AmbiguousEntryPoint {
                entry_point: entry_point.to_owned(),
                count: matching.len(),
            });
        }
    };

    let mut selected = module.entry_points.remove(index);
    if selected.stage != ShaderStage::Compute {
        return Err(NagaError::NonComputeEntryPoint {
            entry_point: entry_point.to_owned(),
            stage: selected.stage,
        });
    }
    if selected.workgroup_size_overrides.is_some() {
        return Err(NagaError::WorkgroupSizeOverridesUnsupported {
            entry_point: entry_point.to_owned(),
        });
    }
    if selected.workgroup_size == [0, 0, 0] && !has_reflected_workgroup_size {
        return Err(NagaError::MissingWorkgroupSizeEvidence {
            entry_point: entry_point.to_owned(),
        });
    }
    if selected.workgroup_size != [0, 0, 0] && selected.workgroup_size != local_size {
        return Err(NagaError::ExistingWorkgroupSizeMismatch {
            entry_point: entry_point.to_owned(),
            existing: selected.workgroup_size,
            requested: local_size,
        });
    }

    selected.workgroup_size = local_size;
    module.entry_points.clear();
    module.entry_points.push(selected);
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ReflectedResourceKind {
    StorageBuffer,
    PodUniform,
}

impl ReflectedResourceKind {
    const fn label(self) -> &'static str {
        match self {
            Self::StorageBuffer => "storage buffer",
            Self::PodUniform => "POD uniform buffer",
        }
    }
}

fn reflected_resource_kind(
    kernel: &KernelReflection,
    ordinal: u32,
    kind: &KernelArgumentKind,
) -> Result<ReflectedResourceKind, NagaError> {
    match kind {
        KernelArgumentKind::StorageBuffer => Ok(ReflectedResourceKind::StorageBuffer),
        KernelArgumentKind::PodUniform { .. } => Ok(ReflectedResourceKind::PodUniform),
        #[allow(unreachable_patterns)]
        _ => Err(NagaError::UnsupportedReflectedArgument {
            kernel: kernel.name.clone(),
            ordinal,
        }),
    }
}

fn reflection_binding_maps(
    reflection: &SpirvReflection,
    selected: &KernelReflection,
) -> Result<
    (
        BTreeMap<(u32, u32), BTreeSet<ReflectedResourceKind>>,
        BTreeMap<(u32, u32), ReflectedResourceKind>,
        BTreeMap<(u32, u32), Vec<(u32, u32)>>,
    ),
    NagaError,
> {
    let mut union = BTreeMap::new();
    let mut selected_bindings = BTreeMap::new();
    let mut selected_pod_ranges = BTreeMap::new();

    for kernel in reflection.kernels() {
        let mut kernel_bindings = BTreeMap::new();
        let mut pod_ranges: BTreeMap<(u32, u32), Vec<(u32, u32)>> = BTreeMap::new();

        for argument in &kernel.arguments {
            let key = (argument.descriptor_set, argument.binding);
            let kind = reflected_resource_kind(kernel, argument.ordinal, &argument.kind)?;

            if let Some(previous) = kernel_bindings.insert(key, kind)
                && !(previous == ReflectedResourceKind::PodUniform
                    && kind == ReflectedResourceKind::PodUniform)
            {
                return Err(NagaError::DuplicateReflectedBinding {
                    kernel: kernel.name.clone(),
                    group: key.0,
                    binding: key.1,
                });
            }

            union.entry(key).or_insert_with(BTreeSet::new).insert(kind);

            if let KernelArgumentKind::PodUniform { offset, size } = &argument.kind {
                let (offset, size) = (*offset, *size);
                let end = offset
                    .checked_add(size)
                    .filter(|_| size != 0)
                    .ok_or_else(|| NagaError::InvalidPodRange {
                        kernel: kernel.name.clone(),
                        ordinal: argument.ordinal,
                        offset,
                        size,
                    })?;
                pod_ranges.entry(key).or_default().push((offset, end));
                if kernel.name == selected.name {
                    selected_pod_ranges
                        .entry(key)
                        .or_insert_with(Vec::new)
                        .push((offset, size));
                }
            }

            if kernel.name == selected.name {
                selected_bindings.insert(key, kind);
            }
        }

        for (key, ranges) in &mut pod_ranges {
            ranges.sort_unstable();
            if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
                return Err(NagaError::OverlappingPodRanges {
                    kernel: kernel.name.clone(),
                    group: key.0,
                    binding: key.1,
                });
            }
        }
    }

    Ok((union, selected_bindings, selected_pod_ranges))
}

fn global_resource_kind(space: AddressSpace) -> Option<ReflectedResourceKind> {
    match space {
        AddressSpace::Storage { .. } => Some(ReflectedResourceKind::StorageBuffer),
        AddressSpace::Uniform => Some(ReflectedResourceKind::PodUniform),
        _ => None,
    }
}

fn is_unbound_resource_space(space: AddressSpace) -> bool {
    matches!(
        space,
        AddressSpace::Uniform
            | AddressSpace::Storage { .. }
            | AddressSpace::Handle
            | AddressSpace::PushConstant
    )
}

fn validate_reflected_resources(
    module: &naga::Module,
    info: &ModuleInfo,
    reflection: &SpirvReflection,
    selected: &KernelReflection,
) -> Result<BTreeMap<(u32, u32), u32>, NagaError> {
    let (union, selected_bindings, selected_pod_ranges) =
        reflection_binding_maps(reflection, selected)?;
    let entry_info = info.get_entry_point(0);
    let entry_point = selected.name.clone();
    let mut active_counts: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    let mut uniform_spans = BTreeMap::new();

    for (handle, global) in module.global_variables.iter() {
        match &global.binding {
            Some(resource) => {
                let key = (resource.group, resource.binding);
                let expected = union.get(&key).ok_or(NagaError::UnexpectedBoundGlobal {
                    group: key.0,
                    binding: key.1,
                })?;
                let actual = global_resource_kind(global.space).ok_or(
                    NagaError::UnsupportedBoundGlobalSpace {
                        group: key.0,
                        binding: key.1,
                        space: global.space,
                    },
                )?;
                if !expected.contains(&actual) {
                    return Err(NagaError::ReflectedBindingKindMismatch {
                        group: key.0,
                        binding: key.1,
                        expected: "one of the reflected per-kernel resource kinds",
                        actual: actual.label(),
                    });
                }

                if !entry_info[handle].is_empty() {
                    if selected_bindings.get(&key) != Some(&actual) {
                        return Err(NagaError::UnexpectedEntryPointBinding {
                            entry_point: entry_point.clone(),
                            group: key.0,
                            binding: key.1,
                        });
                    }
                    *active_counts.entry(key).or_default() += 1;
                }
            }
            None if is_unbound_resource_space(global.space) => {
                return Err(NagaError::UnboundResource {
                    space: global.space,
                });
            }
            None => {}
        }
    }

    for (&key, &kind) in &selected_bindings {
        match active_counts.get(&key).copied().unwrap_or(0) {
            0 => {
                return Err(NagaError::MissingEntryPointBinding {
                    entry_point: entry_point.clone(),
                    group: key.0,
                    binding: key.1,
                });
            }
            1 => {}
            _ => {
                return Err(NagaError::DuplicateActiveBinding {
                    entry_point: entry_point.clone(),
                    group: key.0,
                    binding: key.1,
                });
            }
        }

        if kind == ReflectedResourceKind::PodUniform {
            let (_, global) = module
                .global_variables
                .iter()
                .find(|(handle, global)| {
                    !entry_info[*handle].is_empty()
                        && global.binding.as_ref().is_some_and(|binding| {
                            binding.group == key.0 && binding.binding == key.1
                        })
                })
                .ok_or_else(|| NagaError::MissingEntryPointBinding {
                    entry_point: entry_point.clone(),
                    group: key.0,
                    binding: key.1,
                })?;
            let span = match &module.types[global.ty].inner {
                TypeInner::Struct { span, .. } => *span,
                _ => {
                    return Err(NagaError::UniformBindingNotStruct {
                        entry_point: entry_point.clone(),
                        group: key.0,
                        binding: key.1,
                    });
                }
            };
            let reflected_ranges = selected_pod_ranges.get(&key).cloned().unwrap_or_default();
            let required = reflected_ranges
                .iter()
                .filter_map(|&(offset, size)| offset.checked_add(size))
                .max()
                .unwrap_or(0);
            if required > span {
                return Err(NagaError::PodRangeExceedsUniformSpan {
                    entry_point: entry_point.clone(),
                    group: key.0,
                    binding: key.1,
                    required,
                    span,
                });
            }
            let mut naga_ranges = Vec::new();
            collect_pod_scalar_ranges(
                module,
                global.ty,
                0,
                0,
                &entry_point,
                key,
                &mut naga_ranges,
            )?;
            let mut reflected_ranges = reflected_ranges;
            reflected_ranges.sort_unstable();
            naga_ranges.sort_unstable();
            if reflected_ranges != naga_ranges {
                return Err(NagaError::PodLayoutMismatch {
                    entry_point: entry_point.clone(),
                    group: key.0,
                    binding: key.1,
                });
            }
            uniform_spans.insert(key, span);
        }
    }

    Ok(uniform_spans)
}

fn collect_pod_scalar_ranges(
    module: &naga::Module,
    ty: naga::Handle<naga::Type>,
    base_offset: u32,
    depth: u8,
    entry_point: &str,
    key: (u32, u32),
    output: &mut Vec<(u32, u32)>,
) -> Result<(), NagaError> {
    if depth > 16 {
        return Err(NagaError::PodLayoutMismatch {
            entry_point: entry_point.to_owned(),
            group: key.0,
            binding: key.1,
        });
    }
    match &module.types[ty].inner {
        TypeInner::Struct { members, .. } => {
            for member in members {
                let offset = base_offset.checked_add(member.offset).ok_or_else(|| {
                    NagaError::PodLayoutMismatch {
                        entry_point: entry_point.to_owned(),
                        group: key.0,
                        binding: key.1,
                    }
                })?;
                collect_pod_scalar_ranges(
                    module,
                    member.ty,
                    offset,
                    depth + 1,
                    entry_point,
                    key,
                    output,
                )?;
            }
            Ok(())
        }
        TypeInner::Scalar(scalar)
            if matches!(
                scalar.kind,
                ScalarKind::Sint | ScalarKind::Uint | ScalarKind::Float
            ) && scalar.width != 0 =>
        {
            output.push((base_offset, u32::from(scalar.width)));
            Ok(())
        }
        _ => Err(NagaError::UnsupportedPodMemberType {
            entry_point: entry_point.to_owned(),
            group: key.0,
            binding: key.1,
            offset: base_offset,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compute_entry_point(name: &str, workgroup_size: [u32; 3]) -> naga::EntryPoint {
        naga::EntryPoint {
            name: name.to_owned(),
            stage: ShaderStage::Compute,
            early_depth_test: None,
            workgroup_size,
            workgroup_size_overrides: None,
            function: naga::Function::default(),
        }
    }

    #[test]
    fn local_size_is_nonzero_and_bounded() {
        assert!(validate_local_size([16, 16, 1]).is_ok());
        assert!(matches!(
            validate_local_size([16, 0, 1]),
            Err(NagaError::InvalidLocalSize { .. })
        ));
        assert!(matches!(
            validate_local_size([1_025, 1, 1]),
            Err(NagaError::InvalidLocalSize { .. })
        ));
        assert!(matches!(
            validate_local_size([33, 32, 1]),
            Err(NagaError::InvalidLocalSize { .. })
        ));
    }

    #[test]
    fn retains_and_patches_only_the_selected_compute_entry_point() {
        let mut module = naga::Module::default();
        module
            .entry_points
            .push(compute_entry_point("first", [0, 0, 0]));
        module
            .entry_points
            .push(compute_entry_point("second", [0, 0, 0]));

        retain_and_patch_entry_point(&mut module, "second", [16, 16, 1], true).unwrap();

        assert_eq!(module.entry_points.len(), 1);
        assert_eq!(module.entry_points[0].name, "second");
        assert_eq!(module.entry_points[0].workgroup_size, [16, 16, 1]);
    }

    #[test]
    fn rejects_a_conflicting_literal_workgroup_size() {
        let mut module = naga::Module::default();
        module
            .entry_points
            .push(compute_entry_point("kernel", [8, 8, 1]));

        assert!(matches!(
            retain_and_patch_entry_point(&mut module, "kernel", [16, 16, 1], true),
            Err(NagaError::ExistingWorkgroupSizeMismatch { .. })
        ));
    }

    #[test]
    fn refuses_to_invent_an_unreflected_workgroup_size() {
        let mut module = naga::Module::default();
        module
            .entry_points
            .push(compute_entry_point("kernel", [0, 0, 0]));

        assert!(matches!(
            retain_and_patch_entry_point(&mut module, "kernel", [16, 16, 1], false),
            Err(NagaError::MissingWorkgroupSizeEvidence { .. })
        ));
    }

    #[test]
    #[ignore = "requires AEX_CLSPV_FIXTURE_SPV pointing to a clspv POD-UBO module"]
    fn prepares_external_clspv_fixture() {
        let path = std::env::var_os("AEX_CLSPV_FIXTURE_SPV").expect("fixture path is required");
        let bytes = std::fs::read(path).unwrap();
        let spirv = SpirvModule::parse_bytes(&bytes).unwrap();

        for entry_point in ["InvertColorKernel", "ProcAmp2Kernel"] {
            let prepared = prepare_entry_point(&spirv, entry_point, [16, 16, 1]).unwrap();
            assert_eq!(prepared.entry_point, entry_point);
            assert_eq!(prepared.local_size, [16, 16, 1]);
            assert_eq!(prepared.module.entry_points.len(), 1);
            assert!(prepared.module.overrides.is_empty());
            assert!(prepared.pod_uniform_spans.contains_key(&(0, 2)));
        }
    }
}
