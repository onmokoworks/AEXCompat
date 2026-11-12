/// Finite independent-instance execution. Call `render` for every case in the
/// initial preflight phase, then `start` and replay. The caller owns the bounded
/// list and closes once after replay.
pub struct IndependentCaseBatch {
    session: Option<crate::render_session::RenderSession>,
    static_key: Option<Value>,
    preflight: bool,
    serial: u32,
    smart: bool,
    poisoned: bool,
}

impl Default for IndependentCaseBatch {
    fn default() -> Self {
        Self {
            session: None,
            static_key: None,
            preflight: true,
            serial: 0,
            smart: false,
            poisoned: false,
        }
    }
}

impl IndependentCaseBatch {
    pub fn start(&mut self) -> io::Result<()> {
        if !self.preflight || self.static_key.is_none() {
            return Err(invalid("batch must be preflighted exactly once"));
        }
        self.preflight = false;
        Ok(())
    }

    pub fn invalidated(&self) -> bool {
        self.poisoned
            || self
                .session
                .as_ref()
                .is_some_and(|session| session.invalidation().is_some())
    }

    pub fn render(
        &mut self,
        repository: &Path,
        plugin: &Path,
        hash: &str,
        input: &Path,
        output: &Path,
        parameters: &[InteractiveParameter],
        timing: RenderTiming,
        smart: bool,
        format: RenderPixelFormat,
        context: Option<&crate::render_request::HostContext>,
        dependencies: Vec<ApprovedImageArtifact>,
        deadline_ms: u64,
    ) -> io::Result<Value> {
        render_with_artifact_using(
            repository,
            "experimental",
            plugin,
            hash,
            deadline_ms,
            input,
            output,
            Some(encode_interactive_payload(parameters)?),
            Some(parameters),
            context,
            timing,
            smart,
            format,
            RenderGpuBackend::Cpu,
            None,
            None,
            None,
            None,
            dependencies,
            Vec::new(),
            None,
            false,
            None,
            None,
            Some(self),
        )
    }

    fn execute(&mut self, request: &SessionWrapperRequest<'_>) -> SessionWrapperOutcome {
        match self.execute_inner(request) {
            Ok(report) => SessionWrapperOutcome::Report(report),
            Err(error) => SessionWrapperOutcome::Failure(error),
        }
    }

    fn execute_inner(&mut self, r: &SessionWrapperRequest<'_>) -> io::Result<Value> {
        use crate::render_session::{FrameStatus, RenderSession, SessionOpenRequest};
        if self.invalidated() {
            return Err(invalid("independent worker is invalidated"));
        }
        let roots = in_place_session_search_dirs(
            r.plugin_path,
            r.dependencies,
            r.explicit_dependency_search_dirs,
        )?;
        let key = json!({
            "plugin": r.plugin_path, "route": r.smart, "depth": r.pixel_format.report_name(),
            "width": r.width, "height": r.height,
            "time_step": r.timing.time_step, "total_time": r.timing.total_time,
            "time_scale": r.timing.time_scale, "mask": r.mask_trailer,
            "spatial": r.spatial_trailer, "camera": r.camera_trailer,
            "environment": r.render_environment_trailer,
            "settings": r.conformance_render_settings, "roots": roots,
            "alpha": r.alpha_as_coverage_params,
            "layers": r.layers.iter().filter(|layer| layer.slot != 0).map(|layer| json!({
                "slot": layer.slot, "width": layer.width, "height": layer.height,
                "timed": layer.timed, "sha256": format!("{:x}", Sha256::digest(&layer.rgba)),
            })).collect::<Vec<_>>(),
        });
        if self
            .static_key
            .as_ref()
            .is_some_and(|expected| expected != &key)
        {
            return Err(invalid(
                "independent cases require identical static context and layer contents",
            ));
        }
        if r.aux_manifest.is_some() {
            return Err(invalid(
                "independent cases do not yet support auxiliary channel manifests",
            ));
        }
        self.static_key = Some(key);
        self.smart = r.smart;
        if self.preflight {
            return Ok(json!({"preflight": true}));
        }
        if self.session.is_none() {
            // A failed open/negotiation must never silently reopen for a later case.
            self.poisoned = true;
            let mut session = RenderSession::open(SessionOpenRequest {
                repository: r.repository,
                plugin_path: r.plugin_path,
                plugin_sha256: r.plugin_sha256,
                parameters: r.interactive_parameters,
                payload_override: r.payload_override,
                parameter_animation: None,
                aux_manifest: None,
                world_dump_dir: None,
                output_checksum_detail: false,
                layers: &r.layers,
                mask_trailer: r.mask_trailer.clone(),
                spatial_trailer: r.spatial_trailer.clone(),
                camera_trailer: r.camera_trailer.clone(),
                render_environment_trailer: r.render_environment_trailer.clone(),
                audio_trailer: None,
                alpha_as_coverage_params: r.alpha_as_coverage_params,
                conformance_render_settings: r.conformance_render_settings,
                dependencies: Vec::new(),
                companions: Vec::new(),
                dependency_search_dirs: roots,
                width: r.width,
                height: r.height,
                pixel_format: r.pixel_format,
                time_step: r.timing.time_step,
                total_time: r.timing.total_time,
                time_scale: r.timing.time_scale,
                frame_deadline: Duration::from_millis(r.timeout_ms),
                smart: r.smart,
                gpu_backend: RenderGpuBackend::Cpu,
                gpu_runtime_policy: None,
                launch_environment: Default::default(),
            })?;
            session.require_independent_cases()?;
            self.session = Some(session);
            self.poisoned = false;
        }
        let index = self.serial;
        self.serial += 1;
        self.poisoned = true;
        let outcome = self.session.as_mut().unwrap().render_case(
            index,
            r.timing.current_time,
            r.rgba,
            r.interactive_parameters.unwrap_or_default(),
        )?;
        self.poisoned = false;
        let worker_report = self
            .session
            .as_mut()
            .unwrap()
            .take_case_report()
            .unwrap_or(Value::Null);
        let performance = outcome.performance;
        let depth = outcome.depth_provenance;
        let mut failure = None;
        let (pixels, width, height, origin_x, origin_y) = match outcome.status {
            FrameStatus::Rendered {
                pixels,
                width,
                height,
                origin_x,
                origin_y,
            } => (pixels, width, height, origin_x, origin_y),
            FrameStatus::FrameError {
                render_error,
                missing_dependency,
                return_message,
                selector_crash,
            } => {
                failure = Some(
                    json!({"render_error":render_error,"missing_dependency":missing_dependency,
                    "return_message":return_message,"selector_crash":selector_crash}),
                );
                (Vec::new(), 0, 0, 0, 0)
            }
            FrameStatus::SmartOutputUntouched => {
                failure =
                    Some(json!({"render_error":-6,"host_failure_reason":"smart_output_untouched"}));
                (Vec::new(), 0, 0, 0, 0)
            }
        };
        let (output_origin_ok, parameter_count_ok, spatial_ok) = if failure.is_none() {
            // Same structural/pixel gate as the single-image route; global
            // lifecycle validation remains pending until the one final close.
            self.poisoned = true;
            let validated = validate_interactive_worker_report(
                &worker_report,
                &worker_report,
                &InteractiveGateFacts {
                    smart: r.smart,
                    pixel_format: r.pixel_format,
                    spatial: r.spatial,
                    expected_quality: r.expected_quality,
                    expected_field: r.expected_field,
                    expected_shutter_angle: r.expected_shutter_angle,
                    expected_shutter_phase: r.expected_shutter_phase,
                    custom_ui_action: None,
                    audio_present: false,
                    interactive_parameters: r.interactive_parameters,
                    classification: "ok",
                    time_step: r.timing.time_step,
                    input_width: r.width,
                    input_height: r.height,
                },
            )?;
            self.poisoned = false;
            validated
        } else {
            (false, false, false)
        };
        let empty = width == 0 && height == 0;
        if !empty {
            if let Some(parent) = r.output_path.parent() {
                fs::create_dir_all(parent)?;
            }
            if let Some(raw) = r.preserved_output {
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(raw)?
                    .write_all(&pixels)?;
            }
            let preview = native_rgba_to_preview(&pixels, r.pixel_format)?;
            let image = image::RgbaImage::from_raw(width, height, preview)
                .ok_or_else(|| invalid("independent output dimensions are invalid"))?;
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(r.output_path)?;
            let mut writer = std::io::BufWriter::new(file);
            image
                .write_to(&mut writer, ImageFormat::Png)
                .map_err(|error| invalid(error.to_string()))?;
            writer.flush()?;
        }
        let mut report = build_interactive_image_report(
            &worker_report,
            InteractiveImageReportFacts {
                plugin_id: r.plugin_id.into(),
                smart: r.smart,
                pixel_format: r.pixel_format,
                rendered_width: width,
                rendered_height: height,
                input_width: r.width,
                input_height: r.height,
                output_png: r.output_path.into(),
                timing: r.timing,
                worker_classification: "resident_session".into(),
                diagnostics: json!({"case_report":worker_report}),
                gpu_fallback_used: worker_report["gpu_fallback_used"]
                    .as_bool()
                    .unwrap_or(false),
                gpu_fallback_reason: worker_report["gpu_fallback_reason"]
                    .as_str()
                    .map(str::to_owned),
                gpu_attempt: worker_report
                    .get("gpu_attempt")
                    .filter(|value| !value.is_null())
                    .cloned(),
                secondary_layers: json!(r
                    .layers
                    .iter()
                    .filter(|layer| layer.slot != 0)
                    .map(
                        |layer| json!({"slot":layer.slot,"width":layer.width,"height":layer.height})
                    )
                    .collect::<Vec<_>>()),
                empty_smart_result: empty,
                output_raw: if empty {
                    None
                } else {
                    r.preserved_output.map(|p| p.to_string_lossy().into_owned())
                },
                deep_png_output: false,
                deep_overrange_samples: None,
                world_dump_display: None,
                minidump_display: None,
                output_checksum_detail: false,
                output_origin_ok,
                parameter_count_ok,
                spatial_ok,
                audio_input_sha256: None,
            },
        );
        if let Some(failure) = failure {
            report["passed"] = json!(false);
            report["guard_bytes_intact"] = worker_report["guard_bytes_intact"].clone();
            for (key, value) in failure.as_object().unwrap() {
                report[key] = value.clone();
            }
        }
        report["worker_pid"] = worker_report["worker_pid"].clone();
        report["worker_sha256"] = json!(
            self.session
                .as_ref()
                .and_then(|session| session.worker_sha256())
        );
        report["session_generation"] = json!(1);
        report["lifecycle_telemetry_scope"] =
            json!("process_cumulative_global_setup_pending_close");
        report["case_cleanup"] = json!(true);
        report["visual_diagnostics"] = visual_diagnostics::inspect(
            Some(r.rgba),
            (r.width, r.height),
            &pixels,
            (width, height),
            r.pixel_format,
        );
        report["evidence_provisional"] = json!(true);
        report["origin"] = json!({"x":origin_x,"y":origin_y});
        report["depth_provenance"] = json!(depth);
        report["performance_diagnostics"] = json!(performance);
        report["output_pixels_sha256"] = json!(format!("{:x}", Sha256::digest(&pixels)));
        report["input_pixel_format"] = json!("argb8");
        report["primary_input_temporal_model"] = json!("static");
        report["outcome"] = json!(if report["passed"] == true {
            "rendered"
        } else {
            "render_error"
        });
        Ok(report)
    }

    pub fn close(mut self) -> Value {
        match self.session.take() {
            Some(session) => {
                let mut close = session.close();
                close["independent_cases_clean"] = json!(
                    !self.poisoned
                        && crate::render_session::validate_close_report(&close, self.smart).is_ok()
                );
                close
            }
            None => json!({"independent_cases_clean":false,"reason":"session_not_started"}),
        }
    }
}
