use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use aex_guest_worker::backend::TraceWatchSpec;
use aex_guest_worker::classic::{ClassicHost, ParameterValue};
use aex_guest_worker::pe::PeImage;
use sha2::{Digest, Sha256};

fn main() -> ExitCode {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(command) = args.next() else {
        usage();
        return ExitCode::from(2);
    };
    let Some(path) = args.next().map(PathBuf::from) else {
        usage();
        return ExitCode::from(2);
    };
    let input = args.next().map(PathBuf::from);
    let output = args.next().map(PathBuf::from);
    let mut trailing_args = args.collect::<Vec<_>>();
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
        .map_err(|error| error.to_string())
        .and_then(|bytes| PeImage::parse_and_map(&bytes).map_err(|error| error.to_string()))
        .and_then(|image| match command.to_str() {
            Some("inspect") => {
                serde_json::to_string_pretty(&image.report()).map_err(|error| error.to_string())
            }
            Some("setup") => ClassicHost::new(&image)
                .and_then(|mut host| host.setup())
                .and_then(|report| {
                    serde_json::to_string_pretty(&report).map_err(|error| {
                        aex_guest_worker::classic::ClassicError::Guest(
                            aex_guest_worker::backend::GuestError::Callback(error.to_string()),
                        )
                    })
                })
                .map_err(|error| error.to_string()),
            Some("trace-selector") => {
                let selector = input
                    .as_deref()
                    .and_then(Path::to_str)
                    .ok_or_else(|| "trace selector must be UTF-8".to_string())?;
                ClassicHost::new(&image)
                    .and_then(|mut host| host.trace_setup_selector(selector))
                    .and_then(|trace| {
                        traced_selector_error = selector_error(trace.return_value);
                        serde_json::to_string_pretty(&trace).map_err(|error| {
                            aex_guest_worker::classic::ClassicError::Guest(
                                aex_guest_worker::backend::GuestError::Callback(error.to_string()),
                            )
                        })
                    })
                    .map_err(|error| error.to_string())
            }
            Some("render") => ClassicHost::new(&image)
                .and_then(|mut host| host.render_default_2x2())
                .and_then(|report| {
                    serde_json::to_string_pretty(&report).map_err(|error| {
                        aex_guest_worker::classic::ClassicError::Guest(
                            aex_guest_worker::backend::GuestError::Callback(error.to_string()),
                        )
                    })
                })
                .map_err(|error| error.to_string()),
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
            ),
            _ => Err(
                "command must be inspect, setup, trace-selector, render, render-png, render-trace-png, render-region-png, or census-png".to_string(),
            ),
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
            eprintln!("aex_guest_error: {error}");
            ExitCode::from(1)
        }
    }
}

fn selector_error(return_value: u64) -> Option<i32> {
    let error = return_value as i32;
    (error != 0).then_some(error)
}

fn usage() {
    eprintln!("usage: aex-guest-worker <inspect|setup|render> <x64.aex>");
    eprintln!("       aex-guest-worker trace-selector <x64.aex> <GLOBAL_SETUP|PARAMS_SETUP>");
    eprintln!(
        "       aex-guest-worker render-png <x64.aex> <input.png> <output.png> [name=value | Name@slot=a,r,g,b ...]"
    );
    eprintln!(
        "       aex-guest-worker render-trace-png <x64.aex> <input.png> <output.png> [--watch <spec>] [--watch-output-pixel x,y] [name=value ...]"
    );
    eprintln!(
        "       aex-guest-worker render-region-png <x64.aex> <input.png> <output.png> <left> <top> <right> <bottom> [name=value ...]"
    );
    eprintln!(
        "       aex-guest-worker census-png <x64.aex> <input.png> <output.png> [name=value ...]"
    );
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
) -> Result<String, String> {
    let input_png_sha256 = fs::read(input)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .map_err(|error| format!("read input PNG for provenance: {error}"))?;
    let rgba = image::open(input)
        .map_err(|error| format!("decode input PNG: {error}"))?
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    let mut argb8 = Vec::with_capacity(rgba.as_raw().len());
    for pixel in rgba.as_raw().chunks_exact(4) {
        argb8.extend_from_slice(&[pixel[3], pixel[0], pixel[1], pixel[2]]);
    }
    let (report, execution_traces) = ClassicHost::new(image)
        .and_then(|mut host| {
            if trace {
                host.render_argb8_trace_with_watches(
                    width,
                    height,
                    &argb8,
                    parameter_values,
                    watches.to_vec(),
                    output_pixel,
                )
            } else {
                match (region, census) {
                    (None, true) => host
                        .render_argb8_census(width, height, &argb8, parameter_values)
                        .map(|report| (report, Vec::new())),
                    (Some(region), _) => host
                        .render_argb8_region(width, height, &argb8, parameter_values, region)
                        .map(|report| (report, Vec::new())),
                    (None, false) => host
                        .render_argb8(width, height, &argb8, parameter_values)
                        .map(|report| (report, Vec::new())),
                }
            }
        })
        .map_err(|error| error.to_string())?;
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
    report_json["pixel_bytes"] = serde_json::json!(report.argb8.len());
    report_json["input_png_sha256"] = serde_json::json!(input_png_sha256);
    report_json["output_png"] = serde_json::json!(output);
    if !execution_traces.is_empty() {
        report_json["execution_traces"] = serde_json::to_value(execution_traces)
            .map_err(|error| format!("serialize traces: {error}"))?;
    }
    if let Some(object) = report_json.as_object_mut() {
        object.remove("argb8");
    }
    serde_json::to_string_pretty(&report_json).map_err(|error| error.to_string())
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
        let (name, raw_value) = value
            .split_once('=')
            .ok_or_else(|| format!("parameter assignment must be name=value: {value:?}"))?;
        if name.is_empty() {
            return Err("parameter name must not be empty".into());
        }
        let (parameter_name, slot) = name
            .rsplit_once('@')
            .map(|(parameter_name, raw)| {
                if parameter_name.is_empty() {
                    return Err("parameter name before @slot must not be empty".to_string());
                }
                raw.parse::<usize>()
                    .map_err(|error| format!("invalid parameter slot {name:?}: {error}"))
                    .and_then(|slot| {
                        if slot == 0 {
                            Err("parameter slot must be greater than zero".to_string())
                        } else {
                            Ok(slot)
                        }
                    })
                    .map(|slot| (parameter_name, Some(slot)))
            })
            .transpose()?
            .unwrap_or((name, None));
        if parsed.iter().any(|existing: &ParameterValue| {
            slot.map_or(
                existing.slot.is_none() && existing.name == parameter_name,
                |slot| existing.slot == Some(slot),
            )
        }) {
            return Err(format!("duplicate parameter assignment: {name:?}"));
        }
        let components = raw_value.split(',').collect::<Vec<_>>();
        let (scalar, point, color) = match components.as_slice() {
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
                let mut channels = [0u8; 4];
                for (channel, component) in channels.iter_mut().zip([alpha, red, green, blue]) {
                    let value = component.parse::<u16>().map_err(|error| {
                        format!("invalid ARGB8 component for {name:?}: {error}")
                    })?;
                    if value > u16::from(u8::MAX) {
                        return Err(format!(
                            "ARGB8 component for {name:?} must be between 0 and 255: {value}"
                        ));
                    }
                    *channel = value as u8;
                }
                (None, None, Some(channels))
            }
            _ => {
                return Err(format!(
                    "parameter assignment must contain one scalar, two point components, or four ARGB8 components: {value:?}"
                ));
            }
        };
        parsed.push(ParameterValue {
            name: parameter_name.to_string(),
            slot,
            value: scalar,
            point,
            color,
        });
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{parse_parameter_values, parse_trace_watches, selector_error};
    use std::ffi::OsString;

    #[test]
    fn traced_selector_nonzero_result_is_a_process_error() {
        assert_eq!(selector_error(0), None);
        assert_eq!(selector_error(7), Some(7));
        assert_eq!(selector_error(u64::MAX), Some(-1));
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
        assert_eq!(values[1].value, None);
        assert_eq!(values[1].point, Some([42.0, -7.25]));
        assert_eq!(values[2].slot, Some(4));
        assert_eq!(values[2].value, Some(100.0));
    }

    #[test]
    fn parameter_parser_accepts_strict_argb8_assignments() {
        let values = parse_parameter_values(&[OsString::from("Tint@5=255,1,2,3")]).unwrap();
        assert_eq!(values[0].slot, Some(5));
        assert_eq!(values[0].value, None);
        assert_eq!(values[0].point, None);
        assert_eq!(values[0].color, Some([255, 1, 2, 3]));
    }

    #[test]
    fn parameter_parser_rejects_out_of_range_argb8_components() {
        let error = parse_parameter_values(&[OsString::from("Tint@5=256,1,2,3")]).unwrap_err();
        assert!(error.contains("between 0 and 255"));
    }

    #[test]
    fn trace_watches_are_separated_from_parameter_assignments() {
        let mut values = vec![
            OsString::from("--watch=function=0xcce0,arg=rcx,size=16,when=entry+return"),
            OsString::from("Amount=2.5"),
            OsString::from("--watch"),
            OsString::from("rva=0x350b,register=r9,size=64"),
            OsString::from("--watch-output-pixel=92,841"),
        ];
        let (watches, output_pixel) = parse_trace_watches(&mut values).unwrap();
        assert_eq!(watches.len(), 2);
        assert_eq!(watches[0].function_rva, Some(0xcce0));
        assert_eq!(watches[0].register, "rcx");
        assert_eq!(watches[1].instruction_rva, Some(0x350b));
        assert_eq!(output_pixel, Some([92, 841]));
        assert_eq!(values, [OsString::from("Amount=2.5")]);
    }
}
