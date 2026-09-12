#[derive(Clone, Copy, Debug)]
pub struct RenderTiming {
    pub current_time: i32,
    pub time_step: i32,
    pub total_time: i32,
    pub time_scale: u32,
}

impl Default for RenderTiming {
    fn default() -> Self {
        Self {
            current_time: 0,
            time_step: 1,
            total_time: 1,
            time_scale: 1,
        }
    }
}

impl RenderTiming {
    fn is_valid(self) -> bool {
        self.current_time >= 0
            && self.time_step > 0
            && self.total_time >= self.current_time
            && self.time_scale > 0
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderGpuBackend {
    #[default]
    Auto,
    Cuda,
    OpenCl,
    DirectX,
    Cpu,
}

/// Inputs produced by a GPU module-audit preflight for one render session.
/// The raw report is authenticated again immediately before worker dispatch.
///
/// Every field is a shared borrow or a small `Copy` value, so the whole input is
/// `Copy`: the length-one session wrapper reads it out of a `&SessionWrapperRequest`
/// (#290) without moving, and copying only duplicates references, never the
/// underlying policy, report bytes, or module tables.
#[derive(Clone, Copy)]
pub struct GpuRuntimePolicyInput<'a> {
    pub policy: &'a RuntimeModulePolicy,
    pub module_report_json: &'a [u8],
    pub session_identity: [u8; 32],
    pub sealed_modules: &'a [ApprovedClassifiedModule],
    pub trusted_modules: &'a [ApprovedClassifiedModule],
    pub system32: &'a Path,
}

pub(crate) fn runtime_backend(backend: RenderGpuBackend) -> Option<RuntimeBackend> {
    match backend {
        RenderGpuBackend::Auto | RenderGpuBackend::Cuda => Some(RuntimeBackend::Cuda),
        RenderGpuBackend::OpenCl => Some(RuntimeBackend::Opencl),
        RenderGpuBackend::DirectX => Some(RuntimeBackend::Directx),
        RenderGpuBackend::Cpu => None,
    }
}

pub(crate) fn native_rgba_to_preview(
    bytes: &[u8],
    format: RenderPixelFormat,
) -> io::Result<Vec<u8>> {
    match format {
        RenderPixelFormat::Argb8 => Ok(bytes.to_vec()),
        RenderPixelFormat::Argb16 => {
            if bytes.len() % 8 != 0 {
                return Err(invalid("RGBA16 output is misaligned"));
            }
            Ok(bytes
                .chunks_exact(2)
                .map(|sample| {
                    let value = u16::from_le_bytes([sample[0], sample[1]]);
                    ((u32::from(value.min(32768)) * 255 + 16384) / 32768) as u8
                })
                .collect())
        }
        RenderPixelFormat::Argb32f => {
            if bytes.len() % 16 != 0 {
                return Err(invalid("RGBA32f output is misaligned"));
            }
            Ok(bytes
                .chunks_exact(4)
                .map(|sample| {
                    let value = f32::from_le_bytes(sample.try_into().expect("four-byte sample"));
                    if value.is_finite() {
                        (value.clamp(0.0, 1.0) * 255.0).round() as u8
                    } else {
                        0
                    }
                })
                .collect())
        }
    }
}

/// Opt-in world snapshot dumps and output checksum detail (issue #19). Both
/// default off and only take effect for broker-dispatched image renders.
const WORLD_DUMP_DIR_ENV: &str = "AEXCOMPAT_DUMP_WORLDS_DIR";
const OUTPUT_CHECKSUM_DETAIL_ENV: &str = "AEXCOMPAT_CHECKSUM_DETAIL";

pub(crate) struct WorldDumpDir {
    pub(crate) path: PathBuf,
    display: String,
}

/// Opt-in crash minidump directory (issue #18), resolved for the report only.
/// The worker never receives this path or a dump-file handle: the broker
/// creates the dump file at the Windows launch boundary and hands the worker an
/// inherited pipe (see `minidump_policy`). Dumps contain plug-in memory, so
/// they stay local and the broker-owned file name is never serialized.
fn requested_minidump_directory(repository: &Path) -> io::Result<Option<String>> {
    crate::minidump_policy::configured_directory_display(repository)
}

fn requested_world_dump_dir(repository: &Path) -> io::Result<Option<WorldDumpDir>> {
    if let Some(path) = fixture_world_dump_override() {
        return resolve_world_dump_dir(repository, &path).map(Some);
    }
    match std::env::var_os(WORLD_DUMP_DIR_ENV) {
        Some(value) => resolve_world_dump_dir(repository, Path::new(&value)).map(Some),
        None => Ok(None),
    }
}

/// Fail-closed resolution of the requested dump directory: it must resolve to
/// a broker-managed location under `<repository>/target/`, must not use
/// traversal components, and must start empty so stale snapshots can never be
/// mistaken for this run's output.
fn resolve_world_dump_dir(repository: &Path, requested: &Path) -> io::Result<WorldDumpDir> {
    resolve_managed_dump_dir(repository, requested, true)
}

pub(crate) fn resolve_managed_dump_dir(
    repository: &Path,
    requested: &Path,
    require_empty: bool,
) -> io::Result<WorldDumpDir> {
    if requested.as_os_str().is_empty() {
        return Err(invalid("world dump directory must not be empty"));
    }
    if requested.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    }) {
        return Err(invalid(
            "world dump directory must not contain traversal components",
        ));
    }
    // On Windows, `canonicalize()` returns an extended-length (`\\?\\`) path
    // while callers commonly pass the repository as a plain absolute path.
    // Normalize every lexical-boundary operand to the same representation so
    // a wrapper that has already canonicalized the directory is not mistaken
    // for an escape from the managed tree (#372).
    let lexical_repository_root = strip_extended_prefix(repository);
    let repository_root = strip_extended_prefix(&repository.canonicalize()?);
    let requested = strip_extended_prefix(requested);
    let (resolved, target_root) = if requested.is_absolute() && requested.exists() {
        (
            strip_extended_prefix(&requested.canonicalize()?),
            strip_extended_prefix(&repository_root.join("target").canonicalize()?),
        )
    } else if requested.is_absolute() {
        (requested, lexical_repository_root.join("target"))
    } else {
        (
            lexical_repository_root.join(requested),
            lexical_repository_root.join("target"),
        )
    };
    // Lexical pre-check before creating anything, so a rejected request never
    // leaves a directory outside the broker-managed target tree behind.
    if !resolved.starts_with(&target_root) {
        return Err(invalid(
            "world dump directory must stay under the repository target tree",
        ));
    }
    fs::create_dir_all(&resolved)?;
    let canonical = strip_extended_prefix(&resolved.canonicalize()?);
    let canonical_target = strip_extended_prefix(&repository_root.join("target").canonicalize()?);
    if !canonical.starts_with(&canonical_target) {
        return Err(invalid(
            "world dump directory must stay under the repository target tree",
        ));
    }
    if require_empty && fs::read_dir(&canonical)?.next().is_some() {
        return Err(invalid("world dump directory must start empty"));
    }
    let display = canonical
        .strip_prefix(canonical_target.parent().unwrap_or(&canonical_target))
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| "target/(world-dumps)".into());
    Ok(WorldDumpDir {
        path: canonical,
        display,
    })
}

fn output_checksum_detail_requested() -> bool {
    matches!(
        std::env::var(OUTPUT_CHECKSUM_DETAIL_ENV),
        Ok(value) if value == "1" || value.eq_ignore_ascii_case("true")
    )
}

/// AE 16-bpc white point: ARGB16 transport samples are 0..=32768, not 0..=65535.
const AE_ARGB16_WHITE: u32 = 32768;

/// Expand AE-range RGBA16 transport bytes to full-range PNG16 samples.
/// Over-white samples are clamped exactly like the 8-bit preview path; the
/// count is returned so reports can surface them instead of hiding the clamp.
fn rgba16_transport_to_png16(bytes: &[u8]) -> io::Result<(Vec<u16>, u64)> {
    if bytes.len() % 8 != 0 {
        return Err(invalid("RGBA16 output is misaligned"));
    }
    let mut overrange_samples = 0u64;
    let samples = bytes
        .chunks_exact(2)
        .map(|sample| {
            let value = u32::from(u16::from_le_bytes([sample[0], sample[1]]));
            if value > AE_ARGB16_WHITE {
                overrange_samples += 1;
            }
            ((value.min(AE_ARGB16_WHITE) * 65535 + 16384) / 32768) as u16
        })
        .collect();
    Ok((samples, overrange_samples))
}

#[derive(Clone, Copy, Debug)]
pub enum RenderUiAction {
    Click { point: [u16; 2], color: [f32; 4] },
    Draw,
}

impl RenderUiAction {
    /// Encodes the action in the custom-UI grammar the v:2 session `ui_action`
    /// field carries (`click:v1|x|y|r|g|b|a` / `draw:v1`); it came from the
    /// deleted one-shot argv trailer unchanged. The click color is validated
    /// here (finite, in 0..=1) so a bad value is rejected before it reaches a
    /// worker.
    pub fn encode_ui_field(&self) -> io::Result<String> {
        match self {
            RenderUiAction::Click { point, color } => {
                // The worker (argv parser and the session ui_action decoder)
                // rejects x/y above 8192. Validate here so an out-of-range point
                // is a plain caller error before any transport mutation, not a
                // protocol violation that invalidates a resident session.
                if point[0] > 8192 || point[1] > 8192 {
                    return Err(invalid("custom UI render click point is out of range"));
                }
                if color
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
                {
                    return Err(invalid("custom UI render click color is invalid"));
                }
                Ok(format!(
                    "click:v1|{}|{}|{}|{}|{}|{}",
                    point[0], point[1], color[0], color[1], color[2], color[3]
                ))
            }
            RenderUiAction::Draw => Ok("draw:v1".into()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TimedLayerImage {
    pub slot: u32,
    pub time: AnimationTime,
    pub image_path: PathBuf,
}

fn validate_timed_layer_identities(
    timed_layers: &[TimedLayerImage],
    layer_slots: &HashSet<u32>,
) -> io::Result<()> {
    if timed_layers.len() > 64 {
        return Err(invalid(
            "timed secondary layer image count exceeds the transport limit",
        ));
    }
    for (index, layer) in timed_layers.iter().enumerate() {
        let duplicate = timed_layers[..index].iter().any(|prior| {
            prior.slot == layer.slot
                && i64::from(prior.time.value) * i64::from(layer.time.scale)
                    == i64::from(layer.time.value) * i64::from(prior.time.scale)
        });
        if layer.slot > MAX_PARAMETERS
            || layer.time.scale == 0
            || (layer.slot != 0 && !layer_slots.contains(&layer.slot))
            || duplicate
        {
            return Err(invalid(
                "timed layers require primary slot 0 or a known layer slot, valid rational time, and unique slot/time",
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_animation_bindings(
    parameters: &[InteractiveParameter],
    animations: &[ParameterAnimation],
) -> io::Result<()> {
    let mut parameter_slots = HashSet::new();
    for parameter in parameters {
        if !parameter_slots.insert(parameter.slot) {
            return Err(invalid("interactive parameter slots must be unique"));
        }
    }
    for animation in animations {
        let parameter = parameters
            .iter()
            .find(|parameter| parameter.slot == animation.slot)
            .ok_or_else(|| invalid("parameter animation references an unknown slot"))?;
        for key in &animation.keys {
            let compatible = match (&key.value, parameter.kind.as_str()) {
                (AnimationValue::Scalar { value }, "integer" | "path") => {
                    value.fract() == 0.0
                        && *value >= parameter.minimum
                        && *value <= parameter.maximum
                }
                (AnimationValue::Scalar { value }, "float") => {
                    *value >= parameter.minimum && *value <= parameter.maximum
                }
                (AnimationValue::Color { .. }, "color") => true,
                (AnimationValue::Components { value }, "angle") => value.len() == 1,
                (AnimationValue::Components { value }, "point") => value.len() == 2,
                (AnimationValue::Components { value }, "point3d") => value.len() == 3,
                // Parameter discovery reports PF arbitrary parameters as
                // "arbitrary_data" (see interactive parameter kinds); there is
                // no "arbitrary" kind anywhere in the transport.
                (AnimationValue::Arbitrary { .. }, "arbitrary_data") => true,
                _ => false,
            };
            if !compatible {
                return Err(invalid(
                    "parameter animation value does not match its parameter slot",
                ));
            }
        }
    }
    Ok(())
}

/// The plug-in's identity as observed right now, with a note when it differs
/// from the identity the caller selected.
///
/// Dispatch used to refuse here ("selected AEX changed after session
/// approval"): the hash recorded when the user picked the file had to still
/// match, so rebuilding a plug-in mid-session turned every route into an
/// error until it was re-selected. Since issue #739 the mismatch is a state
/// change, not a fault (the #309 pattern): the freshly observed identity is
/// what binds the dispatch and travels into the report, and the caller's
/// stale selection is logged. The worker still re-hashes the file it is
/// asked to load, so what executed is still recorded.
fn observe_selected_plugin(plugin_path: &Path, selected_sha256: &str) -> io::Result<String> {
    observe_selected_plugin_bytes(&fs::read(plugin_path)?, selected_sha256)
}

/// The same, for callers that already read the bytes.
fn observe_selected_plugin_bytes(bytes: &[u8], selected_sha256: &str) -> io::Result<String> {
    let observed = format!("{:X}", Sha256::digest(bytes));
    if !observed.eq_ignore_ascii_case(selected_sha256) {
        tracing::warn!(
            "the selected AEX changed since it was picked; dispatching the bytes on disk"
        );
    }
    Ok(observed)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

const MAX_AUX_CHANNELS: usize = 16;
const MAX_AUX_CHANNELS_PER_PARAM: usize = 8;
const MAX_AUX_SAMPLES_PER_CHANNEL: usize = 64;
const MAX_AUX_SAMPLES: usize = 256;
const MAX_AUX_SAMPLE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_AUX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

struct AuxTransport {
    manifest_path: PathBuf,
    _cleanup: Cleanup,
}

const AUX_CHANNEL_DEPTH: i32 = i32::from_be_bytes(*b"DPTH");
const AUX_CHANNEL_NORMALS: i32 = i32::from_be_bytes(*b"NRML");
const AUX_CHANNEL_MOTION_VECTORS: i32 = i32::from_be_bytes(*b"MTVR");

fn aux_type_dimension_matches(
    channel_type: i32,
    dimension: u8,
    interpretation: crate::render_request::AuxInterpretation,
) -> bool {
    use crate::render_request::AuxInterpretation;
    match interpretation {
        AuxInterpretation::Depth => channel_type == AUX_CHANNEL_DEPTH && dimension == 1,
        AuxInterpretation::Normals => channel_type == AUX_CHANNEL_NORMALS && dimension == 3,
        AuxInterpretation::MotionVectors => {
            channel_type == AUX_CHANNEL_MOTION_VECTORS && dimension == 2
        }
        AuxInterpretation::Generic => {
            !matches!(
                channel_type,
                AUX_CHANNEL_DEPTH | AUX_CHANNEL_NORMALS | AUX_CHANNEL_MOTION_VECTORS
            ) && (1..=4).contains(&dimension)
        }
    }
}

/// Strip the Windows `\\?\` (or `\\?\UNC\`) extended-length prefix from a path.
///
/// `Path::canonicalize()` — which every repository/transport root passes through
/// — returns verbatim `\\?\C:\...` paths on Windows. The worker's aux-manifest
/// loader rejects such paths: it gates each declared path on
/// `std::filesystem::absolute(p).lexically_normal() == std::filesystem::canonical(p)`,
/// and MSVC's `canonical` drops the `\\?\` prefix while `absolute` keeps it, so a
/// verbatim path never matches its own canonical form and the render exits 3
/// (issue #231). The broker already de-verbatims paths handed to the worker for
/// minidump and trace targets (`strip_extended_prefix` in minidump_policy.rs /
/// trace_policy.rs); the aux manifest and its sidecars must follow suit. The
/// string form is identity on any path without the prefix, so this is a no-op on
/// non-Windows and on already-plain paths.
fn strip_extended_prefix(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn prepare_aux_transport(
    repository: &Path,
    channels: &[crate::render_request::AuxChannel],
    root: &Path,
    nonce: u128,
) -> io::Result<Option<AuxTransport>> {
    if channels.is_empty() {
        return Ok(None);
    }
    if channels.len() > MAX_AUX_CHANNELS {
        return Err(invalid("aux channel count exceeds 16"));
    }
    // The worker's aux loader requires each declared path to equal its own
    // canonical form; a `\\?\` verbatim root (from `Path::canonicalize()`) fails
    // that gate, so hand the manifest and every sidecar plain absolute paths.
    let root = strip_extended_prefix(root);
    let root = root.as_path();
    let allowed_root = repository.canonicalize()?;
    let mut per_param = std::collections::BTreeMap::<u32, usize>::new();
    let mut channel_keys = BTreeSet::new();
    let mut source_paths = HashSet::new();
    let mut sample_count = 0usize;
    let mut total_bytes = 0u64;
    let manifest_path = root.join(format!("aux-manifest-{nonce}.json"));
    let mut cleanup = Cleanup(vec![manifest_path.clone()]);
    let mut manifest_channels = Vec::with_capacity(channels.len());

    for (channel_index, item) in channels.iter().enumerate() {
        let count = per_param.entry(item.param_index).or_default();
        *count += 1;
        if *count > MAX_AUX_CHANNELS_PER_PARAM {
            return Err(invalid("aux channels per parameter exceed 8"));
        }
        let channel = &item.channel;
        if !(1..=4).contains(&channel.dimension)
            || channel.width == 0
            || channel.height == 0
            || channel.width > MAX_DIMENSION
            || channel.height > MAX_DIMENSION
            || channel.name.as_bytes().len() > 63
            || channel.name.as_bytes().contains(&0)
        {
            return Err(invalid("aux channel descriptor is invalid"));
        }
        if !channel_keys.insert((item.param_index, channel.channel_type)) {
            return Err(invalid("duplicate aux channel key"));
        }
        if channel.samples.is_empty() || channel.samples.len() > MAX_AUX_SAMPLES_PER_CHANNEL {
            return Err(invalid("aux sample count per channel is outside 1..64"));
        }
        let pixels = u64::from(channel.width)
            .checked_mul(u64::from(channel.height))
            .ok_or_else(|| invalid("aux pixel count overflows"))?;
        if pixels > MAX_PIXELS {
            return Err(invalid("aux pixel count exceeds 16777216"));
        }
        let packed_row_bytes = u64::from(channel.width)
            .checked_mul(u64::from(channel.dimension))
            .and_then(|value| value.checked_mul(4))
            .ok_or_else(|| invalid("aux row size overflows"))?;
        let native_row_bytes = channel.row_bytes.unwrap_or(
            i32::try_from(packed_row_bytes).map_err(|_| invalid("aux row size exceeds i32"))?,
        );
        let absolute_row_bytes = native_row_bytes
            .checked_abs()
            .ok_or_else(|| invalid("aux row stride overflows"))?
            as u64;
        if absolute_row_bytes < packed_row_bytes
            || channel.downsample_x.numerator <= 0
            || channel.downsample_x.denominator == 0
            || channel.downsample_y.numerator <= 0
            || channel.downsample_y.denominator == 0
            || channel.coordinate_space.is_empty()
            || channel.coordinate_space.len() > 64
            || channel.units.is_empty()
            || channel.units.len() > 64
        {
            return Err(invalid("aux native plane descriptor is invalid"));
        }
        let sample_bytes = absolute_row_bytes
            .checked_mul(u64::from(channel.height))
            .ok_or_else(|| invalid("aux sample size overflows"))?;
        if sample_bytes > MAX_AUX_SAMPLE_BYTES {
            return Err(invalid("aux sample exceeds 256 MiB"));
        }
        let mut times = BTreeSet::new();
        let mut manifest_samples = Vec::with_capacity(channel.samples.len());
        for (sample_index, sample) in channel.samples.iter().enumerate() {
            sample_count = sample_count
                .checked_add(1)
                .ok_or_else(|| invalid("aux sample count overflows"))?;
            if sample_count > MAX_AUX_SAMPLES
                || sample.time_scale == 0
                || !times.insert((sample.time, sample.time_scale))
                || !aux_type_dimension_matches(
                    channel.channel_type,
                    channel.dimension,
                    sample.interpretation,
                )
            {
                return Err(invalid("aux sample metadata is invalid or duplicated"));
            }
            total_bytes = total_bytes
                .checked_add(sample_bytes)
                .ok_or_else(|| invalid("aux total size overflows"))?;
            if total_bytes > MAX_AUX_TOTAL_BYTES {
                return Err(invalid("aux transport exceeds 512 MiB"));
            }
            if sample.path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            }) {
                return Err(invalid("aux sample path traversal forbidden"));
            }
            let requested = if sample.path.is_absolute() {
                sample.path.clone()
            } else {
                repository.join(&sample.path)
            };
            let canonical = requested
                .canonicalize()
                .map_err(|_| invalid("aux sample path is unavailable"))?;
            if !canonical.starts_with(&allowed_root) || !source_paths.insert(canonical.clone()) {
                return Err(invalid(
                    "aux sample path escapes its root or aliases another sample",
                ));
            }
            let bytes = fs::read(&canonical)?;
            if bytes.len() as u64 != sample_bytes {
                return Err(invalid(
                    "aux sample byte length does not match its dimensions",
                ));
            }
            if bytes.chunks_exact(4).any(|value| {
                !f32::from_le_bytes(value.try_into().expect("four-byte float")).is_finite()
            }) {
                return Err(invalid("aux sample contains a non-finite float"));
            }
            let raw_path = root.join(format!("aux-{nonce}-{channel_index}-{sample_index}.f32le"));
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&raw_path)?
                .write_all(&bytes)?;
            cleanup.0.push(raw_path.clone());
            let expected_sha256 = format!("{:x}", Sha256::digest(&bytes));
            let written = fs::read(&raw_path)?;
            if written.len() as u64 != sample_bytes
                || format!("{:x}", Sha256::digest(&written)) != expected_sha256
            {
                return Err(invalid("aux sidecar verification failed after write"));
            }
            manifest_samples.push(json!({
                "time": sample.time, "time_scale": sample.time_scale,
                "path": raw_path, "sampling": sample.sampling,
                "interpretation": sample.interpretation,
                "expected_byte_length": sample_bytes,
                "sha256": expected_sha256,
            }));
        }
        manifest_channels.push(json!({
            "param_index": item.param_index, "type": channel.channel_type,
            "name": channel.name, "data_type": "f32le", "dimension": channel.dimension,
            "width": channel.width, "height": channel.height, "samples": manifest_samples,
            "row_bytes": native_row_bytes, "origin_x": channel.origin_x,
            "origin_y": channel.origin_y, "downsample_x_num": channel.downsample_x.numerator,
            "downsample_x_den": channel.downsample_x.denominator,
            "downsample_y_num": channel.downsample_y.numerator,
            "downsample_y_den": channel.downsample_y.denominator,
            "coordinate_space": channel.coordinate_space, "units": channel.units,
        }));
    }
    let manifest = json!({
        "schema": "aux-manifest-v1",
        "nonce": nonce.to_string(),
        "channels": manifest_channels,
    });
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest_path)?;
    serde_json::to_writer_pretty(&mut output, &manifest)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    Ok(Some(AuxTransport {
        manifest_path,
        _cleanup: cleanup,
    }))
}
