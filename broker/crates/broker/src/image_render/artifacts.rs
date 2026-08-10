use super::RenderPixelFormat;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct RenderArtifactConditions {
    pub premultiplication: String,
    pub working_space: String,
    pub render_mode: String,
    pub comparison_identity: serde_json::Value,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderArtifactKind {
    Raw,
    Float32Exr,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn validate_conditions(conditions: &RenderArtifactConditions) -> io::Result<()> {
    if !matches!(
        conditions.premultiplication.as_str(),
        "straight" | "premultiplied" | "opaque"
    ) || conditions.working_space != "None"
        || conditions.render_mode != "software"
    {
        return Err(invalid("render artifact conditions are not canonical"));
    }
    let identity = conditions
        .comparison_identity
        .as_object()
        .ok_or_else(|| invalid("comparison identity must be an object"))?;
    for key in [
        "plugin_sha256",
        "input_sha256",
        "world_sha256",
        "render_path",
        "pixel_format",
        "timing",
        "requested_parameters",
        "origin",
    ] {
        if !identity.contains_key(key) {
            return Err(invalid(format!("comparison identity lacks {key}")));
        }
    }
    if identity.len() != 8 {
        return Err(invalid("comparison identity has unknown keys"));
    }
    for key in ["plugin_sha256", "input_sha256", "world_sha256"] {
        let value = identity[key]
            .as_str()
            .filter(|value| {
                value.len() == 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            .ok_or_else(|| invalid(format!("comparison identity has invalid {key}")))?;
        let _ = value;
    }
    if !matches!(
        identity["render_path"].as_str(),
        Some("classic" | "smartfx")
    ) || !matches!(
        identity["pixel_format"].as_str(),
        Some("argb8" | "argb16" | "argb32f")
    ) || !identity["requested_parameters"].is_array()
    {
        return Err(invalid("comparison identity values are not canonical"));
    }
    let timing = identity["timing"]
        .as_object()
        .filter(|value| {
            value.len() == 4
                && ["current_time", "time_step", "total_time", "time_scale"]
                    .into_iter()
                    .all(|key| value.get(key).and_then(serde_json::Value::as_i64).is_some())
        })
        .ok_or_else(|| invalid("comparison identity timing is not canonical"))?;
    let origin = identity["origin"]
        .as_object()
        .filter(|value| {
            value.len() == 2
                && ["x", "y"]
                    .into_iter()
                    .all(|key| value.get(key).and_then(serde_json::Value::as_i64).is_some())
        })
        .ok_or_else(|| invalid("comparison identity origin is not canonical"))?;
    let _ = (timing, origin);
    Ok(())
}

fn validate_identity_origin(
    conditions: &RenderArtifactConditions,
    origin_x: i32,
    origin_y: i32,
) -> io::Result<()> {
    if conditions.comparison_identity["origin"] != serde_json::json!({"x":origin_x,"y":origin_y}) {
        return Err(invalid(
            "comparison identity origin does not match artifact origin",
        ));
    }
    Ok(())
}

fn component_bytes(format: RenderPixelFormat) -> usize {
    match format {
        RenderPixelFormat::Argb8 => 1,
        RenderPixelFormat::Argb16 => 2,
        RenderPixelFormat::Argb32f => 4,
    }
}

fn validate(bytes: &[u8], width: u32, height: u32, component: usize) -> io::Result<()> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4 * component))
        .ok_or_else(|| invalid("artifact dimensions overflow"))?;
    if width == 0 || height == 0 || bytes.len() != expected {
        return Err(invalid("artifact byte count does not match dimensions"));
    }
    Ok(())
}

fn rgba_to_argb(bytes: &[u8], component: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for pixel in bytes.chunks_exact(4 * component) {
        out.extend_from_slice(&pixel[3 * component..4 * component]);
        out.extend_from_slice(&pixel[..3 * component]);
    }
    out
}

fn staging_path(final_path: &Path) -> io::Result<PathBuf> {
    let parent = final_path
        .parent()
        .ok_or_else(|| invalid("artifact path has no parent"))?;
    let name = final_path
        .file_name()
        .ok_or_else(|| invalid("artifact path has no name"))?;
    for nonce in 0..1024u32 {
        let path = parent.join(format!(
            ".{}.tmp-{}-{nonce}",
            name.to_string_lossy(),
            std::process::id()
        ));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no artifact staging name available",
    ))
}

fn commit_directory(final_dir: &Path, files: &[(&str, &[u8])]) -> io::Result<()> {
    if final_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "artifact directory exists",
        ));
    }
    fs::create_dir_all(
        final_dir
            .parent()
            .ok_or_else(|| invalid("artifact path has no parent"))?,
    )?;
    let staging = staging_path(final_dir)?;
    fs::create_dir(&staging)?;
    let result = (|| {
        for (name, bytes) in files {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(staging.join(name))?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        fs::rename(&staging, final_dir)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

pub fn write_raw_world_artifact(
    directory: &Path,
    packed_rgba: &[u8],
    width: u32,
    height: u32,
    format: RenderPixelFormat,
    origin_x: i32,
    origin_y: i32,
    conditions: RenderArtifactConditions,
) -> io::Result<serde_json::Value> {
    validate_conditions(&conditions)?;
    validate_identity_origin(&conditions, origin_x, origin_y)?;
    let component = component_bytes(format);
    validate(packed_rgba, width, height, component)?;
    let raw = rgba_to_argb(packed_rgba, component);
    let representation = match format {
        RenderPixelFormat::Argb8 => "unsigned_integer_0_255",
        RenderPixelFormat::Argb16 => "unsigned_integer_0_32768_ae_internal",
        RenderPixelFormat::Argb32f => "ieee754_binary32_raw_words",
    };
    let metadata = serde_json::json!({
        "schema":"aexcompat.render_raw", "schema_version":1, "width":width, "height":height,
        "rowbytes":u64::from(width)*4*component as u64, "row_padding":"excluded", "channel_order":"ARGB",
        "source_world_rowbytes":serde_json::Value::Null,"source_world_row_padding":"not_transported",
        "pixel_format":format.report_name(), "component_bytes":component, "component_representation":representation,
        "endianness":if component == 1 {"not_applicable"} else {"little"},
        "premultiplication":conditions.premultiplication, "working_space":conditions.working_space,
        "render_mode":conditions.render_mode, "comparison_identity":conditions.comparison_identity,
        "origin":{"x":origin_x,"y":origin_y},
        "data_file":"output.bin", "data_size_bytes":raw.len(), "data_sha256":format!("{:x}", Sha256::digest(&raw))
        ,"comparison_boundaries":{"aex_arithmetic":"internal_world_raw","host_export":"not_applicable"}
    });
    let json = serde_json::to_vec_pretty(&metadata)?;
    commit_directory(directory, &[("output.bin", &raw), ("output.json", &json)])?;
    Ok(metadata)
}

fn cstr(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(value.as_bytes());
    out.push(0);
}
fn attribute(out: &mut Vec<u8>, name: &str, kind: &str, value: &[u8]) {
    cstr(out, name);
    cstr(out, kind);
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value);
}

fn encode_exr(rgba: &[u8], width: u32, height: u32) -> io::Result<Vec<u8>> {
    validate(rgba, width, height, 4)?;
    let mut out = Vec::new();
    out.extend_from_slice(&20000630u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    let mut channels = Vec::new();
    for name in ["A", "B", "G", "R"] {
        cstr(&mut channels, name);
        channels.extend_from_slice(&2i32.to_le_bytes());
        channels.extend_from_slice(&[0; 4]);
        channels.extend_from_slice(&1i32.to_le_bytes());
        channels.extend_from_slice(&1i32.to_le_bytes());
    }
    channels.push(0);
    attribute(&mut out, "channels", "chlist", &channels);
    attribute(&mut out, "compression", "compression", &[0]);
    let mut window = Vec::new();
    for v in [0i32, 0, width as i32 - 1, height as i32 - 1] {
        window.extend_from_slice(&v.to_le_bytes());
    }
    attribute(&mut out, "dataWindow", "box2i", &window);
    attribute(&mut out, "displayWindow", "box2i", &window);
    attribute(&mut out, "lineOrder", "lineOrder", &[0]);
    attribute(&mut out, "pixelAspectRatio", "float", &1f32.to_le_bytes());
    attribute(&mut out, "screenWindowCenter", "v2f", &[0; 8]);
    attribute(&mut out, "screenWindowWidth", "float", &1f32.to_le_bytes());
    out.push(0);
    let line_bytes = width as usize * 16;
    let table = out.len();
    out.resize(table + height as usize * 8, 0);
    let first = out.len() as u64;
    for y in 0..height as usize {
        let offset = first + y as u64 * (8 + line_bytes) as u64;
        out[table + y * 8..table + y * 8 + 8].copy_from_slice(&offset.to_le_bytes());
    }
    for y in 0..height as usize {
        out.extend_from_slice(&(y as i32).to_le_bytes());
        out.extend_from_slice(&(line_bytes as u32).to_le_bytes());
        let row = &rgba[y * line_bytes..(y + 1) * line_bytes];
        for channel in [3usize, 2, 1, 0] {
            for pixel in row.chunks_exact(16) {
                out.extend_from_slice(&pixel[channel * 4..channel * 4 + 4]);
            }
        }
    }
    Ok(out)
}

pub fn write_float32_exr_artifact(
    directory: &Path,
    packed_rgba32f: &[u8],
    width: u32,
    height: u32,
    origin_x: i32,
    origin_y: i32,
    conditions: RenderArtifactConditions,
) -> io::Result<serde_json::Value> {
    validate_conditions(&conditions)?;
    validate_identity_origin(&conditions, origin_x, origin_y)?;
    let exr = encode_exr(packed_rgba32f, width, height)?;
    let metadata = serde_json::json!({"schema":"aexcompat.render_exr","schema_version":1,"width":width,"height":height,
        "storage":"scanline","compression":"none","pixel_format":"float32","channel_order":"RGBA",
        "exr_file_channel_order":["A","B","G","R"],"channel_type":"FLOAT32","endianness":"little",
        "source_world_rowbytes":serde_json::Value::Null,"source_world_row_padding":"not_transported",
        "source_transport_order":"RGBA","word_comparison":"raw_u32_little_endian","rgb_policy":"preserve",
        "premultiplication":conditions.premultiplication,"working_space":conditions.working_space,"render_mode":conditions.render_mode,
        "comparison_identity":conditions.comparison_identity,
        "origin":{"x":origin_x,"y":origin_y},"data_file":"output.exr","data_size_bytes":exr.len(),"data_sha256":format!("{:x}",Sha256::digest(&exr)),
        "comparison_boundaries":{"aex_arithmetic":"compare_source_raw_world","host_export":"compare_float32_exr_raw_u32"}});
    let json = serde_json::to_vec_pretty(&metadata)?;
    commit_directory(directory, &[("output.exr", &exr), ("output.json", &json)])?;
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("aexcompat-{name}-{}", std::process::id()))
    }
    fn conditions() -> RenderArtifactConditions {
        RenderArtifactConditions {
            premultiplication: "straight".into(),
            working_space: "None".into(),
            render_mode: "software".into(),
            comparison_identity: serde_json::json!({
                "plugin_sha256":"11".repeat(32), "input_sha256":"22".repeat(32),
                "world_sha256":"33".repeat(32),
                "render_path":"smartfx", "pixel_format":"argb32f",
                "timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},
                "requested_parameters":[], "origin":{"x":0,"y":0}
            }),
        }
    }
    #[test]
    fn raw_pf16_preserves_words() {
        let d = temp("raw16");
        let _ = fs::remove_dir_all(&d);
        let w = [1u16, 2, 32768, 0x1234];
        let b = w.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
        let m =
            write_raw_world_artifact(&d, &b, 1, 1, RenderPixelFormat::Argb16, 0, 0, conditions())
                .unwrap();
        assert_eq!(
            fs::read(d.join("output.bin")).unwrap(),
            [0x34, 0x12, 1, 0, 2, 0, 0, 0x80]
        );
        assert_eq!(m["row_padding"], "excluded");
        assert_eq!(m["rowbytes"], 8);
        assert!(m["source_world_rowbytes"].is_null());
        let persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(d.join("output.json")).unwrap()).unwrap();
        assert_eq!(persisted, m);
        let keys = persisted
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            [
                "channel_order",
                "comparison_boundaries",
                "comparison_identity",
                "component_bytes",
                "component_representation",
                "data_file",
                "data_sha256",
                "data_size_bytes",
                "endianness",
                "height",
                "origin",
                "pixel_format",
                "premultiplication",
                "render_mode",
                "row_padding",
                "rowbytes",
                "schema",
                "schema_version",
                "source_world_row_padding",
                "source_world_rowbytes",
                "width",
                "working_space"
            ]
            .into_iter()
            .collect()
        );
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn raw_pf8_and_pf32_preserve_component_words() {
        for (name, format, rgba, expected) in [
            (
                "raw8",
                RenderPixelFormat::Argb8,
                vec![1, 2, 3, 4],
                vec![4, 1, 2, 3],
            ),
            (
                "raw32",
                RenderPixelFormat::Argb32f,
                [0x7fc12345u32, 0x80000000, 1, 0x3f800000]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect(),
                [0x3f800000u32, 0x7fc12345, 0x80000000, 1]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect(),
            ),
        ] {
            let d = temp(name);
            let _ = fs::remove_dir_all(&d);
            let mut artifact_conditions = conditions();
            artifact_conditions.comparison_identity["origin"] = serde_json::json!({"x":-2,"y":3});
            let metadata =
                write_raw_world_artifact(&d, &rgba, 1, 1, format, -2, 3, artifact_conditions)
                    .unwrap();
            assert_eq!(fs::read(d.join("output.bin")).unwrap(), expected);
            assert!(metadata["source_world_rowbytes"].is_null());
            assert_eq!(metadata["rowbytes"], rgba.len() as u64);
            assert_eq!(metadata["origin"], serde_json::json!({"x":-2,"y":3}));
            fs::remove_dir_all(d).unwrap();
        }
    }
    #[test]
    fn exr_preserves_special_words_and_order() {
        let d = temp("exr");
        let _ = fs::remove_dir_all(&d);
        let w = [0x7fc12345u32, 0x80000000, 0x00000001, 0x3f800000];
        let b = w.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
        let metadata = write_float32_exr_artifact(&d, &b, 1, 1, 0, 0, conditions()).unwrap();
        let persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(d.join("output.json")).unwrap()).unwrap();
        assert_eq!(persisted, metadata);
        assert_eq!(persisted["channel_order"], "RGBA");
        assert_eq!(
            persisted["exr_file_channel_order"],
            serde_json::json!(["A", "B", "G", "R"])
        );
        assert_eq!(persisted["endianness"], "little");
        assert_eq!(persisted["pixel_format"], "float32");
        let e = fs::read(d.join("output.exr")).unwrap();
        assert_eq!(u32::from_le_bytes(e[0..4].try_into().unwrap()), 20000630);
        assert_eq!(u32::from_le_bytes(e[4..8].try_into().unwrap()), 2);
        let compression = e
            .windows(b"compression\0compression\0".len())
            .position(|part| part == b"compression\0compression\0")
            .unwrap();
        let compression_value = compression + b"compression\0compression\0".len() + 4;
        assert_eq!(e[compression_value], 0);
        let mut cursor = 8usize;
        loop {
            let name_end = cursor + e[cursor..].iter().position(|byte| *byte == 0).unwrap();
            if name_end == cursor {
                cursor += 1;
                break;
            }
            cursor = name_end + 1;
            let type_end = cursor + e[cursor..].iter().position(|byte| *byte == 0).unwrap();
            cursor = type_end + 1;
            let size = u32::from_le_bytes(e[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4 + size;
        }
        let offset_table = cursor;
        let block_offset =
            u64::from_le_bytes(e[offset_table..offset_table + 8].try_into().unwrap()) as usize;
        assert_eq!(
            i32::from_le_bytes(e[block_offset..block_offset + 4].try_into().unwrap()),
            0
        );
        assert_eq!(
            u32::from_le_bytes(e[block_offset + 4..block_offset + 8].try_into().unwrap()),
            16
        );
        let got = e[block_offset + 8..block_offset + 24]
            .chunks_exact(4)
            .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(got, [w[3], w[2], w[1], w[0]]);
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let script = "import OpenEXR,sys; f=OpenEXR.File(sys.argv[1]); p=f.channels()['RGBA'].pixels.view('uint32').reshape(-1).tolist(); assert p == [0x7fc12345,0x80000000,0x00000001,0x3f800000], [hex(x) for x in p]";
        let decoded = std::process::Command::new("uv")
            .args(["run", "--project"])
            .arg(&repository)
            .args(["python", "-c", script])
            .arg(d.join("output.exr"))
            .output()
            .expect("launch independent OpenEXR decoder");
        assert!(
            decoded.status.success(),
            "independent OpenEXR word check failed: {}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn artifact_conditions_reject_ambiguous_metadata() {
        let d = temp("invalid-conditions");
        let _ = fs::remove_dir_all(&d);
        let invalid = RenderArtifactConditions {
            premultiplication: "unspecified".into(),
            working_space: "None".into(),
            render_mode: "software".into(),
            comparison_identity: conditions().comparison_identity,
        };
        assert_eq!(
            write_raw_world_artifact(&d, &[0; 4], 1, 1, RenderPixelFormat::Argb8, 0, 0, invalid)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!d.exists());
        for (name, mutate) in [
            ("extra", "extra"),
            ("timing", "timing"),
            ("origin", "origin"),
        ] {
            let target = temp(&format!("invalid-identity-{name}"));
            let _ = fs::remove_dir_all(&target);
            let mut invalid = conditions();
            match mutate {
                "extra" => invalid.comparison_identity["unexpected"] = serde_json::json!(true),
                "timing" => invalid.comparison_identity["timing"] = serde_json::json!({}),
                "origin" => invalid.comparison_identity["origin"] = serde_json::json!({"x":0}),
                _ => unreachable!(),
            }
            assert_eq!(
                write_raw_world_artifact(
                    &target,
                    &[0; 4],
                    1,
                    1,
                    RenderPixelFormat::Argb8,
                    0,
                    0,
                    invalid,
                )
                .unwrap_err()
                .kind(),
                io::ErrorKind::InvalidInput
            );
            assert!(!target.exists());
        }
    }
    #[test]
    fn commit_is_no_overwrite_atomic_set() {
        let d = temp("atomic");
        let _ = fs::remove_dir_all(&d);
        write_raw_world_artifact(
            &d,
            &[0; 4],
            1,
            1,
            RenderPixelFormat::Argb8,
            0,
            0,
            conditions(),
        )
        .unwrap();
        assert_eq!(
            write_raw_world_artifact(
                &d,
                &[1; 4],
                1,
                1,
                RenderPixelFormat::Argb8,
                0,
                0,
                conditions()
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(d.join("output.bin")).unwrap(), [0; 4]);
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn failed_staging_never_publishes_partial_set() {
        let d = temp("partial");
        let _ = fs::remove_dir_all(&d);
        let error = commit_directory(&d, &[("same", b"first"), ("same", b"second")]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(!d.exists());
    }
}
