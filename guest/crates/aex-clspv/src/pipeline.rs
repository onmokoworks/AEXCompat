//! Shared validation path for locally compiled and externally cached SPIR-V.
//!
//! A cache hit is not trusted shader code. It re-enters at
//! [`validate_precompiled`], which performs the same reflection, stripping, and
//! Naga validation as a locally produced compiler result.

use std::{error::Error, fmt};

use sha2::{Digest, Sha256};

use crate::{
    CompileRequest, CompilerError, CompilerIdentity, NagaError, PinnedCompiler, PreparedEntryPoint,
    ReflectionError, SpirvModule, SpirvReflection, prepare_entry_point,
    prepare_entry_point_for_dispatch,
};

const CACHE_KEY_DOMAIN: &[u8] = b"aex-clspv-cache-v1\0";

/// SHA-256 identity of the exact concatenated OpenCL source seen by clspv.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceIdentity([u8; 32]);

impl SourceIdentity {
    pub fn from_source(source: &[u8]) -> Self {
        Self(Sha256::digest(source).into())
    }

    pub const fn from_sha256(digest: [u8; 32]) -> Self {
        Self(digest)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        hex_digest(&self.0)
    }
}

/// Cache-key provenance. The compiler path is deliberately excluded: the same
/// exact compiler binary may run on another host at a different path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactProvenance {
    pub source: SourceIdentity,
    pub normalized_options: Vec<String>,
    pub compiler: CompilerIdentity,
}

impl ArtifactProvenance {
    pub fn cache_key(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(CACHE_KEY_DOMAIN);
        hash.update(self.source.as_bytes());
        hash.update(self.compiler.binary_sha256());
        hash.update((self.normalized_options.len() as u64).to_le_bytes());
        for option in &self.normalized_options {
            hash.update((option.len() as u64).to_le_bytes());
            hash.update(option.as_bytes());
        }
        hash.finalize().into()
    }

    pub fn cache_key_hex(&self) -> String {
        hex_digest(&self.cache_key())
    }
}

/// An untrusted raw SPIR-V cache entry plus the identity of the compilation
/// inputs from which it was produced.
#[derive(Clone, Debug)]
pub struct PrecompiledInput {
    pub raw_spirv: Vec<u8>,
    pub provenance: ArtifactProvenance,
}

/// Strictly parsed and Naga-validated SPIR-V ready for per-dispatch
/// entry-point preparation.
#[derive(Clone)]
pub struct ValidatedArtifact {
    spirv: SpirvModule,
    provenance: ArtifactProvenance,
    raw_spirv_sha256: [u8; 32],
}

impl ValidatedArtifact {
    pub fn reflection(&self) -> &SpirvReflection {
        self.spirv.reflection()
    }

    pub fn stripped_words(&self) -> &[u32] {
        self.spirv.stripped_words()
    }

    pub const fn provenance(&self) -> &ArtifactProvenance {
        &self.provenance
    }

    pub const fn raw_spirv_sha256(&self) -> &[u8; 32] {
        &self.raw_spirv_sha256
    }

    pub fn prepare_entry_point(
        &self,
        entry_point: &str,
        local_size: [u32; 3],
    ) -> Result<PreparedEntryPoint, NagaError> {
        prepare_entry_point(&self.spirv, entry_point, local_size)
    }

    pub fn prepare_entry_point_for_dispatch(
        &self,
        entry_point: &str,
        work_dim: u32,
        local_size: [u32; 3],
    ) -> Result<PreparedEntryPoint, NagaError> {
        prepare_entry_point_for_dispatch(&self.spirv, entry_point, work_dim, local_size)
    }
}

#[derive(Debug)]
pub enum PipelineError {
    Compiler(CompilerError),
    Reflection(ReflectionError),
    Naga {
        entry_point: String,
        source: NagaError,
    },
    InvalidProvenance(String),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compiler(error) => write!(formatter, "clspv compiler failed: {error}"),
            Self::Reflection(error) => write!(formatter, "SPIR-V reflection rejected: {error}"),
            Self::Naga {
                entry_point,
                source,
            } => write!(
                formatter,
                "Naga rejected compute entry point {entry_point:?}: {source}"
            ),
            Self::InvalidProvenance(detail) => {
                write!(formatter, "precompiled provenance rejected: {detail}")
            }
        }
    }
}

impl Error for PipelineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Compiler(error) => Some(error),
            Self::Reflection(error) => Some(error),
            Self::Naga { source, .. } => Some(source),
            Self::InvalidProvenance(_) => None,
        }
    }
}

impl From<CompilerError> for PipelineError {
    fn from(value: CompilerError) -> Self {
        Self::Compiler(value)
    }
}

impl From<ReflectionError> for PipelineError {
    fn from(value: ReflectionError) -> Self {
        Self::Reflection(value)
    }
}

/// Compile an exact source and immediately feed the output through the same
/// validation path used for an external cache hit.
pub fn compile_and_validate(
    compiler: &PinnedCompiler,
    request: &CompileRequest,
) -> Result<ValidatedArtifact, PipelineError> {
    let source = SourceIdentity::from_source(request.source());
    let output = compiler.compile(request)?;
    validate_precompiled(PrecompiledInput {
        raw_spirv: output.spirv().to_vec(),
        provenance: ArtifactProvenance {
            source,
            normalized_options: output.normalized_options().to_vec(),
            compiler: output.compiler_identity().clone(),
        },
    })
}

/// Strictly validate an untrusted cached compiler result.
///
/// Every reflected kernel is Naga-parsed and validated. A neutral `[1, 1, 1]`
/// size is used only for this cache-admission pass when the source did not have
/// a required workgroup size; the actual enqueue size is patched and validated
/// again by [`ValidatedArtifact::prepare_entry_point`].
pub fn validate_precompiled(input: PrecompiledInput) -> Result<ValidatedArtifact, PipelineError> {
    validate_normalized_options(&input.provenance.normalized_options)?;
    let raw_spirv_sha256 = Sha256::digest(&input.raw_spirv).into();
    let spirv = SpirvModule::parse_bytes(&input.raw_spirv)?;
    if spirv.reflection().kernels().is_empty() {
        return Err(PipelineError::InvalidProvenance(
            "reflection contains no kernels".into(),
        ));
    }

    for kernel in spirv.reflection().kernels() {
        let local_size = kernel.required_workgroup_size.unwrap_or([1, 1, 1]);
        let prepared = if spirv.reflection().work_dim_spec_id().is_some() {
            // Cache admission has no enqueue yet. A neutral, valid work_dim
            // exercises override freezing; the observed dimension is supplied
            // and revalidated before the actual dispatch.
            prepare_entry_point_for_dispatch(&spirv, &kernel.name, 3, local_size)
        } else {
            prepare_entry_point(&spirv, &kernel.name, local_size)
        };
        prepared.map_err(|source| PipelineError::Naga {
            entry_point: kernel.name.clone(),
            source,
        })?;
    }

    Ok(ValidatedArtifact {
        spirv,
        provenance: input.provenance,
        raw_spirv_sha256,
    })
}

fn validate_normalized_options(options: &[String]) -> Result<(), PipelineError> {
    let normalized =
        crate::compiler::normalize_options(options).map_err(PipelineError::Compiler)?;
    if normalized != options {
        return Err(PipelineError::InvalidProvenance(
            "compiler options are not in canonical normalized form".into(),
        ));
    }
    Ok(())
}

fn hex_digest(digest: &[u8; 32]) -> String {
    use fmt::Write as _;

    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_is_path_independent_and_input_sensitive() {
        let compiler = CompilerIdentity::from_binary_sha256([7; 32]);
        let base = ArtifactProvenance {
            source: SourceIdentity::from_sha256([3; 32]),
            normalized_options: vec![
                "-cl-std=CL1.2".into(),
                "-cl-kernel-arg-info".into(),
                "-spv-version=1.0".into(),
                "-pod-ubo".into(),
                "-cluster-pod-kernel-args=1".into(),
            ],
            compiler,
        };
        let mut changed = base.clone();
        changed.source = SourceIdentity::from_sha256([4; 32]);

        assert_eq!(base.cache_key(), base.clone().cache_key());
        assert_ne!(base.cache_key(), changed.cache_key());
        assert_eq!(base.cache_key_hex().len(), 64);
    }
}
