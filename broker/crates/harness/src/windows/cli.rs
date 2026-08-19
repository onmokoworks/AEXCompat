fn load_preview(
    ctx: &egui::Context,
    texture_name: &str,
    path: &Path,
) -> Result<egui::TextureHandle, String> {
    let image = decode_preview_image(path)?;
    Ok(ctx.load_texture(texture_name, image, egui::TextureOptions::LINEAR))
}

fn decode_preview_image(path: &Path) -> Result<egui::ColorImage, String> {
    let image = image::open(path).map_err(|error| format!("image decode failed: {error}"))?;
    let rgba = image.into_rgba8();
    if rgba.width() == 0 || rgba.height() == 0 {
        return Err("decoded image has zero width or height".into());
    }
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        rgba.as_raw(),
    ))
}

fn is_supported_input_image(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff" | "webp"
                )
            })
}

fn canonical_deverbatim(path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("path could not be resolved: {error}"))?;
    let text = canonical.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        Ok(PathBuf::from(format!(r"\\{rest}")))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        Ok(PathBuf::from(rest))
    } else {
        Ok(canonical)
    }
}

fn show_preview(ui: &mut egui::Ui, label: &str, texture: Option<&egui::TextureHandle>) {
    ui.label(RichText::new(label).strong());
    let Some(texture) = texture else {
        ui.label("Not available");
        return;
    };
    let available = ui.available_width().max(1.0);
    let scale = (available / texture.size()[0] as f32).min(1.0);
    ui.image((
        texture.id(),
        egui::vec2(
            texture.size()[0] as f32 * scale,
            texture.size()[1] as f32 * scale,
        ),
    ));
}

fn show_viewer_texture(
    ui: &mut egui::Ui,
    label: &str,
    texture: Option<&egui::TextureHandle>,
    zoom: &mut f32,
    pan: &mut egui::Vec2,
) {
    let Some(texture) = texture else {
        ui.centered_and_justified(|ui| {
            ui.label(format!("{label} is not available"));
        });
        return;
    };
    let source = egui::vec2(texture.size()[0] as f32, texture.size()[1] as f32);
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong());
        ui.monospace(format!("{} x {}", texture.size()[0], texture.size()[1]));
        ui.weak("Wheel to zoom / drag to pan");
    });
    let available = ui.available_size().max(egui::vec2(1.0, 1.0));
    let (viewport, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());
    if response.hovered() {
        let scroll = ui.input(|input| input.raw_scroll_delta.y);
        if scroll != 0.0 {
            let previous_zoom = *zoom;
            *zoom = (*zoom * (scroll * 0.0025).exp()).clamp(0.25, 8.0);
            if let Some(pointer) = response.hover_pos() {
                let pointer_from_center = pointer - viewport.center();
                *pan = pointer_from_center - (pointer_from_center - *pan) * (*zoom / previous_zoom);
            }
        }
    }
    if response.dragged_by(egui::PointerButton::Primary)
        || response.dragged_by(egui::PointerButton::Middle)
    {
        *pan += response.drag_delta();
    }
    let fit_scale = (available.x / source.x).min(available.y / source.y);
    let display = source * fit_scale * *zoom;
    let image_rect = egui::Rect::from_center_size(viewport.center() + *pan, display);
    ui.painter().with_clip_rect(viewport).image(
        texture.id(),
        image_rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn inspect_dependency_roots(
    plugin: &Path,
    requested_roots: &[std::ffi::OsString],
) -> Result<Vec<PathBuf>, String> {
    const MAX_ROOTS: usize = aexcompat_broker::plugin_dependency_closure::MAX_SEARCH_ROOTS;
    let plugin = canonical_deverbatim(plugin)
        .map_err(|error| format!("selected AEX could not be resolved: {error}"))?;
    let mut roots = Vec::with_capacity(requested_roots.len() + 1);
    if let Some(parent) = plugin.parent() {
        roots.push(parent.to_path_buf());
    }
    for requested in requested_roots {
        let root = canonical_deverbatim(&PathBuf::from(requested))
            .map_err(|error| format!("dependency root could not be resolved: {error}"))?;
        if !root.is_dir() {
            return Err(format!(
                "dependency root is not a directory: {}",
                root.display()
            ));
        }
        if !roots.iter().any(|existing| existing == &root) {
            roots.push(root);
        }
    }
    if roots.len() > MAX_ROOTS {
        return Err(format!(
            "at most {MAX_ROOTS} total dependency search roots may be supplied"
        ));
    }
    Ok(roots)
}

fn inspect_experimental_with_dependency_roots(
    repository: &Path,
    plugin: &Path,
    approved_sha256: &str,
    requested_roots: &[std::ffi::OsString],
) -> Result<serde_json::Value, String> {
    let roots = inspect_dependency_roots(plugin, requested_roots)?;
    let closure = aexcompat_broker::plugin_dependency_closure::resolve_dependency_closure(
        aexcompat_broker::plugin_dependency_closure::DependencyClosureRequest::new(plugin, &roots),
    )
    .map_err(|error| format!("dependency closure resolution failed: {error}"))?;
    let dependencies = closure.dependencies().to_vec();
    let (parameters, diagnostics) =
        aexcompat_broker::image_render::inspect_experimental_with_approved_dependencies_and_diagnostics(
            repository,
            plugin,
            approved_sha256,
            dependencies.clone(),
        )
        .map_err(|error| format!("parameter inspection failed: {error}"))?;
    let plugin_size = fs::metadata(plugin)
        .map_err(|error| format!("selected AEX metadata failed: {error}"))?
        .len();
    let dependency_report = dependencies
        .iter()
        .map(|dependency| {
            let basename = dependency
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<non-unicode>");
            let sha256 = dependency
                .expected_sha256
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>();
            serde_json::json!({
                "basename": basename,
                "size_bytes": dependency.expected_size,
                "sha256": sha256,
            })
        })
        .collect::<Vec<_>>();
    let provenance = closure
        .provenance()
        .iter()
        .map(|item| {
            serde_json::json!({
                "basename": item.basename,
                "import_derived": item.import_derived,
                "string_derived": item.string_derived,
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "stage": "parameter_inspection",
        "plugin_identity": {
            "sha256": approved_sha256,
            "size_bytes": plugin_size,
        },
        "parameters": parameters,
        "worker_diagnostics": diagnostics,
        "dependency_closure": {
            "search_roots": roots,
            "dependencies": dependency_report,
            "dependency_count": dependencies.len(),
            "total_bytes": closure.total_bytes(),
            "unresolved_import_count": closure.unresolved().len(),
            "unresolved_imports": closure.unresolved(),
            "rejected_import_name_count": closure.rejected_names(),
            "provenance": provenance,
        },
    }))
}

fn repository_root(args: &[std::ffi::OsString]) -> PathBuf {
    let typed_conformance_request = args.get(1).is_some_and(|command| {
        matches!(
            command.to_string_lossy().as_ref(),
            "--render-experimental-request"
                | "--render-experimental-smart-request"
                | "--render-experimental-request-16"
                | "--render-experimental-smart-request-16"
                | "--render-experimental-request-32"
                | "--render-experimental-smart-request-32-cpu"
        )
    });
    if typed_conformance_request && let Some(path) = std::env::var_os("AEXCOMPAT_REPOSITORY_ROOT") {
        let path = PathBuf::from(path);
        if path.is_absolute() && path.is_dir() {
            return path;
        }
    }
    repository_root_from_runtime_paths(
        std::env::current_dir().ok(),
        std::env::current_exe().ok(),
    )
    .unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap()
    })
}

fn repository_root_from_runtime_paths(
    current_dir: Option<PathBuf>,
    current_exe: Option<PathBuf>,
) -> Option<PathBuf> {
    let starts = current_dir
        .into_iter()
        .chain(current_exe.and_then(|path| path.parent().map(Path::to_path_buf)));
    repository_root_from_starts(starts)
}

fn repository_root_from_starts(starts: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    for start in starts {
        for ancestor in start.ancestors() {
            if ancestor.join("guest/Cargo.toml").is_file()
                && ancestor.join("broker/Cargo.toml").is_file()
            {
                return Some(ancestor.to_path_buf());
            }
        }
    }
    None
}

fn cli_contract() -> serde_json::Value {
    serde_json::json!({
        "schema": CLI_CONTRACT_SCHEMA,
        "version": CLI_CONTRACT_VERSION,
        "program": "aexcompat-harness",
        "transport": {
            "success_stdout": "json",
            "failure_stderr": true,
            "failure_exit_code": "nonzero",
            "unknown_or_malformed_arguments": "launch_gui"
        },
        "headless_mode": {
            "prefix": "--headless",
            "unknown_or_malformed_arguments": "structured_stderr_exit_64",
            "gui_launch": false
        },
        "commands": [
            {
                "name": "--compare-images",
                "argv": ["--compare-images", "<reference-image>", "<output-image>"],
                "result": "pixel-comparison-json",
                "exit_code": "0=exact,2=non-exact"
            },
            {
                "name": "--inspect-experimental",
                "argv": ["--inspect-experimental", "<aex>"],
                "result": "parameter-inspection-json"
            },
            {
                "name": "--render-scattermap-fixture",
                "argv": ["--render-scattermap-fixture", "<input-image>", "<output-image>"],
                "result": "session-render-report-json",
                "note": "Approved ScatterMap fixture only; this is not a generic AEX path."
            },
            {
                "name": "--inspect-experimental-with-deps",
                "argv": ["--inspect-experimental-with-deps", "<aex>", "<dependency-root>..."],
                "result": "parameter-inspection-and-dependency-closure-json"
            },
            {
                "name": "--inspect-experimental-dependencies",
                "argv": ["--inspect-experimental-dependencies", "<aex>", "all|missing"],
                "result": "dependency-diagnostic-json"
            },
            {
                "name": "--render-experimental-request",
                "argv": ["--render-experimental-request|--render-experimental-smart-request|--render-experimental-request-16|--render-experimental-smart-request-16|--render-experimental-request-32|--render-experimental-smart-request-32-cpu", "<aex>", "<input-image>", "<output-image>", "<debug-request.json>"],
                "result": "render-report-json"
            },
            {
                "name": "--render-experimental-session",
                "argv": ["--render-experimental-session|--render-experimental-session-param", "<aex>", "<input-image>", "<output-image>", "argb8|argb16|argb32f", "classic|smart", "<current-time>", "<total-time>", "<time-scale>", "<slot>", "<value>"],
                "result": "session-render-report-json",
                "aliases": ["--render-experimental-session", "--render-experimental-session-param"],
                "note": "The final slot/value pair is required only for --render-experimental-session-param."
            },
            {
                "name": "--render-raw",
                "argv": ["--render-raw", "<aex>", "<input-image>", "<output-directory>", "argb8|argb16|argb32f", "classic|smart", "<current-time>", "<total-time>", "<time-scale>"],
                "result": "native-argb-raw-artifact-report-json"
            },
            {
                "name": "--render-exr",
                "argv": ["--render-exr", "<aex>", "<input-image>", "<output-directory>", "argb32f", "classic|smart", "<current-time>", "<total-time>", "<time-scale>"],
                "result": "uncompressed-scanline-float32-exr-artifact-report-json"
            },
            {
                "name": "--render-fixture",
                "argv": ["--render-fixture", "<aex>", "<fixture.json>", "<output-directory>"],
                "result": "declarative-render-fixture-report-json",
                "note": "Fixture asset paths are relative to fixture.json; plug-in path and hash are never embedded."
            },
            {
                "name": "--render-experimental",
                "argv": ["--render-experimental|--render-experimental-auto|--render-experimental-16|--render-experimental-16-deep|--render-experimental-32|--render-experimental-smart|--render-experimental-smart-16|--render-experimental-smart-16-deep|--render-experimental-smart-32|--render-experimental-smart-32-cpu", "<aex>", "<input-image>", "<output-image>"],
                "result": "render-report-json"
            },
            {
                "name": "experimental-probes",
                "aliases": [
                    "--probe-experimental-options-dialog",
                    "--probe-experimental-automatic-options-dialog",
                    "--probe-experimental-nop-render",
                    "--probe-experimental-smart-nop-render",
                    "--probe-experimental-input-buffer-write",
                    "--probe-experimental-smart-input-buffer-write",
                    "--probe-experimental-expand-buffer",
                    "--probe-experimental-shrink-buffer",
                    "--probe-experimental-persistent-sequence",
                    "--probe-experimental-flattened-sequence",
                    "--probe-experimental-copied-flattened-sequence"
                ],
                "argv": ["<probe-command>", "<aex>"],
                "result": "probe-report-json",
                "note": "Every listed probe uses the same one-AEX argument shape."
            },
            {
                "name": "experimental-aegp",
                "aliases": [
                    "--initialize-experimental-aegp",
                    "--dispatch-experimental-aegp-update-menu",
                    "--dispatch-experimental-aegp-idle",
                    "--dispatch-experimental-aegp-command-roundtrip",
                    "--dispatch-experimental-aegp-active-idle-roundtrip",
                    "--dispatch-experimental-aegp-comp-idle-roundtrip",
                    "--dispatch-experimental-aegp-keyframe-roundtrip",
                    "--dispatch-experimental-aegp-seek-roundtrip",
                    "--dispatch-experimental-aegp-trim-roundtrip",
                    "--dispatch-experimental-aegp-switch-roundtrip"
                ],
                "argv": ["<aegp-command>", "<aex>"],
                "result": "aegp-report-json"
            }
        ],
        "safety": {
            "native_aex_execution": true,
            "not_a_security_sandbox": true,
            "paths_are_caller_supplied": true,
            "use_conformance_bundle_for_hashed_reproducible_artifacts": true
        }
    })
}

#[cfg(test)]
mod artifact_cli_contract_tests {
    use super::cli_contract;

    #[test]
    fn raw_and_exr_commands_publish_exact_argument_and_result_contracts() {
        let contract = cli_contract();
        let commands = contract["commands"].as_array().unwrap();
        let command = |name: &str| {
            commands
                .iter()
                .find(|entry| entry["name"] == name)
                .unwrap_or_else(|| panic!("missing {name} command"))
        };
        assert_eq!(command("--render-raw")["argv"].as_array().unwrap().len(), 9);
        assert_eq!(
            command("--render-raw")["result"],
            "native-argb-raw-artifact-report-json"
        );
        assert_eq!(command("--render-exr")["argv"][4], "argb32f");
        assert_eq!(
            command("--render-exr")["result"],
            "uncompressed-scanline-float32-exr-artifact-report-json"
        );
        assert_eq!(
            command("--render-fixture")["argv"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }
}

fn read_plugin_hash(plugin: &Path) -> Result<String, std::io::Error> {
    let bytes = fs::read(plugin)?;
    Ok(format!("{:X}", Sha256::digest(bytes)))
}

fn plugin_read_diagnostic(plugin: &Path, error: &std::io::Error) -> serde_json::Value {
    let _ = plugin;
    serde_json::json!({
        "schema": DIAGNOSTIC_SCHEMA,
        "version": DIAGNOSTIC_VERSION,
        "success": false,
        "classification": "input_error",
        "failure_stage": "input_validation",
        "operation": "read_plugin",
        "path_kind": "aex",
        "error_kind": format!("{:?}", error.kind()),
        "os_code": error.raw_os_error(),
        "message": error.to_string(),
    })
}

fn required_plugin_hash(plugin: &Path) -> String {
    match read_plugin_hash(plugin) {
        Ok(hash) => hash,
        Err(error) => {
            eprintln!("{}", plugin_read_diagnostic(plugin, &error));
            std::process::exit(1);
        }
    }
}

fn cli_inspection_failure_document(message: &str) -> serde_json::Value {
    typed_failure_document(message).unwrap_or_else(|| {
        serde_json::json!({
            "classification": "inspection_error",
            "failure_stage": "parameter_inspection",
            "message": bounded_summary(message),
        })
    })
}

fn required_inspected_plugin_parameters(
    repository: &Path,
    plugin: &Path,
    hash: &str,
) -> Vec<aexcompat_broker::image_render::InteractiveParameter> {
    let inspected = (|| {
        let plugin = canonical_deverbatim(plugin).map_err(std::io::Error::other)?;
        let expected_size = fs::metadata(&plugin)?.len();
        let expected_sha256 = decode_sha256(hash).map_err(std::io::Error::other)?;
        let dependency_search_dirs = plugin.parent().map(Path::to_path_buf).into_iter().collect();
        aexcompat_broker::image_render::inspect_experimental_via_discovery_in_place(
            repository,
            aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact {
                path: plugin,
                expected_sha256,
                expected_size,
            },
            dependency_search_dirs,
        )
    })();
    match inspected {
        Ok((parameters, _)) => parameters,
        Err(error) => {
            eprintln!("{}", cli_inspection_failure_document(&error.to_string()));
            std::process::exit(1);
        }
    }
}

fn required_plugin_parameters(
    repository: &Path,
    plugin: &Path,
    hash: &str,
) -> Vec<aexcompat_broker::image_render::InteractiveParameter> {
    aexcompat_broker::image_render::normalize_default_interactive_parameters(
        &required_inspected_plugin_parameters(repository, plugin, hash),
    )
}

fn print_cli_help() {
    println!(
        "aexcompat-harness\n\nUse --print-cli-contract for machine-readable command metadata. Prefix any command with --headless for agent/CI use.\n\nCommon commands:\n  --inspect-experimental <aex>\n  --inspect-experimental-with-deps <aex> <dependency-root>...\n  --inspect-experimental-dependencies <aex> <all|missing>\n  --render-scattermap-fixture <input-image> <output-image>\n  --render-experimental-request <aex> <input> <output> <debug-request.json>\n  --render-experimental-session <aex> <input> <output> <pixel-format> <classic|smart> <current-time> <total-time> <time-scale>\n\nSuccessful commands write JSON to stdout. Failures write diagnostics to stderr and return a nonzero exit code. Unknown or malformed arguments open the GUI by default; --headless reports a structured CLI failure and exits 64."
    );
}

fn main() -> eframe::Result {
    aexcompat_broker::observability::init();
    let mut args: Vec<_> = std::env::args_os().collect();
    let headless = args.get(1).is_some_and(|arg| arg == "--headless");
    if headless {
        args.remove(1);
    }
    if args.len() == 2 && (args[1] == "--help" || args[1] == "-h") {
        print_cli_help();
        return Ok(());
    }
    if args.len() == 2 && args[1] == "--print-cli-contract" {
        println!("{}", serde_json::to_string_pretty(&cli_contract()).unwrap());
        return Ok(());
    }
    let repository = repository_root(&args);
    if args.len() == 5 && args[1] == "--render-fixture" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::render_declarative_fixture(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 4 && args[1] == "--compare-images" {
        match compare_images(Path::new(&args[2]), Path::new(&args[3])) {
            Ok(comparison) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&comparison.report()).unwrap()
                );
                if !comparison.exact() {
                    std::process::exit(2);
                }
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 5 && args[1] == "--render-experimental-matrix" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let parameters = required_plugin_parameters(&repository, plugin, &hash);
        let report = run_effect_matrix(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            None,
            None,
        );
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return Ok(());
    }
    if args.len() == 6 && args[1] == "--render-experimental-reference-matrix" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let parameters = required_plugin_parameters(&repository, plugin, &hash);
        let report = run_effect_matrix(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[5]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            Some(Path::new(&args[4])),
            None,
        );
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        if report["failed_count"].as_u64().unwrap_or(6) != 0 {
            std::process::exit(2);
        }
        return Ok(());
    }
    if args.len() == 9 && args[1] == "--render-experimental-session-animation" {
        use aexcompat_broker::image_render::{ParameterAnimation, RenderTiming};
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let current_time = args[5].to_string_lossy().parse::<i32>().unwrap_or(-1);
        let total_time = args[6].to_string_lossy().parse::<i32>().unwrap_or(0);
        let time_scale = args[7].to_string_lossy().parse::<u32>().unwrap_or(0);
        let timing = RenderTiming {
            current_time,
            time_step: 1,
            total_time,
            time_scale,
        };
        let sidecar = match fs::read(&args[8])
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<serde_json::Value>(&bytes)
                    .map_err(|error| error.to_string())
            }) {
            Ok(value) => value,
            Err(error) => {
                eprintln!("parameter animation sidecar is invalid: {error}");
                std::process::exit(1);
            }
        };
        let Some(object) = sidecar.as_object() else {
            eprintln!("parameter animation sidecar must be a JSON object");
            std::process::exit(1);
        };
        if object.len() != 2
            || !object.contains_key("schema_version")
            || !object.contains_key("parameters")
            || object
                .get("schema_version")
                .and_then(|value| value.as_u64())
                != Some(1)
        {
            eprintln!("parameter animation sidecar requires schema_version=1 and parameters");
            std::process::exit(1);
        }
        let animations: Vec<ParameterAnimation> =
            match serde_json::from_value(object.get("parameters").cloned().unwrap_or_default()) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("parameter animation sidecar parameters are invalid: {error}");
                    std::process::exit(1);
                }
            };
        let parameters = match aexcompat_broker::image_render::inspect_experimental_with_diagnostics(
            &repository,
            plugin,
            &hash,
        ) {
            Ok((parameters, _diagnostics)) => parameters,
            Err(error) => {
                eprintln!("parameter animation inspection failed: {error}");
                std::process::exit(1);
            }
        };
        let report =
            aexcompat_broker::image_render::render_experimental_image_with_parameter_animation(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                &animations,
                timing,
            );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 4 && args[1] == "--render-scattermap-fixture" {
        let report = aexcompat_broker::image_render::render_scattermap_fixture(
            &repository,
            "scattermap",
            Path::new(&args[2]),
            Path::new(&args[3]),
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 7 && args[1] == "--render-experimental-smart-32-gpu-policy" {
        // GPU single-image render routed through the length-one session (#290):
        // parse the runtime-module policy JSON, run the GPU module-audit preflight
        // to assemble the authenticated policy input, then render an Argb32f smart
        // frame on the requested GPU backend. The session is the only transport
        // (#365).
        use aexcompat_broker::image_render::{RenderGpuBackend, RenderPixelFormat, RenderTiming};
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let gpu_backend = match args[5].to_string_lossy().as_ref() {
            "auto" => RenderGpuBackend::Auto,
            "cuda" => RenderGpuBackend::Cuda,
            "opencl" => RenderGpuBackend::OpenCl,
            "directx" => RenderGpuBackend::DirectX,
            _ => {
                eprintln!("gpu backend must be auto, cuda, opencl, or directx");
                std::process::exit(1);
            }
        };
        let policy = match fs::read(&args[6])
            .and_then(|bytes| aexcompat_broker::runtime_module_policy::parse_and_validate(&bytes))
        {
            Ok(policy) => policy,
            Err(error) => {
                eprintln!("runtime module policy rejected: {error}");
                std::process::exit(1);
            }
        };
        // The preflight seals the same approved dependency artifacts the render
        // dispatches with, so a plug-in that imports one loads in both.
        let dependency_roots = std::env::var_os("AEXCOMPAT_MULTIFILTER_DEPENDENCY_DIRS")
            .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
            .unwrap_or_default();
        let render_dependencies = if dependency_roots.is_empty() {
            Vec::new()
        } else {
            match aexcompat_broker::plugin_dependency_closure::resolve_dependency_closure(
                aexcompat_broker::plugin_dependency_closure::DependencyClosureRequest::new(
                    plugin,
                    &dependency_roots,
                ),
            ) {
                Ok(closure) => closure.dependencies().to_vec(),
                Err(error) => {
                    eprintln!("GPU dependency closure resolution failed: {error}");
                    std::process::exit(1);
                }
            }
        };
        let parameters = if dependency_roots.is_empty() {
            required_plugin_parameters(&repository, plugin, &hash)
        } else {
            match aexcompat_broker::image_render::inspect_experimental_in_place(
                &repository,
                plugin,
                &hash,
                dependency_roots.clone(),
            ) {
                Ok((parameters, _)) => parameters,
                Err(error) => {
                    eprintln!("GPU parameter inspection failed: {error}");
                    std::process::exit(1);
                }
            }
        };
        let parameters =
            aexcompat_broker::image_render::normalize_default_interactive_parameters(&parameters);
        let prepared = match aexcompat_broker::image_render::prepare_gpu_runtime_policy(
            &repository,
            plugin,
            &hash,
            gpu_backend,
            policy,
            render_dependencies.clone(),
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!("GPU runtime policy preparation failed: {error}");
                std::process::exit(1);
            }
        };
        let report = aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies_and_gpu_runtime_policy(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            RenderTiming::default(),
            true,
            RenderPixelFormat::Argb32f,
            None,
            None,
            gpu_backend,
            render_dependencies,
            Some(prepared.as_input()),
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 5 && args[1] == "--inspect-gpu-module-report" {
        // Runs the GPU module-audit preflight in isolation (#290) and prints the
        // classified module report the render path re-authenticates. Mirrors
        // --inspect-experimental-runtime-policy but for the render (not params)
        // producer; used by the A/B gate to verify the preflight independently.
        use aexcompat_broker::image_render::RenderGpuBackend;
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let gpu_backend = match args[3].to_string_lossy().as_ref() {
            "auto" => RenderGpuBackend::Auto,
            "cuda" => RenderGpuBackend::Cuda,
            "opencl" => RenderGpuBackend::OpenCl,
            "directx" => RenderGpuBackend::DirectX,
            _ => {
                eprintln!("gpu backend must be auto, cuda, opencl, or directx");
                std::process::exit(1);
            }
        };
        let policy = match fs::read(&args[4])
            .and_then(|bytes| aexcompat_broker::runtime_module_policy::parse_and_validate(&bytes))
        {
            Ok(policy) => policy,
            Err(error) => {
                eprintln!("runtime module policy rejected: {error}");
                std::process::exit(1);
            }
        };
        match aexcompat_broker::image_render::prepare_gpu_runtime_policy(
            &repository,
            plugin,
            &hash,
            gpu_backend,
            policy,
            Vec::new(),
        ) {
            Ok(prepared) => println!("{}", prepared.report_json()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    let session_command = args.get(1).and_then(|value| value.to_str());
    let session_with_parameter = session_command == Some("--render-experimental-session-param");
    let artifact_command = matches!(session_command, Some("--render-raw" | "--render-exr"));
    if (args.len() == 10
        && (session_command == Some("--render-experimental-session") || artifact_command))
        || (args.len() == 12 && session_with_parameter)
    {
        // Built-artifact probes use this explicit session-only adapter.  The
        // worker no longer accepts the deleted one-shot image argv (#365), so
        // native probe coverage must enter through the same length-one session
        // wrapper as production experimental renders.
        use aexcompat_broker::image_render::{RenderPixelFormat, RenderTiming};
        let pixel_format = match args[5].to_string_lossy().as_ref() {
            "argb8" => RenderPixelFormat::Argb8,
            "argb16" => RenderPixelFormat::Argb16,
            "argb32f" => RenderPixelFormat::Argb32f,
            _ => {
                eprintln!("pixel format must be argb8, argb16, or argb32f");
                std::process::exit(1);
            }
        };
        let smart = match args[6].to_string_lossy().as_ref() {
            "classic" => false,
            "smart" => true,
            _ => {
                eprintln!("render kind must be classic or smart");
                std::process::exit(1);
            }
        };
        let current_time = args[7].to_string_lossy().parse::<i32>().unwrap_or(-1);
        let total_time = args[8].to_string_lossy().parse::<i32>().unwrap_or(0);
        let time_scale = args[9].to_string_lossy().parse::<u32>().unwrap_or(0);
        let timing = RenderTiming {
            current_time,
            time_step: 1,
            total_time,
            time_scale,
        };
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let mut parameters = if session_with_parameter {
            match aexcompat_broker::image_render::inspect_experimental_with_diagnostics(
                &repository,
                plugin,
                &hash,
            ) {
                Ok((parameters, _diagnostics)) => parameters,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        } else {
            required_plugin_parameters(&repository, plugin, &hash)
        };
        if session_with_parameter {
            let slot = match args[10].to_string_lossy().parse::<u32>() {
                Ok(slot) if slot > 0 => slot,
                _ => {
                    eprintln!("parameter slot must be a positive integer");
                    std::process::exit(1);
                }
            };
            let value = match args[11].to_string_lossy().parse::<f64>() {
                Ok(value) if value.is_finite() => value,
                _ => {
                    eprintln!("parameter value must be finite");
                    std::process::exit(1);
                }
            };
            let Some(parameter) = parameters.iter_mut().find(|item| item.slot == slot) else {
                eprintln!("parameter slot {slot} was not discovered");
                std::process::exit(1);
            };
            if value < parameter.minimum || value > parameter.maximum {
                eprintln!(
                    "parameter value {value} is outside slot {slot} range {}..{}",
                    parameter.minimum, parameter.maximum
                );
                std::process::exit(1);
            }
            parameter.value = value;
        }
        let report = if artifact_command {
            use aexcompat_broker::image_render::RenderArtifactKind;
            let kind = if session_command == Some("--render-exr") {
                RenderArtifactKind::Float32Exr
            } else {
                RenderArtifactKind::Raw
            };
            aexcompat_broker::image_render::render_experimental_artifact_at_time(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                timing,
                smart,
                pixel_format,
                kind,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image_at_time_with_format(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                timing,
                smart,
                pixel_format,
            )
        };
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 5
        && matches!(
            args[1].to_string_lossy().as_ref(),
            "--render-experimental"
                | "--render-experimental-auto"
                | "--render-experimental-16"
                | "--render-experimental-16-deep"
                | "--render-experimental-32"
                | "--render-experimental-smart"
                | "--render-experimental-smart-16"
                | "--render-experimental-smart-16-deep"
                | "--render-experimental-smart-32"
                | "--render-experimental-smart-32-cpu"
        )
    {
        use aexcompat_broker::image_render::RenderPixelFormat;
        let command = args[1].to_string_lossy();
        let auto_path = command == "--render-experimental-auto";
        let deep16_png = command.ends_with("-16-deep");
        let pixel_format = if command.ends_with("-16") || deep16_png {
            RenderPixelFormat::Argb16
        } else if command.ends_with("-32") || command.ends_with("-32-cpu") {
            RenderPixelFormat::Argb32f
        } else {
            RenderPixelFormat::Argb8
        };
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let dependency_roots = std::env::var_os("AEXCOMPAT_MULTIFILTER_DEPENDENCY_DIRS")
            .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
            .unwrap_or_default();
        let approved_dependencies = if dependency_roots.is_empty() {
            match approved_adjacent_dependencies(plugin, &hash) {
                Ok(dependencies) => dependencies,
                Err(error) if auto_path => {
                    eprintln!("automatic dependency approval failed: {error}");
                    std::process::exit(1);
                }
                Err(_) => Vec::new(),
            }
        } else {
            match aexcompat_broker::plugin_dependency_closure::resolve_dependency_closure(
                aexcompat_broker::plugin_dependency_closure::DependencyClosureRequest::new(
                    plugin,
                    &dependency_roots,
                ),
            ) {
                Ok(closure) => closure.dependencies().to_vec(),
                Err(error) => {
                    eprintln!("dependency closure resolution failed: {error}");
                    std::process::exit(1);
                }
            }
        };
        let use_approved_dependencies = auto_path || !approved_dependencies.is_empty();
        let (parameters, inspection) = if !dependency_roots.is_empty() {
            match aexcompat_broker::image_render::inspect_experimental_in_place(
                &repository,
                plugin,
                &hash,
                dependency_roots.clone(),
            ) {
                Ok(inspected) => inspected,
                Err(error) => {
                    eprintln!(
                        "automatic render-path selection needs a successful parameter \
                         inspection: {error}"
                    );
                    std::process::exit(1);
                }
            }
        } else if use_approved_dependencies {
            match aexcompat_broker::image_render::inspect_experimental_with_approved_dependencies_and_diagnostics(
                &repository,
                plugin,
                &hash,
                approved_dependencies.clone(),
            ) {
                Ok(inspected) => inspected,
                Err(error) => {
                    eprintln!(
                        "automatic render-path selection needs a successful parameter \
                         inspection: {error}"
                    );
                    std::process::exit(1);
                }
            }
        } else {
            match aexcompat_broker::image_render::inspect_experimental_with_diagnostics(
                &repository,
                plugin,
                &hash,
            ) {
                Ok(inspected) => inspected,
                Err(_) => Default::default(),
            }
        };
        let parameters =
            aexcompat_broker::image_render::normalize_default_interactive_parameters(&parameters);
        let smart_advertised = inspection["smart_render_advertised"]
            .as_bool()
            .unwrap_or(false);
        let smart = if auto_path {
            smart_advertised
        } else {
            command.contains("smart")
        };
        let report = if deep16_png {
            if use_approved_dependencies {
                aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies_and_deep16_png(
                    &repository,
                    plugin,
                    &hash,
                    Path::new(&args[3]),
                    Path::new(&args[4]),
                    &parameters,
                    aexcompat_broker::image_render::RenderTiming::default(),
                    smart,
                    approved_dependencies,
                )
            } else {
                aexcompat_broker::image_render::render_experimental_image_at_time_with_deep16_png(
                    &repository,
                    plugin,
                    &hash,
                    Path::new(&args[3]),
                    Path::new(&args[4]),
                    &parameters,
                    aexcompat_broker::image_render::RenderTiming::default(),
                    smart,
                )
            }
        } else if command.ends_with("-32-cpu") {
            if use_approved_dependencies {
                aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies(
                    &repository,
                    plugin,
                    &hash,
                    Path::new(&args[3]),
                    Path::new(&args[4]),
                    &parameters,
                    aexcompat_broker::image_render::RenderTiming::default(),
                    smart,
                    pixel_format,
                    None,
                    None,
                    aexcompat_broker::image_render::RenderGpuBackend::Cpu,
                    approved_dependencies,
                )
            } else {
                aexcompat_broker::image_render::render_experimental_image_at_time_with_format_context_ui_action_and_gpu_backend(
                    &repository,
                    plugin,
                    &hash,
                    Path::new(&args[3]),
                    Path::new(&args[4]),
                    &parameters,
                    aexcompat_broker::image_render::RenderTiming::default(),
                    smart,
                    pixel_format,
                    None,
                    None,
                    aexcompat_broker::image_render::RenderGpuBackend::Cpu,
                )
            }
        } else if use_approved_dependencies {
            aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                aexcompat_broker::image_render::RenderTiming::default(),
                smart,
                pixel_format,
                None,
                None,
                aexcompat_broker::image_render::RenderGpuBackend::Auto,
                approved_dependencies,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image_at_time_with_format(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                aexcompat_broker::image_render::RenderTiming::default(),
                smart,
                pixel_format,
            )
        };
        match report {
            Ok(mut value) => {
                if auto_path {
                    value["render_path_source"] = serde_json::json!("advertised_out_flags2");
                    value["smart_render_advertised"] = serde_json::json!(smart_advertised);
                }
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6
        && matches!(
            args[1].to_string_lossy().as_ref(),
            "--render-experimental-request"
                | "--render-experimental-smart-request"
                | "--render-experimental-request-16"
                | "--render-experimental-smart-request-16"
                | "--render-experimental-request-32"
                | "--render-experimental-smart-request-32-cpu"
        )
    {
        use aexcompat_broker::image_render::{RenderGpuBackend, RenderPixelFormat};
        let command = args[1].to_string_lossy();
        let smart = command.contains("-smart-");
        let pixel_format = if command.contains("-16") {
            RenderPixelFormat::Argb16
        } else if command.contains("-32") {
            RenderPixelFormat::Argb32f
        } else {
            RenderPixelFormat::Argb8
        };
        let request_path = Path::new(&args[5]);
        let request_bytes = fs::read(request_path).unwrap_or_else(|error| {
            eprintln!("assignment document could not be read: {error}");
            std::process::exit(1);
        });
        if request_bytes.len() > 64 * 1024 {
            eprintln!("assignment document exceeds 64 KiB");
            std::process::exit(1);
        }
        let document: serde_json::Value =
            serde_json::from_slice(&request_bytes).unwrap_or_else(|error| {
                eprintln!("assignment document is not valid JSON: {error}");
                std::process::exit(1);
            });
        let dependencies =
            typed_request_dependencies(&document, request_path).unwrap_or_else(|error| {
                eprintln!("{error}");
                std::process::exit(1);
            });
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let (mut parameters, inspection_diagnostics) =
            aexcompat_broker::image_render::inspect_experimental_with_approved_dependencies_and_diagnostics(
                &repository,
                plugin,
                &hash,
                dependencies.clone(),
            )
        .unwrap_or_else(|error| {
            emit_typed_failure(&error.to_string());
            std::process::exit(1);
        });
        let parameter_metadata = inspection_diagnostics["parameter_metadata"].clone();
        let timing = typed_request_timing(&document).unwrap_or_else(|error| {
            emit_host_request_validation_failure(&error, &parameter_metadata);
            std::process::exit(1);
        });
        let host_context = typed_request_host_context(&document).unwrap_or_else(|error| {
            emit_host_request_validation_failure(&error, &parameter_metadata);
            std::process::exit(1);
        });
        let render_settings = typed_request_render_settings(&document).unwrap_or_else(|error| {
            emit_host_request_validation_failure(&error, &parameter_metadata);
            std::process::exit(1);
        });
        let _render_settings_guard = ConformanceRenderSettingsGuard::install(
            render_settings.as_deref(),
        )
        .unwrap_or_else(|error| {
            emit_host_request_validation_failure(&error, &parameter_metadata);
            std::process::exit(1);
        });
        if let Err(error) = apply_typed_assignments(&mut parameters, &document, Some(request_path))
        {
            emit_host_request_validation_failure(&error, &parameter_metadata);
            std::process::exit(1);
        }
        let report =
            aexcompat_broker::image_render::render_experimental_image_with_approved_dependencies(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                timing,
                smart,
                pixel_format,
                host_context.as_ref(),
                None,
                if command.ends_with("-32-cpu") {
                    RenderGpuBackend::Cpu
                } else {
                    RenderGpuBackend::Auto
                },
                dependencies,
            );
        match report {
            Ok(mut value) => {
                value["parameter_metadata"] = parameter_metadata;
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
            }
            Err(error) => {
                emit_typed_failure_with_parameter_metadata(&error.to_string(), &parameter_metadata);
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6 && args[1] == "--render-experimental-audio-request" {
        let request_bytes = fs::read(Path::new(&args[5])).unwrap_or_else(|error| {
            eprintln!("assignment document could not be read: {error}");
            std::process::exit(1);
        });
        if request_bytes.len() > 64 * 1024 {
            eprintln!("assignment document exceeds 64 KiB");
            std::process::exit(1);
        }
        let document: serde_json::Value =
            serde_json::from_slice(&request_bytes).unwrap_or_else(|error| {
                eprintln!("assignment document is not valid JSON: {error}");
                std::process::exit(1);
            });
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let mut parameters = required_plugin_parameters(&repository, plugin, &hash);
        if let Err(error) =
            apply_typed_assignments(&mut parameters, &document, Some(Path::new(&args[5])))
        {
            eprintln!("{error}");
            std::process::exit(1);
        }
        match aexcompat_broker::image_render::render_experimental_audio(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6 && args[1] == "--render-experimental-image-audio-sidecar" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let parameters = required_plugin_parameters(&repository, plugin, &hash);
        match aexcompat_broker::image_render::render_experimental_image_with_audio_sidecar(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            Path::new(&args[5]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() >= 7
        && args.len() % 2 == 1
        && (args[1] == "--render-experimental-layer-slots"
            || args[1] == "--render-experimental-smart-layer-slots")
    {
        let smart = args[1] == "--render-experimental-smart-layer-slots";
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let mut parameters = required_plugin_parameters(&repository, plugin, &hash);
        if let Err(error) = assign_layer_paths(&mut parameters, &args[5..]) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        let report = aexcompat_broker::image_render::render_experimental_image_at_time_with_format(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            smart,
            aexcompat_broker::image_render::RenderPixelFormat::Argb8,
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() >= 7
        && args.len() % 2 == 1
        && (args[1] == "--render-experimental-param"
            || args[1] == "--render-experimental-smart-param")
    {
        let smart = args[1] == "--render-experimental-smart-param";
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let mut parameters = required_plugin_parameters(&repository, plugin, &hash);
        let mut assigned_slots = Vec::new();
        for assignment in args[5..].chunks_exact(2) {
            let slot = assignment[0].to_string_lossy().parse::<u32>().unwrap_or(0);
            let value = assignment[1]
                .to_string_lossy()
                .parse::<f64>()
                .unwrap_or(f64::NAN);
            if assigned_slots.contains(&slot) {
                eprintln!("parameter slot {slot} was assigned more than once");
                std::process::exit(1);
            }
            let Some(parameter) = parameters.iter_mut().find(|item| item.slot == slot) else {
                eprintln!("AEX exposes no parameter at slot {slot}");
                std::process::exit(1);
            };
            if !value.is_finite() || value < parameter.minimum || value > parameter.maximum {
                eprintln!(
                    "parameter value must be within {}..={}",
                    parameter.minimum, parameter.maximum
                );
                std::process::exit(1);
            }
            parameter.value = value;
            assigned_slots.push(slot);
        }
        let report = aexcompat_broker::image_render::render_experimental_image_at_time_with_format(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            aexcompat_broker::image_render::RenderTiming::default(),
            smart,
            aexcompat_broker::image_render::RenderPixelFormat::Argb8,
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 7 && args[1] == "--render-experimental-smart-32-cpu-time" {
        let plugin = Path::new(&args[2]);
        let frame = args[5].to_string_lossy().parse::<i32>().unwrap_or(-1);
        let fps = args[6].to_string_lossy().parse::<u32>().unwrap_or(0);
        let timing = aexcompat_broker::image_render::RenderTiming {
            current_time: frame,
            time_step: 1,
            total_time: frame.saturating_add(1),
            time_scale: fps,
        };
        let hash = required_plugin_hash(plugin);
        let parameters = required_plugin_parameters(&repository, plugin, &hash);
        let report = aexcompat_broker::image_render::render_experimental_image_at_time_with_format_context_ui_action_and_gpu_backend(
            &repository,
            plugin,
            &hash,
            Path::new(&args[3]),
            Path::new(&args[4]),
            &parameters,
            timing,
            true,
            aexcompat_broker::image_render::RenderPixelFormat::Argb32f,
            None,
            None,
            aexcompat_broker::image_render::RenderGpuBackend::Cpu,
        );
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 7
        && (args[1] == "--render-experimental-time"
            || args[1] == "--render-experimental-smart-time")
    {
        let smart = args[1] == "--render-experimental-smart-time";
        let plugin = Path::new(&args[2]);
        let frame = args[5].to_string_lossy().parse::<i32>().unwrap_or(-1);
        let fps = args[6].to_string_lossy().parse::<u32>().unwrap_or(0);
        let timing = aexcompat_broker::image_render::RenderTiming {
            current_time: frame,
            time_step: 1,
            total_time: frame.saturating_add(1),
            time_scale: fps,
        };
        let hash = required_plugin_hash(plugin);
        let parameters = required_plugin_parameters(&repository, plugin, &hash);
        let report = if smart {
            aexcompat_broker::image_render::render_experimental_smart_image_at_time(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                timing,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image_at_time(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
                timing,
            )
        };
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 6
        && (args[1] == "--render-experimental-layer"
            || args[1] == "--render-experimental-smart-layer")
    {
        let smart = args[1] == "--render-experimental-smart-layer";
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let mut parameters = required_plugin_parameters(&repository, plugin, &hash);
        let Some(layer) = parameters.iter_mut().find(|item| item.kind == "layer") else {
            eprintln!("AEX exposes no secondary layer parameter");
            std::process::exit(1);
        };
        layer.layer_path = Some(PathBuf::from(&args[4]));
        let report = if smart {
            aexcompat_broker::image_render::render_experimental_smart_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[5]),
                &parameters,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[5]),
                &parameters,
            )
        };
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if (6..=13).contains(&args.len())
        && (args[1] == "--render-experimental-layers"
            || args[1] == "--render-experimental-smart-layers")
    {
        let smart = args[1] == "--render-experimental-smart-layers";
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let mut parameters = required_plugin_parameters(&repository, plugin, &hash);
        let layers = parameters
            .iter_mut()
            .filter(|item| item.kind == "layer")
            .collect::<Vec<_>>();
        if args.len() - 5 > layers.len() {
            eprintln!("more secondary images were supplied than observed layer parameters");
            std::process::exit(1);
        }
        for (layer, path) in layers.into_iter().zip(args[5..].iter()) {
            layer.layer_path = Some(PathBuf::from(path));
        }
        let report = if smart {
            aexcompat_broker::image_render::render_experimental_smart_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
            )
        } else {
            aexcompat_broker::image_render::render_experimental_image(
                &repository,
                plugin,
                &hash,
                Path::new(&args[3]),
                Path::new(&args[4]),
                &parameters,
            )
        };
        match report {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() >= 4 && args[1] == "--inspect-experimental-with-deps" {
        let plugin = Path::new(&args[2]);
        let hash = match fs::read(plugin) {
            Ok(bytes) => format!("{:X}", Sha256::digest(bytes)),
            Err(error) => {
                eprintln!("selected AEX could not be read: {error}");
                std::process::exit(1);
            }
        };
        let roots = &args[3..];
        match inspect_experimental_with_dependency_roots(&repository, plugin, &hash, roots) {
            Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--inspect-experimental" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let parameters = required_inspected_plugin_parameters(&repository, plugin, &hash);
        println!("{}", serde_json::to_string_pretty(&parameters).unwrap());
        return Ok(());
    }
    if args.len() == 5 && args[1] == "--inspect-experimental-runtime-policy" {
        let plugin = Path::new(&args[2]);
        let backend = match args[3].to_string_lossy().as_ref() {
            "cuda" => aexcompat_broker::runtime_module_policy::RuntimeBackend::Cuda,
            "opencl" => aexcompat_broker::runtime_module_policy::RuntimeBackend::Opencl,
            "directx" => aexcompat_broker::runtime_module_policy::RuntimeBackend::Directx,
            "opengl" => aexcompat_broker::runtime_module_policy::RuntimeBackend::Opengl,
            _ => {
                eprintln!("runtime policy backend must be cuda, opencl, directx, or opengl");
                std::process::exit(1);
            }
        };
        let policy = match fs::read(&args[4])
            .and_then(|bytes| aexcompat_broker::runtime_module_policy::parse_and_validate(&bytes))
        {
            Ok(policy) => policy,
            Err(error) => {
                eprintln!("runtime module policy rejected: {error}");
                std::process::exit(1);
            }
        };
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::inspect_experimental_with_runtime_policy(
            &repository,
            plugin,
            &hash,
            &policy,
            backend,
        ) {
            Ok((parameters, diagnostics)) => println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "parameters": parameters,
                    "diagnostics": diagnostics,
                }))
                .unwrap()
            ),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 4 && args[1] == "--inspect-experimental-dependencies" {
        let plugin = Path::new(&args[2]);
        let mode = args[3].to_string_lossy();
        if mode != "all" && mode != "missing" {
            eprintln!("dependency mode must be all or missing");
            std::process::exit(1);
        }
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::inspect_experimental_external_dependencies(
            &repository,
            plugin,
            &hash,
            mode == "missing",
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-options-dialog" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::probe_experimental_options_dialog(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-automatic-options-dialog" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::probe_experimental_automatic_options_dialog(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-nop-render" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::probe_experimental_nop_render(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-smart-nop-render" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::probe_experimental_smart_nop_render(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-input-buffer-write" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::probe_experimental_input_buffer_write(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-smart-input-buffer-write" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::probe_experimental_smart_input_buffer_write(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3
        && (args[1] == "--probe-experimental-expand-buffer"
            || args[1] == "--probe-experimental-shrink-buffer")
    {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        let result = if args[1] == "--probe-experimental-expand-buffer" {
            aexcompat_broker::image_render::probe_experimental_expand_buffer(
                &repository,
                plugin,
                &hash,
            )
        } else {
            aexcompat_broker::image_render::probe_experimental_shrink_buffer(
                &repository,
                plugin,
                &hash,
            )
        };
        match result {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-persistent-sequence" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::probe_experimental_persistent_sequence(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-flattened-sequence" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::probe_experimental_flattened_sequence(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--probe-experimental-copied-flattened-sequence" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::probe_experimental_copied_flattened_sequence(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 4 && args[1] == "--trigger-experimental-button" {
        let plugin = Path::new(&args[2]);
        let slot = args[3].to_string_lossy().parse::<u32>().unwrap_or(0);
        let hash = required_plugin_hash(plugin);
        let parameters = match aexcompat_broker::image_render::inspect_experimental(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(parameters) => parameters,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        };
        match aexcompat_broker::image_render::trigger_experimental_button(
            &repository,
            plugin,
            &hash,
            slot,
            &parameters,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 5 && args[1] == "--trigger-experimental-request" {
        let plugin = Path::new(&args[2]);
        let slot = args[3].to_string_lossy().parse::<u32>().unwrap_or(0);
        let request_path = Path::new(&args[4]);
        let request_bytes = fs::read(request_path).unwrap_or_else(|error| {
            eprintln!("assignment document could not be read: {error}");
            std::process::exit(1);
        });
        if request_bytes.len() > 64 * 1024 {
            eprintln!("assignment document exceeds 64 KiB");
            std::process::exit(1);
        }
        let document: serde_json::Value =
            serde_json::from_slice(&request_bytes).unwrap_or_else(|error| {
                eprintln!("assignment document is invalid JSON: {error}");
                std::process::exit(1);
            });
        let hash = required_plugin_hash(plugin);
        let mut parameters =
            aexcompat_broker::image_render::inspect_experimental(&repository, plugin, &hash)
                .unwrap_or_else(|error| {
                    eprintln!("{error}");
                    std::process::exit(1);
                });
        if let Err(error) = apply_typed_assignments(&mut parameters, &document, Some(request_path))
        {
            eprintln!("{error}");
            std::process::exit(1);
        }
        match aexcompat_broker::image_render::trigger_experimental_button(
            &repository,
            plugin,
            &hash,
            slot,
            &parameters,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--initialize-experimental-aegp" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::initialize_experimental_aegp(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-update-menu" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::dispatch_experimental_aegp_update_menu(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-idle" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::dispatch_experimental_aegp_idle(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-command-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::dispatch_experimental_aegp_command_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-active-idle-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::dispatch_experimental_aegp_active_idle_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-comp-idle-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::dispatch_experimental_aegp_comp_idle_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-keyframe-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::dispatch_experimental_aegp_keyframe_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-seek-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::dispatch_experimental_aegp_seek_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-trim-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::dispatch_experimental_aegp_trim_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.len() == 3 && args[1] == "--dispatch-experimental-aegp-switch-roundtrip" {
        let plugin = Path::new(&args[2]);
        let hash = required_plugin_hash(plugin);
        match aexcompat_broker::image_render::dispatch_experimental_aegp_switch_roundtrip(
            &repository,
            plugin,
            &hash,
        ) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if headless {
        eprintln!(
            "{}",
            serde_json::json!({
                "schema": CLI_CONTRACT_SCHEMA,
                "version": CLI_CONTRACT_VERSION,
                "success": false,
                "classification": "cli_usage_error",
                "failure_stage": "argument_validation",
                "message": "unknown or malformed arguments",
                "gui_launched": false
            })
        );
        std::process::exit(64);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AEXCompat Image Harness",
        options,
        Box::new(move |_cc| Ok(Box::new(HarnessApp::new(repository)))),
    )
}
