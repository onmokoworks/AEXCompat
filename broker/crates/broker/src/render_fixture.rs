use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InteractiveParameter {
    pub slot: u32,
    pub name: String,
    pub kind: String,
    pub minimum: f64,
    pub maximum: f64,
    pub value: f64,
    pub choices: Vec<String>,
    pub color: [u8; 4],
    pub components: [f64; 3],
    pub component_count: usize,
    pub layer_path: Option<PathBuf>,
    pub enabled: bool,
    pub visible: bool,
    pub supervised: bool,
    #[serde(default)]
    pub debug_summary: Option<String>,
    #[serde(default)]
    pub custom_ui_events: u32,
    #[serde(default)]
    pub control_size: [u16; 2],
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FixturePixelFormat {
    Argb8,
    Argb16,
    Argb32f,
}

impl FixturePixelFormat {
    pub fn name(self) -> &'static str {
        match self {
            Self::Argb8 => "argb8",
            Self::Argb16 => "argb16",
            Self::Argb32f => "argb32f",
        }
    }

    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Argb8 => 4,
            Self::Argb16 => 8,
            Self::Argb32f => 16,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FixtureFinalArtifact {
    Raw,
    Exr,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureCheckpoint {
    pub id: String,
    pub stage: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureTiming {
    pub current_time: i32,
    pub time_step: i32,
    pub total_time: i32,
    pub time_scale: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureMatrixAxis {
    pub slot: u32,
    pub values: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureWorldOrigin {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureWorldExtent {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureWorldLayout {
    pub slot: Option<u32>,
    pub pixel_format: FixturePixelFormat,
    pub width: u32,
    pub height: u32,
    pub rowbytes: u32,
    pub row_padding: u32,
    pub padding_byte: u8,
    pub origin: FixtureWorldOrigin,
    pub extent: FixtureWorldExtent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureWorlds {
    pub primary: FixtureWorldLayout,
    pub secondary: Vec<FixtureWorldLayout>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclarativeRenderFixture {
    pub schema: String,
    pub schema_version: u32,
    pub primary_layer: PathBuf,
    pub parameters: Vec<InteractiveParameter>,
    pub matrix: Option<Vec<FixtureMatrixAxis>>,
    pub worlds: Option<FixtureWorlds>,
    pub pixel_format: FixturePixelFormat,
    pub render_path: String,
    pub premultiplication: String,
    pub timing: FixtureTiming,
    pub final_artifact: FixtureFinalArtifact,
    pub checkpoints: Vec<FixtureCheckpoint>,
}

pub struct LoadedRenderFixture {
    pub document: DeclarativeRenderFixture,
    pub sha256: String,
    pub input_asset_sha256: String,
    pub primary_layer: PathBuf,
    pub parameters: Vec<InteractiveParameter>,
}

#[derive(Clone, Debug)]
pub struct FixtureCase {
    pub index: usize,
    pub parameters: Vec<InteractiveParameter>,
    pub selections: Vec<(u32, f64)>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureCaseSelection {
    pub slot: u32,
    pub value: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureCaseIdentity {
    pub sha256: String,
    pub fixture_sha256: String,
    pub plugin_sha256: String,
    pub input_asset_sha256: String,
    pub case_index: u32,
    pub selections: Vec<FixtureCaseSelection>,
    pub render_path: String,
    pub pixel_format: String,
    pub checkpoints: Vec<FixtureCheckpoint>,
    pub world_layout_identity: String,
}

impl FixtureCaseIdentity {
    fn digest_payload(&self) -> io::Result<String> {
        let payload = (
            &self.fixture_sha256,
            &self.plugin_sha256,
            &self.input_asset_sha256,
            self.case_index,
            &self.selections,
            &self.render_path,
            &self.pixel_format,
            &self.checkpoints,
            &self.world_layout_identity,
        );
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&payload)?)
        ))
    }

    pub fn validate(&self) -> io::Result<()> {
        let valid_hash = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if !valid_hash(&self.sha256)
            || !valid_hash(&self.fixture_sha256)
            || !valid_hash(&self.plugin_sha256)
            || !valid_hash(&self.input_asset_sha256)
            || self.case_index >= 64
            || self.selections.len() > 6
            || self
                .selections
                .iter()
                .any(|selection| selection.slot == 0 || !selection.value.is_finite())
            || !matches!(self.render_path.as_str(), "classic" | "smart")
            || !matches!(self.pixel_format.as_str(), "argb8" | "argb16" | "argb32f")
            || self.checkpoints.is_empty()
            || self.checkpoints.len() > 16
            || (self.world_layout_identity != "packed-v1"
                && !valid_hash(&self.world_layout_identity))
            || self.sha256 != self.digest_payload()?
        {
            return Err(invalid("fixture case identity is invalid"));
        }
        Ok(())
    }
}

pub fn fixture_case_identity(
    loaded: &LoadedRenderFixture,
    case: &FixtureCase,
    plugin_sha256: &str,
) -> io::Result<FixtureCaseIdentity> {
    let mut identity = FixtureCaseIdentity {
        sha256: String::new(),
        fixture_sha256: loaded.sha256.clone(),
        plugin_sha256: plugin_sha256.to_ascii_lowercase(),
        input_asset_sha256: loaded.input_asset_sha256.clone(),
        case_index: u32::try_from(case.index).map_err(|_| invalid("case index overflows"))?,
        selections: case
            .selections
            .iter()
            .map(|&(slot, value)| FixtureCaseSelection { slot, value })
            .collect(),
        render_path: loaded.document.render_path.clone(),
        pixel_format: loaded.document.pixel_format.name().into(),
        checkpoints: loaded.document.checkpoints.clone(),
        world_layout_identity: match &loaded.document.worlds {
            Some(worlds) => format!("{:x}", Sha256::digest(serde_json::to_vec(worlds)?)),
            None => "packed-v1".into(),
        },
    };
    identity.sha256 = identity.digest_payload()?;
    identity.validate()?;
    Ok(identity)
}

pub fn expand_fixture_cases(loaded: &LoadedRenderFixture) -> Vec<FixtureCase> {
    let mut cases = vec![FixtureCase {
        index: 0,
        parameters: loaded.parameters.clone(),
        selections: Vec::new(),
    }];
    for axis in loaded.document.matrix.as_deref().unwrap_or(&[]) {
        let mut expanded = Vec::with_capacity(cases.len() * axis.values.len());
        for case in cases {
            for &value in &axis.values {
                let mut next = case.clone();
                next.parameters
                    .iter_mut()
                    .find(|parameter| parameter.slot == axis.slot)
                    .expect("validated matrix slot")
                    .value = value;
                next.selections.push((axis.slot, value));
                next.index = expanded.len();
                expanded.push(next);
            }
        }
        cases = expanded;
    }
    cases
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

pub fn fixture_relative(base: &Path, path: &Path) -> io::Result<PathBuf> {
    let portable = path.to_string_lossy();
    let windows_rooted = portable.starts_with('\\')
        || portable
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':');
    let dot_segment = portable
        .split(['/', '\\'])
        .any(|component| matches!(component, "." | ".."));
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || windows_rooted
        || dot_segment
        || path.components().any(|part| {
            matches!(
                part,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return Err(invalid(
            "fixture asset paths must be traversal-free relative paths",
        ));
    }
    Ok(base.join(path))
}

fn validate_parameter_schema(document: &Value) -> io::Result<()> {
    let parameters = document
        .get("parameters")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("fixture parameters must be an array"))?;
    const REQUIRED: [&str; 15] = [
        "slot",
        "name",
        "kind",
        "minimum",
        "maximum",
        "value",
        "choices",
        "color",
        "components",
        "component_count",
        "layer_path",
        "enabled",
        "visible",
        "supervised",
        "control_size",
    ];
    const OPTIONAL: [&str; 2] = ["debug_summary", "custom_ui_events"];
    for parameter in parameters {
        let object = parameter
            .as_object()
            .ok_or_else(|| invalid("fixture parameter must be an object"))?;
        if REQUIRED.iter().any(|key| !object.contains_key(*key))
            || object
                .keys()
                .any(|key| !REQUIRED.contains(&key.as_str()) && !OPTIONAL.contains(&key.as_str()))
        {
            return Err(invalid("fixture parameter schema is not canonical"));
        }
    }
    Ok(())
}

fn validate_fixture(fixture: &DeclarativeRenderFixture) -> io::Result<()> {
    if fixture.schema != "aexcompat.render_fixture" || !matches!(fixture.schema_version, 1 | 2) {
        return Err(invalid(
            "render fixture schema must be aexcompat.render_fixture v1 or v2",
        ));
    }
    if fixture.schema_version == 1 && (fixture.matrix.is_some() || fixture.worlds.is_some()) {
        return Err(invalid(
            "fixture v1 does not support matrix or world layouts",
        ));
    }
    for parameter in &fixture.parameters {
        if parameter.kind == "popup"
            && (parameter.choices.is_empty()
                || parameter.choices.len() > 64
                || parameter.choices.iter().any(|choice| choice.is_empty())
                || parameter.minimum != 1.0
                || parameter.maximum != parameter.choices.len() as f64
                || !parameter.value.is_finite()
                || parameter.value.fract() != 0.0
                || parameter.value < 1.0
                || parameter.value > parameter.maximum
                || parameter.component_count != 0)
        {
            return Err(invalid(
                "fixture popup choices or one-based value are invalid",
            ));
        }
    }
    if let Some(worlds) = &fixture.worlds {
        validate_world_layout(&worlds.primary)?;
        if worlds.primary.slot.is_some() || worlds.secondary.len() > 8 {
            return Err(invalid("fixture primary/secondary world slots are invalid"));
        }
        for (index, world) in worlds.secondary.iter().enumerate() {
            validate_world_layout(world)?;
            let slot = world
                .slot
                .filter(|slot| *slot > 0)
                .ok_or_else(|| invalid("fixture secondary world lacks a layer slot"))?;
            if worlds.secondary[..index]
                .iter()
                .any(|prior| prior.slot == Some(slot))
                || fixture
                    .parameters
                    .iter()
                    .filter(|parameter| parameter.slot == slot && parameter.kind == "layer")
                    .count()
                    != 1
            {
                return Err(invalid(
                    "fixture secondary world slot is not unique layer parameter",
                ));
            }
        }
    }
    let mut case_count = 1usize;
    for (index, axis) in fixture.matrix.as_deref().unwrap_or(&[]).iter().enumerate() {
        if axis.slot == 0
            || axis.values.len() < 2
            || axis.values.len() > 16
            || axis.values.iter().any(|value| !value.is_finite())
            || fixture.matrix.as_deref().unwrap_or(&[])[..index]
                .iter()
                .any(|previous| previous.slot == axis.slot)
        {
            return Err(invalid("fixture matrix axis is invalid"));
        }
        let matching = fixture
            .parameters
            .iter()
            .filter(|parameter| parameter.slot == axis.slot)
            .collect::<Vec<_>>();
        if matching.len() != 1
            || !matches!(matching[0].kind.as_str(), "float" | "integer" | "popup")
            || matching[0].component_count != 0
            || !matching[0].minimum.is_finite()
            || !matching[0].maximum.is_finite()
            || matching[0].minimum > matching[0].maximum
            || axis.values.iter().any(|value| {
                *value < matching[0].minimum
                    || *value > matching[0].maximum
                    || (matches!(matching[0].kind.as_str(), "integer" | "popup")
                        && value.fract() != 0.0)
            })
        {
            return Err(invalid(
                "fixture matrix slot is not a unique scalar parameter",
            ));
        }
        case_count = case_count
            .checked_mul(axis.values.len())
            .ok_or_else(|| invalid("fixture matrix case count overflows"))?;
        if case_count > 64 {
            return Err(invalid("fixture matrix exceeds 64 cases"));
        }
    }
    if !matches!(fixture.render_path.as_str(), "classic" | "smart") {
        return Err(invalid("fixture render_path must be classic or smart"));
    }
    if !matches!(
        fixture.premultiplication.as_str(),
        "straight" | "premultiplied" | "opaque"
    ) {
        return Err(invalid("fixture premultiplication is not canonical"));
    }
    if fixture.timing.time_scale == 0
        || fixture.timing.time_step <= 0
        || fixture.timing.total_time < 0
        || fixture.timing.current_time < 0
        || fixture.timing.current_time > fixture.timing.total_time
    {
        return Err(invalid("fixture timing is invalid"));
    }
    if fixture.final_artifact == FixtureFinalArtifact::Exr
        && fixture.pixel_format != FixturePixelFormat::Argb32f
    {
        return Err(invalid("fixture EXR output requires argb32f"));
    }
    if fixture.checkpoints.is_empty() || fixture.checkpoints.len() > 16 {
        return Err(invalid("fixture must request 1..16 checkpoints"));
    }
    let prefix = fixture.render_path.as_str();
    for (index, checkpoint) in fixture.checkpoints.iter().enumerate() {
        let valid_id = !checkpoint.id.is_empty()
            && checkpoint.id.len() <= 64
            && checkpoint
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        let valid_stage = checkpoint.stage == format!("{prefix}-input")
            || checkpoint.stage == format!("{prefix}-output")
            || checkpoint
                .stage
                .strip_prefix(&format!("{prefix}-layer-slot"))
                .and_then(|slot| slot.parse::<u32>().ok())
                .is_some_and(|slot| slot > 0);
        if !valid_id
            || !valid_stage
            || fixture.checkpoints[..index]
                .iter()
                .any(|prior| prior.id == checkpoint.id || prior.stage == checkpoint.stage)
        {
            return Err(invalid("fixture checkpoint identity or stage is invalid"));
        }
    }
    Ok(())
}

fn validate_world_layout(layout: &FixtureWorldLayout) -> io::Result<()> {
    let packed = layout
        .width
        .checked_mul(layout.pixel_format.bytes_per_pixel() as u32)
        .ok_or_else(|| invalid("fixture world rowbytes overflow"))?;
    if layout.width == 0
        || layout.height == 0
        || layout.width > 4096
        || layout.height > 4096
        || u64::from(layout.width) * u64::from(layout.height) > 16_777_216
        || layout.row_padding > 256
        || layout.row_padding % layout.pixel_format.bytes_per_pixel() as u32 != 0
        || packed.checked_add(layout.row_padding) != Some(layout.rowbytes)
        || layout.origin.x.unsigned_abs() > 4096
        || layout.origin.y.unsigned_abs() > 4096
        || layout.extent.left < 0
        || layout.extent.top < 0
        || layout.extent.right <= layout.extent.left
        || layout.extent.bottom <= layout.extent.top
        || layout.extent.right > layout.width as i32
        || layout.extent.bottom > layout.height as i32
    {
        return Err(invalid("fixture world layout is invalid"));
    }
    Ok(())
}

pub fn load_render_fixture(path: &Path) -> io::Result<LoadedRenderFixture> {
    let bytes = fs::read(path)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let value: Value = serde_json::from_slice(&bytes)?;
    validate_parameter_schema(&value)?;
    let document: DeclarativeRenderFixture = serde_json::from_value(value)?;
    validate_fixture(&document)?;
    let base = path
        .parent()
        .ok_or_else(|| invalid("fixture has no parent"))?;
    let primary_layer = fixture_relative(base, &document.primary_layer)?;
    let mut parameters = document.parameters.clone();
    for parameter in &mut parameters {
        if let Some(layer) = parameter.layer_path.as_deref() {
            parameter.layer_path = Some(fixture_relative(base, layer)?);
        }
    }
    let input_asset_sha256 = if document.schema_version == 2 {
        let mut digest = Sha256::new();
        digest.update([0]);
        digest.update(hash_asset_file(&primary_layer)?);
        for parameter in &parameters {
            if let Some(layer) = &parameter.layer_path {
                digest.update([1]);
                digest.update(parameter.slot.to_le_bytes());
                digest.update(hash_asset_file(layer)?);
            }
        }
        format!("{:x}", digest.finalize())
    } else {
        "00".repeat(32)
    };
    Ok(LoadedRenderFixture {
        document,
        sha256,
        input_asset_sha256,
        primary_layer,
        parameters,
    })
}

fn hash_asset_file(path: &Path) -> io::Result<[u8; 32]> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parameter() -> Value {
        json!({
            "slot":1,"name":"Amount","kind":"float","minimum":0.0,"maximum":100.0,
            "value":50.0,"choices":[],"color":[255,0,0,0],"components":[0.0,0.0,0.0],
            "component_count":0,"layer_path":null,"enabled":true,"visible":true,
            "supervised":false,"control_size":[0,0]
        })
    }

    #[test]
    fn strict_fixture_validation_is_portable() {
        let mut value = json!({
            "schema":"aexcompat.render_fixture","schema_version":1,"primary_layer":"input.png",
            "parameters":[parameter()],"pixel_format":"argb32f","render_path":"smart",
            "premultiplication":"straight","timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},
            "final_artifact":"exr","checkpoints":[{"id":"output","stage":"smart-output"}]
        });
        let fixture: DeclarativeRenderFixture = serde_json::from_value(value.clone()).unwrap();
        validate_fixture(&fixture).unwrap();
        let mut boundary = value.clone();
        boundary["timing"]["current_time"] = json!(1);
        let fixture: DeclarativeRenderFixture = serde_json::from_value(boundary).unwrap();
        validate_fixture(&fixture).unwrap();
        let mut zero_duration = value.clone();
        zero_duration["timing"]["total_time"] = json!(0);
        let fixture: DeclarativeRenderFixture =
            serde_json::from_value(zero_duration.clone()).unwrap();
        validate_fixture(&fixture).unwrap();
        zero_duration["timing"]["current_time"] = json!(1);
        let fixture: DeclarativeRenderFixture = serde_json::from_value(zero_duration).unwrap();
        assert!(validate_fixture(&fixture).is_err());
        let mut negative_duration = value.clone();
        negative_duration["timing"]["total_time"] = json!(-1);
        let fixture: DeclarativeRenderFixture = serde_json::from_value(negative_duration).unwrap();
        assert!(validate_fixture(&fixture).is_err());
        value["unknown"] = json!(true);
        assert!(serde_json::from_value::<DeclarativeRenderFixture>(value).is_err());
        let mut unknown_parameter = parameter();
        unknown_parameter["unknown"] = json!(true);
        assert!(validate_parameter_schema(&json!({"parameters":[unknown_parameter]})).is_err());
        assert!(fixture_relative(Path::new("fixture"), Path::new(r"C:outside.png")).is_err());
        assert!(fixture_relative(Path::new("fixture"), Path::new(r"\outside.png")).is_err());
    }

    #[test]
    fn version_two_fixture_accepts_a_two_value_scalar_matrix() {
        let fixture = json!({
            "schema":"aexcompat.render_fixture","schema_version":2,
            "primary_layer":"input.png","parameters":[parameter()],
            "matrix":[{"slot":1,"values":[20.0,80.0]}],
            "pixel_format":"argb8","render_path":"classic",
            "premultiplication":"straight",
            "timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},
            "final_artifact":"raw",
            "checkpoints":[{"id":"output","stage":"classic-output"}]
        });
        let parsed: DeclarativeRenderFixture = serde_json::from_value(fixture).unwrap();
        validate_fixture(&parsed).unwrap();
        let loaded = LoadedRenderFixture {
            document: parsed,
            sha256: "11".repeat(32),
            input_asset_sha256: "33".repeat(32),
            primary_layer: PathBuf::from("input.png"),
            parameters: vec![serde_json::from_value(parameter()).unwrap()],
        };
        let cases = expand_fixture_cases(&loaded);
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[0].index, 0);
        assert_eq!(cases[0].selections, vec![(1, 20.0)]);
        assert_eq!(cases[1].index, 1);
        assert_eq!(cases[1].selections, vec![(1, 80.0)]);
        assert_eq!(cases[0].parameters[0].value, 20.0);
        assert_eq!(cases[1].parameters[0].value, 80.0);
        let first = fixture_case_identity(&loaded, &cases[0], &"22".repeat(32)).unwrap();
        let second = fixture_case_identity(&loaded, &cases[1], &"22".repeat(32)).unwrap();
        assert_ne!(first.sha256, second.sha256);
        assert_eq!(
            first.sha256,
            fixture_case_identity(&loaded, &cases[0], &"22".repeat(32))
                .unwrap()
                .sha256
        );
        let mut forged = first.clone();
        forged.selections[0].value = 40.0;
        assert!(forged.validate().is_err());
    }

    #[test]
    fn version_two_fixture_expands_one_based_popup_matrix() {
        let mut popup = parameter();
        popup["kind"] = json!("popup");
        popup["minimum"] = json!(1.0);
        popup["maximum"] = json!(2.0);
        popup["value"] = json!(1.0);
        popup["choices"] = json!(["One", "Two"]);
        let fixture = json!({
            "schema":"aexcompat.render_fixture","schema_version":2,
            "primary_layer":"input.png","parameters":[popup],
            "matrix":[{"slot":1,"values":[1.0,2.0]}],
            "pixel_format":"argb8","render_path":"classic",
            "premultiplication":"straight",
            "timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},
            "final_artifact":"raw",
            "checkpoints":[{"id":"output","stage":"classic-output"}]
        });
        let document: DeclarativeRenderFixture = serde_json::from_value(fixture.clone()).unwrap();
        validate_fixture(&document).unwrap();
        let loaded = LoadedRenderFixture {
            parameters: document.parameters.clone(),
            document,
            sha256: "11".repeat(32),
            input_asset_sha256: "33".repeat(32),
            primary_layer: PathBuf::from("input.png"),
        };
        let cases = expand_fixture_cases(&loaded);
        assert_eq!(cases.len(), 2);
        assert_eq!(
            (cases[0].parameters[0].value, cases[1].parameters[0].value),
            (1.0, 2.0)
        );
        assert_ne!(
            fixture_case_identity(&loaded, &cases[0], &"22".repeat(32))
                .unwrap()
                .sha256,
            fixture_case_identity(&loaded, &cases[1], &"22".repeat(32))
                .unwrap()
                .sha256
        );
        for invalid_choice in [0.0, 2.5, 3.0] {
            let mut malformed = fixture.clone();
            malformed["matrix"][0]["values"][1] = json!(invalid_choice);
            let parsed: DeclarativeRenderFixture = serde_json::from_value(malformed).unwrap();
            assert!(validate_fixture(&parsed).is_err());
        }
        let mut no_choices = fixture;
        no_choices["parameters"][0]["choices"] = json!([]);
        let parsed: DeclarativeRenderFixture = serde_json::from_value(no_choices).unwrap();
        assert!(validate_fixture(&parsed).is_err());
    }

    #[test]
    fn version_two_world_layouts_are_strict_and_change_case_identity() {
        let mut fixture = json!({
            "schema":"aexcompat.render_fixture","schema_version":2,
            "primary_layer":"primary.png",
            "parameters":[{
                "slot":1,"name":"Layer","kind":"layer","minimum":0.0,"maximum":1.0,
                "value":0.0,"choices":[],"color":[255,0,0,0],"components":[0.0,0.0,0.0],
                "component_count":0,"layer_path":"secondary.png","enabled":true,
                "visible":true,"supervised":false,"control_size":[0,0]
            },parameter()],
            "matrix":[{"slot":1,"values":[20.0,80.0]}],
            "pixel_format":"argb8","render_path":"classic","premultiplication":"straight",
            "timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},
            "final_artifact":"raw","checkpoints":[{"id":"input","stage":"classic-input"}],
            "worlds":{
                "primary":{
                    "pixel_format":"argb8","width":4,"height":3,"rowbytes":20,
                    "row_padding":4,"padding_byte":165,"origin":{"x":2,"y":-1},
                    "extent":{"left":0,"top":0,"right":4,"bottom":3}
                },
                "secondary":[{
                    "slot":1,"pixel_format":"argb16","width":2,"height":3,"rowbytes":24,
                    "row_padding":8,"padding_byte":90,"origin":{"x":-2,"y":1},
                    "extent":{"left":0,"top":0,"right":2,"bottom":3}
                }]
            }
        });
        fixture["parameters"][1]["slot"] = json!(2);
        fixture["matrix"][0]["slot"] = json!(2);
        let parsed: DeclarativeRenderFixture = serde_json::from_value(fixture.clone()).unwrap();
        validate_fixture(&parsed).unwrap();
        let parameters = parsed.parameters.clone();
        let loaded = LoadedRenderFixture {
            document: parsed,
            sha256: "11".repeat(32),
            input_asset_sha256: "33".repeat(32),
            primary_layer: PathBuf::from("primary.png"),
            parameters,
        };
        let cases = expand_fixture_cases(&loaded);
        let case = &cases[0];
        let original = fixture_case_identity(&loaded, case, &"22".repeat(32)).unwrap();
        assert_ne!(original.world_layout_identity, "packed-v1");
        let mut changed = fixture.clone();
        changed["worlds"]["secondary"][0]["origin"]["x"] = json!(3);
        let mut changed_loaded = loaded;
        changed_loaded.document = serde_json::from_value(changed).unwrap();
        validate_fixture(&changed_loaded.document).unwrap();
        let changed_cases = expand_fixture_cases(&changed_loaded);
        let changed_case = &changed_cases[0];
        assert_ne!(
            original.sha256,
            fixture_case_identity(&changed_loaded, changed_case, &"22".repeat(32))
                .unwrap()
                .sha256
        );
        let mut bad_extent = fixture.clone();
        bad_extent["worlds"]["secondary"][0]["extent"]["right"] = json!(3);
        let malformed: DeclarativeRenderFixture = serde_json::from_value(bad_extent).unwrap();
        assert!(validate_fixture(&malformed).is_err());
        let mut duplicate_slot = fixture.clone();
        let copied = duplicate_slot["worlds"]["secondary"][0].clone();
        duplicate_slot["worlds"]["secondary"]
            .as_array_mut()
            .unwrap()
            .push(copied);
        let malformed: DeclarativeRenderFixture = serde_json::from_value(duplicate_slot).unwrap();
        assert!(validate_fixture(&malformed).is_err());
        fixture["worlds"]["secondary"][0]["rowbytes"] = json!(16);
        let malformed: DeclarativeRenderFixture = serde_json::from_value(fixture).unwrap();
        assert!(validate_fixture(&malformed).is_err());
    }

    #[test]
    fn version_two_case_identity_changes_with_primary_asset_bytes() {
        let scratch = std::env::temp_dir().join(format!(
            "aexcompat-fixture-identity-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        fs::create_dir_all(&scratch).unwrap();
        let primary = scratch.join("input.png");
        let secondary = scratch.join("secondary.png");
        let fixture_path = scratch.join("fixture.json");
        let mut layer = parameter();
        layer["slot"] = json!(2);
        layer["kind"] = json!("layer");
        layer["layer_path"] = json!("secondary.png");
        let fixture = json!({
            "schema":"aexcompat.render_fixture","schema_version":2,
            "primary_layer":"input.png","parameters":[parameter(),layer],"matrix":[],
            "pixel_format":"argb8","render_path":"classic",
            "premultiplication":"straight",
            "timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},
            "final_artifact":"raw",
            "checkpoints":[{"id":"input","stage":"classic-input"}]
        });
        fs::write(&fixture_path, serde_json::to_vec(&fixture).unwrap()).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
            .save(&primary)
            .unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([40, 50, 60, 255]))
            .save(&secondary)
            .unwrap();
        let before = load_render_fixture(&fixture_path).unwrap();
        let before_case = expand_fixture_cases(&before);
        let before_identity =
            fixture_case_identity(&before, &before_case[0], &"22".repeat(32)).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 31, 255]))
            .save(&primary)
            .unwrap();
        let after = load_render_fixture(&fixture_path).unwrap();
        let after_case = expand_fixture_cases(&after);
        let after_identity =
            fixture_case_identity(&after, &after_case[0], &"22".repeat(32)).unwrap();
        assert_eq!(before.sha256, after.sha256, "fixture JSON changed");
        assert_ne!(before_identity.sha256, after_identity.sha256);
        image::RgbaImage::from_pixel(2, 2, image::Rgba([40, 50, 61, 255]))
            .save(&secondary)
            .unwrap();
        let changed_secondary = load_render_fixture(&fixture_path).unwrap();
        let changed_secondary_case = expand_fixture_cases(&changed_secondary);
        let changed_secondary_identity = fixture_case_identity(
            &changed_secondary,
            &changed_secondary_case[0],
            &"22".repeat(32),
        )
        .unwrap();
        assert_ne!(after_identity.sha256, changed_secondary_identity.sha256);
        let _ = fs::remove_dir_all(scratch);
    }

    #[test]
    fn version_two_case_identity_binds_plugin_path_depth_and_checkpoint() {
        let document: DeclarativeRenderFixture = serde_json::from_value(json!({
            "schema":"aexcompat.render_fixture","schema_version":2,
            "primary_layer":"input.png","parameters":[parameter()],"matrix":[],
            "pixel_format":"argb8","render_path":"classic",
            "premultiplication":"straight",
            "timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},
            "final_artifact":"raw",
            "checkpoints":[{"id":"input","stage":"classic-input"}]
        }))
        .unwrap();
        validate_fixture(&document).unwrap();
        let mut loaded = LoadedRenderFixture {
            parameters: document.parameters.clone(),
            document,
            sha256: "11".repeat(32),
            input_asset_sha256: "33".repeat(32),
            primary_layer: PathBuf::from("input.png"),
        };
        let cases = expand_fixture_cases(&loaded);
        let case = &cases[0];
        let original = fixture_case_identity(&loaded, case, &"22".repeat(32)).unwrap();
        assert_eq!(
            original.sha256,
            fixture_case_identity(&loaded, case, &"22".repeat(32))
                .unwrap()
                .sha256
        );
        assert_ne!(
            original.sha256,
            fixture_case_identity(&loaded, case, &"44".repeat(32))
                .unwrap()
                .sha256
        );
        loaded.document.render_path = "smart".into();
        assert_ne!(
            original.sha256,
            fixture_case_identity(&loaded, case, &"22".repeat(32))
                .unwrap()
                .sha256
        );
        loaded.document.render_path = "classic".into();
        loaded.document.pixel_format = FixturePixelFormat::Argb16;
        assert_ne!(
            original.sha256,
            fixture_case_identity(&loaded, case, &"22".repeat(32))
                .unwrap()
                .sha256
        );
        loaded.document.pixel_format = FixturePixelFormat::Argb8;
        loaded.document.checkpoints[0].id = "renamed-input".into();
        assert_ne!(
            original.sha256,
            fixture_case_identity(&loaded, case, &"22".repeat(32))
                .unwrap()
                .sha256
        );
    }
}
