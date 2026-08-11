use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use aex_guest_worker::backend::TraceWatchSpec;
use aex_guest_worker::classic::{ClassicHost, ParameterValue};
use aex_guest_worker::gpu_lifecycle::RenderBackendRequest;
use aex_guest_worker::pe::PeImage;
use aex_guest_worker::pixel::FramePixelFormat;
use sha2::{Digest, Sha256};

struct CommandFailure {
    message: String,
    report_json: Option<String>,
}

impl CommandFailure {
    fn with_classic_report(
        host: &ClassicHost,
        error: aex_guest_worker::classic::ClassicError,
    ) -> Self {
        let message = error.to_string();
        let report_json = serde_json::to_string_pretty(&host.failure_report(&error)).ok();
        Self {
            message,
            report_json,
        }
    }
}

impl From<String> for CommandFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            report_json: None,
        }
    }
}

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(command) = args.next() else {
        usage();
        return ExitCode::from(2);
    };
    if command == "session" {
        return run_session_command(args.collect());
    }
    let Some(path) = args.next().map(PathBuf::from) else {
        usage();
        return ExitCode::from(2);
    };
    let mut remaining_args = args.collect::<Vec<_>>();
    let effect_selector = match extract_effect_selector(&mut remaining_args) {
        Ok(selector) => selector,
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            return ExitCode::from(2);
        }
    };
    let pixel_format = match extract_pixel_format(&mut remaining_args) {
        Ok(format) => format,
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            return ExitCode::from(2);
        }
    };
    let render_backend_explicit = remaining_args
        .iter()
        .any(|argument| argument == "--render-backend" || argument == "--gpu-device-index");
    let render_backend = match extract_render_backend(&mut remaining_args) {
        Ok(backend) => backend,
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            return ExitCode::from(2);
        }
    };
    let input = (!remaining_args.is_empty()).then(|| PathBuf::from(remaining_args.remove(0)));
    let output = (!remaining_args.is_empty()).then(|| PathBuf::from(remaining_args.remove(0)));
    let mut trailing_args = remaining_args;
    let region = if command == "render-region-png" {
        if trailing_args.len() < 4 {
            usage();
            return ExitCode::from(2);
        }
        let coordinates = trailing_args
            .drain(..4)
            .map(|value| {
                value
                    .to_str()
                    .ok_or_else(|| "region coordinate must be UTF-8".to_string())?
                    .parse::<i32>()
                    .map_err(|error| format!("invalid region coordinate: {error}"))
            })
            .collect::<Result<Vec<_>, _>>();
        match coordinates {
            Ok(values) => Some([values[0], values[1], values[2], values[3]]),
            Err(error) => {
                eprintln!("aex_guest_error: {error}");
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };
    let (watches, output_pixel) = match parse_trace_watches(&mut trailing_args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            return ExitCode::from(2);
        }
    };
    if command != "render-trace-png" && (!watches.is_empty() || output_pixel.is_some()) {
        eprintln!(
            "aex_guest_error: --watch and --watch-output-pixel are only valid with render-trace-png"
        );
        return ExitCode::from(2);
    }
    if pixel_format != FramePixelFormat::Argb8
        && command != "render"
        && command != "render-png"
        && command != "render-trace-png"
        && command != "render-region-png"
        && command != "census-png"
    {
        eprintln!("aex_guest_error: --pixel-format is only valid with render commands or session");
        return ExitCode::from(2);
    }
    if render_backend_explicit && command != "render" && command != "render-png" {
        eprintln!(
            "aex_guest_error: render backend options are only valid with render or render-png"
        );
        return ExitCode::from(2);
    }
    if ((command == "render-png"
        || command == "render-trace-png"
        || command == "render-region-png"
        || command == "census-png")
        && (input.is_none() || output.is_none()))
        || (command == "trace-selector" && (input.is_none() || output.is_some()))
        || (command != "render-png"
            && command != "render-trace-png"
            && command != "render-region-png"
            && command != "census-png"
            && command != "trace-selector"
            && (input.is_some() || output.is_some() || !trailing_args.is_empty()))
        || (command == "trace-selector" && !trailing_args.is_empty())
    {
        usage();
        return ExitCode::from(2);
    }

    let mut traced_selector_error = None;
    let result = fs::read(&path)
        .map_err(|error| CommandFailure::from(error.to_string()))
        .and_then(|bytes| {
            PeImage::parse_and_map(&bytes)
                .map_err(|error| CommandFailure::from(error.to_string()))
        })
        .and_then(|image| match command.to_str() {
            Some("inspect") => {
                if effect_selector.is_some() {
                    ClassicHost::new_with_effect(&image, effect_selector.as_deref())
                        .map_err(|error| CommandFailure::from(error.to_string()))?;
                }
                serde_json::to_string_pretty(&image.report())
                    .map_err(|error| CommandFailure::from(error.to_string()))
            }
            Some("setup") => {
                let mut host = ClassicHost::new_with_effect(&image, effect_selector.as_deref())
                    .map_err(|error| CommandFailure::from(error.to_string()))?;
                let report = host
                    .setup()
                    .map_err(|error| CommandFailure::with_classic_report(&host, error))?;
                serde_json::to_string_pretty(&report)
                    .map_err(|error| CommandFailure::from(error.to_string()))
            }
            Some("trace-selector") => {
                let selector = input
                    .as_deref()
                    .and_then(Path::to_str)
                    .ok_or_else(|| CommandFailure::from("trace selector must be UTF-8".to_string()))?;
                let mut host = ClassicHost::new_with_effect(&image, effect_selector.as_deref())
                    .map_err(|error| CommandFailure::from(error.to_string()))?;
                let trace = host
                    .trace_setup_selector(selector)
                    .map_err(|error| CommandFailure::with_classic_report(&host, error))?;
                traced_selector_error = selector_error(trace.return_value);
                serde_json::to_string_pretty(&trace)
                    .map_err(|error| CommandFailure::from(error.to_string()))
            }
            Some("render") => {
                let mut host = ClassicHost::new_with_effect(&image, effect_selector.as_deref())
                    .map_err(|error| CommandFailure::from(error.to_string()))?;
                let report = host
                    .render_default_2x2_format_with_backend(pixel_format, render_backend)
                    .map_err(|error| CommandFailure::with_classic_report(&host, error))?;
                serde_json::to_string_pretty(&report)
                    .map_err(|error| CommandFailure::from(error.to_string()))
            }
            Some("render-png") => render_png(
                &image,
                input.as_deref().expect("validated input path"),
                output.as_deref().expect("validated output path"),
                &parse_parameter_values(&trailing_args)?,
                region,
                false,
                false,
                &[],
                None,
                effect_selector.as_deref(),
                pixel_format,
                render_backend,
            ),
            Some("render-trace-png") => render_png(
                &image,
                input.as_deref().expect("validated input path"),
                output.as_deref().expect("validated output path"),
                &parse_parameter_values(&trailing_args)?,
                None,
                false,
                true,
                &watches,
                output_pixel,
                effect_selector.as_deref(),
                pixel_format,
                render_backend,
            ),
            Some("render-region-png") => render_png(
                &image,
                input.as_deref().expect("validated input path"),
                output.as_deref().expect("validated output path"),
                &parse_parameter_values(&trailing_args)?,
                region,
                false,
                false,
                &[],
                None,
                effect_selector.as_deref(),
                pixel_format,
                render_backend,
            ),
            Some("census-png") => render_png(
                &image,
                input.as_deref().expect("validated input path"),
                output.as_deref().expect("validated output path"),
                &parse_parameter_values(&trailing_args)?,
                None,
                true,
                false,
                &[],
                None,
                effect_selector.as_deref(),
                pixel_format,
                render_backend,
            ),
            _ => Err(CommandFailure::from(
                "command must be inspect, setup, trace-selector, render, render-png, render-trace-png, render-region-png, or census-png".to_string(),
            )),
        });
    match result {
        Ok(json) => {
            println!("{json}");
            if let Some(error) = traced_selector_error {
                eprintln!("aex_guest_error: traced selector returned {error}");
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            if let Some(report_json) = error.report_json {
                println!("{report_json}");
            }
            eprintln!("aex_guest_error: {}", error.message);
            ExitCode::from(1)
        }
    }
}

fn selector_error(return_value: u64) -> Option<i32> {
    let error = return_value as i32;
    (error != 0).then_some(error)
}

fn usage() {
    eprintln!(
        "usage: aex-guest-worker <inspect|setup|render> <x64.aex> [--effect <#index|match-name>] [--pixel-format <argb8|argb16|argb32f>] [--render-backend <cpu|opencl|wgpu-metal>] [--gpu-device-index <n>]"
    );
    eprintln!(
        "       aex-guest-worker session <x64.aex> <input.raw> <output.raw> <width> <height> <time-scale> [--effect <#index|match-name>] [--pixel-format <argb8|argb16|argb32f>]"
    );
    eprintln!(
        "       aex-guest-worker trace-selector <x64.aex> <GLOBAL_SETUP|PARAMS_SETUP> [--effect <#index|match-name>]"
    );
    eprintln!(
        "       aex-guest-worker render-png <x64.aex> <input.png> <output.png> [--effect <#index|match-name>] [--pixel-format <argb8|argb16|argb32f>] [--render-backend <cpu|opencl|wgpu-metal>] [--gpu-device-index <n>] [name=value | name@slot=a,r,g,b ...]"
    );
    eprintln!(
        "       aex-guest-worker render-trace-png <x64.aex> <input.png> <output.png> [--effect <#index|match-name>] [--pixel-format <argb8|argb16|argb32f>] [--watch <spec>] [--watch-output-pixel x,y] [name=value ...]"
    );
    eprintln!(
        "       aex-guest-worker render-region-png <x64.aex> <input.png> <output.png> <left> <top> <right> <bottom> [--effect <#index|match-name>] [--pixel-format <argb8|argb16|argb32f>] [name=value ...]"
    );
    eprintln!(
        "       aex-guest-worker census-png <x64.aex> <input.png> <output.png> [--effect <#index|match-name>] [--pixel-format <argb8|argb16|argb32f>] [name=value ...]"
    );
}

fn extract_effect_selector(
    arguments: &mut Vec<std::ffi::OsString>,
) -> Result<Option<String>, String> {
    let mut selector = None;
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] != "--effect" {
            index += 1;
            continue;
        }
        if selector.is_some() {
            return Err("--effect may be specified only once".into());
        }
        if index + 1 >= arguments.len() {
            return Err("--effect requires #index or an exact match name".into());
        }
        let value = arguments[index + 1]
            .to_str()
            .ok_or_else(|| "--effect selector must be UTF-8".to_string())?;
        if value.is_empty() {
            return Err("--effect selector must not be empty".into());
        }
        selector = Some(value.to_string());
        arguments.drain(index..=index + 1);
    }
    Ok(selector)
}

fn extract_pixel_format(
    arguments: &mut Vec<std::ffi::OsString>,
) -> Result<FramePixelFormat, String> {
    let mut format = None;
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] != "--pixel-format" {
            index += 1;
            continue;
        }
        if format.is_some() {
            return Err("--pixel-format may be specified only once".into());
        }
        if index + 1 >= arguments.len() {
            return Err("--pixel-format requires argb8, argb16, or argb32f".into());
        }
        let value = arguments[index + 1]
            .to_str()
            .ok_or_else(|| "--pixel-format must be UTF-8".to_string())?;
        format = Some(FramePixelFormat::parse(value).map_err(|error| error.to_string())?);
        arguments.drain(index..=index + 1);
    }
    Ok(format.unwrap_or(FramePixelFormat::Argb8))
}

fn extract_render_backend(
    arguments: &mut Vec<std::ffi::OsString>,
) -> Result<RenderBackendRequest, String> {
    let mut backend = None;
    let mut device_index = None;
    let mut index = 0;
    while index < arguments.len() {
        let option = arguments[index].to_str();
        let target = match option {
            Some("--render-backend") => Some("backend"),
            Some("--gpu-device-index") => Some("device"),
            _ => None,
        };
        let Some(target) = target else {
            index += 1;
            continue;
        };
        if index + 1 >= arguments.len() {
            return Err(format!(
                "{} requires a value",
                arguments[index].to_string_lossy()
            ));
        }
        let value = arguments[index + 1]
            .to_str()
            .ok_or_else(|| format!("{} value must be UTF-8", arguments[index].to_string_lossy()))?;
        match target {
            "backend" => {
                if backend.is_some() {
                    return Err("--render-backend may be specified only once".into());
                }
                backend = Some(match value {
                    "cpu" => "cpu",
                    "opencl" => "opencl",
                    "wgpu-metal" => "wgpu-metal",
                    _ => return Err("--render-backend requires cpu, opencl, or wgpu-metal".into()),
                });
            }
            "device" => {
                if device_index.is_some() {
                    return Err("--gpu-device-index may be specified only once".into());
                }
                let parsed = value
                    .parse::<u32>()
                    .map_err(|error| format!("invalid GPU device index: {error}"))?;
                if parsed >= 64 {
                    return Err("--gpu-device-index must be between 0 and 63".into());
                }
                device_index = Some(parsed);
            }
            _ => unreachable!(),
        }
        arguments.drain(index..=index + 1);
    }
    match backend.unwrap_or("cpu") {
        "cpu" if device_index.is_some() => {
            Err("--gpu-device-index requires --render-backend opencl or wgpu-metal".into())
        }
        "cpu" => Ok(RenderBackendRequest::Cpu),
        "opencl" => Ok(RenderBackendRequest::OpenCl {
            device_index: device_index.unwrap_or(0),
        }),
        "wgpu-metal" => Ok(RenderBackendRequest::WgpuMetal {
            device_index: device_index.unwrap_or(0),
        }),
        _ => unreachable!(),
    }
}

fn run_session_command(mut arguments: Vec<std::ffi::OsString>) -> ExitCode {
    let effect_selector = match extract_effect_selector(&mut arguments) {
        Ok(selector) => selector,
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            return ExitCode::from(2);
        }
    };
    let pixel_format = match extract_pixel_format(&mut arguments) {
        Ok(format) => format,
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            return ExitCode::from(2);
        }
    };
    let fixture_layers = match extract_single_path_option(&mut arguments, "--fixture-layers-v1") {
        Ok(value) => value,
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            return ExitCode::from(2);
        }
    };
    let fixture_smart = match extract_single_string_option(&mut arguments, "--fixture-render-path")
    {
        Ok(None) => None,
        Ok(Some(value)) if value == "classic" => Some(false),
        Ok(Some(value)) if value == "smart" => Some(true),
        Ok(Some(_)) => {
            eprintln!("aex_guest_error: --fixture-render-path requires classic or smart");
            return ExitCode::from(2);
        }
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            return ExitCode::from(2);
        }
    };
    if fixture_layers.is_some() && fixture_smart.is_none() {
        eprintln!("aex_guest_error: --fixture-layers-v1 requires --fixture-render-path");
        return ExitCode::from(2);
    }
    if arguments.len() != 6 {
        usage();
        return ExitCode::from(2);
    }
    let path = PathBuf::from(&arguments[0]);
    let input_slot = PathBuf::from(&arguments[1]);
    let output_slot = PathBuf::from(&arguments[2]);
    let parse_u32 = |index: usize, name: &str| {
        arguments[index]
            .to_str()
            .ok_or_else(|| format!("{name} must be UTF-8"))?
            .parse::<u32>()
            .map_err(|error| format!("invalid {name}: {error}"))
    };
    let (width, height, time_scale) = match (
        parse_u32(3, "width"),
        parse_u32(4, "height"),
        parse_u32(5, "time scale"),
    ) {
        (Ok(width), Ok(height), Ok(time_scale)) => (width, height, time_scale),
        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
            eprintln!("aex_guest_error: {error}");
            return ExitCode::from(2);
        }
    };
    let result = fs::read(&path)
        .map_err(|error| format!("read AEX: {error}"))
        .and_then(|bytes| PeImage::parse_and_map(&bytes).map_err(|error| error.to_string()))
        .and_then(|image| {
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            aex_guest_worker::resident::run_resident_session(
                &image,
                &input_slot,
                &output_slot,
                width,
                height,
                time_scale,
                pixel_format,
                effect_selector.as_deref(),
                fixture_layers.as_deref(),
                fixture_smart,
                stdin.lock(),
                stdout.lock(),
            )
            .map_err(|error| error.to_string())
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            ExitCode::from(1)
        }
    }
}

fn extract_single_string_option(
    arguments: &mut Vec<std::ffi::OsString>,
    option: &str,
) -> Result<Option<String>, String> {
    let mut found = None;
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] != option {
            index += 1;
            continue;
        }
        if found.is_some() || index + 1 >= arguments.len() {
            return Err(format!("{option} must be specified once with a value"));
        }
        let value = arguments[index + 1]
            .to_str()
            .ok_or_else(|| format!("{option} value must be UTF-8"))?;
        if value.is_empty() {
            return Err(format!("{option} value must not be empty"));
        }
        found = Some(value.to_owned());
        arguments.drain(index..=index + 1);
    }
    Ok(found)
}

fn extract_single_path_option(
    arguments: &mut Vec<std::ffi::OsString>,
    option: &str,
) -> Result<Option<PathBuf>, String> {
    extract_single_string_option(arguments, option).map(|value| value.map(PathBuf::from))
}

fn render_png(
    image: &PeImage,
    input: &Path,
    output: &Path,
    parameter_values: &[ParameterValue],
    region: Option<[i32; 4]>,
    census: bool,
    trace: bool,
    watches: &[TraceWatchSpec],
    output_pixel: Option<[u32; 2]>,
    effect_selector: Option<&str>,
    pixel_format: FramePixelFormat,
    render_backend: RenderBackendRequest,
) -> Result<String, CommandFailure> {
    let input_png_sha256 = fs::read(input)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .map_err(|error| format!("read input PNG for provenance: {error}"))?;
    let rgba = image::open(input)
        .map_err(|error| format!("decode input PNG: {error}"))?
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    let input_pixels = pixel_format
        .promote_rgba8(rgba.as_raw())
        .map_err(|error| error.to_string())?;
    let mut host = ClassicHost::new_with_effect(image, effect_selector)
        .map_err(|error| CommandFailure::from(error.to_string()))?;
    let render_result = if trace {
        host.render_pixels_trace_with_watches(
            width,
            height,
            pixel_format,
            &input_pixels,
            parameter_values,
            watches.to_vec(),
            output_pixel,
        )
    } else {
        match (region, census) {
            (None, true) => host
                .render_pixels_census(width, height, pixel_format, &input_pixels, parameter_values)
                .map(|report| (report, Vec::new())),
            (Some(region), _) => host
                .render_pixels_region(
                    width,
                    height,
                    pixel_format,
                    &input_pixels,
                    parameter_values,
                    region,
                )
                .map(|report| (report, Vec::new())),
            (None, false) => host
                .render_pixels_with_backend(
                    width,
                    height,
                    pixel_format,
                    &input_pixels,
                    parameter_values,
                    render_backend,
                )
                .map(|report| (report, Vec::new())),
        }
    };
    let (report, execution_traces) =
        render_result.map_err(|error| CommandFailure::with_classic_report(&host, error))?;
    let mut output_rgba = Vec::with_capacity(report.argb8.len());
    for pixel in report.argb8.chunks_exact(4) {
        output_rgba.extend_from_slice(&[pixel[1], pixel[2], pixel[3], pixel[0]]);
    }
    let output_image = image::RgbaImage::from_raw(width, height, output_rgba)
        .ok_or_else(|| "rendered pixel byte count does not match dimensions".to_string())?;
    output_image
        .save_with_format(output, image::ImageFormat::Png)
        .map_err(|error| format!("write output PNG: {error}"))?;
    let mut report_json =
        serde_json::to_value(&report).map_err(|error| format!("serialize report: {error}"))?;
    report_json["pixel_bytes"] = serde_json::json!(report.raw_pixel_bytes);
    report_json["input_png_sha256"] = serde_json::json!(input_png_sha256);
    report_json["output_png"] = serde_json::json!(output);
    if !execution_traces.is_empty() {
        report_json["execution_traces"] = serde_json::to_value(execution_traces)
            .map_err(|error| format!("serialize traces: {error}"))?;
    }
    if let Some(object) = report_json.as_object_mut() {
        object.remove("argb8");
    }
    serde_json::to_string_pretty(&report_json)
        .map_err(|error| CommandFailure::from(error.to_string()))
}

fn parse_trace_watches(
    values: &mut Vec<std::ffi::OsString>,
) -> Result<(Vec<TraceWatchSpec>, Option<[u32; 2]>), String> {
    let mut watches = Vec::new();
    let mut output_pixel = None;
    let mut parameters = Vec::new();
    let mut arguments = values.drain(..).peekable();
    while let Some(value) = arguments.next() {
        let Some(text) = value.to_str() else {
            return Err("trace watch or parameter assignment must be UTF-8".into());
        };
        if text == "--watch-output-pixel" || text.starts_with("--watch-output-pixel=") {
            let coordinate = if let Some(coordinate) = text.strip_prefix("--watch-output-pixel=") {
                coordinate.to_string()
            } else {
                arguments
                    .next()
                    .ok_or_else(|| "--watch-output-pixel requires x,y".to_string())?
                    .into_string()
                    .map_err(|_| "output pixel must be UTF-8".to_string())?
            };
            let (x, y) = coordinate
                .split_once(',')
                .ok_or_else(|| "output pixel must be x,y".to_string())?;
            if output_pixel.is_some() {
                return Err("duplicate --watch-output-pixel".into());
            }
            output_pixel = Some([
                x.parse()
                    .map_err(|error| format!("invalid output pixel x: {error}"))?,
                y.parse()
                    .map_err(|error| format!("invalid output pixel y: {error}"))?,
            ]);
            continue;
        }
        let specification = if text == "--watch" {
            arguments
                .next()
                .ok_or_else(|| "--watch requires a specification".to_string())?
                .into_string()
                .map_err(|_| "trace watch specification must be UTF-8".to_string())?
        } else if let Some(specification) = text.strip_prefix("--watch=") {
            specification.to_string()
        } else {
            parameters.push(value);
            continue;
        };
        let mut function_rva = None;
        let mut instruction_rva = None;
        let mut register = None;
        let mut size = None;
        let mut occurrence = None;
        for field in specification.split(',') {
            let (key, value) = field
                .split_once('=')
                .ok_or_else(|| format!("invalid watch field: {field:?}"))?;
            match key {
                "function" => function_rva = Some(parse_watch_number(value)?),
                "rva" => instruction_rva = Some(parse_watch_number(value)?),
                "arg" | "register" => {
                    register = Some(match value.to_ascii_lowercase().as_str() {
                        "rcx" => "rcx",
                        "rdx" => "rdx",
                        "r8" => "r8",
                        "r9" => "r9",
                        "rax" => "rax",
                        "5" | "stack5" => "stack5",
                        "6" | "stack6" => "stack6",
                        "7" | "stack7" => "stack7",
                        "8" | "stack8" => "stack8",
                        _ => return Err(format!("unsupported watch register: {value:?}")),
                    })
                }
                "size" => {
                    size = Some(
                        value
                            .parse::<usize>()
                            .map_err(|error| format!("invalid watch size: {error}"))?,
                    )
                }
                "occurrence" => {
                    if occurrence.is_some() {
                        return Err("duplicate watch occurrence".into());
                    }
                    let parsed = value
                        .parse::<u64>()
                        .map_err(|error| format!("invalid watch occurrence: {error}"))?;
                    if parsed == 0 {
                        return Err("watch occurrence must be at least 1".into());
                    }
                    occurrence = Some(parsed);
                }
                "when" => {
                    if value != "entry+return" && value != "both" {
                        return Err("watch when must be entry+return or both".into());
                    }
                }
                _ => return Err(format!("unknown watch field: {key:?}")),
            }
        }
        if function_rva.is_some() == instruction_rva.is_some() {
            return Err(
                "watch requires exactly one of function=<rva> or rva=<call-site-rva>".into(),
            );
        }
        let register = register.ok_or_else(|| "watch requires arg=<register>".to_string())?;
        let size = size.ok_or_else(|| "watch requires size=<bytes>".to_string())?;
        if size == 0 || size > 4096 {
            return Err("watch size must be between 1 and 4096 bytes".into());
        }
        watches.push(TraceWatchSpec {
            id: format!("watch-{}", watches.len() + 1),
            function_rva,
            instruction_rva,
            absolute_address: None,
            register,
            size,
            occurrence,
            image_coordinate: None,
            image_row_offset: None,
            image_format: None,
        });
    }
    drop(arguments);
    *values = parameters;
    Ok((watches, output_pixel))
}

fn parse_watch_number(value: &str) -> Result<u64, String> {
    if let Some(hex) = value.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).map_err(|error| format!("invalid watch RVA: {error}"))
    } else {
        value
            .parse()
            .map_err(|error| format!("invalid watch RVA: {error}"))
    }
}

fn parse_parameter_values(values: &[std::ffi::OsString]) -> Result<Vec<ParameterValue>, String> {
    let mut parsed = Vec::with_capacity(values.len());
    for value in values {
        let value = value
            .to_str()
            .ok_or_else(|| "parameter assignment must be UTF-8".to_string())?;
        let (identity, encoded) = value
            .split_once('=')
            .ok_or_else(|| format!("parameter assignment must be name=value: {value:?}"))?;
        let (name, slot) = match identity.rsplit_once('@') {
            Some((name, slot)) => {
                let slot = slot
                    .parse::<usize>()
                    .map_err(|error| format!("invalid parameter slot in {identity:?}: {error}"))?;
                if slot == 0 {
                    return Err("parameter slot must be greater than zero".into());
                }
                (name, Some(slot))
            }
            None => (identity, None),
        };
        if name.is_empty() {
            return Err("parameter name must not be empty".into());
        }
        if parsed
            .iter()
            .any(|existing: &ParameterValue| match (existing.slot, slot) {
                (Some(existing), Some(requested)) => existing == requested,
                (None, None) => existing.name == name,
                _ => false,
            })
        {
            return Err(format!("duplicate parameter assignment: {identity:?}"));
        }
        let components = encoded.split(',').collect::<Vec<_>>();
        let (numeric, point, color) = match components.as_slice() {
            [number] => (
                Some(
                    number
                        .parse()
                        .map_err(|error| format!("invalid value for {name:?}: {error}"))?,
                ),
                None,
                None,
            ),
            [x, y] => (
                None,
                Some([
                    x.parse()
                        .map_err(|error| format!("invalid point x for {name:?}: {error}"))?,
                    y.parse()
                        .map_err(|error| format!("invalid point y for {name:?}: {error}"))?,
                ]),
                None,
            ),
            [alpha, red, green, blue] => {
                let mut color = [0u8; 4];
                for (destination, component) in color.iter_mut().zip([alpha, red, green, blue]) {
                    *destination = component.parse::<u8>().map_err(|_| {
                        format!("ARGB8 component for {name:?} must be an integer from 0 to 255")
                    })?;
                }
                (None, None, Some(color))
            }
            _ => {
                return Err(format!(
                    "parameter assignment must contain one scalar, two point components, or four ARGB8 components: {encoded:?}"
                ));
            }
        };
        parsed.push(ParameterValue {
            slot,
            name: name.to_string(),
            value: numeric,
            color,
            point,
        });
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{
        extract_effect_selector, extract_pixel_format, extract_render_backend,
        parse_parameter_values, parse_trace_watches, selector_error,
    };
    use aex_guest_worker::gpu_lifecycle::RenderBackendRequest;
    use aex_guest_worker::pixel::FramePixelFormat;
    use std::ffi::OsString;

    #[test]
    fn traced_selector_nonzero_result_is_a_process_error() {
        assert_eq!(selector_error(0), None);
        assert_eq!(selector_error(7), Some(7));
        assert_eq!(selector_error(u64::MAX), Some(-1));
    }

    #[test]
    fn effect_selector_is_removed_without_consuming_parameter_assignments() {
        let mut values = vec![
            OsString::from("input.png"),
            OsString::from("--effect"),
            OsString::from("#2"),
            OsString::from("output.png"),
            OsString::from("Amount=5"),
        ];
        assert_eq!(
            extract_effect_selector(&mut values).unwrap().as_deref(),
            Some("#2")
        );
        assert_eq!(
            values,
            [
                OsString::from("input.png"),
                OsString::from("output.png"),
                OsString::from("Amount=5")
            ]
        );
        values.extend([OsString::from("--effect"), OsString::from("duplicate")]);
        values.extend([OsString::from("--effect"), OsString::from("duplicate")]);
        assert!(extract_effect_selector(&mut values).is_err());
    }

    #[test]
    fn pixel_format_is_bounded_removed_and_defaults_to_argb8() {
        let mut values = vec![
            OsString::from("input.png"),
            OsString::from("--pixel-format"),
            OsString::from("argb32f"),
            OsString::from("output.png"),
        ];
        assert_eq!(
            extract_pixel_format(&mut values).unwrap(),
            FramePixelFormat::Argb32f
        );
        assert_eq!(
            values,
            [OsString::from("input.png"), OsString::from("output.png")]
        );
        assert_eq!(
            extract_pixel_format(&mut values).unwrap(),
            FramePixelFormat::Argb8
        );
        values.extend([OsString::from("--pixel-format"), OsString::from("rgba8")]);
        assert!(extract_pixel_format(&mut values).is_err());
    }

    #[test]
    fn render_backend_is_explicit_bounded_and_defaults_to_cpu() {
        let mut values = vec![
            OsString::from("input.png"),
            OsString::from("--gpu-device-index"),
            OsString::from("3"),
            OsString::from("--render-backend"),
            OsString::from("opencl"),
            OsString::from("output.png"),
        ];
        assert_eq!(
            extract_render_backend(&mut values).unwrap(),
            RenderBackendRequest::OpenCl { device_index: 3 }
        );
        assert_eq!(
            values,
            [OsString::from("input.png"), OsString::from("output.png")]
        );
        assert_eq!(
            extract_render_backend(&mut values).unwrap(),
            RenderBackendRequest::Cpu
        );
        values.extend([
            OsString::from("--render-backend"),
            OsString::from("wgpu-metal"),
            OsString::from("--gpu-device-index"),
            OsString::from("5"),
        ]);
        assert_eq!(
            extract_render_backend(&mut values).unwrap(),
            RenderBackendRequest::WgpuMetal { device_index: 5 }
        );
        assert_eq!(
            values,
            [OsString::from("input.png"), OsString::from("output.png")]
        );
        let mut wgpu_default = vec![
            OsString::from("--render-backend"),
            OsString::from("wgpu-metal"),
        ];
        assert_eq!(
            extract_render_backend(&mut wgpu_default).unwrap(),
            RenderBackendRequest::WgpuMetal { device_index: 0 }
        );
        assert!(wgpu_default.is_empty());

        for arguments in [
            vec![OsString::from("--render-backend"), OsString::from("metal")],
            vec![
                OsString::from("--render-backend"),
                OsString::from("cpu"),
                OsString::from("--gpu-device-index"),
                OsString::from("1"),
            ],
            vec![
                OsString::from("--render-backend"),
                OsString::from("opencl"),
                OsString::from("--gpu-device-index"),
                OsString::from("64"),
            ],
        ] {
            assert!(extract_render_backend(&mut arguments.clone()).is_err());
        }
    }

    #[test]
    fn parameter_parser_accepts_slot_qualified_argb8() {
        let values =
            parse_parameter_values(&[OsString::from("Key Color@2=255,64,128,192")]).unwrap();
        assert_eq!(values[0].name, "Key Color");
        assert_eq!(values[0].slot, Some(2));
        assert_eq!(values[0].value, None);
        assert_eq!(values[0].color, Some([255, 64, 128, 192]));
        assert_eq!(values[0].point, None);
    }

    #[test]
    fn parameter_parser_rejects_invalid_argb8_and_duplicate_slots() {
        assert!(parse_parameter_values(&[OsString::from("Tint@2=256,1,2,3")]).is_err());
        assert!(
            parse_parameter_values(&[
                OsString::from("First@2=255,1,2,3"),
                OsString::from("Second@2=255,4,5,6"),
            ])
            .is_err()
        );
    }

    #[test]
    fn parameter_parser_accepts_scalar_and_point_assignments() {
        let values = parse_parameter_values(&[
            OsString::from("Amount=12.5"),
            OsString::from("Center=42,-7.25"),
            OsString::from("Strength@4=100"),
        ])
        .unwrap();
        assert_eq!(values[0].value, Some(12.5));
        assert_eq!(values[0].point, None);
        assert_eq!(values[0].color, None);
        assert_eq!(values[1].value, None);
        assert_eq!(values[1].point, Some([42.0, -7.25]));
        assert_eq!(values[1].color, None);
        assert_eq!(values[2].slot, Some(4));
        assert_eq!(values[2].value, Some(100.0));
    }

    #[test]
    fn trace_watches_are_separated_from_parameter_assignments() {
        let mut values = vec![
            OsString::from(
                "--watch=function=0xcce0,arg=rcx,size=16,when=entry+return,occurrence=2113",
            ),
            OsString::from("Amount=2.5"),
            OsString::from("--watch"),
            OsString::from("rva=0x350b,register=r9,size=64"),
            OsString::from("--watch-output-pixel=92,841"),
        ];
        let (watches, output_pixel) = parse_trace_watches(&mut values).unwrap();
        assert_eq!(watches.len(), 2);
        assert_eq!(watches[0].function_rva, Some(0xcce0));
        assert_eq!(watches[0].register, "rcx");
        assert_eq!(watches[0].occurrence, Some(2113));
        assert_eq!(watches[1].instruction_rva, Some(0x350b));
        assert_eq!(watches[1].occurrence, None);
        assert_eq!(output_pixel, Some([92, 841]));
        assert_eq!(values, [OsString::from("Amount=2.5")]);
    }

    #[test]
    fn trace_watch_occurrence_rejects_zero_malformed_and_duplicate_values() {
        for specification in [
            "--watch=function=0xcce0,arg=rcx,size=16,occurrence=0",
            "--watch=function=0xcce0,arg=rcx,size=16,occurrence=nope",
            "--watch=function=0xcce0,arg=rcx,size=16,occurrence=2,occurrence=3",
        ] {
            let mut values = vec![OsString::from(specification)];
            assert!(parse_trace_watches(&mut values).is_err(), "{specification}");
        }
    }
}
