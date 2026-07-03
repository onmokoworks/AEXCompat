//! No-load identity smoke over generated AEX probe fixture images.
//!
//! This tool consumes only the synthetic fixture manifest produced by
//! `aex_probe_fixture_images`, runs the broker `identity_transport` operation
//! for each generated PNG, and verifies RGBA identity. It does not inspect,
//! start, describe, or render an AEX effect.

#[allow(dead_code)]
#[path = "aex_image_probe.rs"]
mod aex_image_probe;

use anyhow::{bail, Context};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const GENERATED_BY: &str = "aex_probe_fixture_identity_smoke";
const FIXTURE_GENERATOR: &str = "aex_probe_fixture_images";
const FIXTURE_STATUS: &str = "synthetic_fixture_images_ready_no_load";
const SMOKE_STATUS: &str = "fixture_identity_smoke_ready_no_load";
const SMOKE_ROOT_MARKER: &str = "/target/aex-image-probe";
const EXPECTED_IMAGES: &[(&str, &str, &str)] = &[
    ("gradient", "gradient_rgba8.png", "xy-gradient-rgba8"),
    ("checker", "checker_rgba8.png", "checker-rgba8"),
    ("solid_alpha", "solid_alpha_rgba8.png", "solid-alpha-rgba8"),
];
const FORBIDDEN_REPORT_TEXT_TOKENS: &[&str] = &[
    "sha256",
    "base64",
    "binary_payload",
    "payload_bytes",
    "copied_asset",
    "native_load_result",
    "loadlibrary",
    concat!("lib", "loading"),
    "effectmain",
    "rendered_pixels",
    "worker_loaded",
    "plugin_loaded",
];

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cli = Cli::parse(&args)?;
    let report = run_fixture_identity_smoke(&cli.fixture_manifest, &cli.out, &cli.report)?;
    println!(
        "{} {} identity transports at {}",
        report.status, report.transport_count, report.output_root
    );
    Ok(())
}

#[derive(Debug)]
struct Cli {
    fixture_manifest: PathBuf,
    out: PathBuf,
    report: PathBuf,
}

impl Cli {
    fn parse(args: &[String]) -> anyhow::Result<Self> {
        let mut fixture_manifest = None;
        let mut out = None;
        let mut report = None;
        let mut i = 1usize;
        while i < args.len() {
            match args[i].as_str() {
                "--fixture-manifest" => {
                    i += 1;
                    fixture_manifest = args.get(i).map(PathBuf::from);
                }
                "--out" => {
                    i += 1;
                    out = args.get(i).map(PathBuf::from);
                }
                "--report" => {
                    i += 1;
                    report = args.get(i).map(PathBuf::from);
                }
                "--help" | "-h" => print_usage_and_exit(),
                value => bail!("unknown argument {value}"),
            }
            i += 1;
        }
        Ok(Self {
            fixture_manifest: fixture_manifest.context("--fixture-manifest <path> is required")?,
            out: out.context("--out <target/aex-image-probe/...> is required")?,
            report: report.context("--report <target/aex-image-probe/...> is required")?,
        })
    }
}

fn print_usage_and_exit() -> ! {
    eprintln!(
        "usage: aex_probe_fixture_identity_smoke --fixture-manifest target\\aex-probe-fixtures\\manifest.local.json --out target\\aex-image-probe\\fixture-identity-smoke --report target\\aex-image-probe\\fixture-identity-smoke\\smoke.local.json"
    );
    std::process::exit(2);
}

#[derive(Debug, Deserialize)]
struct FixtureManifestInput {
    schema_version: u32,
    generated_by: String,
    publication_status: String,
    status: String,
    pixel_format: String,
    width: u32,
    height: u32,
    image_count: usize,
    native_load_performed: bool,
    render_performed: bool,
    aex_loaded: bool,
    worker_started: bool,
    broker_invoked: bool,
    ofx_route_invoked: bool,
    ae_invoked: bool,
    private_payload_copied: bool,
    images: Vec<FixtureImageInput>,
}

#[derive(Debug, Deserialize)]
struct FixtureImageInput {
    id: String,
    file_name: String,
    relative_path: String,
    width: u32,
    height: u32,
    pixel_format: String,
    pattern: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixtureIdentitySmokeReport {
    pub schema_version: u32,
    pub generated_by: String,
    pub generated_unix_ms: u64,
    pub publication_status: String,
    pub status: String,
    pub fixture_manifest: String,
    pub fixture_manifest_status: String,
    pub output_root: String,
    pub transport_operation: String,
    pub pixel_format: String,
    pub image_count: usize,
    pub transport_count: usize,
    pub identity_pixels_checked_count: usize,
    pub native_load_performed: bool,
    pub render_performed: bool,
    pub aex_loaded: bool,
    pub worker_started: bool,
    pub broker_invoked: bool,
    pub ofx_route_invoked: bool,
    pub ae_invoked: bool,
    pub private_payload_copied: bool,
    pub aex_render_correctness_evidence: bool,
    pub entries: Vec<FixtureIdentitySmokeEntry>,
    pub checks: Vec<FixtureIdentitySmokeCheck>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixtureIdentitySmokeEntry {
    pub id: String,
    pub pattern: String,
    pub input_png: String,
    pub output_png: String,
    pub width: u32,
    pub height: u32,
    pub pixel_format: String,
    pub transport_status: String,
    pub plugin_class: String,
    pub identity_pixels_match: bool,
    pub worker_started: bool,
    pub aex_loaded: bool,
    pub render_performed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixtureIdentitySmokeCheck {
    pub name: String,
    pub status: String,
    pub evidence: String,
}

pub fn run_fixture_identity_smoke(
    fixture_manifest: &Path,
    out_dir: &Path,
    report_path: &Path,
) -> anyhow::Result<FixtureIdentitySmokeReport> {
    let fixture_manifest = resolve_cwd_relative_path(fixture_manifest);
    let out_dir = resolve_cwd_relative_path(out_dir);
    let report_path = resolve_cwd_relative_path(report_path);
    ensure_no_forbidden_report_text("fixture_manifest path", &path_text(&fixture_manifest))?;
    ensure_no_forbidden_report_text("output root path", &path_text(&out_dir))?;
    ensure_no_forbidden_report_text("report path", &path_text(&report_path))?;
    if !is_generated_fixture_path(&fixture_manifest)? {
        bail!("--fixture-manifest must be under target/aex-probe-fixtures");
    }
    if !is_generated_smoke_path(&out_dir)? {
        bail!("--out must be under target/aex-image-probe");
    }
    if !is_generated_smoke_path(&report_path)? {
        bail!("--report must be under target/aex-image-probe");
    }

    let manifest_text = std::fs::read_to_string(&fixture_manifest)
        .with_context(|| format!("failed to read {}", fixture_manifest.display()))?;
    let manifest: FixtureManifestInput =
        serde_json::from_str(&manifest_text).context("fixture manifest should parse")?;
    validate_fixture_manifest(&manifest)?;

    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create output directory {}", out_dir.display()))?;

    let mut output_paths = Vec::new();
    for image in &manifest.images {
        validate_fixture_image(image, manifest.width, manifest.height)?;
        output_paths.push(out_dir.join(format!("{}_identity_rgba8.png", image.id)));
    }
    output_paths.push(report_path.clone());
    for path in &output_paths {
        ensure_create_new_candidate(path)?;
    }

    let manifest_dir = fixture_manifest
        .parent()
        .context("fixture manifest should have a parent directory")?;
    let mut created_outputs = Vec::new();
    let mut entries = Vec::new();
    let smoke_result = (|| -> anyhow::Result<()> {
        for (image, output_png) in manifest.images.iter().zip(output_paths.iter()) {
            let input_png = manifest_dir.join(&image.relative_path);
            if !is_generated_fixture_path(&input_png)? {
                bail!("fixture image must remain under target/aex-probe-fixtures");
            }
            if !input_png.is_file() {
                bail!(
                    "fixture image must be an existing file: {}",
                    input_png.display()
                );
            }
            let input_rgba = image::open(&input_png)
                .with_context(|| format!("failed to open {}", input_png.display()))?
                .to_rgba8();
            if !fixture_pixels_match_expected_pattern(image, &input_rgba) {
                bail!(
                    "fixture image pixels must match generated pattern for {}",
                    image.id
                );
            }
            let request_text = serde_json::to_string(&json!({
                "schema_version": 1,
                "operation": "identity_transport",
                "input_png": path_text(&input_png),
                "output_png": path_text(output_png),
                "pixel_format": "rgba8",
                "frame": {
                    "width": image.width,
                    "height": image.height
                },
                "limits": {
                    "max_width": image.width,
                    "max_height": image.height,
                    "max_bytes": u64::from(image.width) * u64::from(image.height) * 4
                },
                "params": {}
            }))
            .context("identity transport request should serialize")?;
            let transport = aex_image_probe::run_probe_request_text(&request_text, None)
                .context("identity transport should run")?;
            if transport.status != "ok" {
                bail!(
                    "identity transport failed for {} with status {}",
                    image.id,
                    transport.status
                );
            }
            created_outputs.push(output_png.to_path_buf());
            let output_rgba = image::open(output_png)
                .with_context(|| format!("failed to open {}", output_png.display()))?
                .to_rgba8();
            let identity_pixels_match = input_rgba.dimensions() == output_rgba.dimensions()
                && input_rgba.as_raw() == output_rgba.as_raw();
            if !identity_pixels_match {
                bail!("identity pixels should match for {}", image.id);
            }
            entries.push(FixtureIdentitySmokeEntry {
                id: image.id.clone(),
                pattern: image.pattern.clone(),
                input_png: path_text(&input_png),
                output_png: path_text(output_png),
                width: image.width,
                height: image.height,
                pixel_format: image.pixel_format.clone(),
                transport_status: transport.status,
                plugin_class: transport.plugin_class,
                identity_pixels_match,
                worker_started: false,
                aex_loaded: false,
                render_performed: false,
            });
        }
        Ok(())
    })();
    if let Err(err) = smoke_result {
        cleanup_created_outputs(&created_outputs);
        return Err(err);
    }

    let report = FixtureIdentitySmokeReport {
        schema_version: 1,
        generated_by: GENERATED_BY.to_owned(),
        generated_unix_ms: current_unix_ms(),
        publication_status: "local-only".to_owned(),
        status: SMOKE_STATUS.to_owned(),
        fixture_manifest: path_text(&fixture_manifest),
        fixture_manifest_status: manifest.status,
        output_root: display_path(&out_dir, SMOKE_ROOT_MARKER),
        transport_operation: "identity_transport".to_owned(),
        pixel_format: "rgba8".to_owned(),
        image_count: entries.len(),
        transport_count: entries.len(),
        identity_pixels_checked_count: entries.len(),
        native_load_performed: false,
        render_performed: false,
        aex_loaded: false,
        worker_started: false,
        broker_invoked: true,
        ofx_route_invoked: false,
        ae_invoked: false,
        private_payload_copied: false,
        aex_render_correctness_evidence: false,
        entries,
        checks: vec![
            passed_check("fixture_manifest_validated", "synthetic fixture manifest accepted"),
            passed_check(
                "identity_transport_ok",
                "all fixture images completed broker identity_transport",
            ),
            passed_check(
                "rgba_identity_pixels_match",
                "decoded input and output RGBA pixels matched for every image",
            ),
            passed_check(
                "synthetic_fixture_pixels_match",
                "decoded input RGBA pixels matched the expected generated fixture patterns",
            ),
            passed_check("no_aex_input", "no AEX path is read or required"),
            passed_check(
                "no_worker_or_host_invocation",
                "no worker, OFX, or AE process is started",
            ),
            passed_check(
                "not_render_correctness_evidence",
                "identity transport is not AEX render correctness evidence",
            ),
        ],
        notes: vec![
            "This smoke verifies broker PNG identity transport over synthetic inputs only."
                .to_owned(),
            "It is not AEX rendering, parameter discovery, loader approval, or OFX routing evidence."
                .to_owned(),
            "No AEX file is opened, copied, loaded, described, or rendered.".to_owned(),
        ],
    };
    if let Err(err) = write_report_create_new(&report_path, &report) {
        cleanup_created_outputs(&created_outputs);
        return Err(err);
    }
    Ok(report)
}

fn validate_fixture_manifest(manifest: &FixtureManifestInput) -> anyhow::Result<()> {
    if manifest.schema_version != 1 {
        bail!("fixture manifest schema_version must be 1");
    }
    if manifest.generated_by != FIXTURE_GENERATOR {
        bail!("fixture manifest generated_by must be {FIXTURE_GENERATOR}");
    }
    if manifest.publication_status != "local-only" {
        bail!("fixture manifest publication_status must be local-only");
    }
    if manifest.status != FIXTURE_STATUS {
        bail!("fixture manifest status must be {FIXTURE_STATUS}");
    }
    if manifest.pixel_format != "rgba8" {
        bail!("fixture manifest pixel_format must be rgba8");
    }
    if manifest.width == 0 || manifest.height == 0 {
        bail!("fixture dimensions must be nonzero");
    }
    if manifest.image_count != manifest.images.len() {
        bail!("fixture manifest image_count must match images length");
    }
    if manifest.image_count != 3 {
        bail!("fixture manifest must contain the three synthetic images");
    }
    let mut ids: Vec<&str> = manifest
        .images
        .iter()
        .map(|image| image.id.as_str())
        .collect();
    ids.sort_unstable();
    let mut expected_ids: Vec<&str> = EXPECTED_IMAGES.iter().map(|(id, _, _)| *id).collect();
    expected_ids.sort_unstable();
    if ids != expected_ids {
        bail!("fixture manifest image ids must match the generated synthetic set");
    }
    if manifest.native_load_performed
        || manifest.render_performed
        || manifest.aex_loaded
        || manifest.worker_started
        || manifest.broker_invoked
        || manifest.ofx_route_invoked
        || manifest.ae_invoked
        || manifest.private_payload_copied
    {
        bail!("fixture manifest must preserve no-load/no-render boundary");
    }
    Ok(())
}

fn validate_fixture_image(
    image: &FixtureImageInput,
    manifest_width: u32,
    manifest_height: u32,
) -> anyhow::Result<()> {
    if !is_safe_id(&image.id) {
        bail!("fixture image id must be a safe generated identifier");
    }
    let Some((_, expected_file_name, expected_pattern)) = EXPECTED_IMAGES
        .iter()
        .find(|(expected_id, _, _)| *expected_id == image.id)
    else {
        bail!("fixture image id is not part of the generated synthetic set");
    };
    let relative_path = Path::new(&image.relative_path);
    let components: Vec<_> = relative_path.components().collect();
    if relative_path.is_absolute()
        || components.len() != 1
        || !matches!(components.first(), Some(Component::Normal(_)))
    {
        bail!("fixture image relative_path must be a single relative file name");
    }
    if image.relative_path != image.file_name {
        bail!("fixture image relative_path must match file_name");
    }
    if image.file_name != *expected_file_name {
        bail!("fixture image file_name must match the generated synthetic fixture");
    }
    if image.pattern != *expected_pattern {
        bail!("fixture image pattern must match the generated synthetic fixture");
    }
    if !image.file_name.ends_with(".png") {
        bail!("fixture image file_name must end with .png");
    }
    if image.pixel_format != "rgba8" {
        bail!("fixture image pixel_format must be rgba8");
    }
    if image.width != manifest_width || image.height != manifest_height {
        bail!("fixture image dimensions must match manifest dimensions");
    }
    Ok(())
}

fn fixture_pixels_match_expected_pattern(image: &FixtureImageInput, pixels: &RgbaImage) -> bool {
    if pixels.dimensions() != (image.width, image.height) {
        return false;
    }
    match image.id.as_str() {
        "gradient" => pixels.enumerate_pixels().all(|(x, y, pixel)| {
            pixel.0
                == [
                    scaled_byte(x, image.width),
                    scaled_byte(y, image.height),
                    scaled_byte(
                        x + y,
                        image.width.saturating_add(image.height).saturating_sub(1),
                    ),
                    255,
                ]
        }),
        "checker" => {
            let cell = image.width.min(image.height).saturating_div(8).max(1);
            pixels.enumerate_pixels().all(|(x, y, pixel)| {
                let bright = ((x / cell) + (y / cell)) % 2 == 0;
                let expected = if bright {
                    [232, 232, 232, 255]
                } else {
                    [32, 32, 32, 255]
                };
                pixel.0 == expected
            })
        }
        "solid_alpha" => pixels.pixels().all(|pixel| pixel.0 == [96, 168, 255, 128]),
        _ => false,
    }
}

fn scaled_byte(value: u32, max_value: u32) -> u8 {
    if max_value <= 1 {
        return 0;
    }
    ((value.min(max_value - 1) * 255) / (max_value - 1)) as u8
}

fn ensure_no_forbidden_report_text(label: &str, value: &str) -> anyhow::Result<()> {
    let lowered = value.to_ascii_lowercase();
    if let Some(token) = FORBIDDEN_REPORT_TEXT_TOKENS
        .iter()
        .find(|token| lowered.contains(**token))
    {
        bail!("{label} contains forbidden report token {token}");
    }
    Ok(())
}

fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn ensure_create_new_candidate(path: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => bail!("generated smoke output already exists: {}", path.display()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => {
            bail!(
                "generated smoke output metadata should be readable for {}: {err}",
                path.display()
            )
        }
    }
}

fn write_report_create_new(path: &Path, report: &FixtureIdentitySmokeReport) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report dir {}", parent.display()))?;
    }
    let report_text = serde_json::to_string_pretty(report).context("report should serialize")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    if let Err(err) = file.write_all(report_text.as_bytes()) {
        let _ = std::fs::remove_file(path);
        bail!("failed to write {}: {err}", path.display());
    }
    Ok(())
}

fn cleanup_created_outputs(paths: &[PathBuf]) {
    for path in paths.iter().rev() {
        let _ = std::fs::remove_file(path);
    }
}

fn passed_check(name: &str, evidence: &str) -> FixtureIdentitySmokeCheck {
    FixtureIdentitySmokeCheck {
        name: name.to_owned(),
        status: "passed".to_owned(),
        evidence: evidence.to_owned(),
    }
}

fn resolve_cwd_relative_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(path)
}

fn is_generated_fixture_path(path: &Path) -> anyhow::Result<bool> {
    is_under_generated_root(path, &generated_fixture_root())
}

fn is_generated_smoke_path(path: &Path) -> anyhow::Result<bool> {
    is_under_generated_root(path, &generated_smoke_root())
}

fn generated_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-probe-fixtures")
}

fn generated_smoke_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-image-probe")
}

fn is_under_generated_root(path: &Path, root: &Path) -> anyhow::Result<bool> {
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Ok(false);
    }
    let text = normalize_path_text(&path.to_string_lossy());
    let root_text = normalize_path_text(&root.to_string_lossy());
    Ok(
        (text == root_text || text.starts_with(&format!("{root_text}/")))
            && existing_path_and_ancestors_are_plain(path)?,
    )
}

fn existing_path_and_ancestors_are_plain(path: &Path) -> anyhow::Result<bool> {
    for ancestor in path
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
    {
        if existing_path_is_reparse_or_symlink(ancestor)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn existing_path_is_reparse_or_symlink(path: &Path) -> anyhow::Result<bool> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            bail!(
                "generated smoke path metadata should be readable for {}: {err}",
                path.display()
            );
        }
    };
    if metadata.file_type().is_symlink() {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

fn path_text(path: &Path) -> String {
    normalize_path_text(&path.to_string_lossy())
}

fn display_path(path: &Path, marker: &str) -> String {
    let text = path_text(path);
    if let Some(index) = text.find(marker.trim_start_matches('/')) {
        return text[index..].to_owned();
    }
    text
}

fn normalize_path_text(path: &str) -> String {
    let replaced = path.replace('\\', "/");
    let mut prefix = String::new();
    let mut parts = Vec::new();
    for (index, part) in replaced.split('/').enumerate() {
        if index == 0 && part.ends_with(':') {
            prefix = part.to_ascii_lowercase();
            continue;
        }
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value.to_owned()),
        }
    }
    let joined = parts.join("/");
    if prefix.is_empty() {
        joined
    } else {
        format!("{prefix}/{joined}")
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
