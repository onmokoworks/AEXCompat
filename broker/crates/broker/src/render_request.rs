use crate::fixture_profiles::maskoffset::{
    OracleMask, OracleMaskVertex, bezier_mask_argb8_hash, mask_scene_argb8_hash,
    rectangle_mask_argb8_hash, source_argb8_hash,
};
use crate::fixture_profiles::scattermap::expected_argb8_hash;
use crate::fixture_profiles::{ParameterizedRenderAdapter, RegisteredProfile};
use crate::host_core::approved_artifact::{ApprovedLoadTree, load_v2_load_tree};
use crate::host_core::descriptor_manifest::{LoadedManifest, load as load_manifest};
use crate::host_core::parameter::{
    ParameterValue, PluginProfile, ValidatedAssignments, ValidationError, ValueKind,
    apply_defaults, encode_worker_payload, validate_assignments,
};
use crate::render_approval::{ApprovalIdentity, admit_launch};
use crate::secure_launch::{SecureLaunchRequest, secure_launch_in_place};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const REQUEST_LIMIT: u64 = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema_version: u32,
    plugin_id: String,
    assignments: Assignments,
    host_context: Option<HostContext>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostContext {
    pub mask_scene: MaskScene,
    #[serde(default)]
    pub spatial: Option<SpatialContext>,
    #[serde(default)]
    pub active_camera: Option<ActiveCamera>,
    #[serde(default)]
    pub scene_layers: Vec<SceneLayer>,
    #[serde(default)]
    pub render_environment: Option<RenderEnvironment>,
    #[serde(default)]
    pub aux_channels: Vec<AuxChannel>,
    /// Layer parameter slots whose pre-effect alpha plane may be exposed as COVR.
    /// No other auxiliary plane is inferred from RGBA pixels.
    #[serde(default)]
    pub alpha_as_coverage_params: Vec<u32>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveCamera {
    pub layer: CameraLayerIdentity,
    pub anchor: [f64; 3],
    pub position: [f64; 3],
    pub scale: [f64; 3],
    pub rotation_degrees: [f64; 3],
    pub zoom: f64,
    pub in_point: CameraTime,
    pub duration: CameraTime,
    /// Two bounded linear snapshots; absent retains the static v1 policy.
    #[serde(default)]
    pub keyframes: Option<[CameraKeyframe; 2]>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraKeyframe {
    pub time: CameraTime,
    pub anchor: [f64; 3],
    pub position: [f64; 3],
    pub scale: [f64; 3],
    pub rotation_degrees: [f64; 3],
    pub zoom: f64,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CameraLayerIdentity {
    pub project_id: u64,
    pub object_id: u64,
    pub generation: u32,
    pub index: u8,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneLayer {
    pub layer: CameraLayerIdentity,
    #[serde(default)]
    pub parent: Option<CameraLayerIdentity>,
    pub anchor: [f64; 3],
    pub position: [f64; 3],
    pub scale: [f64; 3],
    pub rotation_degrees: [f64; 3],
    pub is_3d: bool,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraTime {
    pub value: i32,
    pub scale: u32,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuxChannel {
    pub param_index: u32,
    pub channel: AuxChannelDescriptor,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuxChannelDescriptor {
    #[serde(rename = "type")]
    pub channel_type: i32,
    pub name: String,
    pub data_type: AuxDataType,
    pub dimension: u8,
    pub width: u32,
    pub height: u32,
    /// Signed native plane stride. Negative values describe bottom-up storage.
    #[serde(default)]
    pub row_bytes: Option<i32>,
    #[serde(default)]
    pub origin_x: i32,
    #[serde(default)]
    pub origin_y: i32,
    #[serde(default = "unit_scale")]
    pub downsample_x: RationalScale,
    #[serde(default = "unit_scale")]
    pub downsample_y: RationalScale,
    #[serde(default = "default_aux_coordinate_space")]
    pub coordinate_space: String,
    #[serde(default = "default_aux_units")]
    pub units: String,
    pub samples: Vec<AuxChannelSample>,
}

fn unit_scale() -> RationalScale {
    RationalScale {
        numerator: 1,
        denominator: 1,
    }
}

fn default_aux_coordinate_space() -> String {
    "source_pixel".into()
}
fn default_aux_units() -> String {
    "unitless".into()
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuxDataType {
    F32le,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuxChannelSample {
    pub time: i32,
    pub time_scale: u32,
    pub path: PathBuf,
    pub sampling: AuxSampling,
    pub interpretation: AuxInterpretation,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AuxSampling {
    Exact,
    Hold,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AuxInterpretation {
    Depth,
    Normals,
    MotionVectors,
    Generic,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderEnvironment {
    pub quality: RenderQuality,
    pub field: RenderField,
    pub shutter_angle: f64,
    pub shutter_phase: f64,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderQuality {
    Low,
    High,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderField {
    Frame,
    Upper,
    Lower,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpatialContext {
    pub downsample_x: RationalScale,
    pub downsample_y: RationalScale,
    pub pixel_aspect_ratio: RationalScale,
    #[serde(default)]
    pub full_resolution_width: Option<u32>,
    #[serde(default)]
    pub full_resolution_height: Option<u32>,
    #[serde(default)]
    pub pre_effect_source_origin_x: Option<i32>,
    #[serde(default)]
    pub pre_effect_source_origin_y: Option<i32>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RationalScale {
    pub numerator: i32,
    pub denominator: u32,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaskScene {
    pub masks: Vec<MaskShape>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaskShape {
    pub open: bool,
    pub vertices: Vec<MaskPoint>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaskPoint {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub tangent_in: Option<MaskTangent>,
    #[serde(default)]
    pub tangent_out: Option<MaskTangent>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaskTangent {
    pub x: f64,
    pub y: f64,
}

struct Assignments(BTreeMap<String, ParameterValue>);

impl<'de> Deserialize<'de> for Assignments {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct AssignmentsVisitor;

        impl<'de> Visitor<'de> for AssignmentsVisitor {
            type Value = Assignments;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a map of unique parameter ids to numeric values")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut values = BTreeMap::new();
                while let Some((id, value)) = map.next_entry::<String, ParameterValue>()? {
                    if values.insert(id.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate parameter id: {id}"
                        )));
                    }
                }
                Ok(Assignments(values))
            }
        }

        deserializer.deserialize_map(AssignmentsVisitor)
    }
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    gate: &'static str,
    plugin_id: String,
    assignment_count: usize,
    accepted: bool,
    native_dispatch_permitted: bool,
    native_process_started: bool,
    errors: Vec<ValidationError>,
}

fn evaluate(
    request: &Request,
    profile: &PluginProfile,
) -> io::Result<(ValidatedAssignments, usize, Vec<ValidationError>)> {
    if !matches!(request.schema_version, 2 | 3 | 4 | 5)
        || request.schema_version == 2
            && request
                .assignments
                .0
                .values()
                .any(|value| !matches!(value, ParameterValue::Numeric(_)))
        || matches!(request.schema_version, 4 | 5) && request.host_context.is_none()
        || request.schema_version < 4 && request.host_context.is_some()
        || request.schema_version < 5
            && request
                .host_context
                .as_ref()
                .is_some_and(|context| !context.aux_channels.is_empty())
    {
        return Err(invalid("render request identity mismatch"));
    }
    let assignment_count = request.assignments.0.len();
    let (validated, errors) =
        validate_assignments(profile, &request.assignments.0).map_err(invalid)?;
    let effective = apply_defaults(profile, &validated);
    Ok((effective, assignment_count, errors))
}

fn validate_mask_context(context: &HostContext) -> io::Result<(usize, usize)> {
    if context.mask_scene.masks.len() > 8 {
        return Err(invalid("host mask count exceeds 8"));
    }
    let mut vertex_count = 0usize;
    for mask in &context.mask_scene.masks {
        let minimum = if mask.open { 2 } else { 3 };
        if !(minimum..=64).contains(&mask.vertices.len()) {
            return Err(invalid("host mask vertex count outside enabled range"));
        }
        vertex_count += mask.vertices.len();
        if vertex_count > 128 {
            return Err(invalid("host mask total vertex count exceeds 128"));
        }
        if mask.vertices.iter().any(|point| {
            [
                Some((point.x, point.y)),
                point.tangent_in.map(|value| (value.x, value.y)),
                point.tangent_out.map(|value| (value.x, value.y)),
            ]
            .into_iter()
            .flatten()
            .any(|(x, y)| {
                !x.is_finite()
                    || !y.is_finite()
                    || x < -32768.0
                    || x > 32768.0
                    || y < -32768.0
                    || y > 32768.0
            })
        }) {
            return Err(invalid("host mask coordinate is outside bounded range"));
        }
    }
    Ok((context.mask_scene.masks.len(), vertex_count))
}

pub(crate) fn encode_mask_context(context: &HostContext) -> io::Result<String> {
    validate_mask_context(context)?;
    let mut encoded = String::from("v2|");
    for (mask_index, mask) in context.mask_scene.masks.iter().enumerate() {
        if mask_index != 0 {
            encoded.push(';');
        }
        encoded.push_str(if mask.open { "1:" } else { "0:" });
        for (vertex_index, point) in mask.vertices.iter().enumerate() {
            if vertex_index != 0 {
                encoded.push('/');
            }
            let tangent_in = point.tangent_in.unwrap_or(MaskTangent { x: 0.0, y: 0.0 });
            let tangent_out = point.tangent_out.unwrap_or(MaskTangent { x: 0.0, y: 0.0 });
            encoded.push_str(&format!(
                "{},{},{},{},{},{}",
                point.x, point.y, tangent_in.x, tangent_in.y, tangent_out.x, tangent_out.y
            ));
        }
    }
    if encoded.len() > 8192 {
        return Err(invalid("host mask transport exceeds 8192 bytes"));
    }
    Ok(encoded)
}

pub(crate) fn encode_spatial_context(context: &HostContext) -> io::Result<Option<String>> {
    let Some(spatial) = context.spatial else {
        return Ok(None);
    };
    let scales = [
        spatial.downsample_x,
        spatial.downsample_y,
        spatial.pixel_aspect_ratio,
    ];
    if scales.iter().any(|scale| {
        scale.numerator <= 0
            || scale.numerator > 1_000_000
            || scale.denominator == 0
            || scale.denominator > 1_000_000
    }) {
        return Err(invalid("host spatial ratio is outside enabled range"));
    }
    let origin = match (
        spatial.pre_effect_source_origin_x,
        spatial.pre_effect_source_origin_y,
    ) {
        (None, None) => None,
        (Some(x), Some(y)) if (-32768..=32768).contains(&x) && (-32768..=32768).contains(&y) => {
            Some((x, y))
        }
        _ => return Err(invalid("host pre-effect source origin is invalid")),
    };
    match (
        spatial.full_resolution_width,
        spatial.full_resolution_height,
        origin,
    ) {
        (None, None, None) => Ok(Some(format!(
            "spatial:v1|{},{},{},{},{},{}",
            scales[0].numerator,
            scales[0].denominator,
            scales[1].numerator,
            scales[1].denominator,
            scales[2].numerator,
            scales[2].denominator
        ))),
        (Some(width), Some(height), None)
            if (1..=32768).contains(&width) && (1..=32768).contains(&height) =>
        {
            Ok(Some(format!(
                "spatial:v2|{},{},{},{},{},{},{},{}",
                scales[0].numerator,
                scales[0].denominator,
                scales[1].numerator,
                scales[1].denominator,
                scales[2].numerator,
                scales[2].denominator,
                width,
                height
            )))
        }
        (width, height, Some((origin_x, origin_y)))
            if width.is_none() == height.is_none()
                && width.is_none_or(|value| (1..=32768).contains(&value))
                && height.is_none_or(|value| (1..=32768).contains(&value)) =>
        {
            Ok(Some(format!(
                "spatial:v3|{},{},{},{},{},{},{},{},{},{}",
                scales[0].numerator,
                scales[0].denominator,
                scales[1].numerator,
                scales[1].denominator,
                scales[2].numerator,
                scales[2].denominator,
                width.unwrap_or(0),
                height.unwrap_or(0),
                origin_x,
                origin_y
            )))
        }
        _ => Err(invalid("host full-resolution dimensions are invalid")),
    }
}

pub(crate) fn encode_active_camera(context: &HostContext) -> io::Result<Option<String>> {
    encode_scene_snapshot(&context.scene_layers, context.active_camera.as_ref())
}

pub(crate) fn encode_scene_snapshot(
    layers: &[SceneLayer],
    camera: Option<&ActiveCamera>,
) -> io::Result<Option<String>> {
    let encoded_camera = camera.map(encode_camera).transpose()?;
    if layers.is_empty() {
        return Ok(encoded_camera);
    }
    if layers.len() > 3 {
        return Err(invalid("scene requires at most three authored layers"));
    }
    let mut records = Vec::with_capacity(layers.len());
    for (index, layer) in layers.iter().enumerate() {
        if layers[..index].iter().any(|previous| {
            previous.layer.index == layer.layer.index
                || previous.layer.object_id == layer.layer.object_id
        }) || layer.layer.project_id != layers[0].layer.project_id
            || camera.is_some_and(|camera| {
                camera.layer.project_id != layer.layer.project_id
                    || camera.layer.index == layer.layer.index
                    || camera.layer.object_id == layer.layer.object_id
            })
        {
            return Err(invalid("scene layer identity collision or foreign project"));
        }
        // Apply the same bounded transform contract as the existing camera path.
        let validated = encode_camera(&ActiveCamera {
            layer: layer.layer,
            anchor: layer.anchor,
            position: layer.position,
            scale: layer.scale,
            rotation_degrees: layer.rotation_degrees,
            zoom: 1.0,
            in_point: CameraTime { value: 0, scale: 1 },
            duration: CameraTime {
                value: 10,
                scale: 1,
            },
            keyframes: None,
        })?;
        if !layer.is_3d
            && (layer.position[2] != 0.0
                || layer.anchor[2] != 0.0
                || layer.rotation_degrees[0] != 0.0
                || layer.rotation_degrees[1] != 0.0
                || layer.scale[2] != 100.0)
        {
            return Err(invalid("2D layer has unsupported out-of-plane transform"));
        }
        let parent = layer.parent.unwrap_or(CameraLayerIdentity {
            project_id: 0,
            object_id: 0,
            generation: 0,
            index: 0,
        });
        let mut fields = vec![
            layer.layer.project_id.to_string(),
            layer.layer.object_id.to_string(),
            layer.layer.generation.to_string(),
            layer.layer.index.to_string(),
            parent.project_id.to_string(),
            parent.object_id.to_string(),
            parent.generation.to_string(),
            parent.index.to_string(),
            u8::from(layer.is_3d).to_string(),
        ];
        fields.extend(
            validated
                .split_once('|')
                .unwrap()
                .1
                .split(',')
                .skip(9)
                .map(str::to_owned),
        );
        records.push(fields.join(","));
    }
    for layer in layers {
        let mut cursor = Some(layer.layer);
        let mut visited = [false; 3];
        let mut determinant = 1.0;
        while let Some(identity) = cursor {
            let current = layers
                .iter()
                .find(|candidate| candidate.layer == identity)
                .ok_or_else(|| invalid("scene parent is absent, foreign or stale"))?;
            let index = usize::from(current.layer.index);
            if visited[index] {
                return Err(invalid("scene parent cycle"));
            }
            visited[index] = true;
            determinant *= current.scale.iter().product::<f64>() / 1_000_000.0;
            cursor = current.parent;
        }
        if !determinant.is_finite() || determinant <= 1.001e-12 {
            return Err(invalid("scene composed transform is singular"));
        }
    }
    Ok(Some(format!(
        "scene-graph:v1|{}!{}",
        encoded_camera.unwrap_or_default(),
        records.join(";")
    )))
}

pub(crate) fn encode_camera(camera: &ActiveCamera) -> io::Result<String> {
    let camera = *camera;
    if camera.layer.project_id == 0
        || camera.layer.project_id > i32::MAX as u64
        || camera.layer.object_id == 0
        || camera.layer.object_id > i32::MAX as u64
        || camera.layer.generation == 0
        || camera.layer.index >= 3
    {
        return Err(invalid(
            "active camera layer identity is outside enabled range",
        ));
    }
    let start = camera.in_point;
    let duration = camera.duration;
    if start.value < 0
        || duration.value <= 0
        || start.scale == 0
        || duration.scale == 0
        || start.scale > 1_000_000
        || duration.scale > 1_000_000
        || start.value as i128 * duration.scale as i128
            + duration.value as i128 * start.scale as i128
            > 10 * start.scale as i128 * duration.scale as i128
    {
        return Err(invalid("active camera time range is invalid"));
    }
    if !camera.zoom.is_finite() || camera.zoom <= 0.0 || camera.zoom > 1_000_000_000.0 {
        return Err(invalid("active camera zoom is invalid"));
    }
    for value in camera.anchor.into_iter().chain(camera.position) {
        if !value.is_finite() || value.abs() > 1_000_000.0 {
            return Err(invalid("active camera translation is invalid"));
        }
    }
    for value in camera.scale {
        if !value.is_finite() || !(0.01..=10_000.0).contains(&value) {
            return Err(invalid("active camera scale is singular or out of range"));
        }
    }
    let scale_determinant = camera.scale.into_iter().product::<f64>() / 1_000_000.0;
    if !scale_determinant.is_finite() || scale_determinant <= 1.001e-12 {
        return Err(invalid("active camera scale is singular or out of range"));
    }
    for value in camera.rotation_degrees {
        if !value.is_finite() || value.abs() > 36_000.0 {
            return Err(invalid("active camera rotation is invalid"));
        }
    }
    let mut fields = vec![
        camera.layer.project_id.to_string(),
        camera.layer.object_id.to_string(),
        camera.layer.generation.to_string(),
        camera.layer.index.to_string(),
        start.value.to_string(),
        start.scale.to_string(),
        duration.value.to_string(),
        duration.scale.to_string(),
    ];
    for value in [camera.zoom]
        .into_iter()
        .chain(camera.anchor)
        .chain(camera.position)
        .chain(camera.scale)
        .chain(camera.rotation_degrees)
    {
        fields.push(value.to_bits().to_string());
    }
    let version = if let Some(keyframes) = camera.keyframes {
        let [first, second] = keyframes.map(|keyframe| keyframe.time);
        if [first, second].into_iter().any(|time| {
            time.value < 0
                || time.scale == 0
                || time.scale > 1_000_000
                || i64::from(time.value) >= 10 * i64::from(time.scale)
        }) || i64::from(first.value) * i64::from(second.scale)
            >= i64::from(second.value) * i64::from(first.scale)
        {
            return Err(invalid("active camera keyframe times are invalid"));
        }
        for keyframe in keyframes {
            let snapshot = ActiveCamera {
                anchor: keyframe.anchor,
                position: keyframe.position,
                scale: keyframe.scale,
                rotation_degrees: keyframe.rotation_degrees,
                zoom: keyframe.zoom,
                keyframes: None,
                ..camera
            };
            // Reuse exactly the static transform/zoom validation and bit encoding.
            let encoded = encode_camera(&snapshot)?;
            fields.extend([
                keyframe.time.value.to_string(),
                keyframe.time.scale.to_string(),
            ]);
            fields.extend(
                encoded
                    .split('|')
                    .nth(1)
                    .expect("camera fields")
                    .split(',')
                    .skip(8)
                    .map(str::to_owned),
            );
        }
        "v2"
    } else {
        "v1"
    };
    Ok(format!("scene-camera:{version}|{}", fields.join(",")))
}

pub(crate) fn encode_render_environment(context: &HostContext) -> io::Result<Option<String>> {
    let Some(environment) = context.render_environment else {
        return Ok(None);
    };
    if !environment.shutter_angle.is_finite()
        || !(0.0..=1.0).contains(&environment.shutter_angle)
        || !environment.shutter_phase.is_finite()
        || !(-1.0..=1.0).contains(&environment.shutter_phase)
    {
        return Err(invalid("host render environment is outside enabled range"));
    }
    let quality = match environment.quality {
        RenderQuality::Low => 0,
        RenderQuality::High => 1,
    };
    let field = match environment.field {
        RenderField::Frame => 0,
        RenderField::Upper => 1,
        RenderField::Lower => 2,
    };
    let angle = (environment.shutter_angle * 65536.0).round() as i32;
    let phase = (environment.shutter_phase * 65536.0).round() as i32;
    Ok(Some(format!("render:v1|{quality},{field},{angle},{phase}")))
}

fn expected_hash(adapter: ParameterizedRenderAdapter, effective: &ValidatedAssignments) -> String {
    match adapter {
        ParameterizedRenderAdapter::ScatterMap => expected_argb8_hash(effective),
        ParameterizedRenderAdapter::MaskOffsetRectangle => rectangle_mask_argb8_hash(effective),
    }
}

fn descriptors(
    repository: &Path,
    plugin_id: &str,
    registered: &RegisteredProfile,
) -> io::Result<LoadedManifest> {
    load_manifest(repository, plugin_id, registered.descriptor_manifest)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn launch_approved_in_place(
    approved: &ApprovedLoadTree,
    worker: &Path,
    args_before_plugin: &[String],
    args_after_plugin: &[String],
    repository: &Path,
    timeout_ms: u64,
) -> io::Result<crate::secure_launch::SecureLaunchResult> {
    let plugin_path = fs::canonicalize(&approved.main.source)?;
    let dirs = approved.dependency_search_dirs()?;
    let joined = crate::secure_image_dispatch::joined_dependency_search_dirs(&dirs)?;
    let mut tail = args_after_plugin.to_vec();
    tail.extend(["--dependency-dirs-v1".to_owned(), joined]);
    secure_launch_in_place(
        &plugin_path,
        SecureLaunchRequest {
            worker_program: worker,
            worker_expected_sha256: approved.worker_sha256,
            worker_expected_size: approved.worker_byte_size,
            args_before_plugin,
            args_after_plugin: &tail,
            repository,
            require_module_audit: true,
            launch_environment: Default::default(),
            staged_worker_assets: &[],
        },
        Some(Duration::from_millis(timeout_ms)),
        None,
    )
}

pub(crate) fn validate_image_buffer_layout(
    width: u64,
    height: u64,
    rowbytes: u64,
    bytes_per_pixel: u64,
    capacity: Option<u64>,
    max_dimension: u64,
    max_pixels: u64,
    max_buffer_bytes: u64,
) -> io::Result<u64> {
    if width == 0 || height == 0 || width > max_dimension || height > max_dimension {
        return Err(invalid("image dimensions are outside the enabled range"));
    }
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| invalid("image pixel count overflows"))?;
    if pixels > max_pixels {
        return Err(invalid("image pixel count exceeds the enabled limit"));
    }
    let minimum_rowbytes = width
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| invalid("image row size overflows"))?;
    if bytes_per_pixel == 0 || rowbytes < minimum_rowbytes {
        return Err(invalid("image rowbytes is smaller than a packed row"));
    }
    let required = rowbytes
        .checked_mul(height)
        .ok_or_else(|| invalid("image buffer size overflows"))?;
    if required > max_buffer_bytes {
        return Err(invalid("image buffer exceeds the enabled byte limit"));
    }
    if capacity.is_some_and(|capacity| capacity < required || capacity > max_buffer_bytes) {
        return Err(invalid(
            "image buffer capacity is inconsistent with its layout",
        ));
    }
    Ok(required)
}

fn worker_echo_matches(
    report: &Value,
    profile: &PluginProfile,
    effective: &ValidatedAssignments,
) -> bool {
    let Some(items) = report.get("requested_parameters").and_then(Value::as_array) else {
        return false;
    };
    items.len() == profile.descriptors.len()
        && items
            .iter()
            .zip(&profile.descriptors)
            .all(|(item, descriptor)| {
                let expected_kind = match descriptor.kind {
                    ValueKind::Integer => "integer",
                    ValueKind::Float => "float",
                    ValueKind::Color => "color",
                };
                let value_matches = match descriptor.kind {
                    ValueKind::Integer | ValueKind::Float => {
                        item.get("value").and_then(Value::as_f64)
                            == effective
                                .get(&descriptor.id)
                                .and_then(|value| value.numeric())
                    }
                    ValueKind::Color => {
                        item.get("value")
                            == effective
                                .get(&descriptor.id)
                                .and_then(|value| serde_json::to_value(value).ok())
                                .as_ref()
                    }
                };
                item.get("id").and_then(Value::as_str) == Some(descriptor.id.as_str())
                    && item.get("slot").and_then(Value::as_u64) == Some(descriptor.slot as u64)
                    && item.get("kind").and_then(Value::as_str) == Some(expected_kind)
                    && value_matches
            })
}

fn resolve_inside(
    repository: &Path,
    path: &Path,
    relative_root: &str,
    create_parent: bool,
) -> io::Result<PathBuf> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(invalid("path traversal forbidden"));
    }
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repository.join(path)
    };
    if resolved.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err(invalid("JSON path required"));
    }
    let root = repository.join(relative_root);
    if create_parent {
        fs::create_dir_all(&root)?;
        fs::create_dir_all(
            resolved
                .parent()
                .ok_or_else(|| invalid("path parent missing"))?,
        )?;
    }
    let parent = resolved
        .parent()
        .ok_or_else(|| invalid("path parent missing"))?;
    if !parent.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(invalid("path outside broker-owned root"));
    }
    if !create_parent && !resolved.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(invalid("input resolves outside broker-owned root"));
    }
    Ok(resolved)
}

pub fn run(repository: &Path, request_path: &Path, output_path: &Path) -> io::Result<bool> {
    let request_path = resolve_inside(repository, request_path, "target/render-requests", false)?;
    let output_path = resolve_inside(
        repository,
        output_path,
        "target/render-request-results",
        true,
    )?;
    let metadata = fs::metadata(&request_path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > REQUEST_LIMIT {
        return Err(invalid("render request size invalid"));
    }
    let request: Request = serde_json::from_slice(&fs::read(request_path)?)
        .map_err(|error| invalid(format!("invalid render request: {error}")))?;
    if request.host_context.is_some() {
        return Err(invalid("host context requires SmartFX request execution"));
    }
    let profile = crate::fixture_profiles::find(&request.plugin_id)
        .ok_or_else(|| invalid("unknown plugin profile"))?;
    let manifest = descriptors(repository, &request.plugin_id, profile)?;
    let (_, assignment_count, errors) = evaluate(&request, &manifest.profile)?;
    let accepted = errors.is_empty();
    let report = Report {
        schema_version: 1,
        gate: "pre_dispatch_parameter_validation",
        plugin_id: request.plugin_id,
        assignment_count,
        accepted,
        native_dispatch_permitted: accepted,
        native_process_started: false,
        errors,
    };
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    Ok(accepted)
}

pub fn execute(repository: &Path, request_path: &Path, output_path: &Path) -> io::Result<bool> {
    crate::trace_policy::validate_broker_trace_directory(repository)?;
    let request_path = resolve_inside(repository, request_path, "target/render-requests", false)?;
    let output_path = resolve_inside(
        repository,
        output_path,
        "target/render-request-render-results",
        true,
    )?;
    let metadata = fs::metadata(&request_path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > REQUEST_LIMIT {
        return Err(invalid("render request size invalid"));
    }
    let request: Request = serde_json::from_slice(&fs::read(request_path)?)
        .map_err(|error| invalid(format!("invalid render request: {error}")))?;
    if request.host_context.is_some() {
        return Err(invalid("host context is not supported by classic render"));
    }
    let profile = crate::fixture_profiles::find(&request.plugin_id)
        .ok_or_else(|| invalid("unknown plugin profile"))?;
    let worker_spec = profile
        .classic_worker
        .ok_or_else(|| invalid("classic render is not supported for plugin profile"))?;
    let manifest = descriptors(repository, &request.plugin_id, profile)?;
    let (effective, assignment_count, errors) = evaluate(&request, &manifest.profile)?;
    if !errors.is_empty() {
        let report = json!({"schema_version":1,"stage":"parameterized_classic_render",
            "plugin_id":request.plugin_id,"assignment_count":assignment_count,"accepted":false,
            "native_process_started":false,"errors":errors,"passed":false});
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output_path)?;
        serde_json::to_writer_pretty(&mut output, &report)
            .map_err(|error| invalid(error.to_string()))?;
        output.write_all(b"\n")?;
        return Ok(false);
    }
    let worker = repository.join(worker_spec.executable);
    let expected = expected_hash(profile.parameterized_render, &effective);
    let payload = encode_worker_payload(&manifest.profile, &effective).map_err(invalid)?;
    let mut runs = Vec::new();
    let mut approved_fixture_sha256 = String::new();
    let mut approved_receipt_id = String::new();
    let mut approved_identity = None;
    for _ in 0..2 {
        // secure_launch consumes both the sealed tree and the worker stage, so
        // reload the schema-v2 receipt for each determinism run rather than
        // reusing mutable state. The plug-in reaches the worker through the
        // sealed tree instead of an argv path, closing the TOCTOU window a
        // worker-side re-open would leave (issue #312).
        let approved = load_v2_load_tree(repository, &request.plugin_id, worker_spec.selection)?;
        approved_receipt_id = approved.receipt_id.clone();
        let receipt_worker = if approved.worker_path.is_absolute() {
            approved.worker_path.clone()
        } else {
            repository.join(&approved.worker_path)
        };
        let fixture_sha256 = approved
            .main
            .expected_sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if !manifest.plugin_sha256.eq_ignore_ascii_case(&fixture_sha256) {
            return Err(invalid("descriptor manifest plugin digest mismatch"));
        }
        // Reported identity only: the digest is pinned against `manifest` above on
        // every run, and the cross-run drift check is the identity tuple below.
        if approved_fixture_sha256.is_empty() {
            approved_fixture_sha256 = fixture_sha256.to_ascii_uppercase();
        }
        let worker_sha256 = approved.worker_sha256;
        let worker_byte_size = approved.worker_byte_size;
        let timeout_ms = approved.timeout_ms;
        let image_set_digest = approved.image_set_digest();
        // The receipt is reloaded per determinism run, so pin the whole approved
        // identity across runs. The sealed manifest digest covers the main entry and
        // every dependency; the fixture digest alone would miss a swapped worker
        // build or dependency set (#312 review).
        let identity = ApprovalIdentity {
            sealed_manifest_sha256: image_set_digest,
            worker_sha256,
            worker_byte_size,
            timeout_ms,
        };
        let receipt_worker = admit_launch(
            &worker,
            &receipt_worker,
            approved_identity.as_ref(),
            &identity,
        )?;
        approved_identity = Some(identity);
        let args_before_plugin = [worker_spec.request_mode.to_string()];
        let args_after_plugin = [fixture_sha256, payload.clone()];
        let isolated = launch_approved_in_place(
            &approved,
            &receipt_worker,
            &args_before_plugin,
            &args_after_plugin,
            repository,
            timeout_ms,
        )?;
        let report: Value = serde_json::from_str(isolated.stdout.trim())
            .unwrap_or_else(|_| json!({"status":"worker_report_unavailable"}));
        runs.push((isolated.classification, report));
    }
    let valid_run = |classification: crate::ExitClassification, report: &Value| {
        classification.as_str() == "ok"
            && report.get("request_mode") == Some(&Value::Bool(true))
            && report.get("render_error") == Some(&json!(0))
            && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
            && worker_echo_matches(report, &manifest.profile, &effective)
            && report
                .get("output_sha256")
                .and_then(Value::as_str)
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&expected))
    };
    let deterministic = runs[0].1.get("output_sha256") == runs[1].1.get("output_sha256");
    let passed = deterministic
        && runs
            .iter()
            .all(|(classification, report)| valid_run(*classification, report));
    let summarize = |item: &(crate::ExitClassification, Value)| {
        json!({
        "classification":item.0.as_str(),"render_error":item.1.get("render_error"),
        "output_sha256":item.1.get("output_sha256"),"guard_bytes_intact":item.1.get("guard_bytes_intact"),
        "request_mode":item.1.get("request_mode"),
        "requested_parameters":item.1.get("requested_parameters")})
    };
    let report = json!({"schema_version":1,"stage":"parameterized_classic_render",
        "plugin_id":request.plugin_id,"receipt_id":approved_receipt_id,
        "fixture_sha256":approved_fixture_sha256,"assignment_count":assignment_count,
        "accepted":true,"native_process_started":true,"parameters":effective,
        "expected_oracle_sha256":expected,
        "run_1":summarize(&runs[0]),"run_2":summarize(&runs[1]),"deterministic":deterministic,
        "broker_survived":true,"passed":passed});
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    Ok(passed)
}

pub fn execute_smart(
    repository: &Path,
    request_path: &Path,
    output_path: &Path,
) -> io::Result<bool> {
    crate::trace_policy::validate_broker_trace_directory(repository)?;
    let request_path = resolve_inside(repository, request_path, "target/render-requests", false)?;
    let output_path = resolve_inside(
        repository,
        output_path,
        "target/smart-request-render-results",
        true,
    )?;
    let metadata = fs::metadata(&request_path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > REQUEST_LIMIT {
        return Err(invalid("render request size invalid"));
    }
    let request: Request = serde_json::from_slice(&fs::read(request_path)?)
        .map_err(|error| invalid(format!("invalid render request: {error}")))?;
    if request
        .host_context
        .as_ref()
        .is_some_and(|context| context.active_camera.is_some() || !context.scene_layers.is_empty())
    {
        return Err(invalid(
            "active camera is not supported by the fixture SmartFX render route",
        ));
    }
    let profile = crate::fixture_profiles::find(&request.plugin_id)
        .ok_or_else(|| invalid("unknown plugin profile"))?;
    let worker_spec = profile
        .smart_worker
        .ok_or_else(|| invalid("SmartFX render is not supported for plugin profile"))?;
    let manifest = descriptors(repository, &request.plugin_id, profile)?;
    let (effective, assignment_count, errors) = evaluate(&request, &manifest.profile)?;
    let host_context_counts = request
        .host_context
        .as_ref()
        .map(validate_mask_context)
        .transpose()?;
    let host_context_shape_counts = request.host_context.as_ref().map(|context| {
        let open_masks = context
            .mask_scene
            .masks
            .iter()
            .filter(|mask| mask.open)
            .count();
        let tangent_vertices = context
            .mask_scene
            .masks
            .iter()
            .flat_map(|mask| &mask.vertices)
            .filter(|point| {
                point
                    .tangent_in
                    .into_iter()
                    .chain(point.tangent_out)
                    .any(|value| value.x != 0.0 || value.y != 0.0)
            })
            .count();
        (open_masks, tangent_vertices)
    });
    let expected_mask_lifetime_count = request
        .host_context
        .as_ref()
        .map(|context| u64::from(!context.mask_scene.masks.is_empty()));
    if request.host_context.is_some() && worker_spec.request_mode != "--smart-mask-request" {
        return Err(invalid(
            "profile has no approved host mask context capability",
        ));
    }
    if !errors.is_empty() {
        let report = json!({"schema_version":1,"stage":"parameterized_smartfx_render",
            "plugin_id":request.plugin_id,"assignment_count":assignment_count,"accepted":false,
            "native_process_started":false,"errors":errors,"passed":false});
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output_path)?;
        serde_json::to_writer_pretty(&mut output, &report)
            .map_err(|error| invalid(error.to_string()))?;
        output.write_all(b"\n")?;
        return Ok(false);
    }
    let worker = repository.join(worker_spec.executable);
    let expected = if let Some(context) = &request.host_context {
        let masks = context
            .mask_scene
            .masks
            .iter()
            .map(|mask| OracleMask {
                open: mask.open,
                vertices: mask
                    .vertices
                    .iter()
                    .map(|point| {
                        let tangent_in = point.tangent_in.unwrap_or(MaskTangent { x: 0.0, y: 0.0 });
                        let tangent_out =
                            point.tangent_out.unwrap_or(MaskTangent { x: 0.0, y: 0.0 });
                        OracleMaskVertex {
                            x: point.x,
                            y: point.y,
                            tangent_in_x: tangent_in.x,
                            tangent_in_y: tangent_in.y,
                            tangent_out_x: tangent_out.x,
                            tangent_out_y: tangent_out.y,
                        }
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        bezier_mask_argb8_hash(&effective, &masks)
            .ok_or_else(|| invalid("host mask context has no independent oracle"))?
    } else {
        expected_hash(profile.parameterized_render, &effective)
    };
    let mut args = vec![
        if request.host_context.is_some() {
            "--smart-mask-context-request".to_string()
        } else {
            worker_spec.request_mode.to_string()
        },
        encode_worker_payload(&manifest.profile, &effective).map_err(invalid)?,
    ];
    if let Some(context) = &request.host_context {
        args.push(encode_mask_context(context)?);
    }
    let mut runs = Vec::new();
    let mut secure_launches = Vec::new();
    let mut approved_fixture_sha256 = String::new();
    let mut approved_receipt_id = String::new();
    let mut approved_identity = None;
    for _ in 0..2 {
        // secure_launch consumes both the sealed tree and worker stage. Reload the
        // schema-v2 receipt so each determinism run has an independent stage.
        let approved = load_v2_load_tree(repository, &request.plugin_id, worker_spec.selection)?;
        approved_receipt_id = approved.receipt_id.clone();
        let receipt_worker = if approved.worker_path.is_absolute() {
            approved.worker_path.clone()
        } else {
            repository.join(&approved.worker_path)
        };
        let plugin_basename = approved.main.relative_basename.clone();
        let fixture_sha256 = approved
            .main
            .expected_sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if !manifest.plugin_sha256.eq_ignore_ascii_case(&fixture_sha256) {
            return Err(invalid("descriptor manifest plugin digest mismatch"));
        }
        // Reported identity only: the digest is pinned against `manifest` above on
        // every run, and the cross-run drift check is the identity tuple below.
        if approved_fixture_sha256.is_empty() {
            approved_fixture_sha256 = fixture_sha256.to_ascii_uppercase();
        }
        let worker_sha256 = approved.worker_sha256;
        let worker_byte_size = approved.worker_byte_size;
        let timeout_ms = approved.timeout_ms;
        let image_set_digest = approved.image_set_digest();
        // The receipt is reloaded per determinism run, so pin the whole approved
        // identity across runs. The sealed manifest digest covers the main entry and
        // every dependency; the fixture digest alone would miss a swapped worker
        // build or dependency set (#312 review).
        let identity = ApprovalIdentity {
            sealed_manifest_sha256: image_set_digest,
            worker_sha256,
            worker_byte_size,
            timeout_ms,
        };
        let receipt_worker = admit_launch(
            &worker,
            &receipt_worker,
            approved_identity.as_ref(),
            &identity,
        )?;
        approved_identity = Some(identity);
        let sealed_manifest_sha256 = image_set_digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let args_before_plugin = [args[0].clone()];
        let mut args_after_plugin = vec![fixture_sha256, args[1].clone()];
        if let Some(host_context) = args.get(2) {
            args_after_plugin.push(host_context.clone());
        }
        let isolated = launch_approved_in_place(
            &approved,
            &receipt_worker,
            &args_before_plugin,
            &args_after_plugin,
            repository,
            timeout_ms,
        )?;
        let report: Value = serde_json::from_str(isolated.stdout.trim())
            .unwrap_or_else(|_| json!({"status":"worker_report_unavailable"}));
        secure_launches.push(json!({
            "launch_mode":"in_place_authenticated_worker",
            "worker_authenticated":true,
            "worker_size_bytes":worker_byte_size,
            "worker_sha256":worker_sha256.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            "plugin_basename":plugin_basename,
            "plugin_sha256":approved_fixture_sha256,
            "sealed_manifest_sha256":sealed_manifest_sha256,
            "module_audit_required":true,
            "module_audit":report.get("module_audit"),
            // Present only when the worker put a window on its private desktop
            // (issue #351). The one-shot report is hand-projected, so without
            // this the observation would exist on the launch result and in the
            // session diagnostics but vanish from the parameterized report.
            // Titles are path-redacted where they are captured.
            "dismissed_windows":isolated.dismissed_windows
        }));
        runs.push((isolated.classification, report));
    }
    let valid_run = |classification: crate::ExitClassification, report: &Value| {
        classification.as_str() == "ok"
            && report.get("request_mode") == Some(&Value::Bool(true))
            && report.get("pre_render_error") == Some(&json!(0))
            && report.get("smart_render_error") == Some(&json!(0))
            && report.get("result_rects_valid") == Some(&Value::Bool(true))
            && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
            && report
                .get("suite_acquires")
                .and_then(Value::as_u64)
                .is_some_and(|acquires| {
                    report
                        .get("suite_releases")
                        .and_then(Value::as_u64)
                        .is_some_and(|releases| {
                            report
                                .get("live_suite_reference_count")
                                .and_then(Value::as_u64)
                                == Some(acquires.saturating_sub(releases))
                                && acquires >= releases
                                && acquires <= 64
                        })
                })
            && report.get("handle_lifetimes_balanced") == Some(&Value::Bool(true))
            && report.get("live_handle_count").and_then(Value::as_u64) == Some(0)
            && report.get("live_handle_bytes").and_then(Value::as_u64) == Some(0)
            && report
                .get("invalid_handle_operations")
                .and_then(Value::as_u64)
                == Some(0)
            && report.get("handles_created") == report.get("handles_disposed")
            && report.get("handle_locks") == report.get("handle_unlocks")
            && worker_echo_matches(report, &manifest.profile, &effective)
            && request.host_context.as_ref().is_none_or(|_| {
                report.get("mask_scene_id").and_then(Value::as_str) == Some("request_v4")
                    && report.get("mask_count").and_then(Value::as_u64)
                        == host_context_counts.map(|counts| counts.0 as u64)
                    && report.get("mask_open_count").and_then(Value::as_u64)
                        == host_context_shape_counts.map(|counts| counts.0 as u64)
                    && report
                        .get("mask_tangent_vertex_count")
                        .and_then(Value::as_u64)
                        == host_context_shape_counts.map(|counts| counts.1 as u64)
                    && report.get("mask_lifetimes_balanced") == Some(&Value::Bool(true))
                    && [
                        "mask_handles_acquired",
                        "mask_handles_disposed",
                        "stream_handles_acquired",
                        "stream_handles_disposed",
                        "stream_values_acquired",
                        "stream_values_disposed",
                    ]
                    .iter()
                    .all(|field| {
                        report.get(*field).and_then(Value::as_u64) == expected_mask_lifetime_count
                    })
            })
            && report
                .get("output_sha256")
                .and_then(Value::as_str)
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&expected))
    };
    let deterministic = runs[0].1.get("output_sha256") == runs[1].1.get("output_sha256");
    let passed = deterministic
        && runs
            .iter()
            .all(|(classification, report)| valid_run(*classification, report));
    let summarize = |item: &(crate::ExitClassification, Value)| {
        json!({
        "classification":item.0.as_str(),"pre_render_error":item.1.get("pre_render_error"),
        "smart_render_error":item.1.get("smart_render_error"),"output_sha256":item.1.get("output_sha256"),
        "result_rects_valid":item.1.get("result_rects_valid"),
        "guard_bytes_intact":item.1.get("guard_bytes_intact"),"request_mode":item.1.get("request_mode"),
        "mask_scene_id":item.1.get("mask_scene_id"),"mask_count":item.1.get("mask_count"),
        "mask_open_count":item.1.get("mask_open_count"),
        "mask_tangent_vertex_count":item.1.get("mask_tangent_vertex_count"),
        "mask_lifetimes_balanced":item.1.get("mask_lifetimes_balanced"),
        "mask_handles_acquired":item.1.get("mask_handles_acquired"),
        "mask_handles_disposed":item.1.get("mask_handles_disposed"),
        "stream_handles_acquired":item.1.get("stream_handles_acquired"),
        "stream_handles_disposed":item.1.get("stream_handles_disposed"),
        "stream_values_acquired":item.1.get("stream_values_acquired"),
        "stream_values_disposed":item.1.get("stream_values_disposed"),
        "suite_leases_balanced":item.1.get("suite_leases_balanced"),
        "suite_acquires":item.1.get("suite_acquires"),
        "suite_releases":item.1.get("suite_releases"),
        "live_suite_lease_count":item.1.get("live_suite_lease_count"),
        "live_suite_reference_count":item.1.get("live_suite_reference_count"),
        "live_suite_leases":item.1.get("live_suite_leases"),
        "handle_lifetimes_balanced":item.1.get("handle_lifetimes_balanced"),
        "handles_created":item.1.get("handles_created"),
        "handles_disposed":item.1.get("handles_disposed"),
        "handle_locks":item.1.get("handle_locks"),
        "handle_unlocks":item.1.get("handle_unlocks"),
        "live_handle_count":item.1.get("live_handle_count"),
        "live_handle_bytes":item.1.get("live_handle_bytes"),
        "invalid_handle_operations":item.1.get("invalid_handle_operations"),
        "requested_parameters":item.1.get("requested_parameters")})
    };
    let report = json!({"schema_version":1,"stage":"parameterized_smartfx_render",
        "plugin_id":request.plugin_id,"receipt_id":approved_receipt_id,
        "fixture_sha256":approved_fixture_sha256,"assignment_count":assignment_count,
        "accepted":true,"native_process_started":true,"parameters":effective,
        "host_context_mask_count":host_context_counts.map(|counts| counts.0),
        "host_context_vertex_count":host_context_counts.map(|counts| counts.1),
        "host_context_open_mask_count":host_context_shape_counts.map(|counts| counts.0),
        "host_context_tangent_vertex_count":host_context_shape_counts.map(|counts| counts.1),
        "expected_oracle_sha256":expected,
        "run_1":summarize(&runs[0]),"run_2":summarize(&runs[1]),
        "secure_launch_1":secure_launches[0],"secure_launch_2":secure_launches[1],
        "secure_launch_count":2,"normal_token_fallback":false,"deterministic":deterministic,
        "broker_survived":true,"passed":passed});
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    Ok(passed)
}

pub fn execute_smart_suite_fault(
    repository: &Path,
    plugin_id: &str,
    fault_id: &str,
    output_path: &Path,
) -> io::Result<bool> {
    crate::trace_policy::validate_broker_trace_directory(repository)?;
    let (
        worker_mode,
        expect_crash,
        expect_lifetime_rejection,
        expect_suite_rejection,
        expect_handle_rejection,
        expect_world_rejection,
        expect_pixel_format_rejection,
        expect_outline_rejection,
    ) = match fault_id {
        "mask_count_error" => (
            "--smart-mask-count-error-request",
            false,
            false,
            false,
            false,
            false,
            false,
            false,
        ),
        "mask_count_crash" => (
            "--smart-mask-count-crash-request",
            true,
            false,
            false,
            false,
            false,
            false,
            false,
        ),
        "mask_double_dispose" => (
            "--smart-mask-double-dispose-request",
            false,
            true,
            false,
            false,
            false,
            false,
            false,
        ),
        "stream_dispose_with_live_value" => (
            "--smart-stream-live-value-dispose-request",
            false,
            true,
            false,
            false,
            false,
            false,
            false,
        ),
        "suite_release_without_acquire" => (
            "--smart-suite-release-without-acquire-request",
            false,
            false,
            true,
            false,
            false,
            false,
            false,
        ),
        "handle_resize_while_locked" => (
            "--smart-handle-resize-while-locked-request",
            false,
            false,
            false,
            true,
            false,
            false,
            false,
        ),
        "outline_mutation" => (
            "--smart-outline-mutation-request",
            false,
            false,
            false,
            false,
            false,
            false,
            true,
        ),
        "mask_attribute_ownership" => (
            "--smart-mask-attribute-request",
            false,
            false,
            false,
            false,
            false,
            false,
            true,
        ),
        "stream_metadata_ownership" => (
            "--smart-stream-metadata-ownership-request",
            false,
            false,
            false,
            false,
            false,
            false,
            true,
        ),
        "keyframe_ownership" => (
            "--smart-keyframe-ownership-request",
            false,
            false,
            false,
            false,
            false,
            false,
            true,
        ),
        "dynamic_stream_tree" => (
            "--smart-dynamic-stream-tree-request",
            false,
            false,
            false,
            false,
            false,
            false,
            true,
        ),
        "aegp_memory_strings" => (
            "--smart-aegp-memory-strings-request",
            false,
            false,
            false,
            false,
            false,
            false,
            true,
        ),
        "world_double_dispose" => (
            "--smart-world-double-dispose-request",
            false,
            false,
            false,
            false,
            true,
            false,
            false,
        ),
        "pixel_format_registry" => (
            "--smart-pixel-format-registry-request",
            false,
            false,
            false,
            false,
            false,
            true,
            false,
        ),
        "world_allocation_limit" => (
            "--smart-world-allocation-limit-request",
            false,
            false,
            false,
            false,
            true,
            false,
            false,
        ),
        _ => return Err(invalid("unknown fixed suite fault")),
    };
    let output_path = resolve_inside(
        repository,
        output_path,
        "target/smart-suite-fault-results",
        true,
    )?;
    let profile = crate::fixture_profiles::find(plugin_id)
        .ok_or_else(|| invalid("unknown plugin profile"))?;
    let worker_spec = profile
        .smart_worker
        .filter(|spec| spec.request_mode == "--smart-mask-request")
        .ok_or_else(|| invalid("profile has no approved mask-suite fault capability"))?;
    let manifest = descriptors(repository, plugin_id, profile)?;
    let effective = apply_defaults(&manifest.profile, &ValidatedAssignments::new());
    let worker = repository.join(worker_spec.executable);
    let payload = encode_worker_payload(&manifest.profile, &effective).map_err(invalid)?;
    let mut runs = Vec::new();
    let mut approved_fixture_sha256 = String::new();
    let mut approved_receipt_id = String::new();
    let mut approved_identity = None;
    for _ in 0..2 {
        // secure_launch consumes both the sealed tree and the worker stage, so
        // reload the schema-v2 receipt for each determinism run. The plug-in
        // reaches the worker through the sealed tree instead of an argv path
        // (issue #312).
        let approved = load_v2_load_tree(repository, plugin_id, worker_spec.selection)?;
        approved_receipt_id = approved.receipt_id.clone();
        let receipt_worker = if approved.worker_path.is_absolute() {
            approved.worker_path.clone()
        } else {
            repository.join(&approved.worker_path)
        };
        let fixture_sha256 = approved
            .main
            .expected_sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if !manifest.plugin_sha256.eq_ignore_ascii_case(&fixture_sha256) {
            return Err(invalid("descriptor manifest plugin digest mismatch"));
        }
        // Reported identity only: the digest is pinned against `manifest` above on
        // every run, and the cross-run drift check is the identity tuple below.
        if approved_fixture_sha256.is_empty() {
            approved_fixture_sha256 = fixture_sha256.to_ascii_uppercase();
        }
        let worker_sha256 = approved.worker_sha256;
        let worker_byte_size = approved.worker_byte_size;
        let timeout_ms = approved.timeout_ms;
        let image_set_digest = approved.image_set_digest();
        // The receipt is reloaded per determinism run, so pin the whole approved
        // identity across runs. The sealed manifest digest covers the main entry and
        // every dependency; the fixture digest alone would miss a swapped worker
        // build or dependency set (#312 review).
        let identity = ApprovalIdentity {
            sealed_manifest_sha256: image_set_digest,
            worker_sha256,
            worker_byte_size,
            timeout_ms,
        };
        let receipt_worker = admit_launch(
            &worker,
            &receipt_worker,
            approved_identity.as_ref(),
            &identity,
        )?;
        approved_identity = Some(identity);
        let args_before_plugin = [worker_mode.to_string()];
        let args_after_plugin = [fixture_sha256, payload.clone()];
        let isolated = launch_approved_in_place(
            &approved,
            &receipt_worker,
            &args_before_plugin,
            &args_after_plugin,
            repository,
            timeout_ms,
        )?;
        let report: Value = serde_json::from_str(isolated.stdout.trim())
            .unwrap_or_else(|_| json!({"status":"worker_report_unavailable"}));
        runs.push((isolated.classification, report));
    }
    let fallback_hash = source_argb8_hash();
    let fallback_valid = |classification: crate::ExitClassification, report: &Value| {
        classification.as_str() == "ok"
            && report.get("pre_render_error") == Some(&json!(0))
            && report.get("smart_render_error") == Some(&json!(0))
            && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
            && worker_echo_matches(report, &manifest.profile, &effective)
            && report
                .get("output_sha256")
                .and_then(Value::as_str)
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&fallback_hash))
    };
    let passed = if expect_crash {
        runs.iter()
            .all(|(classification, _)| classification.as_str() == "crashed")
    } else if expect_lifetime_rejection {
        runs.iter().all(|(classification, report)| {
            classification.as_str() == "ok"
                && report.get("pre_render_error") == Some(&json!(0))
                && report.get("smart_render_error") == Some(&json!(0))
                && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
                && report.get("lifetime_fault_observed") == Some(&Value::Bool(true))
                && report.get("mask_lifetimes_balanced") == Some(&Value::Bool(true))
        })
    } else if expect_suite_rejection {
        runs.iter().all(|(classification, report)| {
            classification.as_str() == "ok"
                && report.get("pre_render_error") == Some(&json!(0))
                && report.get("smart_render_error") == Some(&json!(0))
                && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
                && report.get("suite_fault_observed") == Some(&Value::Bool(true))
                && report
                    .get("suite_acquires")
                    .and_then(Value::as_u64)
                    .is_some_and(|acquires| {
                        report
                            .get("suite_releases")
                            .and_then(Value::as_u64)
                            .is_some_and(|releases| {
                                report
                                    .get("live_suite_reference_count")
                                    .and_then(Value::as_u64)
                                    == Some(acquires.saturating_sub(releases))
                                    && acquires >= releases
                            })
                    })
        })
    } else if expect_handle_rejection {
        runs.iter().all(|(classification, report)| {
            classification.as_str() == "ok"
                && report.get("pre_render_error") == Some(&json!(0))
                && report.get("smart_render_error") == Some(&json!(0))
                && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
                && report.get("handle_fault_observed") == Some(&Value::Bool(true))
                && report.get("handle_lifetimes_balanced") == Some(&Value::Bool(true))
                && report.get("live_handle_count") == Some(&json!(0))
                && report.get("live_handle_bytes") == Some(&json!(0))
                && report.get("invalid_handle_operations") == Some(&json!(1))
                && report.get("handles_created") == report.get("handles_disposed")
                && report.get("handle_locks") == report.get("handle_unlocks")
        })
    } else if expect_world_rejection {
        runs.iter().all(|(classification, report)| {
            classification.as_str() == "ok"
                && report.get("pre_render_error") == Some(&json!(0))
                && report.get("smart_render_error") == Some(&json!(0))
                && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
                && report.get("world_fault_observed") == Some(&Value::Bool(true))
                && report.get("world_lifetimes_balanced") == Some(&Value::Bool(true))
                && report.get("worlds_created") == report.get("worlds_disposed")
                && report.get("live_world_count") == Some(&json!(0))
                && report.get("live_world_bytes") == Some(&json!(0))
                && report.get("invalid_world_operations") == Some(&json!(1))
        })
    } else if expect_pixel_format_rejection {
        runs.iter().all(|(classification, report)| {
            classification.as_str() == "ok"
                && report.get("pre_render_error") == Some(&json!(0))
                && report.get("smart_render_error") == Some(&json!(0))
                && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
                && report.get("pixel_format_fault_observed") == Some(&Value::Bool(true))
                && report.get("pixel_format_add_calls") == Some(&json!(3))
                && report.get("pixel_format_clear_calls") == Some(&json!(2))
                && report.get("supported_pixel_format_count") == Some(&json!(0))
                && report.get("invalid_pixel_format_operations") == Some(&json!(2))
        })
    } else if expect_outline_rejection {
        runs.iter().all(|(classification, report)| {
            classification.as_str() == "ok"
                && report.get("pre_render_error") == Some(&json!(0))
                && report.get("smart_render_error") == Some(&json!(0))
                && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
                && (if fault_id == "outline_mutation" {
                    report.get("outline_fault_observed") == Some(&Value::Bool(true))
                        && report.get("outline_mutations") == Some(&json!(8))
                        && report.get("invalid_outline_operations") == Some(&json!(1))
                } else if fault_id == "stream_metadata_ownership" {
                    report.get("stream_metadata_fault_observed") == Some(&Value::Bool(true))
                        && report.get("stream_metadata_queries") == Some(&json!(9))
                        && report.get("stream_duplicates") == Some(&json!(1))
                        && report.get("invalid_stream_operations") == Some(&json!(2))
                } else if fault_id == "keyframe_ownership" {
                    report.get("keyframe_fault_observed") == Some(&Value::Bool(true))
                        && report.get("keyframe_mutations") == Some(&json!(12))
                        && report.get("invalid_keyframe_operations") == Some(&json!(2))
                } else if fault_id == "dynamic_stream_tree" {
                    report.get("dynamic_stream_fault_observed") == Some(&Value::Bool(true))
                        && report.get("dynamic_stream_mutations") == Some(&json!(8))
                        && report.get("invalid_dynamic_stream_operations") == Some(&json!(1))
                } else if fault_id == "aegp_memory_strings" {
                    report.get("aegp_memory_fault_observed") == Some(&Value::Bool(true))
                        && report.get("aegp_memory_created") == Some(&json!(3))
                        && report.get("aegp_memory_freed") == Some(&json!(3))
                        && report.get("live_aegp_memory_handles") == Some(&json!(0))
                        && report.get("live_aegp_memory_bytes") == Some(&json!(0))
                        && report.get("invalid_aegp_memory_operations") == Some(&json!(1))
                } else {
                    report.get("mask_attribute_fault_observed") == Some(&Value::Bool(true))
                        && report.get("mask_mutations") == Some(&json!(11))
                        && report.get("invalid_mask_operations") == Some(&json!(1))
                })
                && report.get("mask_lifetimes_balanced") == Some(&Value::Bool(true))
        })
    } else {
        runs.iter()
            .all(|(classification, report)| fallback_valid(*classification, report))
    };
    let summarize = |item: &(crate::ExitClassification, Value)| {
        let mut summary = json!({
            "classification":item.0.as_str(),
            "pre_render_error":item.1.get("pre_render_error"),
            "smart_render_error":item.1.get("smart_render_error"),
            "output_sha256":item.1.get("output_sha256"),
            "guard_bytes_intact":item.1.get("guard_bytes_intact"),
            "lifetime_fault_observed":item.1.get("lifetime_fault_observed"),
            "mask_lifetimes_balanced":item.1.get("mask_lifetimes_balanced"),
            "suite_fault_observed":item.1.get("suite_fault_observed"),
            "suite_leases_balanced":item.1.get("suite_leases_balanced"),
            "suite_acquires":item.1.get("suite_acquires"),
            "suite_releases":item.1.get("suite_releases"),
            "live_suite_lease_count":item.1.get("live_suite_lease_count"),
            "live_suite_leases":item.1.get("live_suite_leases"),
            "live_suite_reference_count":item.1.get("live_suite_reference_count"),
            "handle_fault_observed":item.1.get("handle_fault_observed"),
            "handle_lifetimes_balanced":item.1.get("handle_lifetimes_balanced"),
            "handles_created":item.1.get("handles_created"),
            "handles_disposed":item.1.get("handles_disposed"),
            "handle_locks":item.1.get("handle_locks"),
            "handle_unlocks":item.1.get("handle_unlocks"),
            "live_handle_count":item.1.get("live_handle_count"),
            "live_handle_bytes":item.1.get("live_handle_bytes"),
            "invalid_handle_operations":item.1.get("invalid_handle_operations"),
            "world_fault_observed":item.1.get("world_fault_observed"),
            "world_lifetimes_balanced":item.1.get("world_lifetimes_balanced"),
            "worlds_created":item.1.get("worlds_created"),
            "worlds_disposed":item.1.get("worlds_disposed"),
            "live_world_count":item.1.get("live_world_count"),
            "live_world_bytes":item.1.get("live_world_bytes"),
            "invalid_world_operations":item.1.get("invalid_world_operations"),
            "pixel_format_fault_observed":item.1.get("pixel_format_fault_observed"),
            "pixel_format_add_calls":item.1.get("pixel_format_add_calls"),
            "pixel_format_clear_calls":item.1.get("pixel_format_clear_calls"),
            "supported_pixel_format_count":item.1.get("supported_pixel_format_count"),
            "invalid_pixel_format_operations":item.1.get("invalid_pixel_format_operations"),
            "outline_fault_observed":item.1.get("outline_fault_observed"),
            "outline_mutations":item.1.get("outline_mutations"),
            "invalid_outline_operations":item.1.get("invalid_outline_operations"),
            "mask_attribute_fault_observed":item.1.get("mask_attribute_fault_observed"),
            "mask_mutations":item.1.get("mask_mutations"),
            "invalid_mask_operations":item.1.get("invalid_mask_operations")
        });
        let object = summary.as_object_mut().expect("summary is an object");
        for field in [
            "stream_metadata_fault_observed",
            "stream_metadata_queries",
            "stream_duplicates",
            "invalid_stream_operations",
            "keyframe_fault_observed",
            "keyframe_mutations",
            "invalid_keyframe_operations",
            "dynamic_stream_fault_observed",
            "dynamic_stream_queries",
            "dynamic_stream_mutations",
            "invalid_dynamic_stream_operations",
            "aegp_memory_fault_observed",
            "aegp_memory_created",
            "aegp_memory_freed",
            "live_aegp_memory_handles",
            "live_aegp_memory_bytes",
            "invalid_aegp_memory_operations",
        ] {
            object.insert(
                field.to_string(),
                item.1.get(field).cloned().unwrap_or(Value::Null),
            );
        }
        summary
    };
    let report = json!({
        "schema_version":1,"stage":"smartfx_suite_fault","plugin_id":plugin_id,
        "receipt_id":approved_receipt_id,"fixture_sha256":approved_fixture_sha256,
        "fault_id":fault_id,"expected_outcome":if expect_crash { "worker_crash" } else if expect_lifetime_rejection || expect_suite_rejection || expect_handle_rejection || expect_world_rejection || expect_pixel_format_rejection || expect_outline_rejection { "callback_error_rejected" } else { "plugin_fallback" },
        "expected_fallback_sha256":if expect_crash || expect_lifetime_rejection || expect_suite_rejection || expect_handle_rejection || expect_world_rejection || expect_pixel_format_rejection || expect_outline_rejection { Value::Null } else { json!(fallback_hash) },
        "run_1":summarize(&runs[0]),"run_2":summarize(&runs[1]),
        "broker_survived":true,"passed":passed
    });
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    Ok(passed)
}

pub fn execute_smart_mask_scene(
    repository: &Path,
    plugin_id: &str,
    scene_case_id: &str,
    output_path: &Path,
) -> io::Result<bool> {
    crate::trace_policy::validate_broker_trace_directory(repository)?;
    let (scene_id, mask_index, expected_count) = match scene_case_id {
        "empty" => ("empty", 1.0, 0),
        "translated_rectangle" => ("translated_rectangle", 1.0, 1),
        "two_rectangles_first" => ("two_rectangles", 1.0, 2),
        "two_rectangles_second" => ("two_rectangles", 2.0, 2),
        _ => return Err(invalid("unknown fixed mask scene")),
    };
    let output_path = resolve_inside(
        repository,
        output_path,
        "target/smart-mask-scene-results",
        true,
    )?;
    let profile = crate::fixture_profiles::find(plugin_id)
        .ok_or_else(|| invalid("unknown plugin profile"))?;
    let worker_spec = profile
        .smart_worker
        .filter(|spec| spec.request_mode == "--smart-mask-request")
        .ok_or_else(|| invalid("profile has no approved mask-scene capability"))?;
    let manifest = descriptors(repository, plugin_id, profile)?;
    let mut effective = apply_defaults(&manifest.profile, &ValidatedAssignments::new());
    effective.insert("mask_index".into(), ParameterValue::Numeric(mask_index));
    let expected = mask_scene_argb8_hash(&effective, scene_id)
        .ok_or_else(|| invalid("mask scene has no independent oracle"))?;
    let worker = repository.join(worker_spec.executable);
    let payload = encode_worker_payload(&manifest.profile, &effective).map_err(invalid)?;
    let mut runs = Vec::new();
    let mut approved_fixture_sha256 = String::new();
    let mut approved_receipt_id = String::new();
    let mut approved_identity = None;
    for _ in 0..2 {
        // secure_launch consumes both the sealed tree and the worker stage, so
        // reload the schema-v2 receipt for each determinism run. The plug-in
        // reaches the worker through the sealed tree instead of an argv path
        // (issue #312).
        let approved = load_v2_load_tree(repository, plugin_id, worker_spec.selection)?;
        approved_receipt_id = approved.receipt_id.clone();
        let receipt_worker = if approved.worker_path.is_absolute() {
            approved.worker_path.clone()
        } else {
            repository.join(&approved.worker_path)
        };
        let fixture_sha256 = approved
            .main
            .expected_sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if !manifest.plugin_sha256.eq_ignore_ascii_case(&fixture_sha256) {
            return Err(invalid("descriptor manifest plugin digest mismatch"));
        }
        // Reported identity only: the digest is pinned against `manifest` above on
        // every run, and the cross-run drift check is the identity tuple below.
        if approved_fixture_sha256.is_empty() {
            approved_fixture_sha256 = fixture_sha256.to_ascii_uppercase();
        }
        let worker_sha256 = approved.worker_sha256;
        let worker_byte_size = approved.worker_byte_size;
        let timeout_ms = approved.timeout_ms;
        let image_set_digest = approved.image_set_digest();
        // The receipt is reloaded per determinism run, so pin the whole approved
        // identity across runs. The sealed manifest digest covers the main entry and
        // every dependency; the fixture digest alone would miss a swapped worker
        // build or dependency set (#312 review).
        let identity = ApprovalIdentity {
            sealed_manifest_sha256: image_set_digest,
            worker_sha256,
            worker_byte_size,
            timeout_ms,
        };
        let receipt_worker = admit_launch(
            &worker,
            &receipt_worker,
            approved_identity.as_ref(),
            &identity,
        )?;
        approved_identity = Some(identity);
        let args_before_plugin = ["--smart-mask-scene-request".to_string()];
        let args_after_plugin = [fixture_sha256, payload.clone(), scene_id.to_string()];
        let isolated = launch_approved_in_place(
            &approved,
            &receipt_worker,
            &args_before_plugin,
            &args_after_plugin,
            repository,
            timeout_ms,
        )?;
        let report: Value = serde_json::from_str(isolated.stdout.trim())
            .unwrap_or_else(|_| json!({"status":"worker_report_unavailable"}));
        runs.push((isolated.classification, report));
    }
    let valid_run = |classification: crate::ExitClassification, report: &Value| {
        classification.as_str() == "ok"
            && report.get("pre_render_error") == Some(&json!(0))
            && report.get("smart_render_error") == Some(&json!(0))
            && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
            && report.get("mask_scene_id").and_then(Value::as_str) == Some(scene_id)
            && report.get("mask_count").and_then(Value::as_u64) == Some(expected_count)
            && worker_echo_matches(report, &manifest.profile, &effective)
            && report
                .get("output_sha256")
                .and_then(Value::as_str)
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&expected))
    };
    let deterministic = runs[0].1.get("output_sha256") == runs[1].1.get("output_sha256");
    let passed = deterministic
        && runs
            .iter()
            .all(|(classification, report)| valid_run(*classification, report));
    let summarize = |item: &(crate::ExitClassification, Value)| {
        json!({
            "classification":item.0.as_str(),
            "pre_render_error":item.1.get("pre_render_error"),
            "smart_render_error":item.1.get("smart_render_error"),
            "output_sha256":item.1.get("output_sha256"),
            "guard_bytes_intact":item.1.get("guard_bytes_intact"),
            "mask_scene_id":item.1.get("mask_scene_id"),
            "mask_count":item.1.get("mask_count")
        })
    };
    let report = json!({
        "schema_version":1,"stage":"smartfx_mask_scene","plugin_id":plugin_id,
        "receipt_id":approved_receipt_id,"fixture_sha256":approved_fixture_sha256,
        "scene_case_id":scene_case_id,"host_scene_id":scene_id,"mask_index":mask_index,
        "expected_mask_count":expected_count,"expected_oracle_sha256":expected,
        "run_1":summarize(&runs[0]),"run_2":summarize(&runs[1]),
        "deterministic":deterministic,"broker_survived":true,"passed":passed
    });
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    serde_json::to_writer_pretty(&mut output, &report)
        .map_err(|error| invalid(error.to_string()))?;
    output.write_all(b"\n")?;
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aux_channel_json_is_strict_and_f32le_only() {
        let json = r#"{"mask_scene":{"masks":[]},"aux_channels":[{"param_index":0,"channel":{"type":1146111048,"name":"depth","data_type":"f32le","dimension":1,"width":2,"height":1,"samples":[{"time":0,"time_scale":30,"path":"target/render-requests/depth.f32","sampling":"exact","interpretation":"depth"}]}}]}"#;
        let context: HostContext = serde_json::from_str(json).unwrap();
        assert_eq!(context.aux_channels.len(), 1);
        assert!(
            serde_json::from_str::<HostContext>(
                &json.replace("\"data_type\":\"f32le\"", "\"data_type\":\"f64le\"")
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<HostContext>(
                &json.replace("\"dimension\":1", "\"dimension\":1,\"unknown\":true")
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<HostContext>(
                &json.replace("\"param_index\":0", "\"param_index\":0,\"param_index\":1")
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<HostContext>(
                &json.replace("\"sampling\":\"exact\"", "\"sampling\":\"nearest\"")
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<HostContext>(&json.replace(
                "\"interpretation\":\"depth\"",
                "\"interpretation\":\"position\""
            ))
            .is_err()
        );
    }

    #[test]
    fn image_buffer_layout_checks_stride_capacity_and_limits() {
        assert_eq!(
            validate_image_buffer_layout(13, 9, 64, 4, Some(576), 4096, 16_777_216, 67_108_864)
                .unwrap(),
            576
        );
        assert!(
            validate_image_buffer_layout(13, 9, 51, 4, Some(576), 4096, 16_777_216, 67_108_864)
                .is_err()
        );
        assert!(
            validate_image_buffer_layout(13, 9, 64, 4, Some(575), 4096, 16_777_216, 67_108_864)
                .is_err()
        );
        assert!(
            validate_image_buffer_layout(
                4096, 4096, 65_536, 16, None, 4096, 16_777_216, 67_108_864
            )
            .is_err()
        );
    }

    #[test]
    fn image_buffer_layout_rejects_zero_and_checked_arithmetic_overflow() {
        assert!(
            validate_image_buffer_layout(0, 1, 4, 4, None, u64::MAX, u64::MAX, u64::MAX).is_err()
        );
        assert!(
            validate_image_buffer_layout(
                u64::MAX,
                2,
                u64::MAX,
                1,
                None,
                u64::MAX,
                u64::MAX,
                u64::MAX
            )
            .is_err()
        );
        assert!(
            validate_image_buffer_layout(1, 2, u64::MAX, 1, None, u64::MAX, u64::MAX, u64::MAX)
                .is_err()
        );
    }
    use serde_json::Value;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn repository() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-render-request-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("target/render-requests")).unwrap();
        fs::create_dir_all(root.join("profiles/scattermap")).unwrap();
        fs::create_dir_all(root.join("profiles/maskoffset")).unwrap();
        fs::write(
            root.join("profiles/scattermap/parameter_descriptors.json"),
            include_bytes!("../../../../profiles/scattermap/parameter_descriptors.json"),
        )
        .unwrap();
        fs::write(
            root.join("profiles/maskoffset/parameter_descriptors.json"),
            include_bytes!("../../../../profiles/maskoffset/parameter_descriptors.json"),
        )
        .unwrap();
        root
    }

    #[test]
    fn generic_worker_echo_is_descriptor_order_and_value_bound() {
        let profile = PluginProfile {
            id: "example".into(),
            descriptors: vec![crate::host_core::parameter::Descriptor {
                id: "radius".into(),
                display_name: "Radius".into(),
                slot: 3,
                observed_type: 10,
                minimum: Some(0.0),
                maximum: Some(10.0),
                default_value: ParameterValue::Numeric(2.5),
                kind: ValueKind::Float,
            }],
        };
        let effective =
            ValidatedAssignments::from([("radius".into(), ParameterValue::Numeric(2.5))]);
        let valid = json!({"requested_parameters":[
            {"id":"radius","slot":3,"kind":"float","value":2.5}
        ]});
        assert!(worker_echo_matches(&valid, &profile, &effective));
        let drifted = json!({"requested_parameters":[
            {"id":"radius","slot":4,"kind":"float","value":2.5}
        ]});
        assert!(!worker_echo_matches(&drifted, &profile, &effective));
    }

    #[test]
    fn accepted_request_still_does_not_start_native_code() {
        let root = repository();
        let request = root.join("target/render-requests/valid.json");
        let output = root.join("target/render-request-results/valid.json");
        fs::write(&request, br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"amount":500,"direction":1,"seed":10000,"mix":0,"invert_map":1}}"#).unwrap();
        assert!(run(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
        assert_eq!(report["native_dispatch_permitted"], true);
        assert_eq!(report["native_process_started"], false);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn color_requires_v3_and_is_accepted_without_native_dispatch() {
        let root = repository();
        let request = root.join("target/render-requests/color.json");
        let output = root.join("target/render-request-results/color.json");
        let assignments = r#""fill_color":{"alpha":255,"red":20,"green":180,"blue":70}"#;
        fs::write(
            &request,
            format!(
                r#"{{"schema_version":2,"plugin_id":"maskoffset","assignments":{{{assignments}}}}}"#
            ),
        )
        .unwrap();
        assert!(run(&root, &request, &output).is_err());
        assert!(!output.exists());
        fs::write(
            &request,
            format!(
                r#"{{"schema_version":3,"plugin_id":"maskoffset","assignments":{{{assignments}}}}}"#
            ),
        )
        .unwrap();
        assert!(run(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(report["accepted"], true);
        assert_eq!(report["native_process_started"], false);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_suite_fault_fails_before_output_or_native_lookup() {
        let root = repository();
        let output = root.join("target/smart-suite-fault-results/unknown.json");
        assert!(execute_smart_suite_fault(&root, "maskoffset", "arbitrary", &output).is_err());
        assert!(!output.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_mask_scene_fails_before_output_or_native_lookup() {
        let root = repository();
        let output = root.join("target/smart-mask-scene-results/unknown.json");
        assert!(execute_smart_mask_scene(&root, "maskoffset", "arbitrary", &output).is_err());
        assert!(!output.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn request_v4_mask_context_is_bounded_and_transport_stable() {
        let request: Request = serde_json::from_str(
            r#"{"schema_version":4,"plugin_id":"maskoffset","assignments":{},"host_context":{"mask_scene":{"masks":[{"open":false,"vertices":[{"x":2,"y":2},{"x":10,"y":2},{"x":10,"y":8},{"x":2,"y":8}]}]}}}"#,
        )
        .unwrap();
        let context = request.host_context.as_ref().unwrap();
        assert_eq!(validate_mask_context(context).unwrap(), (1, 4));
        assert_eq!(
            encode_mask_context(context).unwrap(),
            "v2|0:2,2,0,0,0,0/10,2,0,0,0,0/10,8,0,0,0,0/2,8,0,0,0,0"
        );

        let profile = PluginProfile {
            id: "maskoffset".into(),
            descriptors: vec![],
        };
        assert!(evaluate(&request, &profile).is_ok());
    }

    #[test]
    fn request_v4_retains_authored_layer_parent_graph() {
        let input = json!({
            "mask_scene": {"masks": []},
            "scene_layers": [
                {
                    "layer": {"project_id": 1, "object_id": 2801, "generation": 1, "index": 0},
                    "parent": {"project_id": 1, "object_id": 2802, "generation": 1, "index": 1},
                    "anchor": [0, 0, 0], "position": [10, 20, 30],
                    "scale": [100, 100, 100], "rotation_degrees": [0, 0, 0], "is_3d": true
                },
                {
                    "layer": {"project_id": 1, "object_id": 2802, "generation": 1, "index": 1},
                    "parent": null,
                    "anchor": [0, 0, 0], "position": [40, 50, 60],
                    "scale": [100, 100, 100], "rotation_degrees": [0, 0, 0], "is_3d": true
                }
            ]
        });
        let context: HostContext = serde_json::from_value(input).unwrap();
        let encoded = encode_active_camera(&context).unwrap().unwrap();
        assert!(encoded.starts_with("scene-graph:v1|!"));
        let records: Vec<_> = encoded.split_once('!').unwrap().1.split(';').collect();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record.split(',').count() == 21));
        let mut invalid = context.clone();
        invalid.scene_layers[0].parent.as_mut().unwrap().generation = 2;
        assert!(encode_active_camera(&invalid).is_err());
        invalid = context.clone();
        invalid.scene_layers[1].parent = Some(invalid.scene_layers[0].layer);
        assert!(encode_active_camera(&invalid).is_err());
        invalid = context.clone();
        invalid.scene_layers[0].scale[0] = f64::NAN;
        assert!(encode_active_camera(&invalid).is_err());
        invalid = context.clone();
        invalid.scene_layers[1].layer.index = 0;
        assert!(encode_active_camera(&invalid).is_err());
        invalid = context.clone();
        invalid.scene_layers[0].is_3d = false;
        assert!(encode_active_camera(&invalid).is_err());
        let serialized = serde_json::to_value(context).unwrap();
        assert_eq!(serialized["scene_layers"][0]["position"][0], 10.0);
        assert_eq!(serialized["scene_layers"][0]["parent"]["object_id"], 2802);
        assert_eq!(serialized["scene_layers"][1]["layer"]["object_id"], 2802);
    }

    #[test]
    fn request_v4_accepts_an_authored_active_camera_snapshot() {
        let request: Request = serde_json::from_str(
            r#"{"schema_version":4,"plugin_id":"maskoffset","assignments":{},"host_context":{"mask_scene":{"masks":[]},"active_camera":{"layer":{"project_id":1,"object_id":2807,"generation":1,"index":2},"anchor":[0.0,0.0,0.0],"position":[10.0,20.0,30.0],"scale":[100.0,100.0,100.0],"rotation_degrees":[0.0,0.0,0.0],"zoom":800.0,"in_point":{"value":0,"scale":30},"duration":{"value":300,"scale":30}}}}"#,
        )
        .unwrap();
        let encoded = encode_active_camera(request.host_context.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert!(encoded.starts_with("scene-camera:v1|1,2807,1,2,0,30,300,30,"));
        assert_eq!(encoded.split(',').count(), 21);
        let mut invalid = request.host_context.unwrap();
        invalid.active_camera.as_mut().unwrap().zoom = f64::NAN;
        assert!(encode_active_camera(&invalid).is_err());
        invalid.active_camera.as_mut().unwrap().zoom = 800.0;
        invalid.active_camera.as_mut().unwrap().scale = [0.01; 3];
        assert!(encode_active_camera(&invalid).is_err());
    }

    #[test]
    fn camera_keyframes_encode_exact_bits_and_reject_invalid_time_or_transform() {
        let mut camera: ActiveCamera = serde_json::from_value(json!({
            "layer": {"project_id": 1, "object_id": 2807, "generation": 1, "index": 2},
            "anchor": [0,0,0], "position": [10,20,30], "scale": [100,100,100],
            "rotation_degrees": [0,0,0], "zoom": 800,
            "in_point": {"value":0,"scale":30},
            "duration": {"value":300,"scale":30}
        }))
        .unwrap();
        let static_encoded = encode_camera(&camera).unwrap();
        let first = CameraKeyframe {
            time: CameraTime { value: 1, scale: 1 },
            anchor: camera.anchor,
            position: camera.position,
            scale: camera.scale,
            rotation_degrees: camera.rotation_degrees,
            zoom: camera.zoom,
        };
        let second = CameraKeyframe {
            time: CameraTime { value: 4, scale: 2 },
            position: [30.0, 40.0, 50.0],
            zoom: 1200.0,
            ..first
        };
        camera.keyframes = Some([first, second]);
        let encoded = encode_camera(&camera).unwrap();
        let fields: Vec<u64> = encoded
            .split('|')
            .nth(1)
            .unwrap()
            .split(',')
            .map(|value| value.parse().unwrap())
            .collect();
        assert_eq!(fields.len(), 51);
        assert_eq!(&fields[21..23], &[1, 1]);
        assert_eq!(&fields[36..38], &[4, 2]);
        assert_eq!(f64::from_bits(fields[23]), 800.0);
        assert_eq!(f64::from_bits(fields[38]), 1200.0);
        assert_eq!(f64::from_bits(fields[42]), 30.0);
        for time in [
            CameraTime { value: 2, scale: 2 },
            CameraTime { value: 0, scale: 1 },
            CameraTime { value: 1, scale: 0 },
            CameraTime {
                value: 11,
                scale: 1,
            },
            CameraTime {
                value: 10,
                scale: 1,
            },
        ] {
            camera.keyframes.as_mut().unwrap()[1].time = time;
            assert!(encode_camera(&camera).is_err());
        }
        camera.keyframes = Some([first, second]);
        camera.keyframes.as_mut().unwrap()[1].scale = [0.01; 3];
        assert!(encode_camera(&camera).is_err());
        camera.keyframes = Some([first, second]);
        camera.keyframes.as_mut().unwrap()[1].zoom = f64::NAN;
        assert!(encode_camera(&camera).is_err());
        camera.keyframes = None;
        assert_eq!(encode_camera(&camera).unwrap(), static_encoded);
        let mut malformed = serde_json::to_value(camera).unwrap();
        malformed["keyframes"] = json!([first]);
        assert!(serde_json::from_value::<ActiveCamera>(malformed).is_err());
    }

    #[test]
    fn fixture_smart_route_rejects_camera_it_cannot_render() {
        let root = repository();
        let request = root.join("target/render-requests/camera.json");
        let output = root.join("target/smart-request-render-results/camera.json");
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(
            &request,
            br#"{"schema_version":4,"plugin_id":"maskoffset","assignments":{},"host_context":{"mask_scene":{"masks":[]},"active_camera":{"layer":{"project_id":1,"object_id":2807,"generation":1,"index":2},"anchor":[0,0,0],"position":[0,0,0],"scale":[100,100,100],"rotation_degrees":[0,0,0],"zoom":800,"in_point":{"value":0,"scale":30},"duration":{"value":300,"scale":30}}}}"#,
        )
        .unwrap();
        let error = execute_smart(&root, &request, &output).unwrap_err();
        assert!(error.to_string().contains("active camera"), "{error}");
        assert!(!output.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn spatial_context_is_bounded_and_transport_stable() {
        let context: HostContext = serde_json::from_str(
            r#"{"mask_scene":{"masks":[]},"spatial":{"downsample_x":{"numerator":1,"denominator":2},"downsample_y":{"numerator":3,"denominator":4},"pixel_aspect_ratio":{"numerator":10,"denominator":11}}}"#,
        )
        .unwrap();
        assert_eq!(
            encode_spatial_context(&context).unwrap().as_deref(),
            Some("spatial:v1|1,2,3,4,10,11")
        );

        let dimensions: HostContext = serde_json::from_str(
            r#"{"mask_scene":{"masks":[]},"spatial":{"downsample_x":{"numerator":1,"denominator":2},"downsample_y":{"numerator":1,"denominator":2},"pixel_aspect_ratio":{"numerator":1,"denominator":1},"full_resolution_width":74,"full_resolution_height":46}}"#,
        )
        .unwrap();
        assert_eq!(
            encode_spatial_context(&dimensions).unwrap().as_deref(),
            Some("spatial:v2|1,2,1,2,1,1,74,46")
        );

        let origin: HostContext = serde_json::from_str(
            r#"{"mask_scene":{"masks":[]},"spatial":{"downsample_x":{"numerator":1,"denominator":2},"downsample_y":{"numerator":1,"denominator":2},"pixel_aspect_ratio":{"numerator":1,"denominator":1},"full_resolution_width":74,"full_resolution_height":46,"pre_effect_source_origin_x":-7,"pre_effect_source_origin_y":9}}"#,
        )
        .unwrap();
        assert_eq!(
            encode_spatial_context(&origin).unwrap().as_deref(),
            Some("spatial:v3|1,2,1,2,1,1,74,46,-7,9")
        );

        let unpaired: HostContext = serde_json::from_str(
            r#"{"mask_scene":{"masks":[]},"spatial":{"downsample_x":{"numerator":1,"denominator":1},"downsample_y":{"numerator":1,"denominator":1},"pixel_aspect_ratio":{"numerator":1,"denominator":1},"full_resolution_width":74}}"#,
        )
        .unwrap();
        assert!(encode_spatial_context(&unpaired).is_err());

        let unpaired_origin: HostContext = serde_json::from_str(
            r#"{"mask_scene":{"masks":[]},"spatial":{"downsample_x":{"numerator":1,"denominator":1},"downsample_y":{"numerator":1,"denominator":1},"pixel_aspect_ratio":{"numerator":1,"denominator":1},"pre_effect_source_origin_x":1}}"#,
        )
        .unwrap();
        assert!(encode_spatial_context(&unpaired_origin).is_err());

        let invalid: HostContext = serde_json::from_str(
            r#"{"mask_scene":{"masks":[]},"spatial":{"downsample_x":{"numerator":1,"denominator":0},"downsample_y":{"numerator":1,"denominator":1},"pixel_aspect_ratio":{"numerator":1,"denominator":1}}}"#,
        )
        .unwrap();
        assert!(encode_spatial_context(&invalid).is_err());
    }

    #[test]
    fn render_environment_is_fixed_point_bounded_and_transport_stable() {
        let context: HostContext = serde_json::from_str(
            r#"{"mask_scene":{"masks":[]},"render_environment":{"quality":"low","field":"upper","shutter_angle":0.5,"shutter_phase":-0.25}}"#,
        )
        .unwrap();
        assert_eq!(
            encode_render_environment(&context).unwrap().as_deref(),
            Some("render:v1|0,1,32768,-16384")
        );

        let invalid: HostContext = serde_json::from_str(
            r#"{"mask_scene":{"masks":[]},"render_environment":{"quality":"high","field":"frame","shutter_angle":1.01,"shutter_phase":0}}"#,
        )
        .unwrap();
        assert!(encode_render_environment(&invalid).is_err());
    }

    #[test]
    fn mask_context_accepts_open_and_rejects_excess_and_legacy_injection() {
        let open: Request = serde_json::from_str(
            r#"{"schema_version":4,"plugin_id":"maskoffset","assignments":{},"host_context":{"mask_scene":{"masks":[{"open":true,"vertices":[{"x":0,"y":0},{"x":1,"y":0},{"x":0,"y":1}]}]}}}"#,
        )
        .unwrap();
        let open_context = open.host_context.as_ref().unwrap();
        assert!(validate_mask_context(open_context).is_ok());
        assert!(
            encode_mask_context(open_context)
                .unwrap()
                .starts_with("v2|1:")
        );

        let legacy: Request = serde_json::from_str(
            r#"{"schema_version":3,"plugin_id":"maskoffset","assignments":{},"host_context":{"mask_scene":{"masks":[]}}}"#,
        )
        .unwrap();
        let profile = PluginProfile {
            id: "maskoffset".into(),
            descriptors: vec![],
        };
        assert!(evaluate(&legacy, &profile).is_err());

        let excessive = HostContext {
            mask_scene: MaskScene {
                masks: vec![MaskShape {
                    open: false,
                    vertices: vec![
                        MaskPoint {
                            x: 0.0,
                            y: 0.0,
                            tangent_in: None,
                            tangent_out: None,
                        };
                        65
                    ],
                }],
            },
            spatial: None,
            active_camera: None,
            scene_layers: Vec::new(),
            render_environment: None,
            aux_channels: Vec::new(),
            alpha_as_coverage_params: Vec::new(),
        };
        assert!(validate_mask_context(&excessive).is_err());

        let invalid_tangent: Request = serde_json::from_str(
            r#"{"schema_version":4,"plugin_id":"maskoffset","assignments":{},"host_context":{"mask_scene":{"masks":[{"open":false,"vertices":[{"x":0,"y":0,"tangent_out":{"x":32769,"y":0}},{"x":1,"y":0},{"x":0,"y":1}]}]}}}"#,
        )
        .unwrap();
        assert!(validate_mask_context(invalid_tangent.host_context.as_ref().unwrap()).is_err());
    }

    #[test]
    fn host_context_requires_an_approved_profile_capability_before_native_lookup() {
        let root = repository();
        let request = root.join("target/render-requests/scattermap-context.json");
        let output = root.join("target/smart-request-render-results/scattermap-context.json");
        fs::write(
            &request,
            br#"{"schema_version":4,"plugin_id":"scattermap","assignments":{},"host_context":{"mask_scene":{"masks":[]}}}"#,
        )
        .unwrap();
        assert!(execute_smart(&root, &request, &output).is_err());
        assert!(!output.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejected_request_records_no_native_process_and_create_new_output() {
        let root = repository();
        let request = root.join("target/render-requests/rejected.json");
        let output = root.join("target/render-request-results/rejected.json");
        fs::write(
            &request,
            br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"direction":4}}"#,
        )
        .unwrap();
        assert!(!run(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(report["errors"][0]["code"], "parameter_out_of_range");
        assert_eq!(report["native_process_started"], false);
        assert!(run(&root, &request, &output).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn strict_json_rejects_unknown_and_duplicate_assignments() {
        for (name, body) in [
            ("unknown", br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"other":1}}"#.as_slice()),
            ("duplicate", br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"direction":1,"direction":2}}"#.as_slice()),
        ] {
            let root = repository();
            let request = root.join(format!("target/render-requests/{name}.json"));
            let output = root.join(format!("target/render-request-results/{name}.json"));
            fs::write(&request, body).unwrap();
            assert!(run(&root, &request, &output).is_err());
            assert!(!output.exists());
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn unknown_plugin_profile_fails_before_any_output() {
        let root = repository();
        let request = root.join("target/render-requests/unknown-plugin.json");
        let output = root.join("target/render-request-results/unknown-plugin.json");
        fs::write(
            &request,
            br#"{"schema_version":2,"plugin_id":"unknown-aex","assignments":{}}"#,
        )
        .unwrap();
        assert!(run(&root, &request, &output).is_err());
        assert!(!output.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn requires_json_paths_inside_broker_owned_roots() {
        let root = repository();
        let request = root.join("target/render-requests/request.txt");
        let output = root.join("target/render-request-results/report.json");
        fs::write(&request, b"{}").unwrap();
        assert!(run(&root, &request, &output).is_err());
        assert!(!output.exists());
        assert!(run(&root, &root.join("outside.json"), &output).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn execution_route_rejects_before_worker_launch() {
        let root = repository();
        let request = root.join("target/render-requests/rejected-execution.json");
        let output = root.join("target/render-request-render-results/rejected.json");
        fs::write(
            &request,
            br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"direction":4}}"#,
        )
        .unwrap();
        assert!(!execute(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
        assert_eq!(report["native_process_started"], false);
        assert_eq!(report["passed"], false);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn smart_execution_route_rejects_before_worker_launch() {
        let root = repository();
        let request = root.join("target/render-requests/rejected-smart-execution.json");
        let output = root.join("target/smart-request-render-results/rejected.json");
        fs::write(
            &request,
            br#"{"schema_version":2,"plugin_id":"scattermap","assignments":{"mix":100.1}}"#,
        )
        .unwrap();
        assert!(!execute_smart(&root, &request, &output).unwrap());
        let report: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
        assert_eq!(report["stage"], "parameterized_smartfx_render");
        assert_eq!(report["native_process_started"], false);
        fs::remove_dir_all(root).unwrap();
    }
}
