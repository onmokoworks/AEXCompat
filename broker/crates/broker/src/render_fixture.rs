use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
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

#[derive(Clone, Debug, Deserialize)]
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
pub struct DeclarativeRenderFixture {
    pub schema: String,
    pub schema_version: u32,
    pub primary_layer: PathBuf,
    pub parameters: Vec<InteractiveParameter>,
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
    pub primary_layer: PathBuf,
    pub parameters: Vec<InteractiveParameter>,
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
    if fixture.schema != "aexcompat.render_fixture" || fixture.schema_version != 1 {
        return Err(invalid(
            "render fixture schema must be aexcompat.render_fixture v1",
        ));
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
    Ok(LoadedRenderFixture {
        document,
        sha256,
        primary_layer,
        parameters,
    })
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
}
