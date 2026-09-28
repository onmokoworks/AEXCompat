use crate::render_fixture::{
    FixtureCase, FixtureCaseIdentity, FixtureFinalArtifact, FixturePixelFormat,
    LoadedRenderFixture, expand_fixture_cases, fixture_case_identity, load_render_fixture,
};
use sha2::Sha256 as FixtureSha256;
use std::cell::RefCell;

thread_local! {
    static FIXTURE_WORLD_DUMP_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static FIXTURE_RENDER_SETTINGS_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
    static FIXTURE_CASE_IDENTITY_OVERRIDE: RefCell<Option<FixtureCaseIdentity>> = const { RefCell::new(None) };
    static FIXTURE_WORLD_LAYOUT_OVERRIDE: RefCell<Option<crate::render_session::DiagnosticWorldLayout>> = const { RefCell::new(None) };
    static FIXTURE_SECONDARY_LAYOUTS_OVERRIDE: RefCell<Option<Vec<crate::render_session::SessionLayerLayout>>> = const { RefCell::new(None) };
    static FIXTURE_CAPTURE_OVERRIDE: RefCell<Option<Vec<FixtureCapture>>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct FixtureCapture {
    stage: String,
    path: PathBuf,
}

fn fixture_world_dump_override() -> Option<PathBuf> {
    FIXTURE_WORLD_DUMP_OVERRIDE.with(|value| value.borrow().clone())
}

fn fixture_render_settings_override() -> Option<String> {
    FIXTURE_RENDER_SETTINGS_OVERRIDE.with(|value| value.borrow().clone())
}

fn fixture_case_identity_override() -> Option<FixtureCaseIdentity> {
    FIXTURE_CASE_IDENTITY_OVERRIDE.with(|value| value.borrow().clone())
}

fn fixture_world_layout_override() -> Option<crate::render_session::DiagnosticWorldLayout> {
    FIXTURE_WORLD_LAYOUT_OVERRIDE.with(|value| *value.borrow())
}

fn fixture_secondary_layouts_override() -> Option<Vec<crate::render_session::SessionLayerLayout>> {
    FIXTURE_SECONDARY_LAYOUTS_OVERRIDE.with(|value| value.borrow().clone())
}

fn fixture_capture_override() -> Option<Vec<FixtureCapture>> {
    FIXTURE_CAPTURE_OVERRIDE.with(|value| value.borrow().clone())
}

struct FixtureOverridesGuard;

impl FixtureOverridesGuard {
    fn set(
        world_dump: PathBuf,
        render_settings: String,
        case_identity: Option<FixtureCaseIdentity>,
        world_layout: Option<crate::render_session::DiagnosticWorldLayout>,
        secondary_layouts: Option<Vec<crate::render_session::SessionLayerLayout>>,
        captures: Option<Vec<FixtureCapture>>,
    ) -> io::Result<Self> {
        let occupied = FIXTURE_WORLD_DUMP_OVERRIDE.with(|value| value.borrow().is_some())
            || FIXTURE_RENDER_SETTINGS_OVERRIDE.with(|value| value.borrow().is_some())
            || FIXTURE_CASE_IDENTITY_OVERRIDE.with(|value| value.borrow().is_some())
            || FIXTURE_WORLD_LAYOUT_OVERRIDE.with(|value| value.borrow().is_some())
            || FIXTURE_SECONDARY_LAYOUTS_OVERRIDE.with(|value| value.borrow().is_some())
            || FIXTURE_CAPTURE_OVERRIDE.with(|value| value.borrow().is_some());
        if occupied {
            return Err(invalid(
                "nested declarative fixture render is not supported",
            ));
        }
        FIXTURE_WORLD_DUMP_OVERRIDE.with(|value| *value.borrow_mut() = Some(world_dump));
        FIXTURE_RENDER_SETTINGS_OVERRIDE.with(|value| *value.borrow_mut() = Some(render_settings));
        FIXTURE_CASE_IDENTITY_OVERRIDE.with(|value| *value.borrow_mut() = case_identity);
        FIXTURE_WORLD_LAYOUT_OVERRIDE.with(|value| *value.borrow_mut() = world_layout);
        FIXTURE_SECONDARY_LAYOUTS_OVERRIDE.with(|value| *value.borrow_mut() = secondary_layouts);
        FIXTURE_CAPTURE_OVERRIDE.with(|value| *value.borrow_mut() = captures);
        Ok(Self)
    }
}

impl Drop for FixtureOverridesGuard {
    fn drop(&mut self) {
        FIXTURE_WORLD_DUMP_OVERRIDE.with(|value| *value.borrow_mut() = None);
        FIXTURE_RENDER_SETTINGS_OVERRIDE.with(|value| *value.borrow_mut() = None);
        FIXTURE_CASE_IDENTITY_OVERRIDE.with(|value| *value.borrow_mut() = None);
        FIXTURE_WORLD_LAYOUT_OVERRIDE.with(|value| *value.borrow_mut() = None);
        FIXTURE_SECONDARY_LAYOUTS_OVERRIDE.with(|value| *value.borrow_mut() = None);
        FIXTURE_CAPTURE_OVERRIDE.with(|value| *value.borrow_mut() = None);
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
    let cases = expand_fixture_cases(&loaded);
    if loaded.document.schema_version == 1 {
        return render_single_declarative_fixture(
            repository,
            plugin_path,
            approved_sha256,
            &loaded,
            &cases[0].parameters,
            None,
            output_directory,
        );
    }
    let plugin_sha256 = format!("{:x}", FixtureSha256::digest(fs::read(plugin_path)?));
    render_fixture_cases(
        &loaded,
        &cases,
        &plugin_sha256,
        output_directory,
        |case, identity, output| {
            render_single_declarative_fixture(
                repository,
                plugin_path,
                approved_sha256,
                &loaded,
                &case.parameters,
                Some(identity.clone()),
                output,
            )
        },
    )
}

fn render_fixture_cases<F>(
    loaded: &LoadedRenderFixture,
    cases: &[FixtureCase],
    plugin_sha256: &str,
    output_directory: &Path,
    mut render_case: F,
) -> io::Result<Value>
where
    F: FnMut(&FixtureCase, &FixtureCaseIdentity, &Path) -> io::Result<Value>,
{
    fs::create_dir_all(
        output_directory
            .parent()
            .ok_or_else(|| invalid("fixture output has no parent"))?,
    )?;
    let staging = fixture_staging_path(output_directory)?;
    let result = (|| {
        let mut case_reports = Vec::with_capacity(cases.len());
        for case in cases {
            let identity = fixture_case_identity(loaded, case, plugin_sha256)?;
            let relative = format!("cases/{}", identity.sha256);
            let case_output = staging.join(&relative);
            let report = render_case(case, &identity, &case_output)
                .map_err(|error| invalid(format!("fixture case {} failed: {error}", case.index)))?;
            case_reports.push(json!({
                "case_identity":identity,
                "artifact_directory":relative,
                "report":report,
            }));
        }
        fs::rename(&staging, output_directory)?;
        Ok(json!({
            "schema":"aexcompat.render_fixture_report", "schema_version":2,
            "fixture_sha256":loaded.sha256,
            "complete":true,
            "cases":case_reports,
        }))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn render_single_declarative_fixture(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    loaded: &LoadedRenderFixture,
    parameters: &[InteractiveParameter],
    case_identity: Option<FixtureCaseIdentity>,
    output_directory: &Path,
) -> io::Result<Value> {
    if output_directory.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "fixture output exists",
        ));
    }
    let fixture_sha256 = loaded.sha256.clone();
    let fixture = &loaded.document;
    let primary = &loaded.primary_layer;
    let (world_layout, secondary_layouts) = if let Some(worlds) = &fixture.worlds {
        let world = &worlds.primary;
        let image_dimensions = image::image_dimensions(primary)
            .map_err(|error| invalid(format!("fixture primary image dimensions: {error}")))?;
        if image_dimensions != (world.width, world.height) {
            return Err(invalid("fixture primary layout must match image size"));
        }
        let primary_layout = crate::render_session::DiagnosticWorldLayout {
            input_row_padding: world.row_padding,
            input_padding_byte: (world.padding_byte != 0x5a).then_some(world.padding_byte),
            input_pixel_format: (world.pixel_format != fixture.pixel_format)
                .then_some(RenderPixelFormat::from(world.pixel_format)),
            input_origin_x: world.origin.x,
            input_origin_y: world.origin.y,
            extent_hint: Some([
                world.extent.left,
                world.extent.top,
                world.extent.right,
                world.extent.bottom,
            ]),
            ..Default::default()
        };
        let mut secondaries = Vec::with_capacity(worlds.secondary.len());
        for layer in &worlds.secondary {
            let slot = layer
                .slot
                .ok_or_else(|| invalid("secondary layout has no slot"))?;
            let path = parameters
                .iter()
                .find(|parameter| parameter.slot == slot)
                .and_then(|parameter| parameter.layer_path.as_deref())
                .ok_or_else(|| invalid("secondary layout has no layer image"))?;
            let image_dimensions = image::image_dimensions(path)
                .map_err(|error| invalid(format!("fixture secondary image dimensions: {error}")))?;
            if image_dimensions != (layer.width, layer.height) {
                return Err(invalid("fixture secondary layout must match image size"));
            }
            secondaries.push(crate::render_session::SessionLayerLayout {
                slot,
                pixel_format: RenderPixelFormat::from(layer.pixel_format),
                row_padding: layer.row_padding,
                padding_byte: layer.padding_byte,
                origin_x: layer.origin.x,
                origin_y: layer.origin.y,
                extent: [
                    layer.extent.left,
                    layer.extent.top,
                    layer.extent.right,
                    layer.extent.bottom,
                ],
            });
        }
        (Some(primary_layout), Some(secondaries))
    } else {
        (None, None)
    };
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
    let dump_dir = repository.join("target").join(format!(
        "fixture-world-dumps-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let captures = if let Some(worlds) = &fixture.worlds {
        let matched = fixture
            .checkpoints
            .iter()
            .filter(|checkpoint| {
                checkpoint.stage == format!("{}-input", fixture.render_path)
                    || worlds.secondary.iter().any(|world| {
                        world.slot.is_some_and(|slot| {
                            checkpoint.stage == format!("{}-layer-slot{slot}", fixture.render_path)
                        })
                    })
            })
            .collect::<Vec<_>>();
        Some(
            matched
                .iter()
                .map(|checkpoint| FixtureCapture {
                    stage: checkpoint.stage.clone(),
                    path: staging.join(format!(".captured-world-{}.bin", checkpoint.id)),
                })
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };
    fs::create_dir(&staging)
        .map_err(|error| invalid(format!("fixture case staging create failed: {error}")))?;
    let result = (|| {
        fs::create_dir_all(&dump_dir)
            .map_err(|error| invalid(format!("fixture dump directory create failed: {error}")))?;
        let _override_guard = FixtureOverridesGuard::set(
            dump_dir.clone(),
            format!("v1|{}|0|-|0|software", fixture.premultiplication),
            case_identity,
            world_layout,
            secondary_layouts,
            captures.clone(),
        )?;
        let rendered = render_experimental_artifact_at_time(
            repository,
            plugin_path,
            approved_sha256,
            primary,
            &staging.join("final"),
            parameters,
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
            if let Some(spec) = captures
                .as_ref()
                .and_then(|captures| captures.iter().find(|spec| spec.stage == checkpoint.stage))
            {
                let observed = read_captured_world(&spec.path)?;
                let expected = fixture
                    .worlds
                    .as_ref()
                    .and_then(|worlds| {
                        if checkpoint.stage == format!("{}-input", fixture.render_path) {
                            Some(&worlds.primary)
                        } else {
                            worlds.secondary.iter().find(|world| {
                                world.slot.is_some_and(|slot| {
                                    checkpoint.stage
                                        == format!("{}-layer-slot{slot}", fixture.render_path)
                                })
                            })
                        }
                    })
                    .ok_or_else(|| invalid("captured checkpoint has no declared world"))?;
                if observed.width != expected.width
                    || observed.height != expected.height
                    || observed.pixel_bytes != expected.pixel_format.bytes_per_pixel() as u32
                    || observed.rowbytes != expected.rowbytes
                    || observed.origin_x != expected.origin.x
                    || observed.origin_y != expected.origin.y
                    || observed.extent
                        != [
                            expected.extent.left,
                            expected.extent.top,
                            expected.extent.right,
                            expected.extent.bottom,
                        ]
                    || observed
                        .raw_argb
                        .chunks_exact(observed.rowbytes as usize)
                        .any(|row| {
                            row[(observed.width * observed.pixel_bytes) as usize..]
                                .iter()
                                .any(|byte| *byte != expected.padding_byte)
                        })
                {
                    return Err(invalid(format!(
                        "captured world differs from fixture layout: stage={} observed={}x{}x{} rowbytes={} origin=({}, {}) extent={:?}; expected={}x{}x{} rowbytes={} origin=({}, {}) extent={:?}; padding_byte={}",
                        checkpoint.stage,
                        observed.width, observed.height, observed.pixel_bytes, observed.rowbytes,
                        observed.origin_x, observed.origin_y, observed.extent,
                        expected.width, expected.height, expected.pixel_format.bytes_per_pixel(),
                        expected.rowbytes, expected.origin.x, expected.origin.y,
                        [expected.extent.left, expected.extent.top, expected.extent.right, expected.extent.bottom],
                        expected.padding_byte,
                    )));
                }
                let mut conditions = base_conditions.clone();
                conditions.comparison_identity["world_sha256"] =
                    Value::String(format!("{:x}", FixtureSha256::digest(&observed.raw_argb)));
                conditions.comparison_identity["origin"] =
                    json!({"x":observed.origin_x,"y":observed.origin_y});
                let metadata = write_strided_world_checkpoint_artifact(
                    &staging.join("checkpoints").join(&checkpoint.id),
                    &observed.raw_argb,
                    observed.width,
                    observed.height,
                    RenderPixelFormat::from(expected.pixel_format),
                    observed.rowbytes,
                    observed.origin_x,
                    observed.origin_y,
                    observed.extent,
                    conditions,
                    &checkpoint.id,
                    &checkpoint.stage,
                    &fixture_sha256,
                )?;
                checkpoint_reports.insert(checkpoint.id.clone(), metadata);
                continue;
            }
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
        if let Some(captures) = &captures {
            for spec in captures {
                fs::remove_file(&spec.path)?;
            }
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
    fn handle_checkpoint_reader_rejects_truncation_and_wrong_geometry() {
        let directory = std::env::temp_dir().join(format!(
            "aexcompat-handle-record-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("record.bin");
        let mut record = vec![0u8; 56 + 72];
        record[..8].copy_from_slice(b"AEXWRAW1");
        for (index, value) in [4i32, 3, 4, 24, -2, 3, 0, 0, 4, 3].into_iter().enumerate() {
            record[8 + index * 4..12 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        record[48..56].copy_from_slice(&72u64.to_le_bytes());
        for row in record[56..].chunks_exact_mut(24) {
            row[..16].fill(1);
            row[16..].fill(0x5a);
        }
        fs::write(&path, &record).unwrap();
        let captured = read_captured_world(&path).unwrap();
        assert_eq!(
            (captured.rowbytes, captured.origin_x, captured.origin_y),
            (24, -2, 3)
        );
        assert_eq!(captured.raw_argb.len(), 72);
        record[48..56].copy_from_slice(&71u64.to_le_bytes());
        fs::write(&path, &record).unwrap();
        assert!(read_captured_world(&path).is_err());
        record[48..56].copy_from_slice(&72u64.to_le_bytes());
        record.truncate(56 + 71);
        fs::write(&path, &record).unwrap();
        assert!(read_captured_world(&path).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fixture_overrides_are_thread_local_and_restore_on_drop() {
        let _guard = FixtureOverridesGuard::set(
            PathBuf::from("target/fixture-dumps"),
            "v1|straight|0|-|0|software".into(),
            None,
            None,
            None,
            None,
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

    #[test]
    fn failed_second_matrix_case_never_publishes_the_first_case() {
        let directory = std::env::temp_dir().join(format!(
            "aexcompat-matrix-atomic-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        fs::create_dir(&directory).unwrap();
        let fixture_path = directory.join("fixture.json");
        let fixture = json!({
            "schema":"aexcompat.render_fixture", "schema_version":2,
            "primary_layer":"primary.png",
            "parameters":[{
                "slot":1,"name":"Amount","kind":"float","minimum":0.0,"maximum":255.0,
                "value":20.0,"choices":[],"color":[255,0,0,0],"components":[0.0,0.0,0.0],
                "component_count":0,"layer_path":null,"enabled":true,"visible":true,
                "supervised":false,"control_size":[0,0]
            }],
            "matrix":[{"slot":1,"values":[20.0,200.0]}],
            "pixel_format":"argb8","render_path":"classic","premultiplication":"straight",
            "timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},
            "final_artifact":"raw","checkpoints":[{"id":"input","stage":"classic-input"}]
        });
        fs::write(&fixture_path, serde_json::to_vec(&fixture).unwrap()).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([40, 60, 80, 255]))
            .save(directory.join("primary.png"))
            .unwrap();
        let loaded = load_render_fixture(&fixture_path).unwrap();
        let cases = expand_fixture_cases(&loaded);
        let output = directory.join("result");
        let mut reached = Vec::new();
        let error = render_fixture_cases(
            &loaded,
            &cases,
            &"22".repeat(32),
            &output,
            |case, _, case_output| {
                reached.push(case.index);
                if case.index == 1 {
                    return Err(invalid("injected case failure"));
                }
                fs::create_dir_all(case_output)?;
                fs::write(case_output.join("output.bin"), [1, 2, 3, 4])?;
                Ok(json!({"passed":true}))
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("fixture case 1 failed"));
        assert_eq!(reached, vec![0, 1]);
        assert!(!output.exists(), "partial matrix output was published");
        assert!(
            fs::read_dir(&directory).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("fixture-tmp")),
            "failed matrix left a staging directory"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
