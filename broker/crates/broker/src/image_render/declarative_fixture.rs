use crate::render_fixture::{FixtureFinalArtifact, FixturePixelFormat, load_render_fixture};
use sha2::Sha256 as FixtureSha256;
use std::cell::RefCell;

thread_local! {
    static FIXTURE_WORLD_DUMP_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static FIXTURE_RENDER_SETTINGS_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn fixture_world_dump_override() -> Option<PathBuf> {
    FIXTURE_WORLD_DUMP_OVERRIDE.with(|value| value.borrow().clone())
}

fn fixture_render_settings_override() -> Option<String> {
    FIXTURE_RENDER_SETTINGS_OVERRIDE.with(|value| value.borrow().clone())
}

struct FixtureOverridesGuard;

impl FixtureOverridesGuard {
    fn set(world_dump: PathBuf, render_settings: String) -> io::Result<Self> {
        let occupied = FIXTURE_WORLD_DUMP_OVERRIDE.with(|value| value.borrow().is_some())
            || FIXTURE_RENDER_SETTINGS_OVERRIDE.with(|value| value.borrow().is_some());
        if occupied {
            return Err(invalid(
                "nested declarative fixture render is not supported",
            ));
        }
        FIXTURE_WORLD_DUMP_OVERRIDE.with(|value| *value.borrow_mut() = Some(world_dump));
        FIXTURE_RENDER_SETTINGS_OVERRIDE.with(|value| *value.borrow_mut() = Some(render_settings));
        Ok(Self)
    }
}

impl Drop for FixtureOverridesGuard {
    fn drop(&mut self) {
        FIXTURE_WORLD_DUMP_OVERRIDE.with(|value| *value.borrow_mut() = None);
        FIXTURE_RENDER_SETTINGS_OVERRIDE.with(|value| *value.borrow_mut() = None);
    }
}

impl From<FixturePixelFormat> for RenderPixelFormat {
    fn from(value: FixturePixelFormat) -> Self {
        match value {
            FixturePixelFormat::Argb8 => Self::Argb8,
            FixturePixelFormat::Argb16 => Self::Argb16,
            FixturePixelFormat::Argb32f => Self::Argb32f,
        }
    }
}

fn fixture_staging_path(output: &Path) -> io::Result<PathBuf> {
    let parent = output
        .parent()
        .ok_or_else(|| invalid("fixture output has no parent"))?;
    let name = output
        .file_name()
        .ok_or_else(|| invalid("fixture output has no name"))?;
    for nonce in 0..1024u32 {
        let candidate = parent.join(format!(
            ".{}.fixture-tmp-{}-{nonce}",
            name.to_string_lossy(),
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no fixture staging name available",
    ))
}

fn find_checkpoint_dump(
    directory: &Path,
    stage: &str,
    format: RenderPixelFormat,
) -> io::Result<(Vec<u8>, u32, u32)> {
    let extension = match format {
        RenderPixelFormat::Argb8 => "rgba8",
        RenderPixelFormat::Argb16 => "rgba16le",
        RenderPixelFormat::Argb32f => "rgba32f-le",
    };
    let marker = format!("-{stage}-");
    let suffix = format!(".{extension}");
    let mut found = None;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(dimensions) = name
            .strip_suffix(&suffix)
            .and_then(|name| name.split_once(&marker).map(|(_, dimensions)| dimensions))
        else {
            continue;
        };
        let (width, height) = dimensions
            .split_once('x')
            .and_then(|(width, height)| {
                Some((width.parse::<u32>().ok()?, height.parse::<u32>().ok()?))
            })
            .ok_or_else(|| invalid("checkpoint dump dimensions are invalid"))?;
        if found.is_some() {
            return Err(invalid("checkpoint dump is ambiguous"));
        }
        found = Some((fs::read(entry.path())?, width, height));
    }
    found.ok_or_else(|| invalid(format!("requested checkpoint was not produced: {stage}")))
}

/// Run one strict, shareable fixture. Asset paths inside the fixture are relative
/// to the fixture file; plug-in selection and its approved hash stay outside it.
pub fn render_declarative_fixture(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    fixture_path: &Path,
    output_directory: &Path,
) -> io::Result<Value> {
    if output_directory.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "fixture output exists",
        ));
    }
    let loaded = load_render_fixture(fixture_path)?;
    let fixture_sha256 = loaded.sha256;
    let fixture = loaded.document;
    let primary = loaded.primary_layer;
    let parameters = loaded.parameters;
    let format = RenderPixelFormat::from(fixture.pixel_format);
    let smart = fixture.render_path == "smart";
    let timing = RenderTiming {
        current_time: fixture.timing.current_time,
        time_step: fixture.timing.time_step,
        total_time: fixture.timing.total_time,
        time_scale: fixture.timing.time_scale,
    };
    let kind = match fixture.final_artifact {
        FixtureFinalArtifact::Raw => RenderArtifactKind::Raw,
        FixtureFinalArtifact::Exr => RenderArtifactKind::Float32Exr,
    };
    fs::create_dir_all(
        output_directory
            .parent()
            .ok_or_else(|| invalid("fixture output has no parent"))?,
    )?;
    let staging = fixture_staging_path(output_directory)?;
    fs::create_dir(&staging)?;
    let dump_dir = repository.join("target").join(format!(
        "fixture-world-dumps-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir_all(&dump_dir)?;
    let result = (|| {
        let _override_guard = FixtureOverridesGuard::set(
            dump_dir.clone(),
            format!("v1|{}|0|-|0|software", fixture.premultiplication),
        )?;
        let rendered = render_experimental_artifact_at_time(
            repository,
            plugin_path,
            approved_sha256,
            &primary,
            &staging.join("final"),
            &parameters,
            timing,
            smart,
            format,
            kind,
        );
        let report = rendered?;
        let final_metadata = report
            .get("render_artifact")
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("fixture render lacks artifact metadata"))?;
        let base_conditions = RenderArtifactConditions {
            premultiplication: final_metadata["premultiplication"]
                .as_str()
                .ok_or_else(|| invalid("artifact premultiplication is absent"))?
                .into(),
            working_space: final_metadata["working_space"]
                .as_str()
                .ok_or_else(|| invalid("artifact working space is absent"))?
                .into(),
            render_mode: final_metadata["render_mode"]
                .as_str()
                .ok_or_else(|| invalid("artifact render mode is absent"))?
                .into(),
            comparison_identity: final_metadata["comparison_identity"].clone(),
        };
        let mut checkpoint_reports = serde_json::Map::new();
        for checkpoint in &fixture.checkpoints {
            let (bytes, width, height) =
                find_checkpoint_dump(&dump_dir, &checkpoint.stage, format)?;
            let mut conditions = base_conditions.clone();
            conditions.comparison_identity["world_sha256"] =
                Value::String(format!("{:x}", FixtureSha256::digest(&bytes)));
            conditions.comparison_identity["origin"] = json!({"x":0,"y":0});
            let metadata = write_raw_world_checkpoint_artifact(
                &staging.join("checkpoints").join(&checkpoint.id),
                &bytes,
                width,
                height,
                format,
                0,
                0,
                conditions,
                &checkpoint.id,
                &checkpoint.stage,
                &fixture_sha256,
            )?;
            checkpoint_reports.insert(checkpoint.id.clone(), metadata);
        }
        fs::rename(&staging, output_directory)?;
        Ok(json!({
            "schema":"aexcompat.render_fixture_report", "schema_version":1,
            "pixel_format":format.report_name(), "render_path":fixture.render_path,
            "final_artifact":report["render_artifact"], "checkpoints":checkpoint_reports
        }))
    })();
    let _ = fs::remove_dir_all(&dump_dir);
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

#[cfg(test)]
mod declarative_fixture_tests {
    use super::*;

    #[test]
    fn fixture_schema_rejects_unknown_fields_and_non_pf32_exr() {
        let directory =
            std::env::temp_dir().join(format!("aexcompat-fixture-schema-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).unwrap();
        let path = directory.join("fixture.json");
        let unknown = br#"{"schema":"aexcompat.render_fixture","schema_version":1,"primary_layer":"input.png","parameters":[],"pixel_format":"argb8","render_path":"classic","premultiplication":"straight","timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},"final_artifact":"raw","checkpoints":[{"id":"input","stage":"classic-input"}],"extra":true}"#;
        fs::write(&path, unknown).unwrap();
        assert!(load_render_fixture(&path).is_err());
        let invalid_exr = br#"{"schema":"aexcompat.render_fixture","schema_version":1,"primary_layer":"input.png","parameters":[],"pixel_format":"argb16","render_path":"classic","premultiplication":"straight","timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},"final_artifact":"exr","checkpoints":[{"id":"input","stage":"classic-input"}]}"#;
        fs::write(&path, invalid_exr).unwrap();
        assert!(load_render_fixture(&path).is_err());
        assert!(
            crate::render_fixture::fixture_relative(
                Path::new("fixture"),
                Path::new(r"\outside.png")
            )
            .is_err()
        );
        assert!(
            crate::render_fixture::fixture_relative(
                Path::new("fixture"),
                Path::new(r"C:outside.png")
            )
            .is_err()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn checkpoint_dump_reader_preserves_float_words_and_channel_order() {
        let directory =
            std::env::temp_dir().join(format!("aexcompat-fixture-dump-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir(&directory).unwrap();
        let words = [0x7fc0_1234u32, 0x8000_0000, 0x0000_0001, 0x3f80_0000];
        let bytes = words
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        fs::write(directory.join("000-classic-input-1x1.rgba32f-le"), &bytes).unwrap();
        let (read, width, height) =
            find_checkpoint_dump(&directory, "classic-input", RenderPixelFormat::Argb32f).unwrap();
        assert_eq!((width, height), (1, 1));
        assert_eq!(read, bytes);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fixture_overrides_are_thread_local_and_restore_on_drop() {
        let _guard = FixtureOverridesGuard::set(
            PathBuf::from("target/fixture-dumps"),
            "v1|straight|0|-|0|software".into(),
        )
        .unwrap();
        assert!(fixture_world_dump_override().is_some());
        assert_eq!(
            std::thread::spawn(|| (
                fixture_world_dump_override(),
                fixture_render_settings_override()
            ))
            .join()
            .unwrap(),
            (None, None)
        );
        drop(_guard);
        assert_eq!(fixture_world_dump_override(), None);
        assert_eq!(fixture_render_settings_override(), None);
    }
}
