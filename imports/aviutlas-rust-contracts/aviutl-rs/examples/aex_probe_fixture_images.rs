//! Synthetic RGBA8 fixture image generator for the AEX image probe path.
//!
//! This tool creates developer-owned PNG inputs only. It does not inspect,
//! start, describe, or render an AEX effect.

use anyhow::{bail, Context};
use image::{ImageEncoder, Rgba, RgbaImage};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const GENERATED_BY: &str = "aex_probe_fixture_images";
const OUTPUT_ROOT_MARKER: &str = "/target/aex-probe-fixtures";
const MAX_SIZE: u32 = 4096;

#[derive(Debug, Clone, Serialize)]
pub struct FixtureManifest {
    pub schema_version: u32,
    pub generated_by: String,
    pub generated_unix_ms: u64,
    pub publication_status: String,
    pub status: String,
    pub output_root: String,
    pub pixel_format: String,
    pub width: u32,
    pub height: u32,
    pub image_count: usize,
    pub native_load_performed: bool,
    pub render_performed: bool,
    pub aex_loaded: bool,
    pub worker_started: bool,
    pub broker_invoked: bool,
    pub ofx_route_invoked: bool,
    pub ae_invoked: bool,
    pub private_payload_copied: bool,
    pub images: Vec<FixtureImageEntry>,
    pub checks: Vec<FixtureCheck>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixtureImageEntry {
    pub id: String,
    pub file_name: String,
    pub relative_path: String,
    pub width: u32,
    pub height: u32,
    pub pixel_format: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixtureCheck {
    pub name: String,
    pub status: String,
    pub evidence: String,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cli = Cli::parse(&args)?;
    let manifest = generate_fixture_images(&cli.out, cli.size, &cli.manifest)?;
    println!(
        "{} {} images at {}",
        manifest.status, manifest.image_count, manifest.output_root
    );
    Ok(())
}

#[derive(Debug)]
struct Cli {
    out: PathBuf,
    size: u32,
    manifest: PathBuf,
}

impl Cli {
    fn parse(args: &[String]) -> anyhow::Result<Self> {
        let mut out = None;
        let mut size = Some(128u32);
        let mut manifest = None;
        let mut i = 1usize;
        while i < args.len() {
            match args[i].as_str() {
                "--out" => {
                    i += 1;
                    out = args.get(i).map(PathBuf::from);
                }
                "--size" => {
                    i += 1;
                    let raw = args.get(i).context("--size requires a value")?;
                    size = Some(raw.parse().context("--size must be an integer")?);
                }
                "--manifest" => {
                    i += 1;
                    manifest = args.get(i).map(PathBuf::from);
                }
                "--help" | "-h" => print_usage_and_exit(),
                value => bail!("unknown argument {value}"),
            }
            i += 1;
        }
        Ok(Self {
            out: out.context("--out <target/aex-probe-fixtures/...> is required")?,
            size: size.unwrap_or(128),
            manifest: manifest.context("--manifest <path> is required")?,
        })
    }
}

fn print_usage_and_exit() -> ! {
    eprintln!(
        "usage: aex_probe_fixture_images --out target\\aex-probe-fixtures --size 128 --manifest target\\aex-probe-fixtures\\manifest.local.json"
    );
    std::process::exit(2);
}

pub fn generate_fixture_images(
    out_dir: &Path,
    size: u32,
    manifest_path: &Path,
) -> anyhow::Result<FixtureManifest> {
    if size == 0 || size > MAX_SIZE {
        bail!("--size must be between 1 and {MAX_SIZE}");
    }
    let out_dir = resolve_cwd_relative_path(out_dir);
    let manifest_path = resolve_cwd_relative_path(manifest_path);
    if !is_generated_fixture_path(&out_dir)? {
        bail!("--out must be under target/aex-probe-fixtures");
    }
    if !is_generated_fixture_path(&manifest_path)? {
        bail!("--manifest must be under target/aex-probe-fixtures");
    }

    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("failed to create output directory {}", out_dir.display()))?;

    let specs = [
        ("gradient", "gradient_rgba8.png", "xy-gradient-rgba8"),
        ("checker", "checker_rgba8.png", "checker-rgba8"),
        ("solid_alpha", "solid_alpha_rgba8.png", "solid-alpha-rgba8"),
    ];
    let output_paths: Vec<PathBuf> = specs
        .iter()
        .map(|(_, file_name, _)| out_dir.join(file_name))
        .chain(std::iter::once(manifest_path.clone()))
        .collect();
    for path in &output_paths {
        ensure_create_new_candidate(path)?;
    }

    let mut images = Vec::new();
    let mut created_paths = Vec::new();
    let write_result = (|| -> anyhow::Result<()> {
        for (id, file_name, pattern) in specs {
            let path = out_dir.join(file_name);
            let image = match id {
                "gradient" => gradient_image(size, size),
                "checker" => checker_image(size, size),
                "solid_alpha" => solid_alpha_image(size, size),
                _ => unreachable!("all fixture image specs are known"),
            };
            write_png_create_new(&path, &image)?;
            created_paths.push(path);
            images.push(FixtureImageEntry {
                id: id.to_owned(),
                file_name: file_name.to_owned(),
                relative_path: file_name.to_owned(),
                width: size,
                height: size,
                pixel_format: "rgba8".to_owned(),
                pattern: pattern.to_owned(),
            });
        }
        Ok(())
    })();
    if let Err(err) = write_result {
        cleanup_created_outputs(&created_paths);
        return Err(err);
    }

    let manifest = FixtureManifest {
        schema_version: 1,
        generated_by: GENERATED_BY.to_owned(),
        generated_unix_ms: current_unix_ms(),
        publication_status: "local-only".to_owned(),
        status: "synthetic_fixture_images_ready_no_load".to_owned(),
        output_root: manifest_display_path(&out_dir),
        pixel_format: "rgba8".to_owned(),
        width: size,
        height: size,
        image_count: images.len(),
        native_load_performed: false,
        render_performed: false,
        aex_loaded: false,
        worker_started: false,
        broker_invoked: false,
        ofx_route_invoked: false,
        ae_invoked: false,
        private_payload_copied: false,
        images,
        checks: vec![
            passed_check(
                "output_root_confined",
                "all generated files are under target/aex-probe-fixtures",
            ),
            passed_check(
                "synthetic_rgba8_only",
                "all PNG pixels are generated RGBA8 data",
            ),
            passed_check("no_aex_input", "no AEX path is read or required"),
            passed_check(
                "no_worker_broker_or_host_invocation",
                "no worker, broker, OFX, or AE process is started",
            ),
            passed_check(
                "no_private_payload_copy",
                "no private project/media payload is copied",
            ),
            passed_check(
                "create_new_outputs",
                "PNG and manifest outputs are written with create-new semantics",
            ),
            passed_check(
                "all_outputs_preflighted",
                "all PNG and manifest outputs are checked before any PNG is written",
            ),
            passed_check(
                "rollback_on_late_write_failure",
                "newly created PNG outputs are removed if a later PNG or manifest write fails",
            ),
            passed_check(
                "no_symlink_or_reparse_output_ancestors",
                "existing output ancestors are rejected when they are symlinks or reparse points",
            ),
        ],
        notes: vec![
            "Fixture images are synthetic developer inputs only.".to_owned(),
            "This manifest is not loader approval and not pixel correctness evidence.".to_owned(),
            "No AEX file is opened, copied, loaded, described, or rendered.".to_owned(),
        ],
    };

    if let Err(err) = write_manifest_create_new(&manifest_path, &manifest) {
        cleanup_created_outputs(&created_paths);
        return Err(err);
    }
    Ok(manifest)
}

fn ensure_create_new_candidate(path: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => bail!("generated output already exists: {}", path.display()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => {
            bail!(
                "generated output metadata should be readable for {}: {err}",
                path.display()
            )
        }
    }
}

fn cleanup_created_outputs(paths: &[PathBuf]) {
    for path in paths.iter().rev() {
        let _ = std::fs::remove_file(path);
    }
}

fn write_manifest_create_new(path: &Path, manifest: &FixtureManifest) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create manifest dir {}", parent.display()))?;
    }
    let manifest_text =
        serde_json::to_string_pretty(manifest).context("manifest should serialize")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    if let Err(err) = file.write_all(manifest_text.as_bytes()) {
        let _ = std::fs::remove_file(path);
        bail!("failed to write {}: {err}", path.display());
    }
    Ok(())
}

fn write_png_create_new(path: &Path, image: &RgbaImage) -> anyhow::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    let encoder = image::codecs::png::PngEncoder::new(file);
    if let Err(err) = encoder.write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ColorType::Rgba8.into(),
    ) {
        let _ = std::fs::remove_file(path);
        bail!("failed to write {}: {err}", path.display());
    }
    Ok(())
}

fn gradient_image(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        let red = scaled_byte(x, width);
        let green = scaled_byte(y, height);
        let blue = scaled_byte(x + y, width.saturating_add(height).saturating_sub(1));
        Rgba([red, green, blue, 255])
    })
}

fn checker_image(width: u32, height: u32) -> RgbaImage {
    let cell = width.min(height).saturating_div(8).max(1);
    RgbaImage::from_fn(width, height, |x, y| {
        let bright = ((x / cell) + (y / cell)) % 2 == 0;
        if bright {
            Rgba([232, 232, 232, 255])
        } else {
            Rgba([32, 32, 32, 255])
        }
    })
}

fn solid_alpha_image(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |_x, _y| Rgba([96, 168, 255, 128]))
}

fn scaled_byte(value: u32, max_value: u32) -> u8 {
    if max_value <= 1 {
        return 0;
    }
    ((value.min(max_value - 1) * 255) / (max_value - 1)) as u8
}

fn passed_check(name: &str, evidence: &str) -> FixtureCheck {
    FixtureCheck {
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
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Ok(false);
    }
    let text = normalize_path_text(&path.to_string_lossy());
    let root = generated_fixture_root();
    let root_text = normalize_path_text(&root.to_string_lossy());
    Ok(
        (text == root_text || text.starts_with(&format!("{root_text}/")))
            && existing_path_and_ancestors_are_plain(path)?,
    )
}

fn generated_fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-probe-fixtures")
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
                "generated fixture path metadata should be readable for {}: {err}",
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

fn manifest_display_path(path: &Path) -> String {
    let text = normalize_path_text(&path.to_string_lossy());
    if let Some(index) = text.find(OUTPUT_ROOT_MARKER.trim_start_matches('/')) {
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
