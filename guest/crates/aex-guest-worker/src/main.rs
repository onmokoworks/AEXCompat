use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use aex_guest_worker::classic::{ClassicHost, ParameterValue};
use aex_guest_worker::pe::PeImage;

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
    if ((command == "render-png" || command == "render-region-png" || command == "census-png")
        && (input.is_none() || output.is_none()))
        || (command != "render-png"
            && command != "render-region-png"
            && command != "census-png"
            && (input.is_some() || output.is_some() || !trailing_args.is_empty()))
    {
        usage();
        return ExitCode::from(2);
    }

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
            ),
            Some("render-region-png") => render_png(
                &image,
                input.as_deref().expect("validated input path"),
                output.as_deref().expect("validated output path"),
                &parse_parameter_values(&trailing_args)?,
                region,
                false,
            ),
            Some("census-png") => render_png(
                &image,
                input.as_deref().expect("validated input path"),
                output.as_deref().expect("validated output path"),
                &parse_parameter_values(&trailing_args)?,
                None,
                true,
            ),
            _ => Err(
                "command must be inspect, setup, render, render-png, render-region-png, or census-png".to_string(),
            ),
        });
    match result {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("aex_guest_error: {error}");
            ExitCode::from(1)
        }
    }
}

fn usage() {
    eprintln!("usage: aex-guest-worker <inspect|setup|render> <x64.aex>");
    eprintln!(
        "       aex-guest-worker render-png <x64.aex> <input.png> <output.png> [name=value ...]"
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
) -> Result<String, String> {
    let rgba = image::open(input)
        .map_err(|error| format!("decode input PNG: {error}"))?
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    let mut argb8 = Vec::with_capacity(rgba.as_raw().len());
    for pixel in rgba.as_raw().chunks_exact(4) {
        argb8.extend_from_slice(&[pixel[3], pixel[0], pixel[1], pixel[2]]);
    }
    let report = ClassicHost::new(image)
        .and_then(|mut host| match (region, census) {
            (None, true) => host.render_argb8_census(width, height, &argb8, parameter_values),
            (Some(region), _) => {
                host.render_argb8_region(width, height, &argb8, parameter_values, region)
            }
            (None, false) => host.render_argb8(width, height, &argb8, parameter_values),
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
    report_json["output_png"] = serde_json::json!(output);
    if let Some(object) = report_json.as_object_mut() {
        object.remove("argb8");
    }
    serde_json::to_string_pretty(&report_json).map_err(|error| error.to_string())
}

fn parse_parameter_values(values: &[std::ffi::OsString]) -> Result<Vec<ParameterValue>, String> {
    let mut parsed = Vec::with_capacity(values.len());
    for value in values {
        let value = value
            .to_str()
            .ok_or_else(|| "parameter assignment must be UTF-8".to_string())?;
        let (name, number) = value
            .split_once('=')
            .ok_or_else(|| format!("parameter assignment must be name=value: {value:?}"))?;
        if name.is_empty() {
            return Err("parameter name must not be empty".into());
        }
        if parsed
            .iter()
            .any(|existing: &ParameterValue| existing.name == name)
        {
            return Err(format!("duplicate parameter assignment: {name:?}"));
        }
        parsed.push(ParameterValue {
            name: name.to_string(),
            value: number
                .parse()
                .map_err(|error| format!("invalid value for {name:?}: {error}"))?,
        });
    }
    Ok(parsed)
}
