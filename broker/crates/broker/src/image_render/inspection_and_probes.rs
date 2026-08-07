fn inspect_experimental_with_diagnostics_and_runtime_policy(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    dependencies: Vec<ApprovedImageArtifact>,
    runtime_policy: Option<(&RuntimeModulePolicy, RuntimeBackend)>,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    inspect_experimental_impl(
        repository,
        plugin_path,
        approved_sha256,
        dependencies,
        Vec::new(),
        runtime_policy,
    )
}

#[allow(clippy::too_many_arguments)]
fn inspect_experimental_impl(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    mut dependencies: Vec<ApprovedImageArtifact>,
    dependency_search_dirs: Vec<std::path::PathBuf>,
    runtime_policy: Option<(&RuntimeModulePolicy, RuntimeBackend)>,
) -> io::Result<(Vec<InteractiveParameter>, Value)> {
    // In-place inspection (issue #751): the loader resolves the closure, so
    // staged dependencies and resources cannot ride the same launch. Runtime
    // policy inspection remains outside #815's GPU render-session migration.
    if !dependency_search_dirs.is_empty() && (!dependencies.is_empty() || runtime_policy.is_some())
    {
        return Err(invalid(
            "in-place inspection cannot combine approved dependencies or a runtime policy",
        ));
    }
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--l2-params-only".into()];
    let mut args_after_plugin = vec![actual.to_ascii_lowercase()];
    let authorization = runtime_policy
        .map(|(policy, backend)| {
            prepare_runtime_authorization_transport(repository, policy, backend)
        })
        .transpose()?;
    if let Some(authorization) = &authorization {
        authorization.append_launch(false, &mut args_after_plugin, &mut dependencies);
    }
    let started = Instant::now();
    // Parameter inspection runs with no deadline (issue #354). A watchdog here
    // contains nothing the job object does not already contain, and it decides
    // discovery results by wall-clock: a plug-in still mapping its sealed
    // closure was reported as "timed out", and that verdict was then cached.
    // Containment stays — the job object kills the tree when the launch handle
    // drops, and the sealed root is still torn down.
    let isolated = if !dependencies.is_empty() || !dependency_search_dirs.is_empty() {
        crate::secure_image_dispatch::dispatch_secure_image(SecureImageDispatch {
            repository,
            worker_kind: WorkerKind::L2,
            plugin: ApprovedImageArtifact {
                path: plugin_path.to_path_buf(),
                expected_sha256: decode_sha256_hex(approved_sha256)?,
                expected_size: fs::metadata(plugin_path)?.len(),
            },
            dependencies,
            dependency_search_dirs,
            args_before_plugin: &args_before_plugin,
            args_after_plugin: &args_after_plugin,
            timeout: None,
            launch_environment: Default::default(),
        })?
    } else {
        dispatch_approved_image(
            repository,
            WorkerKind::L2,
            plugin_path,
            approved_sha256,
            &args_before_plugin,
            &args_after_plugin,
            None,
        )?
    };
    let mut diagnostics = isolated_worker_diagnostics(&isolated, started.elapsed().as_millis());
    let worker_report: Option<Value> = serde_json::from_str(isolated.stdout.trim()).ok();
    if let Some(report) = &worker_report {
        propagate_missing_suites(&mut diagnostics, report);
        propagate_unsupported_suite_calls(&mut diagnostics, report);
        propagate_suite_call_slot_probe(&mut diagnostics, report);
        propagate_selector_invocations(&mut diagnostics, report);
        propagate_host_callback_timeline(&mut diagnostics, report);
        propagate_compute_cache_timeline(&mut diagnostics, report);
        propagate_extended_lookup_timeline(&mut diagnostics, report);
        propagate_extended_allocation_timeline(&mut diagnostics, report);
        propagate_suite_timeline(&mut diagnostics, report);
        let selector_phase = diagnostics
            .get("failure_stage")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let Some(summary) = module_audit_failure_summary(report, selector_phase.as_deref()) {
            diagnostics["module_audit_failure"] = summary;
        }
    }
    if isolated.classification.as_str() != "ok" {
        if diagnostics.get("module_audit_failure").is_none()
            && let Some(summary) = failed_module_audit_summary(&isolated.stdout)
        {
            diagnostics["module_audit_failure"] = summary;
        }
        return Err(invalid(format!(
            "AEX parameter inspection worker failed safely: {diagnostics}"
        )));
    }
    let report = worker_report.ok_or_else(|| invalid("inspection worker report is invalid"))?;
    if let Some(summary) = report.get("module_audit").and_then(module_audit_summary) {
        diagnostics["module_audit"] = summary;
    }
    let advertised_out_flags = report
        .get("out_flags")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("inspection report has no valid out_flags"))?;
    let advertised_out_flags2 = report
        .get("out_flags2")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("inspection report has no valid out_flags2"))?;
    let audio_effect_only = advertised_out_flags & (1_u64 << 31) != 0;
    diagnostics["advertised_out_flags"] = json!(advertised_out_flags);
    diagnostics["advertised_out_flags2"] = json!(advertised_out_flags2);
    diagnostics["smart_render_advertised"] = json!(smart_render_advertised(advertised_out_flags2));
    diagnostics["audio_effect_only"] = json!(audio_effect_only);
    diagnostics["image_render_supported"] = json!(!audio_effect_only);
    diagnostics["runtime_module_policy_applied"] = json!(runtime_policy.is_some());
    if report.get("params_setup_error") != Some(&json!(0)) {
        return Err(invalid("AEX rejected PF_PARAMS_SETUP"));
    }
    let rows = report
        .get("parameters")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("inspection report has no parameters"))?;
    let mut parameters = Vec::new();
    let mut parameter_metadata = Vec::new();
    let custom_ui_events = report
        .get("custom_ui")
        .and_then(|value| value.get("events"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    for row in rows {
        let observed_type = row
            .get("type")
            .and_then(Value::as_i64)
            .ok_or_else(|| invalid("inspection parameter has no numeric type"))?;
        let observed_index = row
            .get("index")
            .and_then(Value::as_u64)
            .filter(|value| *value <= u64::from(u16::MAX))
            .ok_or_else(|| invalid("inspection parameter has no bounded index"))?;
        let default = row.get("default").and_then(Value::as_f64).unwrap_or(0.0);
        let ui_flags = row.get("ui_flags").and_then(Value::as_u64).unwrap_or(0);
        let default_color = row.get("default_color");
        let channel = |name: &str| {
            default_color
                .and_then(|value| value.get(name))
                .and_then(Value::as_u64)
                .unwrap_or(if name == "alpha" { 255 } else { 0 }) as u8
        };
        let known_metadata_kind = match observed_type {
            0 => "layer",
            1 => "slider",
            2 => "fixed_slider",
            3 => "angle",
            4 => "checkbox",
            5 => "color",
            6 => "point",
            7 => "popup",
            8 => "custom",
            9 => "no_data",
            10 => "float_slider",
            11 => "arbitrary_data",
            12 => "path",
            13 => "group_start",
            14 => "group_end",
            15 => "button",
            18 => "point3d",
            16 => "reserved16",
            17 => "reserved17",
            _ => "",
        };
        let metadata_kind = if known_metadata_kind.is_empty() {
            format!("unknown_{observed_type}")
        } else {
            known_metadata_kind.to_owned()
        };
        let runtime_kind = match observed_type {
            0 => "layer",
            3 => "angle",
            5 => "color",
            6 => "point",
            2 | 10 => "float",
            8 => "custom",
            9 => "no_data",
            11 => "arbitrary_data",
            12 => "path",
            13 => "group_start",
            14 => "group_end",
            15 => "button",
            18 => "point3d",
            _ => "integer",
        };
        let host_minimum = if observed_type == 12 {
            0.0
        } else {
            row.get("valid_min")
                .and_then(Value::as_f64)
                .unwrap_or(default)
        };
        let host_maximum = if observed_type == 12 {
            1024.0
        } else {
            row.get("valid_max")
                .and_then(Value::as_f64)
                .unwrap_or(default)
        };
        let observed_host_range = row
            .get("valid_min")
            .and_then(Value::as_f64)
            .zip(row.get("valid_max").and_then(Value::as_f64));
        let observed_user_range = row
            .get("slider_min")
            .and_then(Value::as_f64)
            .zip(row.get("slider_max").and_then(Value::as_f64));
        let component_count = match observed_type {
            3 => 1,
            6 => 2,
            18 => 3,
            _ => 0,
        };
        let initial_value = if let Some(value) = row.get("default").and_then(Value::as_f64) {
            json!(value)
        } else if observed_type == 5 {
            let color = row.get("default_color");
            ["alpha", "red", "green", "blue"]
                .iter()
                .map(|name| {
                    color
                        .and_then(|value| value.get(name))
                        .and_then(Value::as_u64)
                })
                .collect::<Option<Vec<_>>>()
                .map(Value::from)
                .unwrap_or(Value::Null)
        } else if component_count > 0 {
            row.get("default_components")
                .and_then(Value::as_array)
                .filter(|values| values.len() >= component_count)
                .and_then(|values| {
                    values
                        .iter()
                        .take(component_count)
                        .map(Value::as_f64)
                        .collect::<Option<Vec<_>>>()
                })
                .map(Value::from)
                .unwrap_or(Value::Null)
        } else if observed_type == 0 {
            row.get("layer_default").cloned().unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        parameter_metadata.push(json!({
            "index": observed_index,
            "type": metadata_kind,
            "initial_value": initial_value,
            "host_range": observed_host_range.map(|(minimum, maximum)| json!({"minimum": minimum, "maximum": maximum})),
            "user_range": observed_user_range.map(|(minimum, maximum)| json!({"minimum": minimum, "maximum": maximum}))
        }));
        if !matches!(
            observed_type,
            0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 | 18
        ) {
            continue;
        }
        parameters.push(InteractiveParameter {
            slot: observed_index as u32,
            name: row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Parameter")
                .to_owned(),
            kind: runtime_kind.into(),
            minimum: host_minimum,
            maximum: host_maximum,
            value: default,
            choices: row
                .get("choices")
                .and_then(Value::as_str)
                .map(|text| text.split('|').map(str::to_owned).collect())
                .unwrap_or_default(),
            color: [
                channel("alpha"),
                channel("red"),
                channel("green"),
                channel("blue"),
            ],
            components: {
                let mut result = [0.0; 3];
                if let Some(values) = row.get("default_components").and_then(Value::as_array) {
                    for (index, value) in values.iter().take(3).enumerate() {
                        result[index] = value.as_f64().unwrap_or(0.0);
                    }
                }
                result
            },
            component_count,
            layer_path: None,
            enabled: ui_flags & (1 << 5) == 0,
            visible: ui_flags & (1 << 9) == 0,
            supervised: row.get("flags").and_then(Value::as_u64).unwrap_or(0) & (1 << 6) != 0,
            debug_summary: row
                .get("arbitrary_summary")
                .and_then(Value::as_str)
                .map(str::to_owned),
            custom_ui_events,
            control_size: [
                row.get("ui_width").and_then(Value::as_u64).unwrap_or(0) as u16,
                row.get("ui_height").and_then(Value::as_u64).unwrap_or(0) as u16,
            ],
        });
    }
    diagnostics["parameter_metadata"] = Value::Array(parameter_metadata);
    Ok((parameters, diagnostics))
}

pub fn probe_experimental_custom_ui_cursor(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--l2-adjust-cursor".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI cursor worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI cursor report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("cursor") != Some(&json!(13))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI cursor contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_draw(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--l2-draw-event".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI draw worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI draw report is invalid"))?;
    let command_count = report
        .get("drawbot_paint_rect_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + report
            .get("drawbot_fill_path_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
        + report
            .get("drawbot_stroke_path_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
        + report
            .get("overlay_stroke_path_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0);
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || command_count == 0
        || report.get("drawbot_objects_created") != report.get("drawbot_objects_released")
        || report.get("drawbot_invalid_operations") != Some(&json!(0))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI draw contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_lifecycle(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--l2-ui-lifecycle".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI lifecycle worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI lifecycle report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("ui_lifecycle"))
        || report.get("lifecycle_errors") != Some(&json!([0, 0, 0, 0, -1]))
        || report.get("lifecycle_context_stable") != Some(&json!(true))
        || report.get("lifecycle_host_state_cleared") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI lifecycle contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_idle(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--l2-ui-idle".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI idle worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI idle report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("ui_idle"))
        || report.get("lifecycle_errors") != Some(&json!([0, 0, 0, 0, 0]))
        || report.get("lifecycle_context_stable") != Some(&json!(true))
        || report.get("lifecycle_host_state_cleared") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI idle contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_keydown(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    point: [u16; 2],
    keycode: u32,
    modifiers: u16,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    if keycode & 0x3fff_0000 != 0 {
        return Err(invalid("custom UI keycode contains unsupported bits"));
    }
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--l2-ui-keydown".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        format!("{},{},{},{}", point[0], point[1], keycode, modifiers),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI keydown worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI keydown report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("ui_keydown"))
        || report.get("lifecycle_errors") != Some(&json!([0, 0, 0, 0, 0]))
        || report.get("keydown_code") != Some(&json!(keycode))
        || report.get("keydown_modifiers") != Some(&json!(modifiers))
        || report.get("lifecycle_context_stable") != Some(&json!(true))
        || report.get("lifecycle_host_state_cleared") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI keydown contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_mouse_exited(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--l2-ui-mouse-exited".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI mouse-exited worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI mouse-exited report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("ui_mouse_exited"))
        || !matches!(
            report.get("event_target").and_then(Value::as_str),
            Some("layer" | "comp")
        )
        || report.get("lifecycle_errors") != Some(&json!([0, 0, 0, 0, 0]))
        || report.get("lifecycle_context_stable") != Some(&json!(true))
        || report.get("lifecycle_host_state_cleared") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI mouse-exited contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_click(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    point: [u16; 2],
    color: [f32; 4],
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    if color
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err(invalid("custom UI click color is invalid"));
    }
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let payload = format!(
        "{},{},{},{},{},{}",
        point[0], point[1], color[0], color[1], color[2], color[3]
    );
    let args_before_plugin = vec!["--l2-click-event".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        payload,
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI click worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI click report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("do_click"))
        || report
            .get("event_out_flags")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            & 9
            != 9
        || report.get("app_color_picker_calls") != Some(&json!(1))
        || report.get("app_invalidate_rect_calls") != Some(&json!(1))
        || report.get("changed_value") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI click contract failed"));
    }
    Ok(report)
}

pub fn probe_experimental_custom_ui_drag(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    start: [u16; 2],
    end: [u16; 2],
    steps: u8,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    if steps == 0 || steps > 32 {
        return Err(invalid("custom UI drag step count is invalid"));
    }
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--l2-drag-event".into()];
    let args_after_plugin = vec![
        actual.to_ascii_lowercase(),
        format!("{},{},{},{},{}", start[0], start[1], end[0], end[1], steps),
        encode_interactive_payload(parameters)?,
    ];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(8_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid("custom UI drag worker failed safely"));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("custom UI drag report is invalid"))?;
    if report.get("status") != Some(&json!("event_completed"))
        || report.get("event_assignments_applied") != Some(&json!(true))
        || report.get("event_type") != Some(&json!("drag_sequence"))
        || report.get("drag_requested") != Some(&json!(true))
        || report.get("drag_calls") != Some(&json!(steps))
        || report.get("drag_terminated") != Some(&json!(true))
        || report.get("handle_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("custom UI drag contract failed"));
    }
    Ok(report)
}

pub fn trigger_experimental_button(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
    slot: u32,
    parameters: &[InteractiveParameter],
) -> io::Result<Value> {
    if slot == 0 || slot > MAX_PARAMETERS {
        return Err(invalid("button parameter slot is invalid"));
    }
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    if !parameters
        .iter()
        .any(|parameter| parameter.slot == slot && parameter.supervised)
    {
        return Err(invalid("parameter is not supervised by the AEX"));
    }
    let payload = encode_interactive_payload(parameters)?;
    let args_before_plugin = vec!["--user-changed".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase(), slot.to_string(), payload];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEX button worker failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("button worker report is invalid"))?;
    if report.get("user_changed_param_requested") != Some(&json!(true))
        || report.get("user_changed_param_slot") != Some(&json!(slot))
        || report.get("user_changed_param_error") != Some(&json!(0))
    {
        return Err(invalid("AEX rejected PF_Cmd_USER_CHANGED_PARAM"));
    }
    Ok(report)
}

pub fn initialize_experimental_aegp(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--aegp-init".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP initialization failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP worker report is invalid"))?;
    if report.get("stage") != Some(&json!("aegp_init"))
        || report.get("init_error") != Some(&json!(0))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP initialization contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_update_menu(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--aegp-update-menu".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP update-menu event failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP event worker report is invalid"))?;
    if report.get("event_requested") != Some(&json!("update_menu"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("hooks_invoked").and_then(Value::as_u64) == Some(0)
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP update-menu event contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_idle(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--aegp-idle".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP idle event failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP idle worker report is invalid"))?;
    if report.get("event_requested") != Some(&json!("idle"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("hooks_invoked").and_then(Value::as_u64) == Some(0)
        || report
            .get("idle_max_sleep")
            .and_then(Value::as_i64)
            .unwrap_or(-1)
            < 0
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP idle event contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_command_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--aegp-command-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP command roundtrip failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP command worker report is invalid"))?;
    if report.get("event_requested") != Some(&json!("command_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("command_hooks_invoked").and_then(Value::as_u64) != Some(2)
        || report.get("command_handled_count").and_then(Value::as_u64) != Some(2)
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP command roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_active_idle_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--aegp-active-idle-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP active idle roundtrip failed safely: {}",
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP active idle report is invalid"))?;
    if report.get("event_requested") != Some(&json!("active_idle_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("command_hooks_invoked").and_then(Value::as_u64) != Some(2)
        || report.get("command_handled_count").and_then(Value::as_u64) != Some(2)
        || report.get("hooks_invoked").and_then(Value::as_u64) != Some(1)
        || report.get("idle_max_sleep").and_then(Value::as_i64) != Some(33)
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP active idle roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_comp_idle_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--aegp-comp-idle-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(5_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP comp idle roundtrip failed safely ({}, exit {}): {}",
            isolated.classification.as_str(),
            isolated.exit_code,
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP comp idle report is invalid"))?;
    let effect_count_calls = report
        .get("effect_count_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let effect_contract_valid = effect_count_calls == 0
        || (report
            .get("effect_acquires")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
            && report.get("effect_acquires") == report.get("effect_disposes")
            && report
                .get("effect_metadata_calls")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                >= 4);
    let stream_acquires = report
        .get("stream_acquires")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let scene_layer_count = report
        .get("scene_layer_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let idle_ticks = 3;
    let effect_param_value_calls = report
        .get("effect_param_value_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let expected_streams = if stream_acquires > 0 {
        idle_ticks * scene_layer_count * 7 + effect_param_value_calls
    } else {
        0
    };
    let layer_name_calls = report
        .get("layer_name_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let item_name_calls = report
        .get("item_name_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let effect_param_name_calls = report
        .get("effect_param_name_calls")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let item_metadata_contract_valid = item_name_calls == 0
        || (item_name_calls == idle_ticks
            && report.get("item_duration_calls").and_then(Value::as_u64) == Some(idle_ticks));
    let layer_name_contract_valid = layer_name_calls == 0
        || (layer_name_calls == idle_ticks * scene_layer_count
            && item_name_calls == idle_ticks
            && report.get("aegp_memory_created").and_then(Value::as_u64)
                == Some(layer_name_calls * 2 + item_name_calls + effect_param_name_calls)
            && report.get("aegp_memory_freed") == report.get("aegp_memory_created"));
    let stream_contract_valid = stream_acquires == expected_streams
        && (stream_acquires == 0
            || (effect_param_value_calls > 0
                && effect_param_value_calls <= 31
                && effect_param_name_calls == effect_param_value_calls
                && report.get("stream_acquires") == report.get("stream_disposes")
                && report.get("stream_value_acquires") == report.get("stream_value_disposes")
                && report.get("stream_value_acquires").and_then(Value::as_u64)
                    == Some(expected_streams)
                && report.get("keyframe_count_calls").and_then(Value::as_u64)
                    == Some(expected_streams)
                && report
                    .get("keyframed_stream_reports")
                    .and_then(Value::as_u64)
                    == Some(1)
                && report
                    .get("stream_sampled_selector_mask")
                    .and_then(Value::as_u64)
                    == Some(799)));
    if report.get("event_requested") != Some(&json!("comp_idle_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("command_hooks_invoked").and_then(Value::as_u64) != Some(2)
        || report.get("command_handled_count").and_then(Value::as_u64) != Some(2)
        || report.get("menu_hooks_invoked").and_then(Value::as_u64) != Some(4)
        || report.get("command_enable_calls").and_then(Value::as_u64) != Some(4)
        || report.get("command_check_calls").and_then(Value::as_u64) != Some(4)
        || report
            .get("command_checked_true_calls")
            .and_then(Value::as_u64)
            != Some(3)
        || report
            .get("command_checked_false_calls")
            .and_then(Value::as_u64)
            != Some(1)
        || report.get("hooks_invoked").and_then(Value::as_u64) != Some(idle_ticks)
        || scene_layer_count != 3
        || report
            .get("scene_selected_layer_count")
            .and_then(Value::as_u64)
            != Some(2)
        || report.get("idle_max_sleep").and_then(Value::as_i64) != Some(33)
        || report
            .get("scene_first_observed_frame")
            .and_then(Value::as_i64)
            != Some(1)
        || report
            .get("scene_last_observed_frame")
            .and_then(Value::as_i64)
            != Some(3)
        || report
            .get("item_current_time_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            == 0
        || report
            .get("comp_from_item_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            == 0
        || report
            .get("comp_framerate_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            == 0
        || (report
            .get("layer_count_calls")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
            && report
                .get("layer_attribute_calls")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                < idle_ticks * scene_layer_count * 4)
        || report
            .get("suite_acquires")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            < 4
        || report.get("suite_leases_balanced") != Some(&json!(true))
        || report.get("effect_lifetimes_balanced") != Some(&json!(true))
        || !effect_contract_valid
        || report.get("stream_lifetimes_balanced") != Some(&json!(true))
        || report.get("aegp_memory_lifetimes_balanced") != Some(&json!(true))
        || !layer_name_contract_valid
        || !item_metadata_contract_valid
        || report.get("collection_lifetimes_balanced") != Some(&json!(true))
        || (report
            .get("collection_creates")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
            && (report.get("collection_creates") != report.get("collection_disposes")
                || report.get("collection_creates").and_then(Value::as_u64) != Some(idle_ticks)
                || report.get("collection_item_reads").and_then(Value::as_u64)
                    != Some(idle_ticks * 2)))
        || !stream_contract_valid
    {
        return Err(invalid("AEGP comp idle roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_keyframe_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--aegp-keyframe-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(8_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP keyframe roundtrip failed safely ({}, exit {}): {}",
            isolated.classification.as_str(),
            isolated.exit_code,
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP keyframe roundtrip report is invalid"))?;
    if report.get("event_requested") != Some(&json!("keyframe_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("keyframe_pipe_connected") != Some(&json!(true))
        || report.get("keyframe_pipe_request_sent") != Some(&json!(true))
        || report.get("keyframe_pipe_response_received") != Some(&json!(true))
        || report.get("keyframe_pipe_response_valid") != Some(&json!(true))
        || report
            .get("keyframe_pipe_response_bytes")
            .and_then(Value::as_u64)
            != Some(192)
        || report.get("keyframe_time_calls").and_then(Value::as_u64) != Some(2)
        || report.get("keyframe_value_calls").and_then(Value::as_u64) != Some(2)
        || report
            .get("keyframe_interpolation_calls")
            .and_then(Value::as_u64)
            != Some(2)
        || report
            .get("keyframed_stream_reports")
            .and_then(Value::as_u64)
            != Some(2)
        || report.get("stream_acquires").and_then(Value::as_u64) != Some(71)
        || report.get("stream_disposes") != report.get("stream_acquires")
        || report.get("stream_value_acquires").and_then(Value::as_u64) != Some(69)
        || report.get("stream_value_disposes") != report.get("stream_value_acquires")
        || report.get("aegp_memory_created").and_then(Value::as_u64) != Some(26)
        || report.get("aegp_memory_freed") != report.get("aegp_memory_created")
        || report.get("effect_lifetimes_balanced") != Some(&json!(true))
        || report.get("stream_lifetimes_balanced") != Some(&json!(true))
        || report.get("collection_lifetimes_balanced") != Some(&json!(true))
        || report.get("aegp_memory_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP keyframe roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_seek_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--aegp-seek-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(8_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP seek roundtrip failed safely ({}, exit {}): {}",
            isolated.classification.as_str(),
            isolated.exit_code,
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP seek roundtrip report is invalid"))?;
    if report.get("event_requested") != Some(&json!("seek_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("seek_pipe_connected") != Some(&json!(true))
        || report.get("seek_pipe_request_sent") != Some(&json!(true))
        || report.get("seek_pipe_ack_received") != Some(&json!(true))
        || report.get("seek_pipe_ack_valid") != Some(&json!(true))
        || report
            .get("item_set_current_time_calls")
            .and_then(Value::as_u64)
            != Some(1)
        || report
            .get("item_last_set_time_value")
            .and_then(Value::as_i64)
            != Some(75)
        || report
            .get("item_last_set_time_scale")
            .and_then(Value::as_u64)
            != Some(30)
        || report.get("scene_current_frame").and_then(Value::as_i64) != Some(75)
        || report.get("effect_lifetimes_balanced") != Some(&json!(true))
        || report.get("stream_lifetimes_balanced") != Some(&json!(true))
        || report.get("collection_lifetimes_balanced") != Some(&json!(true))
        || report.get("aegp_memory_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP seek roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_trim_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--aegp-trim-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(8_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP trim roundtrip failed safely ({}, exit {}): {}",
            isolated.classification.as_str(),
            isolated.exit_code,
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP trim roundtrip report is invalid"))?;
    if report.get("event_requested") != Some(&json!("trim_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("trim_pipe_connected") != Some(&json!(true))
        || report.get("trim_pipe_request_sent") != Some(&json!(true))
        || report.get("trim_pipe_ack_received") != Some(&json!(true))
        || report.get("trim_pipe_ack_valid") != Some(&json!(true))
        || report.get("layer_trim_set_calls").and_then(Value::as_u64) != Some(1)
        || report.get("layer_1_in_point_value").and_then(Value::as_i64) != Some(30)
        || report.get("layer_1_in_point_scale").and_then(Value::as_u64) != Some(30)
        || report.get("layer_1_duration_value").and_then(Value::as_i64) != Some(210)
        || report.get("layer_1_duration_scale").and_then(Value::as_u64) != Some(30)
        || report.get("effect_lifetimes_balanced") != Some(&json!(true))
        || report.get("stream_lifetimes_balanced") != Some(&json!(true))
        || report.get("collection_lifetimes_balanced") != Some(&json!(true))
        || report.get("aegp_memory_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP trim roundtrip contract failed"));
    }
    Ok(report)
}

pub fn dispatch_experimental_aegp_switch_roundtrip(
    repository: &Path,
    plugin_path: &Path,
    approved_sha256: &str,
) -> io::Result<Value> {
    let actual = observe_selected_plugin(plugin_path, approved_sha256)?;
    let args_before_plugin = vec!["--aegp-switch-roundtrip".into()];
    let args_after_plugin = vec![actual.to_ascii_lowercase()];
    let isolated = dispatch_approved_image(
        repository,
        WorkerKind::L2,
        plugin_path,
        approved_sha256,
        &args_before_plugin,
        &args_after_plugin,
        Some(Duration::from_millis(8_000)),
    )?;
    if isolated.classification.as_str() != "ok" {
        return Err(invalid(format!(
            "AEGP switch roundtrip failed safely ({}, exit {}): {}",
            isolated.classification.as_str(),
            isolated.exit_code,
            isolated.stderr.trim()
        )));
    }
    let report: Value = serde_json::from_str(isolated.stdout.trim())
        .map_err(|_| invalid("AEGP switch roundtrip report is invalid"))?;
    if report.get("event_requested") != Some(&json!("switch_roundtrip"))
        || report.get("event_error") != Some(&json!(0))
        || report.get("switch_pipe_connected") != Some(&json!(true))
        || report.get("switch_pipe_request_sent") != Some(&json!(true))
        || report.get("switch_pipe_ack_received") != Some(&json!(true))
        || report.get("switch_pipe_ack_valid") != Some(&json!(true))
        || report.get("layer_flag_set_calls").and_then(Value::as_u64) != Some(4)
        || report.get("layer_1_flags").and_then(Value::as_u64) != Some(0x4026)
        || report.get("layer_2_flags").and_then(Value::as_u64) != Some(0x5)
        || report.get("layer_3_flags").and_then(Value::as_u64) != Some(0x5)
        || report.get("effect_lifetimes_balanced") != Some(&json!(true))
        || report.get("stream_lifetimes_balanced") != Some(&json!(true))
        || report.get("collection_lifetimes_balanced") != Some(&json!(true))
        || report.get("aegp_memory_lifetimes_balanced") != Some(&json!(true))
        || report.get("suite_leases_balanced") != Some(&json!(true))
    {
        return Err(invalid("AEGP switch roundtrip contract failed"));
    }
    Ok(report)
}

/// Encodes interactive parameters into the worker payload transport form
/// (`v2|`..`v5|`). Public so integration tests can compute the exact payload
/// a session frame carries; production callers stay inside the crate.
pub fn encode_interactive_payload(parameters: &[InteractiveParameter]) -> io::Result<String> {
    let parameters = parameters
        .iter()
        .filter(|item| {
            !matches!(
                item.kind.as_str(),
                "layer" | "group_start" | "group_end" | "button" | "custom" | "no_data"
            )
        })
        .collect::<Vec<_>>();
    let mut payload = if parameters.iter().any(|item| item.kind == "arbitrary_data") {
        "v5|".to_owned()
    } else if parameters
        .iter()
        .any(|item| matches!(item.kind.as_str(), "angle" | "point" | "point3d"))
    {
        "v4|".to_owned()
    } else if parameters.iter().any(|item| item.kind == "color") {
        "v3|".to_owned()
    } else {
        "v2|".to_owned()
    };
    for (index, item) in parameters.iter().enumerate() {
        if item.slot == 0
            || item.slot > MAX_PARAMETERS
            || !item.value.is_finite()
            || item.value < item.minimum
            || item.value > item.maximum
        {
            return Err(invalid("interactive parameter is out of range"));
        }
        if index != 0 {
            payload.push(';');
        }
        let id = format!("param_{}", item.slot);
        match item.kind.as_str() {
            "integer" | "path" if item.value.fract() == 0.0 => {
                payload.push_str(&format!("{id}@{}:i32={}", item.slot, item.value as i64))
            }
            "float" => payload.push_str(&format!("{id}@{}:f64={}", item.slot, item.value)),
            "color" => payload.push_str(&format!(
                "{id}@{}:argb8={},{},{},{}",
                item.slot, item.color[0], item.color[1], item.color[2], item.color[3]
            )),
            "angle" | "point" | "point3d" => {
                let expected = match item.kind.as_str() {
                    "angle" => 1,
                    "point" => 2,
                    _ => 3,
                };
                if item.component_count != expected
                    || item.components[..expected]
                        .iter()
                        .any(|value| !value.is_finite() || *value < -32768.0 || *value > 32768.0)
                {
                    return Err(invalid("interactive component parameter is invalid"));
                }
                let values = item.components[..expected]
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                payload.push_str(&format!("{id}@{}:{}={values}", item.slot, item.kind));
            }
            "arbitrary_data" => {
                let text = item
                    .debug_summary
                    .as_deref()
                    .ok_or_else(|| invalid("arbitrary parameter has no printable text"))?;
                if text.is_empty() || text.len() > 4096 || text.as_bytes().contains(&0) {
                    return Err(invalid("arbitrary parameter text is invalid"));
                }
                let encoded = text
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                payload.push_str(&format!("{id}@{}:arbhex={encoded}", item.slot));
            }
            _ => return Err(invalid("unsupported interactive parameter kind")),
        }
    }
    if payload.len() > 16384 {
        return Err(invalid("interactive parameter payload is too large"));
    }
    Ok(payload)
}
