#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IndependentBatchManifest {
    schema: String,
    schema_version: u32,
    plugin: PathBuf,
    route: String,
    isolation: String,
    frame_deadline_ms: u64,
    cases: Vec<IndependentBatchEntry>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IndependentBatchEntry {
    id: String,
    input: PathBuf,
    request: PathBuf,
    output: PathBuf,
}

fn read_batch_document(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("JSON document exceeds size limit".into());
    }
    Ok(bytes)
}

fn parse_batch_manifest(bytes: &[u8]) -> Result<IndependentBatchManifest, String> {
    if bytes.len() > 1024 * 1024 {
        return Err("manifest exceeds 1 MiB".into());
    }
    let manifest: IndependentBatchManifest =
        serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if manifest.schema != "aexcompat.independent_case_batch"
        || manifest.schema_version != 1
        || manifest.isolation != "fresh_sequence"
        || manifest.cases.is_empty()
        || manifest.cases.len() > 128
        || !(1..=600_000).contains(&manifest.frame_deadline_ms)
        || !matches!(
            manifest.route.as_str(),
            "smart8" | "smart16" | "smart32" | "classic8" | "classic16" | "classic32"
        )
    {
        return Err("unsupported batch schema, isolation, route, count or deadline".into());
    }
    let mut ids = std::collections::HashSet::new();
    if !manifest.plugin.is_absolute() {
        return Err("plugin path must be absolute".into());
    }
    for case in &manifest.cases {
        if case.id.is_empty()
            || case.id.len() > 240
            || case.id.chars().any(char::is_control)
            || !ids.insert(&case.id)
        {
            return Err("case IDs must be nonempty, bounded and unique".into());
        }
        if [&case.input, &case.request, &case.output]
            .iter()
            .any(|path| !path.is_absolute())
        {
            return Err("case paths must be absolute".into());
        }
    }
    Ok(manifest)
}

fn batch_output_identity(path: &Path) -> Result<String, String> {
    let parent = path
        .parent()
        .ok_or("output has no parent")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .ok_or("output has no filename")?
        .to_str()
        .ok_or("output name is not UTF-8")?;
    if name.ends_with(['.', ' ']) || name.contains(':') {
        return Err("ambiguous output filename".into());
    }
    Ok(parent.join(name).to_string_lossy().to_lowercase())
}

fn run_independent_case_batch(repository: &Path, path: &Path) -> Result<serde_json::Value, String> {
    use aexcompat_broker::image_render::{IndependentCaseBatch, RenderPixelFormat};
    let started = std::time::Instant::now();
    let manifest = parse_batch_manifest(&read_batch_document(path, 1024 * 1024)?)?;
    let format = if manifest.route.ends_with("16") {
        RenderPixelFormat::Argb16
    } else if manifest.route.ends_with("32") {
        RenderPixelFormat::Argb32f
    } else {
        RenderPixelFormat::Argb8
    };
    let smart = manifest.route.starts_with("smart");
    let mut destinations = std::collections::HashSet::new();
    let mut documents = Vec::new();
    let mut dependencies = Vec::new();
    for case in &manifest.cases {
        let mut outputs = vec![case.output.clone()];
        if manifest.route.ends_with("16") {
            outputs.push(case.output.with_extension("rgba16le"));
        }
        if manifest.route.ends_with("32") {
            outputs.push(case.output.with_extension("rgba32f-le"));
        }
        for output in outputs {
            if output.exists() || !destinations.insert(batch_output_identity(&output)?) {
                return Err(
                    "output paths must be fresh and unique (including raw sidecars)".into(),
                );
            }
        }
        let bytes = read_batch_document(&case.request, 64 * 1024)?;
        let document = serde_json::from_slice::<BatchJsonValue>(&bytes)
            .map_err(|e| e.to_string())?
            .0;
        if aexcompat_broker::memory_fill::parse_request(&bytes)?.is_some() {
            return Err(
                "diagnostics.memory_fill is unsupported for independent case batches".into(),
            );
        }
        dependencies.push(typed_request_dependencies(&document, &case.request)?);
        documents.push(document);
    }
    // Dependency identity is part of the homogeneous key, before inspection.
    let dependency_keys = dependencies
        .iter()
        .map(|items| {
            items
                .iter()
                .map(|item| format!("{}:{:?}", item.path.display(), item.expected_sha256))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if dependency_keys.iter().any(|key| key != &dependency_keys[0]) {
        return Err("batch dependencies differ".into());
    }
    let hash = {
        use sha2::Digest;
        format!(
            "{:x}",
            sha2::Sha256::digest(std::fs::read(&manifest.plugin).map_err(|e| e.to_string())?)
        )
    };
    let inspect_started = std::time::Instant::now();
    let (metadata, inspection) = aexcompat_broker::image_render::inspect_experimental_with_approved_dependencies_and_diagnostics(
        repository, &manifest.plugin, &hash, dependencies[0].clone()).map_err(|e| e.to_string())?;
    let inspect_ms = inspect_started.elapsed().as_secs_f64() * 1000.0;
    let flags = inspection["advertised_out_flags"]
        .as_u64()
        .ok_or("missing capability flags")?;
    let flags2 = inspection["advertised_out_flags2"]
        .as_u64()
        .ok_or("missing capability flags2")?;
    if inspection["image_render_supported"] != true
        || (smart && inspection["smart_render_advertised"] != true)
        || (!smart && inspection["smart_render_advertised"] == true)
        || (format == RenderPixelFormat::Argb16 && flags & (1 << 25) == 0)
        || (format == RenderPixelFormat::Argb32f && flags2 & (1 << 12) == 0)
    {
        return Err(
            "batch route/depth must be explicitly advertised; no fallback is permitted".into(),
        );
    }
    let mut prepared = Vec::new();
    let mut batch = IndependentCaseBatch::default();
    for (index, case) in manifest.cases.iter().enumerate() {
        let document = &documents[index];
        let mut parameters = metadata.clone();
        apply_typed_render_assignments(&mut parameters, document, Some(&case.request))?;
        let timing = typed_request_timing(document)?;
        let context = typed_request_host_context(document)?;
        let settings = typed_request_render_settings(document)?;
        let _guard = ConformanceRenderSettingsGuard::install(settings.as_deref())?;
        batch
            .render(
                repository,
                &manifest.plugin,
                &hash,
                &case.input,
                &case.output,
                &parameters,
                timing,
                smart,
                format,
                context.as_ref(),
                dependencies[index].clone(),
                manifest.frame_deadline_ms,
            )
            .map_err(|e| e.to_string())?;
        prepared.push((parameters, timing, context, settings));
    }
    batch.start().map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    let mut stopped = false;
    for (index, case) in manifest.cases.iter().enumerate() {
        if stopped {
            results.push(serde_json::json!({"id":case.id,"case_index":index,"outcome":"unattempted","passed":false}));
            continue;
        }
        let (parameters, timing, context, settings) = &prepared[index];
        let _guard = ConformanceRenderSettingsGuard::install(settings.as_deref())?;
        let case_start = std::time::Instant::now();
        let result = batch.render(
            repository,
            &manifest.plugin,
            &hash,
            &case.input,
            &case.output,
            parameters,
            *timing,
            smart,
            format,
            context.as_ref(),
            dependencies[index].clone(),
            manifest.frame_deadline_ms,
        );
        let mut result = match result {
            Ok(report) => report,
            Err(error) => {
                serde_json::json!({"passed":false,"error":error.to_string(),"outcome":"case_error"})
            }
        };
        stopped = batch.invalidated();
        result["id"] = serde_json::json!(case.id);
        result["case_index"] = serde_json::json!(index);
        result["wall_ms"] = serde_json::json!(case_start.elapsed().as_secs_f64() * 1000.0);
        result["parameter_metadata"] = inspection["parameter_metadata"].clone();
        results.push(result);
    }
    let close = batch.close();
    let clean = close["independent_cases_clean"] == true;
    for result in &mut results {
        result["evidence_provisional"] = serde_json::json!(false);
        result["evidence_valid"] = serde_json::json!(clean && result["passed"] == true);
        result["plugin_sha256"] = close["final_report"]["worker_admitted_plugin_sha256"].clone();
        result["requested_route"] = serde_json::json!(manifest.route);
        if !clean && result["passed"] == true {
            result["render_passed_before_close"] = serde_json::json!(true);
            result["passed"] = serde_json::json!(false);
            result["outcome"] = serde_json::json!("invalidated_at_close");
        }
    }
    Ok(
        serde_json::json!({"schema":"aexcompat.independent_case_batch","schema_version":1,
        "isolation":"fresh_sequence","route":manifest.route,"cases":results,
        "plugin_sha256":close["final_report"]["worker_admitted_plugin_sha256"],"inspection":inspection,
        "passed":clean && results.iter().all(|r| r["passed"] == true),"complete":clean,
        "inspection_ms":inspect_ms,"wall_ms":started.elapsed().as_secs_f64()*1000.0,
        "session_close":close}),
    )
}

struct BatchJsonValue(serde_json::Value);

impl<'de> serde::Deserialize<'de> for BatchJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(BatchJsonVisitor)
    }
}

struct BatchJsonVisitor;

impl<'de> serde::de::Visitor<'de> for BatchJsonVisitor {
    type Value = BatchJsonValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(BatchJsonValue(serde_json::Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(BatchJsonValue(serde_json::json!(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(BatchJsonValue(serde_json::json!(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .map(BatchJsonValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(BatchJsonValue(serde_json::Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(BatchJsonValue(serde_json::Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(BatchJsonValue(serde_json::Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(BatchJsonValue(serde_json::Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<BatchJsonValue>()? {
            values.push(value.0);
        }
        Ok(BatchJsonValue(serde_json::Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate object key: {key}"
                )));
            }
            let value = object.next_value::<BatchJsonValue>()?;
            values.insert(key, value.0);
        }
        Ok(BatchJsonValue(serde_json::Value::Object(values)))
    }
}

#[cfg(test)]
mod independent_batch_tests {
    use super::*;
    fn manifest() -> serde_json::Value {
        serde_json::json!({"schema":"aexcompat.independent_case_batch","schema_version":1,
            "plugin":"C:/fixture.aex","route":"smart8","isolation":"fresh_sequence",
            "frame_deadline_ms":120000,"cases":[{"id":"A","input":"C:/in.png",
            "request":"C:/request.json","output":"C:/out.png"}]})
    }
    fn parse(value: &serde_json::Value) -> bool {
        parse_batch_manifest(&serde_json::to_vec(value).unwrap()).is_ok()
    }
    #[test]
    fn manifest_accepts_only_bounded_homogeneous_contract() {
        assert!(parse(&manifest()));
        for (key, value) in [
            ("schema_version", serde_json::json!(2)),
            ("isolation", serde_json::json!("persistent")),
            ("route", serde_json::json!("auto")),
            ("frame_deadline_ms", serde_json::json!(0)),
            ("cases", serde_json::json!([])),
            ("plugin", serde_json::json!("relative.aex")),
        ] {
            let mut value_manifest = manifest();
            value_manifest[key] = value;
            assert!(!parse(&value_manifest));
        }
        let mut value = manifest();
        value["cases"] = serde_json::json!(vec![value["cases"][0].clone(); 129]);
        assert!(!parse(&value));
        assert!(parse_batch_manifest(&vec![b' '; 1024 * 1024 + 1]).is_err());
    }
    #[test]
    fn duplicate_ids_fields_and_unknown_fields_are_rejected() {
        let mut value = manifest();
        let case = value["cases"][0].clone();
        value["cases"].as_array_mut().unwrap().push(case);
        assert!(!parse(&value));
        value = manifest();
        value["unexpected"] = serde_json::json!(true);
        assert!(!parse(&value));
        let text =
            serde_json::to_string(&manifest())
                .unwrap()
                .replacen("{", "{\"schema_version\":1,", 1);
        assert!(parse_batch_manifest(text.as_bytes()).is_err());
        for text in [
            r#"{"assignments":[],"assignments":[]}"#,
            r#"{"assignments":[{"slot":1,"value":2,"value":3}]}"#,
        ] {
            assert!(serde_json::from_str::<BatchJsonValue>(text).is_err());
        }
    }
    #[test]
    fn strict_request_preserves_values_and_rejects_trailing_input() {
        let text = r#"{"schema_version":1,"assignments":[{"slot":1,"value":2}],"optional":null}"#;
        assert_eq!(
            serde_json::from_str::<BatchJsonValue>(text).unwrap().0,
            serde_json::from_str::<serde_json::Value>(text).unwrap()
        );
        assert!(serde_json::from_str::<BatchJsonValue>(&format!("{text} true")).is_err());
    }
}
