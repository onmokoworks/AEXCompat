use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use aex_clspv::{
    CompileRequest, CompilerIdentity, CompilerOutput, KernelArgumentKind, KernelReflection,
    PinnedCompiler, SourceIdentity, ValidatedArtifact, validate_precompiled,
};
use aex_wgpu_compute::{
    BindingAccess, BufferBinding, DispatchReport, NagaDispatchDescriptor, ObjectCounts, Session,
    ValidatedNagaModule,
};
use sha2::{Digest, Sha256};

use crate::gpu_lifecycle::{
    WgpuAdapterEvidence, WgpuArtifactEvidence, WgpuDispatchEvidence, WgpuResourceCounts,
    WgpuRuntimeEvidence,
};

use super::{
    CL_INVALID_ARG_INDEX, CL_INVALID_ARG_SIZE, CL_INVALID_ARG_VALUE, CL_INVALID_GLOBAL_OFFSET,
    CL_INVALID_GLOBAL_WORK_SIZE, CL_INVALID_KERNEL_ARGS, CL_INVALID_PROGRAM_EXECUTABLE,
    CL_INVALID_WORK_DIMENSION, CL_INVALID_WORK_GROUP_SIZE, OpenClRuntimeError,
};

const LOCAL_COMPILER_PATH_ENV: &str = "AEXCOMPAT_CLSPV_PATH";
const LOCAL_COMPILER_SHA256_ENV: &str = "AEXCOMPAT_CLSPV_SHA256";
const PRECOMPILED_SPIRV_ENV: &str = "AEXCOMPAT_WGPU_PRECOMPILED_SPIRV";
const PRECOMPILED_SOURCE_SHA256_ENV: &str = "AEXCOMPAT_WGPU_PRECOMPILED_SOURCE_SHA256";
const PRECOMPILED_SPIRV_SHA256_ENV: &str = "AEXCOMPAT_WGPU_PRECOMPILED_SPIRV_SHA256";
const PRECOMPILED_COMPILER_SHA256_ENV: &str = "AEXCOMPAT_WGPU_PRECOMPILED_COMPILER_SHA256";
const MAX_PRECOMPILED_SPIRV_BYTES: usize = 16 * 1024 * 1024;
const MAX_WGPU_ARTIFACT_EVIDENCE: usize = 256;
const MAX_WGPU_DISPATCH_EVIDENCE: usize = 1_024;
const ALLOWED_GUEST_OPTIONS: [&str; 2] = ["-cl-single-precision-constant", "-cl-fast-relaxed-math"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum WgpuCompilerConfig {
    Local {
        compiler_path: PathBuf,
        compiler_sha256: [u8; 32],
    },
    Precompiled {
        spirv_path: PathBuf,
        source_sha256: [u8; 32],
        spirv_sha256: [u8; 32],
        compiler_sha256: [u8; 32],
    },
}

impl WgpuCompilerConfig {
    pub(super) fn from_process_env() -> Result<Self, String> {
        Self::from_lookup(|key| env::var_os(key))
    }

    fn from_lookup(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Result<Self, String> {
        let local_path = read_env_value(&mut lookup, LOCAL_COMPILER_PATH_ENV)?;
        let local_sha = read_env_value(&mut lookup, LOCAL_COMPILER_SHA256_ENV)?;
        let precompiled_path = read_env_value(&mut lookup, PRECOMPILED_SPIRV_ENV)?;
        let precompiled_source = read_env_value(&mut lookup, PRECOMPILED_SOURCE_SHA256_ENV)?;
        let precompiled_spirv = read_env_value(&mut lookup, PRECOMPILED_SPIRV_SHA256_ENV)?;
        let precompiled_compiler = read_env_value(&mut lookup, PRECOMPILED_COMPILER_SHA256_ENV)?;

        let local_present = local_path.is_some() || local_sha.is_some();
        let precompiled_present = precompiled_path.is_some()
            || precompiled_source.is_some()
            || precompiled_spirv.is_some()
            || precompiled_compiler.is_some();
        if local_present && precompiled_present {
            return Err("wgpu compiler configuration sets both local and precompiled modes".into());
        }
        if !local_present && !precompiled_present {
            return Err(
                "wgpu compiler configuration is missing; select one pinned toolchain mode".into(),
            );
        }

        if local_present {
            let compiler_path = required_env_path(local_path, LOCAL_COMPILER_PATH_ENV)?;
            let compiler_sha256 = required_env_sha256(local_sha, LOCAL_COMPILER_SHA256_ENV)?;
            return Ok(Self::Local {
                compiler_path,
                compiler_sha256,
            });
        }

        Ok(Self::Precompiled {
            spirv_path: required_env_path(precompiled_path, PRECOMPILED_SPIRV_ENV)?,
            source_sha256: required_env_sha256(precompiled_source, PRECOMPILED_SOURCE_SHA256_ENV)?,
            spirv_sha256: required_env_sha256(precompiled_spirv, PRECOMPILED_SPIRV_SHA256_ENV)?,
            compiler_sha256: required_env_sha256(
                precompiled_compiler,
                PRECOMPILED_COMPILER_SHA256_ENV,
            )?,
        })
    }
}

fn read_env_value(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
    key: &str,
) -> Result<Option<String>, String> {
    lookup(key)
        .map(|value| {
            value
                .into_string()
                .map_err(|_| format!("{key} is not valid Unicode"))
        })
        .transpose()
}

fn required_env_path(value: Option<String>, key: &str) -> Result<PathBuf, String> {
    let value = value.ok_or_else(|| format!("{key} is required for the selected wgpu mode"))?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!("{key} must be an absolute path"));
    }
    Ok(path)
}

fn required_env_sha256(value: Option<String>, key: &str) -> Result<[u8; 32], String> {
    let value = value.ok_or_else(|| format!("{key} is required for the selected wgpu mode"))?;
    parse_sha256(&value).map_err(|detail| format!("{key}: {detail}"))
}

fn parse_sha256(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 || !value.is_ascii() {
        return Err("SHA-256 must contain exactly 64 hexadecimal characters".into());
    }
    let mut digest = [0u8; 32];
    for (index, slot) in digest.iter_mut().enumerate() {
        let offset = index * 2;
        *slot = u8::from_str_radix(&value[offset..offset + 2], 16)
            .map_err(|_| "SHA-256 contains a non-hexadecimal character".to_string())?;
    }
    Ok(digest)
}

enum WgpuToolchain {
    Local(PinnedCompiler),
    Precompiled {
        raw_spirv: Vec<u8>,
        source_sha256: [u8; 32],
        compiler: CompilerIdentity,
    },
}

pub(super) struct WgpuExecutor {
    session: Session,
    toolchain: WgpuToolchain,
}

pub(super) struct WgpuProgramBuild {
    pub(super) artifact: ValidatedArtifact,
    pub(super) evidence: WgpuArtifactEvidence,
}

impl WgpuExecutor {
    pub(super) fn new(device_index: u32, config: WgpuCompilerConfig) -> Result<Self, String> {
        let toolchain = match config {
            WgpuCompilerConfig::Local {
                compiler_path,
                compiler_sha256,
            } => WgpuToolchain::Local(
                PinnedCompiler::with_expected_sha256(compiler_path, compiler_sha256)
                    .map_err(|error| error.to_string())?,
            ),
            WgpuCompilerConfig::Precompiled {
                spirv_path,
                source_sha256,
                spirv_sha256,
                compiler_sha256,
            } => {
                let raw_spirv = read_precompiled_spirv(&spirv_path)?;
                let actual_sha256: [u8; 32] = Sha256::digest(&raw_spirv).into();
                if actual_sha256 != spirv_sha256 {
                    return Err(format!(
                        "precompiled SPIR-V SHA-256 mismatch: expected {}, actual {}",
                        hex_digest(&spirv_sha256),
                        hex_digest(&actual_sha256)
                    ));
                }
                WgpuToolchain::Precompiled {
                    raw_spirv,
                    source_sha256,
                    compiler: CompilerIdentity::for_precompiled(compiler_sha256),
                }
            }
        };
        let session =
            Session::select_metal(device_index as usize).map_err(|error| error.to_string())?;
        Ok(Self { session, toolchain })
    }

    pub(super) fn initial_evidence(&self) -> WgpuRuntimeEvidence {
        let adapter = self.session.adapter_report();
        WgpuRuntimeEvidence {
            executor_available: true,
            adapter: Some(WgpuAdapterEvidence {
                index: adapter.index,
                name: adapter.name.clone(),
                backend: adapter.backend.clone(),
                device_type: adapter.device_type.clone(),
                vendor: adapter.vendor,
                device: adapter.device,
                driver: adapter.driver.clone(),
                driver_info: adapter.driver_info.clone(),
            }),
            ..WgpuRuntimeEvidence::default()
        }
    }

    pub(super) fn build_program(
        &self,
        source: &str,
        build_options: Option<&str>,
    ) -> Result<WgpuProgramBuild, String> {
        let guest_options = normalize_guest_build_options(build_options)?;
        let source_identity = SourceIdentity::from_source(source.as_bytes());
        let request = CompileRequest::new(source.as_bytes().to_vec(), guest_options);
        let (artifact, mode) = match &self.toolchain {
            WgpuToolchain::Local(compiler) => {
                let output: CompilerOutput = compiler
                    .compile(&request)
                    .map_err(|error| error.to_string())?;
                (
                    validate_precompiled(output.into()).map_err(|error| error.to_string())?,
                    "local-pinned",
                )
            }
            WgpuToolchain::Precompiled {
                raw_spirv,
                source_sha256,
                compiler,
            } => {
                if source_identity.as_bytes() != source_sha256 {
                    return Err(format!(
                        "precompiled source SHA-256 mismatch: expected {}, actual {}",
                        hex_digest(source_sha256),
                        source_identity.to_hex()
                    ));
                }
                let output =
                    CompilerOutput::from_precompiled(raw_spirv.clone(), &request, compiler.clone())
                        .map_err(|error| error.to_string())?;
                (
                    validate_precompiled(output.into()).map_err(|error| error.to_string())?,
                    "precompiled-pinned",
                )
            }
        };
        let provenance = artifact.provenance();
        let evidence = WgpuArtifactEvidence {
            toolchain_mode: mode.into(),
            source_sha256: provenance.source.to_hex(),
            compiler_sha256: provenance.compiler.binary_sha256_hex(),
            raw_spirv_sha256: hex_digest(artifact.raw_spirv_sha256()),
            normalized_options: provenance.normalized_options.clone(),
        };
        Ok(WgpuProgramBuild { artifact, evidence })
    }

    pub(super) fn dispatch(
        &self,
        plan: &WgpuDispatchPlan,
    ) -> Result<DispatchReport, aex_wgpu_compute::Error> {
        let bindings = plan
            .bindings
            .iter()
            .map(|binding| BufferBinding {
                binding: binding.binding,
                access: binding.access,
                bytes: &binding.bytes,
            })
            .collect::<Vec<_>>();
        self.session.dispatch_naga(
            &plan.module,
            &NagaDispatchDescriptor {
                label: Some("AEXCompat OpenCL compatibility dispatch"),
                entry_point: &plan.entry_point,
                bindings: &bindings,
                constants: &[],
                workgroups: plan.geometry.groups,
            },
        )
    }

    pub(super) fn live_objects(&self) -> ObjectCounts {
        self.session.live_objects()
    }
}

fn read_precompiled_spirv(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("precompiled SPIR-V is unavailable: {error}"))?;
    if !metadata.is_file() {
        return Err("precompiled SPIR-V is not a regular file".into());
    }
    if metadata.len() > MAX_PRECOMPILED_SPIRV_BYTES as u64 {
        return Err(format!(
            "precompiled SPIR-V exceeds {MAX_PRECOMPILED_SPIRV_BYTES} bytes"
        ));
    }
    let file =
        File::open(path).map_err(|error| format!("opening precompiled SPIR-V failed: {error}"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_PRECOMPILED_SPIRV_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("reading precompiled SPIR-V failed: {error}"))?;
    if bytes.len() > MAX_PRECOMPILED_SPIRV_BYTES {
        return Err(format!(
            "precompiled SPIR-V exceeds {MAX_PRECOMPILED_SPIRV_BYTES} bytes"
        ));
    }
    Ok(bytes)
}

fn normalize_guest_build_options(options: Option<&str>) -> Result<Vec<String>, String> {
    let mut selected = [false; ALLOWED_GUEST_OPTIONS.len()];
    for option in options.unwrap_or_default().split_whitespace() {
        let Some(index) = ALLOWED_GUEST_OPTIONS
            .iter()
            .position(|allowed| *allowed == option)
        else {
            return Err(format!(
                "unsupported OpenCL build option for wgpu backend: {option}"
            ));
        };
        if selected[index] {
            return Err(format!("duplicate OpenCL build option: {option}"));
        }
        selected[index] = true;
    }
    Ok(ALLOWED_GUEST_OPTIONS
        .iter()
        .enumerate()
        .filter(|(index, _)| selected[*index])
        .map(|(_, option)| (*option).to_owned())
        .collect())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WgpuArgumentExpectation {
    StorageBuffer,
    PodUniform { size: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum WgpuKernelArgumentValue {
    StorageBuffer(u64),
    PodUniform(Vec<u8>),
}

pub(super) struct WgpuKernel {
    artifact: ValidatedArtifact,
    reflection: KernelReflection,
    arguments: BTreeMap<u32, WgpuKernelArgumentValue>,
}

impl WgpuKernel {
    pub(super) fn new(artifact: ValidatedArtifact, name: &str) -> Result<Self, OpenClRuntimeError> {
        let reflection = artifact.reflection().kernel(name).cloned().ok_or_else(|| {
            OpenClRuntimeError::new(
                CL_INVALID_PROGRAM_EXECUTABLE,
                format!("clspv reflection does not contain kernel {name:?}"),
            )
        })?;
        if reflection.declared_argument_count as usize != reflection.arguments.len() {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_PROGRAM_EXECUTABLE,
                format!(
                    "kernel {name:?} reflection declares {} arguments but describes {}",
                    reflection.declared_argument_count,
                    reflection.arguments.len()
                ),
            ));
        }
        for (expected, argument) in reflection.arguments.iter().enumerate() {
            if argument.ordinal != expected as u32 {
                return Err(OpenClRuntimeError::new(
                    CL_INVALID_PROGRAM_EXECUTABLE,
                    format!(
                        "kernel {name:?} reflection argument ordinals are not contiguous from zero"
                    ),
                ));
            }
            if argument.descriptor_set != 0 {
                return Err(OpenClRuntimeError::new(
                    CL_INVALID_PROGRAM_EXECUTABLE,
                    format!(
                        "kernel {name:?} argument {} uses unsupported bind group {}",
                        argument.ordinal, argument.descriptor_set
                    ),
                ));
            }
        }
        Ok(Self {
            artifact,
            reflection,
            arguments: BTreeMap::new(),
        })
    }

    pub(super) fn argument_expectation(
        &self,
        index: u32,
    ) -> Result<WgpuArgumentExpectation, OpenClRuntimeError> {
        let argument = self
            .reflection
            .arguments
            .iter()
            .find(|argument| argument.ordinal == index)
            .ok_or_else(|| {
                OpenClRuntimeError::new(
                    CL_INVALID_ARG_INDEX,
                    format!(
                        "kernel {:?} has no reflected argument {index}",
                        self.reflection.name
                    ),
                )
            })?;
        match argument.kind {
            KernelArgumentKind::StorageBuffer => Ok(WgpuArgumentExpectation::StorageBuffer),
            KernelArgumentKind::PodUniform { size, .. } => {
                Ok(WgpuArgumentExpectation::PodUniform {
                    size: size as usize,
                })
            }
            #[allow(unreachable_patterns)]
            _ => Err(OpenClRuntimeError::new(
                CL_INVALID_PROGRAM_EXECUTABLE,
                format!(
                    "kernel {:?} argument {index} has unsupported reflection",
                    self.reflection.name
                ),
            )),
        }
    }

    pub(super) fn set_raw_argument(
        &mut self,
        index: u32,
        bytes: &[u8],
    ) -> Result<(), OpenClRuntimeError> {
        match self.argument_expectation(index)? {
            WgpuArgumentExpectation::PodUniform { size } if bytes.len() == size => {
                self.arguments
                    .insert(index, WgpuKernelArgumentValue::PodUniform(bytes.to_vec()));
                Ok(())
            }
            WgpuArgumentExpectation::PodUniform { size } => Err(OpenClRuntimeError::new(
                CL_INVALID_ARG_SIZE,
                format!(
                    "kernel {:?} argument {index} is {} bytes, received {}",
                    self.reflection.name,
                    size,
                    bytes.len()
                ),
            )),
            WgpuArgumentExpectation::StorageBuffer => Err(OpenClRuntimeError::new(
                CL_INVALID_ARG_VALUE,
                format!(
                    "kernel {:?} argument {index} requires a cl_mem token",
                    self.reflection.name
                ),
            )),
        }
    }

    pub(super) fn set_buffer_argument(
        &mut self,
        index: u32,
        token: u64,
    ) -> Result<(), OpenClRuntimeError> {
        match self.argument_expectation(index)? {
            WgpuArgumentExpectation::StorageBuffer => {
                self.arguments
                    .insert(index, WgpuKernelArgumentValue::StorageBuffer(token));
                Ok(())
            }
            WgpuArgumentExpectation::PodUniform { size } => Err(OpenClRuntimeError::new(
                CL_INVALID_ARG_SIZE,
                format!(
                    "kernel {:?} argument {index} requires exactly {size} POD bytes",
                    self.reflection.name
                ),
            )),
        }
    }

    pub(super) fn prepare_dispatch(
        &self,
        global_offset: Option<&[usize]>,
        global: &[usize],
        local: Option<&[usize]>,
        mut buffer_bytes: impl FnMut(u64) -> Result<Vec<u8>, String>,
    ) -> Result<WgpuDispatchPlan, OpenClRuntimeError> {
        let geometry = validate_work_geometry(global_offset, global, local)?;
        if self.arguments.len() != self.reflection.arguments.len()
            || self
                .reflection
                .arguments
                .iter()
                .any(|argument| !self.arguments.contains_key(&argument.ordinal))
        {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_KERNEL_ARGS,
                format!(
                    "kernel {:?} does not have every reflected argument set",
                    self.reflection.name
                ),
            ));
        }

        let mut storage_tokens = BTreeSet::new();
        for value in self.arguments.values() {
            if let WgpuKernelArgumentValue::StorageBuffer(token) = value
                && !storage_tokens.insert(*token)
            {
                return Err(OpenClRuntimeError::new(
                    CL_INVALID_ARG_VALUE,
                    format!(
                        "kernel {:?} reuses cl_mem token {token:#x}; aliasing is unsupported",
                        self.reflection.name
                    ),
                ));
            }
        }

        let prepared = if self.artifact.reflection().work_dim_spec_id().is_some() {
            self.artifact.prepare_entry_point_for_dispatch(
                &self.reflection.name,
                geometry.work_dim,
                geometry.local,
            )
        } else {
            self.artifact
                .prepare_entry_point(&self.reflection.name, geometry.local)
        }
        .map_err(|error| {
            OpenClRuntimeError::new(
                CL_INVALID_PROGRAM_EXECUTABLE,
                format!(
                    "preparing kernel {:?} for dispatch failed: {error}",
                    self.reflection.name
                ),
            )
        })?;

        let mut uniform_bytes = prepared
            .pod_uniform_spans
            .iter()
            .map(|(&(group, binding), &span)| {
                if group != 0 {
                    return Err(OpenClRuntimeError::new(
                        CL_INVALID_PROGRAM_EXECUTABLE,
                        format!("POD uniform binding {group}:{binding} is outside group 0"),
                    ));
                }
                let bytes = aligned_uniform_size(span as usize).ok_or_else(|| {
                    OpenClRuntimeError::new(
                        CL_INVALID_ARG_SIZE,
                        format!("POD uniform binding 0:{binding} size overflow"),
                    )
                })?;
                Ok((binding, vec![0u8; bytes]))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let mut bindings = Vec::new();
        let mut output_tokens = BTreeMap::new();
        let mut storage_bindings = 0usize;

        for reflected in &self.reflection.arguments {
            let value = self
                .arguments
                .get(&reflected.ordinal)
                .expect("complete argument map was validated");
            match (&reflected.kind, value) {
                (
                    KernelArgumentKind::StorageBuffer,
                    WgpuKernelArgumentValue::StorageBuffer(token),
                ) => {
                    let access = active_storage_access(&prepared, reflected.binding)?;
                    let bytes = buffer_bytes(*token).map_err(|detail| {
                        OpenClRuntimeError::new(
                            CL_INVALID_ARG_VALUE,
                            format!(
                                "kernel {:?} argument {} buffer is unavailable: {detail}",
                                self.reflection.name, reflected.ordinal
                            ),
                        )
                    })?;
                    if access == BindingAccess::ReadWriteStorage {
                        output_tokens.insert(reflected.binding, *token);
                    }
                    bindings.push(OwnedWgpuBinding {
                        binding: reflected.binding,
                        access,
                        bytes,
                    });
                    storage_bindings += 1;
                }
                (
                    KernelArgumentKind::PodUniform { offset, size },
                    WgpuKernelArgumentValue::PodUniform(bytes),
                ) => {
                    let uniform = uniform_bytes.get_mut(&reflected.binding).ok_or_else(|| {
                        OpenClRuntimeError::new(
                            CL_INVALID_PROGRAM_EXECUTABLE,
                            format!(
                                "kernel {:?} argument {} has no validated POD uniform span",
                                self.reflection.name, reflected.ordinal
                            ),
                        )
                    })?;
                    let start = *offset as usize;
                    let end = start.checked_add(*size as usize).ok_or_else(|| {
                        OpenClRuntimeError::new(
                            CL_INVALID_ARG_SIZE,
                            "POD uniform argument range overflow",
                        )
                    })?;
                    if end > uniform.len() || bytes.len() != *size as usize {
                        return Err(OpenClRuntimeError::new(
                            CL_INVALID_ARG_SIZE,
                            format!(
                                "kernel {:?} argument {} does not fit its POD uniform",
                                self.reflection.name, reflected.ordinal
                            ),
                        ));
                    }
                    uniform[start..end].copy_from_slice(bytes);
                }
                _ => {
                    return Err(OpenClRuntimeError::new(
                        CL_INVALID_ARG_VALUE,
                        format!(
                            "kernel {:?} argument {} does not match reflection",
                            self.reflection.name, reflected.ordinal
                        ),
                    ));
                }
            }
        }
        let uniform_bindings = uniform_bytes.len();
        bindings.extend(
            uniform_bytes
                .into_iter()
                .map(|(binding, bytes)| OwnedWgpuBinding {
                    binding,
                    access: BindingAccess::Uniform,
                    bytes,
                }),
        );
        bindings.sort_by_key(|binding| binding.binding);
        if bindings
            .windows(2)
            .any(|pair| pair[0].binding == pair[1].binding)
        {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_PROGRAM_EXECUTABLE,
                format!(
                    "kernel {:?} reflection reuses a descriptor binding",
                    self.reflection.name
                ),
            ));
        }
        let upload_bytes = bindings
            .iter()
            .map(|binding| binding.bytes.len() as u64)
            .sum();
        let module = ValidatedNagaModule::new(prepared.module).map_err(|error| {
            OpenClRuntimeError::new(
                CL_INVALID_PROGRAM_EXECUTABLE,
                format!(
                    "kernel {:?} validated Naga module was rejected by wgpu: {error}",
                    self.reflection.name
                ),
            )
        })?;
        Ok(WgpuDispatchPlan {
            module,
            entry_point: prepared.entry_point,
            geometry,
            bindings,
            output_tokens,
            storage_bindings,
            uniform_bindings,
            upload_bytes,
        })
    }
}

fn active_storage_access(
    prepared: &aex_clspv::PreparedEntryPoint,
    binding: u32,
) -> Result<BindingAccess, OpenClRuntimeError> {
    let entry_info = prepared.info.get_entry_point(0);
    let mut found = None;
    for (handle, variable) in prepared.module.global_variables.iter() {
        if entry_info[handle].is_empty()
            || !variable
                .binding
                .as_ref()
                .is_some_and(|resource| resource.group == 0 && resource.binding == binding)
        {
            continue;
        }
        let access = match variable.space {
            aex_wgpu_compute::naga::AddressSpace::Storage { access }
                if access.contains(aex_wgpu_compute::naga::StorageAccess::STORE) =>
            {
                BindingAccess::ReadWriteStorage
            }
            aex_wgpu_compute::naga::AddressSpace::Storage { .. } => BindingAccess::ReadOnlyStorage,
            _ => {
                return Err(OpenClRuntimeError::new(
                    CL_INVALID_PROGRAM_EXECUTABLE,
                    format!("binding 0:{binding} is not a storage buffer"),
                ));
            }
        };
        if found.replace(access).is_some() {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_PROGRAM_EXECUTABLE,
                format!("binding 0:{binding} resolves to multiple active globals"),
            ));
        }
    }
    found.ok_or_else(|| {
        OpenClRuntimeError::new(
            CL_INVALID_PROGRAM_EXECUTABLE,
            format!("binding 0:{binding} has no active storage global"),
        )
    })
}

fn aligned_uniform_size(span: usize) -> Option<usize> {
    span.checked_add(15)
        .map(|value| value & !15)
        .filter(|value| *value != 0)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WgpuWorkGeometry {
    pub(super) work_dim: u32,
    pub(super) global: [u64; 3],
    pub(super) local: [u32; 3],
    pub(super) groups: [u32; 3],
}

fn validate_work_geometry(
    global_offset: Option<&[usize]>,
    global: &[usize],
    local: Option<&[usize]>,
) -> Result<WgpuWorkGeometry, OpenClRuntimeError> {
    if global.is_empty() || global.len() > 3 {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_WORK_DIMENSION,
            format!("wgpu work dimension {} is outside 1..=3", global.len()),
        ));
    }
    if let Some(offsets) = global_offset {
        if offsets.len() != global.len() || offsets.iter().any(|offset| *offset != 0) {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_GLOBAL_OFFSET,
                "wgpu backend supports only zero global work offsets",
            ));
        }
    }
    let local = local.ok_or_else(|| {
        OpenClRuntimeError::new(
            CL_INVALID_WORK_GROUP_SIZE,
            "wgpu backend requires an explicit local work size",
        )
    })?;
    if local.len() != global.len() {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_WORK_GROUP_SIZE,
            "wgpu local work dimensions do not match global work dimensions",
        ));
    }

    let mut padded_global = [1u64; 3];
    let mut padded_local = [1u32; 3];
    let mut groups = [1u32; 3];
    for axis in 0..global.len() {
        let global_axis = global[axis];
        let local_axis = local[axis];
        if global_axis == 0 {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_GLOBAL_WORK_SIZE,
                format!("wgpu global work size axis {axis} is zero"),
            ));
        }
        if local_axis == 0 || !global_axis.is_multiple_of(local_axis) {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_WORK_GROUP_SIZE,
                format!(
                    "wgpu global work size {global_axis} is not divisible by local size {local_axis} on axis {axis}"
                ),
            ));
        }
        let local_axis = u32::try_from(local_axis).map_err(|_| {
            OpenClRuntimeError::new(
                CL_INVALID_WORK_GROUP_SIZE,
                format!("wgpu local work size axis {axis} does not fit u32"),
            )
        })?;
        let group_axis = u32::try_from(global_axis / local[axis]).map_err(|_| {
            OpenClRuntimeError::new(
                CL_INVALID_GLOBAL_WORK_SIZE,
                format!("wgpu workgroup count axis {axis} does not fit u32"),
            )
        })?;
        padded_global[axis] = global_axis as u64;
        padded_local[axis] = local_axis;
        groups[axis] = group_axis;
    }
    Ok(WgpuWorkGeometry {
        work_dim: global.len() as u32,
        global: padded_global,
        local: padded_local,
        groups,
    })
}

pub(super) struct OwnedWgpuBinding {
    pub(super) binding: u32,
    pub(super) access: BindingAccess,
    pub(super) bytes: Vec<u8>,
}

pub(super) struct WgpuDispatchPlan {
    module: ValidatedNagaModule,
    entry_point: String,
    pub(super) geometry: WgpuWorkGeometry,
    bindings: Vec<OwnedWgpuBinding>,
    pub(super) output_tokens: BTreeMap<u32, u64>,
    pub(super) storage_bindings: usize,
    pub(super) uniform_bindings: usize,
    pub(super) upload_bytes: u64,
}

impl WgpuDispatchPlan {
    pub(super) fn evidence(&self, download_bytes: u64) -> WgpuDispatchEvidence {
        WgpuDispatchEvidence {
            kernel: self.entry_point.clone(),
            work_dim: self.geometry.work_dim,
            global: self.geometry.global,
            local: self.geometry.local,
            groups: self.geometry.groups,
            storage_bindings: self.storage_bindings,
            uniform_bindings: self.uniform_bindings,
            upload_bytes: self.upload_bytes,
            download_bytes,
        }
    }
}

pub(super) fn add_artifact_evidence(
    evidence: &mut WgpuRuntimeEvidence,
    artifact: WgpuArtifactEvidence,
) {
    if evidence.artifacts.len() < MAX_WGPU_ARTIFACT_EVIDENCE {
        evidence.artifacts.push(artifact);
    }
}

pub(super) fn add_dispatch_evidence(
    evidence: &mut WgpuRuntimeEvidence,
    dispatch: WgpuDispatchEvidence,
) {
    evidence.upload_bytes = evidence.upload_bytes.saturating_add(dispatch.upload_bytes);
    evidence.download_bytes = evidence
        .download_bytes
        .saturating_add(dispatch.download_bytes);
    if evidence.dispatches.len() < MAX_WGPU_DISPATCH_EVIDENCE {
        evidence.dispatches.push(dispatch);
    }
}

pub(super) fn add_resource_counts(total: &mut WgpuResourceCounts, counts: ObjectCounts) {
    total.buffers = total.buffers.saturating_add(counts.buffers);
    total.staging_buffers = total.staging_buffers.saturating_add(counts.staging_buffers);
    total.shader_modules = total.shader_modules.saturating_add(counts.shader_modules);
    total.bind_group_layouts = total
        .bind_group_layouts
        .saturating_add(counts.bind_group_layouts);
    total.pipeline_layouts = total
        .pipeline_layouts
        .saturating_add(counts.pipeline_layouts);
    total.pipelines = total.pipelines.saturating_add(counts.pipelines);
    total.bind_groups = total.bind_groups.saturating_add(counts.bind_groups);
    total.command_buffers = total.command_buffers.saturating_add(counts.command_buffers);
}

pub(super) fn replace_resource_counts(destination: &mut WgpuResourceCounts, counts: ObjectCounts) {
    *destination = WgpuResourceCounts {
        buffers: counts.buffers,
        staging_buffers: counts.staging_buffers,
        shader_modules: counts.shader_modules,
        bind_group_layouts: counts.bind_group_layouts,
        pipeline_layouts: counts.pipeline_layouts,
        pipelines: counts.pipelines,
        bind_groups: counts.bind_groups,
        command_buffers: counts.command_buffers,
    };
}

fn hex_digest(digest: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(values: &[(&str, &str)]) -> Result<WgpuCompilerConfig, String> {
        WgpuCompilerConfig::from_lookup(|key| {
            values
                .iter()
                .find(|(candidate, _)| *candidate == key)
                .map(|(_, value)| OsString::from(value))
        })
    }

    #[test]
    fn compiler_config_rejects_missing_partial_and_mixed_modes() {
        assert!(config(&[]).unwrap_err().contains("missing"));
        assert!(
            config(&[(LOCAL_COMPILER_PATH_ENV, "/tmp/clspv")])
                .unwrap_err()
                .contains(LOCAL_COMPILER_SHA256_ENV)
        );
        assert!(
            config(&[
                (LOCAL_COMPILER_PATH_ENV, "/tmp/clspv"),
                (LOCAL_COMPILER_SHA256_ENV, &"11".repeat(32)),
                (PRECOMPILED_SPIRV_ENV, "/tmp/kernel.spv"),
            ])
            .unwrap_err()
            .contains("both")
        );
    }

    #[test]
    fn compiler_config_accepts_complete_modes_without_exposing_paths() {
        let local = config(&[
            (LOCAL_COMPILER_PATH_ENV, "/tmp/clspv"),
            (LOCAL_COMPILER_SHA256_ENV, &"11".repeat(32)),
        ])
        .unwrap();
        assert!(matches!(local, WgpuCompilerConfig::Local { .. }));

        let precompiled = config(&[
            (PRECOMPILED_SPIRV_ENV, "/tmp/kernel.spv"),
            (PRECOMPILED_SOURCE_SHA256_ENV, &"22".repeat(32)),
            (PRECOMPILED_SPIRV_SHA256_ENV, &"33".repeat(32)),
            (PRECOMPILED_COMPILER_SHA256_ENV, &"44".repeat(32)),
        ])
        .unwrap();
        assert!(matches!(
            precompiled,
            WgpuCompilerConfig::Precompiled { .. }
        ));
    }

    #[test]
    fn build_options_are_bounded_and_canonical() {
        assert_eq!(
            normalize_guest_build_options(Some(
                "-cl-fast-relaxed-math -cl-single-precision-constant"
            ))
            .unwrap(),
            vec![
                "-cl-single-precision-constant".to_string(),
                "-cl-fast-relaxed-math".to_string()
            ]
        );
        assert!(normalize_guest_build_options(Some("-I/tmp")).is_err());
        assert!(
            normalize_guest_build_options(Some("-cl-fast-relaxed-math -cl-fast-relaxed-math"))
                .is_err()
        );
    }

    #[test]
    fn work_geometry_requires_local_divisibility_and_zero_offset() {
        let geometry = validate_work_geometry(Some(&[0, 0]), &[64, 32], Some(&[16, 8])).unwrap();
        assert_eq!(geometry.work_dim, 2);
        assert_eq!(geometry.global, [64, 32, 1]);
        assert_eq!(geometry.local, [16, 8, 1]);
        assert_eq!(geometry.groups, [4, 4, 1]);
        assert!(validate_work_geometry(None, &[64], None).is_err());
        assert!(validate_work_geometry(Some(&[1]), &[64], Some(&[16])).is_err());
        assert!(validate_work_geometry(None, &[63], Some(&[16])).is_err());
    }

    #[test]
    fn pod_uniform_spans_are_padded_to_wgpu_alignment() {
        assert_eq!(aligned_uniform_size(20), Some(32));
        assert_eq!(aligned_uniform_size(36), Some(48));
        assert_eq!(aligned_uniform_size(0), None);
    }
}
