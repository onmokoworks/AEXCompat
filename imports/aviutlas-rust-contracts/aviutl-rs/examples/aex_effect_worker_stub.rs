//! Handshake-only AEX worker stub.
//!
//! This executable exists to test the broker/worker process boundary. It does
//! not accept a `.aex` path, does not load plug-ins, and does not call
//! the native effect entrypoint.

use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerTransportManifest {
    schema_version: u32,
    transport_protocol_version: u32,
    pixel_format: String,
    raw_rgba_path: String,
    generated_root: String,
    width: u32,
    height: u32,
    row_stride_bytes: u64,
    decoded_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerIdentityManifest {
    schema_version: u32,
    identity_protocol_version: u32,
    generated_by: String,
    generated_unix_ms: u64,
    max_manifest_age_ms: u64,
    allowlist_id: String,
    operation: String,
    canonical_plugin_path: String,
    expected_extension: String,
    expected_class: String,
    fixture_status: String,
    publication_status: String,
    license_status: String,
    classifier_status: String,
    classifier_inferred_class: String,
    observed_size_bytes: u64,
    observed_modified_unix_ms: Option<u64>,
    max_plugin_bytes: u64,
    loader_approval_status: String,
    sandbox_profile: String,
    sandbox_profile_status: String,
    worker_revalidation_status: String,
    binary_evidence_mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerLoaderTicketManifest {
    schema_version: u32,
    ticket_protocol_version: u32,
    generated_by: String,
    generated_unix_ms: u64,
    max_ticket_age_ms: u64,
    publication_status: String,
    status: String,
    native_load_performed: bool,
    worker_may_load_plugin: bool,
    broker_may_load_plugin: bool,
    allowlist_id: String,
    operation: String,
    selected_loader_entry: WorkerLoaderTicketEntry,
    required_runtime_evidence: WorkerLoaderTicketRuntimeEvidence,
    planned_stages: Vec<WorkerLoaderTicketStage>,
    denied_surfaces: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerLoaderTicketEntry {
    effect_id: String,
    normalized_plugin_path: String,
    path_match_status: String,
    allowlist_operation_status: String,
    entry_ready: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerLoaderTicketRuntimeEvidence {
    worker_identity_revalidation_required: String,
    worker_attestation_required: String,
    sandbox_preflight_required: String,
    job_object_required: String,
    handle_inheritance_required: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerLoaderTicketStage {
    stage: String,
    status: String,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|arg| arg == "--handshake") {
        eprintln!("usage: aex_effect_worker_stub --handshake --protocol-version 1");
        std::process::exit(2);
    }
    if has_forbidden_plugin_arg(&args) {
        println!(
            "{}",
            json!({
                "worker_protocol_version": 1,
                "status": "worker_protocol_error",
                "aex_loading": "disabled",
                "transport_error": "plugin path arguments are not accepted"
            })
        );
        return;
    }
    if protocol_version_arg(&args).as_deref() != Some("1") {
        println!(
            "{}",
            json!({
                "worker_protocol_version": 1,
                "status": "worker_protocol_error",
                "aex_loading": "disabled",
                "transport_error": "unsupported protocol version"
            })
        );
        return;
    }

    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_default()
        .to_ascii_lowercase();

    if exe_name.contains("timeout") {
        std::thread::sleep(Duration::from_millis(500));
        return;
    }
    if exe_name.contains("crash") {
        eprintln!("synthetic worker crash before loading is enabled");
        std::process::exit(7);
    }
    if exe_name.contains("malformed") {
        println!("not-json");
        return;
    }
    if exe_name.contains("bad_protocol") {
        println!(
            "{}",
            json!({
                "worker_protocol_version": 2,
                "status": "worker_ready",
                "aex_loading": "disabled"
            })
        );
        return;
    }

    let transport_status = match transport_manifest_arg(&args) {
        Some(path) => match validate_transport_manifest(&path) {
            Ok(()) => "validated",
            Err(reason) => {
                println!(
                    "{}",
                    json!({
                        "worker_protocol_version": 1,
                        "status": "worker_protocol_error",
                        "aex_loading": "disabled",
                        "transport_error": reason
                    })
                );
                return;
            }
        },
        None => "not_provided",
    };
    let worker_revalidation = match identity_manifest_arg(&args) {
        Some(path) => match validate_identity_manifest(&path) {
            Ok(allowlist_id) => Some(json!({
                "status": "passed",
                "allowlist_id": allowlist_id
            })),
            Err(reason) => {
                println!(
                    "{}",
                    json!({
                        "worker_protocol_version": 1,
                        "status": "worker_protocol_error",
                        "aex_loading": "disabled",
                        "worker_revalidation": {
                            "status": "denied",
                            "denied_reason": reason
                        }
                    })
                );
                return;
            }
        },
        None => None,
    };
    let loader_ticket = match loader_ticket_arg(&args) {
        Some(path) => match validate_loader_ticket_manifest(&path) {
            Ok(allowlist_id) => Some(json!({
                "status": "accepted_no_load",
                "allowlist_id": allowlist_id,
                "native_load_performed": false,
                "worker_may_load_plugin": false
            })),
            Err(reason) => {
                println!(
                    "{}",
                    json!({
                        "worker_protocol_version": 1,
                        "status": "worker_protocol_error",
                        "aex_loading": "disabled",
                        "loader_ticket": {
                            "status": "denied",
                            "denied_reason": reason
                        }
                    })
                );
                return;
            }
        },
        None => None,
    };

    let mut response = json!({
        "worker_protocol_version": 1,
        "status": "worker_ready",
        "aex_loading": "disabled",
        "transport_status": transport_status,
        "sandbox_attestation": sandbox_attestation(&args),
        "worker": "aex_effect_worker_stub"
    });
    if let Some(worker_revalidation) = worker_revalidation {
        response["worker_revalidation"] = worker_revalidation;
    }
    if let Some(loader_ticket) = loader_ticket {
        response["loader_ticket"] = loader_ticket;
    }
    println!("{response}");
}

fn sandbox_attestation(args: &[String]) -> serde_json::Value {
    let current_dir = std::env::current_dir()
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"));
    let worker_exe_name = std::env::current_exe().ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
    });
    let generated_root_confined = current_dir
        .as_deref()
        .map(|path| is_generated_path(Path::new(path)))
        .unwrap_or(false);
    let shell_env_name = ["Com", "Spec"].concat();
    let inheritance_sentinel = inheritance_sentinel_arg(args);
    let sentinel_inherited = inheritance_sentinel
        .as_deref()
        .and_then(inheritance_sentinel_inherited);
    let handle_inheritance_disabled =
        inheritance_sentinel.is_some() && sentinel_inherited == Some(false);

    json!({
        "schema_version": 1,
        "platform": std::env::consts::OS,
        "current_dir": current_dir,
        "generated_root_confined": generated_root_confined,
        "env_path_absent": std::env::var_os("PATH").is_none(),
        "env_comspec_absent": std::env::var_os(shell_env_name).is_none(),
        "env_count": std::env::vars_os().count(),
        "stdin_contract": "null",
        "worker_exe_name": worker_exe_name,
        "inheritance_sentinel_provided": inheritance_sentinel.is_some(),
        "inheritance_sentinel_inherited": sentinel_inherited,
        "handle_inheritance_disabled": handle_inheritance_disabled
    })
}

fn inheritance_sentinel_arg(args: &[String]) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == "--inheritance-sentinel")
        .map(|window| window[1].clone())
}

#[cfg(windows)]
fn inheritance_sentinel_inherited(raw: &str) -> Option<bool> {
    let handle_value = raw.parse::<usize>().ok()?;
    let mut flags = 0u32;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetHandleInformation(hobject: *mut core::ffi::c_void, lpdwflags: *mut u32) -> i32;
    }
    let ok = unsafe { GetHandleInformation(handle_value as *mut _, &mut flags) != 0 };
    Some(ok)
}

#[cfg(not(windows))]
fn inheritance_sentinel_inherited(_raw: &str) -> Option<bool> {
    None
}

fn has_forbidden_plugin_arg(args: &[String]) -> bool {
    args.iter().any(|arg| {
        let lowered = arg.to_ascii_lowercase();
        lowered == "--plugin-path" || lowered.contains(".aex")
    })
}

fn transport_manifest_arg(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|window| window[0] == "--transport-manifest")
        .map(|window| PathBuf::from(&window[1]))
}

fn identity_manifest_arg(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|window| window[0] == "--identity-manifest")
        .map(|window| PathBuf::from(&window[1]))
}

fn loader_ticket_arg(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|window| window[0] == "--loader-ticket")
        .map(|window| PathBuf::from(&window[1]))
}

fn protocol_version_arg(args: &[String]) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == "--protocol-version")
        .map(|window| window[1].clone())
}

fn validate_transport_manifest(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || !path.is_file() {
        return Err("transport manifest must be an existing absolute file".to_owned());
    }
    if !is_generated_path(path) {
        return Err("transport manifest must stay under generated root".to_owned());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|err| format!("transport manifest should read: {err}"))?;
    if text.to_ascii_lowercase().contains(".aex") {
        return Err("transport manifest must not expose .aex paths".to_owned());
    }
    let manifest: WorkerTransportManifest =
        serde_json::from_str(&text).map_err(|err| format!("transport manifest JSON: {err}"))?;
    if manifest.schema_version != 1 || manifest.transport_protocol_version != 1 {
        return Err("unsupported transport protocol".to_owned());
    }
    if manifest.pixel_format != "rgba8" {
        return Err("transport pixel_format must be rgba8".to_owned());
    }
    if manifest.width == 0 || manifest.height == 0 {
        return Err("transport dimensions must be non-zero".to_owned());
    }
    let raw_path = PathBuf::from(&manifest.raw_rgba_path);
    let generated_root = PathBuf::from(&manifest.generated_root);
    if !raw_path.is_absolute() || !raw_path.is_file() {
        return Err("raw_rgba_path must be an existing absolute file".to_owned());
    }
    if !is_generated_path(&raw_path) || !is_generated_path(&generated_root) {
        return Err("raw transport paths must stay under generated root".to_owned());
    }
    if !path_is_within(&raw_path, &generated_root) {
        return Err("raw_rgba_path must be inside generated_root".to_owned());
    }
    let expected_bytes = u64::from(manifest.width)
        .checked_mul(u64::from(manifest.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "transport dimensions overflow byte count".to_owned())?;
    let expected_stride = u64::from(manifest.width)
        .checked_mul(4)
        .ok_or_else(|| "transport row stride overflow".to_owned())?;
    if manifest.row_stride_bytes != expected_stride {
        return Err("row_stride_bytes must match width".to_owned());
    }
    if manifest.decoded_bytes != expected_bytes {
        return Err("decoded_bytes must match dimensions".to_owned());
    }
    let actual_bytes = raw_path
        .metadata()
        .map_err(|err| format!("raw_rgba_path metadata: {err}"))?
        .len();
    if actual_bytes != expected_bytes {
        return Err("raw_rgba_path length must match dimensions".to_owned());
    }
    Ok(())
}

fn validate_identity_manifest(path: &Path) -> Result<String, String> {
    if !path.is_absolute() || !path.is_file() {
        return Err("identity manifest must be an existing absolute file".to_owned());
    }
    if !is_generated_path(path) {
        return Err("identity manifest must stay under generated root".to_owned());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|err| format!("identity manifest should read: {err}"))?;
    let manifest: WorkerIdentityManifest =
        serde_json::from_str(&text).map_err(|err| format!("identity manifest JSON: {err}"))?;
    if manifest.schema_version != 1 || manifest.identity_protocol_version != 1 {
        return Err("unsupported identity protocol".to_owned());
    }
    if manifest.generated_by != "aex_image_probe" {
        return Err("generated_by must be aex_image_probe".to_owned());
    }
    if manifest.max_manifest_age_ms == 0 || manifest.max_manifest_age_ms > 30_000 {
        return Err("max_manifest_age_ms must be between 1 and 30000".to_owned());
    }
    let age_ms = current_unix_ms().saturating_sub(manifest.generated_unix_ms);
    if age_ms > manifest.max_manifest_age_ms {
        return Err("identity manifest is stale".to_owned());
    }
    if manifest.allowlist_id.trim().is_empty() {
        return Err("allowlist_id is required".to_owned());
    }
    if !matches!(manifest.operation.as_str(), "describe" | "render_png") {
        return Err("operation is not supported for revalidation".to_owned());
    }
    if manifest.expected_extension != ".aex" {
        return Err("expected_extension must be .aex".to_owned());
    }
    if manifest.expected_class != "classic-effect" {
        return Err("expected_class must be classic-effect".to_owned());
    }
    if manifest.fixture_status != "local-build-candidate" {
        return Err("fixture_status must be local-build-candidate".to_owned());
    }
    if !matches!(
        manifest.publication_status.as_str(),
        "local-only" | "public-candidate-reviewed" | "public-candidate"
    ) {
        return Err("publication_status is not approved for revalidation".to_owned());
    }
    if !matches!(
        manifest.license_status.as_str(),
        "local-only-reviewed" | "local-only-unpublished" | "reviewed"
    ) {
        return Err("license_status is not approved for revalidation".to_owned());
    }
    if !matches!(
        manifest.classifier_status.as_str(),
        "classified_from_inventory"
            | "classified_from_adjacent_source"
            | "candidate_for_contract_probe"
    ) {
        return Err("classifier_status is not approved for revalidation".to_owned());
    }
    if !matches!(
        manifest.classifier_inferred_class.as_str(),
        "classic-effect" | "classic-effect-candidate"
    ) {
        return Err("classifier_inferred_class is not classic effect compatible".to_owned());
    }
    if manifest.loader_approval_status != "approved-local-only" {
        return Err("loader_approval_status must be approved-local-only".to_owned());
    }
    if manifest.sandbox_profile != "windows-job-object-v0" {
        return Err("sandbox_profile must be windows-job-object-v0".to_owned());
    }
    if manifest.sandbox_profile_status != "implemented-v0" {
        return Err("sandbox_profile_status must be implemented-v0".to_owned());
    }
    if manifest.worker_revalidation_status != "required" {
        return Err("worker_revalidation_status must be required".to_owned());
    }
    if manifest.max_plugin_bytes == 0 {
        return Err("max_plugin_bytes must be non-zero".to_owned());
    }
    if manifest.binary_evidence_mode != "metadata-only" {
        return Err("binary_evidence_mode must be metadata-only".to_owned());
    }

    let plugin_path = PathBuf::from(&manifest.canonical_plugin_path);
    if !plugin_path.is_absolute() || !plugin_path.is_file() {
        return Err("canonical_plugin_path must be an existing absolute file".to_owned());
    }
    if !plugin_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("aex"))
        .unwrap_or(false)
    {
        return Err("canonical_plugin_path must end with .aex".to_owned());
    }
    let canonical = plugin_path
        .canonicalize()
        .map_err(|err| format!("canonical_plugin_path should canonicalize: {err}"))?;
    if normalize_path_text(&canonical.to_string_lossy())
        != normalize_path_text(&manifest.canonical_plugin_path)
    {
        return Err("canonical_plugin_path does not match worker canonical path".to_owned());
    }
    let metadata = plugin_path
        .metadata()
        .map_err(|err| format!("canonical_plugin_path metadata: {err}"))?;
    if metadata.len() == 0 || metadata.len() > manifest.max_plugin_bytes {
        return Err("canonical_plugin_path size is outside allowed bounds".to_owned());
    }
    if metadata.len() != manifest.observed_size_bytes {
        return Err("observed_size_bytes does not match worker metadata".to_owned());
    }
    if let Some(expected_modified) = manifest.observed_modified_unix_ms {
        let observed_modified = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64);
        if observed_modified != Some(expected_modified) {
            return Err("observed_modified_unix_ms does not match worker metadata".to_owned());
        }
    }
    Ok(manifest.allowlist_id)
}

fn validate_loader_ticket_manifest(path: &Path) -> Result<String, String> {
    if !path.is_absolute() || !path.is_file() {
        return Err("loader ticket must be an existing absolute file".to_owned());
    }
    if !is_generated_path(path) {
        return Err("loader ticket must stay under generated root".to_owned());
    }
    let metadata = path
        .metadata()
        .map_err(|err| format!("loader ticket metadata: {err}"))?;
    if metadata.len() == 0 || metadata.len() > 64 * 1024 {
        return Err("loader ticket size is outside allowed bounds".to_owned());
    }
    let text =
        std::fs::read_to_string(path).map_err(|err| format!("loader ticket should read: {err}"))?;
    let lowered = text.to_ascii_lowercase();
    for token in forbidden_loader_ticket_tokens() {
        if lowered.contains(&token) {
            return Err("loader ticket contains a forbidden serialized token".to_owned());
        }
    }
    let ticket: WorkerLoaderTicketManifest =
        serde_json::from_str(&text).map_err(|err| format!("loader ticket JSON: {err}"))?;
    if ticket.schema_version != 1 || ticket.ticket_protocol_version != 1 {
        return Err("unsupported loader ticket protocol".to_owned());
    }
    if ticket.generated_by != "aex_image_probe" {
        return Err("loader ticket generated_by must be aex_image_probe".to_owned());
    }
    if ticket.max_ticket_age_ms == 0 || ticket.max_ticket_age_ms > 30_000 {
        return Err("loader ticket max age must be between 1 and 30000".to_owned());
    }
    let age_ms = current_unix_ms().saturating_sub(ticket.generated_unix_ms);
    if age_ms > ticket.max_ticket_age_ms {
        return Err("loader ticket is stale".to_owned());
    }
    if ticket.publication_status != "local-only" {
        return Err("loader ticket publication_status must be local-only".to_owned());
    }
    if ticket.status != "accepted_no_load" {
        return Err("loader ticket status must be accepted_no_load".to_owned());
    }
    if ticket.native_load_performed
        || ticket.worker_may_load_plugin
        || ticket.broker_may_load_plugin
    {
        return Err("loader ticket must keep all native-load permissions false".to_owned());
    }
    if ticket.allowlist_id.trim().is_empty() {
        return Err("loader ticket allowlist_id is required".to_owned());
    }
    if ticket.operation != "render_png" {
        return Err("loader ticket operation must be render_png".to_owned());
    }
    let entry = &ticket.selected_loader_entry;
    if entry.effect_id != ticket.allowlist_id {
        return Err("loader ticket selected_loader_entry.effect_id mismatch".to_owned());
    }
    if !entry
        .normalized_plugin_path
        .to_ascii_lowercase()
        .ends_with(".aex")
    {
        return Err("loader ticket normalized plugin path must end with .aex".to_owned());
    }
    if entry.path_match_status != "matched_normalized_path" {
        return Err("loader ticket path_match_status must be matched_normalized_path".to_owned());
    }
    if entry.allowlist_operation_status != "render_png" || !entry.entry_ready {
        return Err("loader ticket selected loader entry is not render-ready".to_owned());
    }
    let runtime = &ticket.required_runtime_evidence;
    if runtime.worker_identity_revalidation_required != "passed"
        || runtime.worker_attestation_required != "passed"
        || runtime.sandbox_preflight_required != "passed"
        || runtime.job_object_required != "assigned-with-kill-on-close"
        || runtime.handle_inheritance_required != "sentinel_not_inherited-with-explicit-handle-list"
    {
        return Err("loader ticket runtime evidence requirements are not satisfied".to_owned());
    }
    if ticket.planned_stages.len() != 7
        || ticket
            .planned_stages
            .iter()
            .any(|stage| stage.status != "planned_not_run")
    {
        return Err("loader ticket planned stages must all be planned_not_run".to_owned());
    }
    for stage in [
        "load",
        "global_setup",
        "params_setup",
        "sequence_setup",
        "render",
        "sequence_teardown",
        "global_teardown",
    ] {
        if !ticket
            .planned_stages
            .iter()
            .any(|entry| entry.stage == stage)
        {
            return Err(format!("loader ticket missing planned stage {stage}"));
        }
    }
    for surface in [
        "AEGP",
        "AEIO",
        "SmartFX-only",
        "GPU",
        "custom UI",
        "audio",
        "layer checkout",
        "file/network APIs",
    ] {
        if !ticket.denied_surfaces.iter().any(|item| item == surface) {
            return Err(format!("loader ticket missing denied surface {surface}"));
        }
    }
    if !ticket.notes.iter().any(|note| {
        note == "Worker validated loader ticket metadata only; no native AEX load was performed."
    }) {
        return Err("loader ticket missing required no-load note".to_owned());
    }
    Ok(ticket.allowlist_id)
}

fn forbidden_loader_ticket_tokens() -> Vec<String> {
    vec![
        "sha256".to_string(),
        "base64".to_string(),
        ["load", "library"].concat(),
        ["lib", "loading"].concat(),
        ["effect", "main"].concat(),
        "output_png".to_string(),
        "input_png".to_string(),
        "rendered_pixels".to_string(),
    ]
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn is_generated_path(path: &Path) -> bool {
    normalize_path_text(&path.to_string_lossy()).contains("/target/aex-image-probe/")
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = normalize_path_text(&path.to_string_lossy());
    let root = normalize_path_text(&root.to_string_lossy());
    path == root || path.starts_with(&format!("{root}/"))
}

fn normalize_path_text(path: &str) -> String {
    let replaced = path.replace('\\', "/").to_ascii_lowercase();
    let mut prefix = String::new();
    let mut parts = Vec::new();
    for (index, part) in replaced.split('/').enumerate() {
        if index == 0 && part.ends_with(':') {
            prefix = part.to_owned();
            continue;
        }
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    if prefix.is_empty() {
        format!("/{}", parts.join("/"))
    } else {
        format!("{prefix}/{}", parts.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::current_dir()
            .unwrap()
            .join("target")
            .join("aex-image-probe")
            .join("worker-stub-tests")
            .join(name)
    }

    fn write_raw(root: &Path, name: &str, bytes: usize) -> PathBuf {
        std::fs::create_dir_all(root).unwrap();
        let path = root.join(name);
        std::fs::write(&path, vec![0x7f; bytes]).unwrap();
        path
    }

    fn write_manifest(root: &Path, name: &str, text: &str) -> PathBuf {
        std::fs::create_dir_all(root).unwrap();
        let path = root.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn write_fake_plugin(root: &Path, name: &str, bytes: usize) -> PathBuf {
        std::fs::create_dir_all(root).unwrap();
        let path = root.join(name);
        std::fs::write(&path, vec![0x42; bytes]).unwrap();
        path
    }

    fn modified_unix_ms(path: &Path) -> Option<u64> {
        path.metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as u64)
    }

    fn valid_manifest_text(root: &Path, raw: &Path, width: u32, height: u32) -> String {
        let decoded_bytes = u64::from(width) * u64::from(height) * 4;
        serde_json::json!({
            "schema_version": 1,
            "transport_protocol_version": 1,
            "pixel_format": "rgba8",
            "raw_rgba_path": raw.to_string_lossy().replace('\\', "/"),
            "generated_root": root.to_string_lossy().replace('\\', "/"),
            "width": width,
            "height": height,
            "row_stride_bytes": u64::from(width) * 4,
            "decoded_bytes": decoded_bytes
        })
        .to_string()
    }

    fn valid_identity_manifest_text(_root: &Path, plugin: &Path) -> String {
        let canonical = std::fs::canonicalize(plugin).unwrap();
        serde_json::json!({
            "schema_version": 1,
            "identity_protocol_version": 1,
            "generated_by": "aex_image_probe",
            "generated_unix_ms": current_unix_ms(),
            "max_manifest_age_ms": 30000,
            "allowlist_id": "classic-test",
            "operation": "render_png",
            "canonical_plugin_path": canonical.to_string_lossy().replace('\\', "/"),
            "expected_extension": ".aex",
            "expected_class": "classic-effect",
            "fixture_status": "local-build-candidate",
            "publication_status": "local-only",
            "license_status": "local-only-reviewed",
            "classifier_status": "candidate_for_contract_probe",
            "classifier_inferred_class": "classic-effect",
            "observed_size_bytes": plugin.metadata().unwrap().len(),
            "observed_modified_unix_ms": modified_unix_ms(plugin),
            "max_plugin_bytes": 4096,
            "loader_approval_status": "approved-local-only",
            "sandbox_profile": "windows-job-object-v0",
            "sandbox_profile_status": "implemented-v0",
            "worker_revalidation_status": "required",
            "binary_evidence_mode": "metadata-only"
        })
        .to_string()
    }

    fn valid_loader_ticket_text() -> String {
        serde_json::json!({
            "schema_version": 1,
            "ticket_protocol_version": 1,
            "generated_by": "aex_image_probe",
            "generated_unix_ms": current_unix_ms(),
            "max_ticket_age_ms": 30000,
            "publication_status": "local-only",
            "status": "accepted_no_load",
            "native_load_performed": false,
            "worker_may_load_plugin": false,
            "broker_may_load_plugin": false,
            "allowlist_id": "classic-test",
            "operation": "render_png",
            "selected_loader_entry": {
                "effect_id": "classic-test",
                "normalized_plugin_path": "d:\\aviutlas\\local\\classictest.aex",
                "path_match_status": "matched_normalized_path",
                "allowlist_operation_status": "render_png",
                "entry_ready": true
            },
            "required_runtime_evidence": {
                "worker_identity_revalidation_required": "passed",
                "worker_attestation_required": "passed",
                "sandbox_preflight_required": "passed",
                "job_object_required": "assigned-with-kill-on-close",
                "handle_inheritance_required": "sentinel_not_inherited-with-explicit-handle-list"
            },
            "planned_stages": [
                {"stage": "load", "status": "planned_not_run"},
                {"stage": "global_setup", "status": "planned_not_run"},
                {"stage": "params_setup", "status": "planned_not_run"},
                {"stage": "sequence_setup", "status": "planned_not_run"},
                {"stage": "render", "status": "planned_not_run"},
                {"stage": "sequence_teardown", "status": "planned_not_run"},
                {"stage": "global_teardown", "status": "planned_not_run"}
            ],
            "denied_surfaces": [
                "AEGP",
                "AEIO",
                "SmartFX-only",
                "GPU",
                "custom UI",
                "audio",
                "layer checkout",
                "file/network APIs"
            ],
            "notes": [
                "Worker validated loader ticket metadata only; no native AEX load was performed."
            ]
        })
        .to_string()
    }

    #[test]
    fn worker_rejects_plugin_path_arguments() {
        assert!(has_forbidden_plugin_arg(&[
            "worker".to_owned(),
            "--plugin-path".to_owned(),
            "D:/private/Fixture.aex".to_owned(),
        ]));
        assert!(has_forbidden_plugin_arg(&[
            "worker".to_owned(),
            "D:/private/Fixture.aex".to_owned(),
        ]));
        assert!(!has_forbidden_plugin_arg(&[
            "worker".to_owned(),
            "--transport-manifest".to_owned(),
            "D:/repo/target/aex-image-probe/worker-transport.json".to_owned(),
        ]));
    }

    #[test]
    fn worker_identity_manifest_accepts_valid_local_metadata() {
        let root = test_root("identity-valid");
        let plugin = write_fake_plugin(&root, "ClassicTest.aex", 32);
        let manifest = write_manifest(
            &root,
            "worker-identity-valid.json",
            &valid_identity_manifest_text(&root, &plugin),
        );

        assert_eq!(
            validate_identity_manifest(&manifest).unwrap(),
            "classic-test"
        );
    }

    #[test]
    fn worker_identity_manifest_rejects_unknown_fields_and_statuses() {
        let root = test_root("identity-reject");
        let plugin = write_fake_plugin(&root, "ClassicTest.aex", 32);
        let mut value: serde_json::Value =
            serde_json::from_str(&valid_identity_manifest_text(&root, &plugin)).unwrap();
        value["extra"] = serde_json::json!(true);
        let manifest = write_manifest(&root, "worker-identity-unknown.json", &value.to_string());
        assert!(validate_identity_manifest(&manifest)
            .unwrap_err()
            .contains("JSON"));

        let mut value: serde_json::Value =
            serde_json::from_str(&valid_identity_manifest_text(&root, &plugin)).unwrap();
        value["loader_approval_status"] = serde_json::json!("unknown");
        let manifest = write_manifest(&root, "worker-identity-status.json", &value.to_string());
        assert!(validate_identity_manifest(&manifest)
            .unwrap_err()
            .contains("loader_approval"));
    }

    #[test]
    fn worker_identity_manifest_rejects_metadata_mismatch_and_stale_manifest() {
        let root = test_root("identity-mismatch");
        let plugin = write_fake_plugin(&root, "ClassicTest.aex", 32);
        let mut value: serde_json::Value =
            serde_json::from_str(&valid_identity_manifest_text(&root, &plugin)).unwrap();
        value["observed_size_bytes"] = serde_json::json!(31);
        let manifest = write_manifest(&root, "worker-identity-size.json", &value.to_string());
        assert!(validate_identity_manifest(&manifest)
            .unwrap_err()
            .contains("observed_size_bytes"));

        let mut value: serde_json::Value =
            serde_json::from_str(&valid_identity_manifest_text(&root, &plugin)).unwrap();
        value["generated_unix_ms"] = serde_json::json!(1u64);
        value["max_manifest_age_ms"] = serde_json::json!(1u64);
        let manifest = write_manifest(&root, "worker-identity-stale.json", &value.to_string());
        assert!(validate_identity_manifest(&manifest)
            .unwrap_err()
            .contains("stale"));
    }

    #[test]
    fn worker_loader_ticket_accepts_no_load_metadata() {
        let root = test_root("loader-ticket-valid");
        let ticket = write_manifest(
            &root,
            "worker-loader-ticket-valid.json",
            &valid_loader_ticket_text(),
        );

        assert_eq!(
            validate_loader_ticket_manifest(&ticket).unwrap(),
            "classic-test"
        );
    }

    #[test]
    fn worker_loader_ticket_rejects_load_claims_and_stale_tickets() {
        let root = test_root("loader-ticket-reject");
        let mut value: serde_json::Value =
            serde_json::from_str(&valid_loader_ticket_text()).unwrap();
        value["native_load_performed"] = serde_json::json!(true);
        let ticket = write_manifest(&root, "worker-loader-ticket-load.json", &value.to_string());
        assert!(validate_loader_ticket_manifest(&ticket)
            .unwrap_err()
            .contains("native-load permissions"));

        let mut value: serde_json::Value =
            serde_json::from_str(&valid_loader_ticket_text()).unwrap();
        value["generated_unix_ms"] = serde_json::json!(1u64);
        value["max_ticket_age_ms"] = serde_json::json!(1u64);
        let ticket = write_manifest(&root, "worker-loader-ticket-stale.json", &value.to_string());
        assert!(validate_loader_ticket_manifest(&ticket)
            .unwrap_err()
            .contains("stale"));
    }

    #[test]
    fn worker_loader_ticket_rejects_identity_and_stage_mismatches() {
        let root = test_root("loader-ticket-mismatch");
        let mut value: serde_json::Value =
            serde_json::from_str(&valid_loader_ticket_text()).unwrap();
        value["selected_loader_entry"]["effect_id"] = serde_json::json!("other-id");
        let ticket = write_manifest(&root, "worker-loader-ticket-id.json", &value.to_string());
        assert!(validate_loader_ticket_manifest(&ticket)
            .unwrap_err()
            .contains("effect_id"));

        let mut value: serde_json::Value =
            serde_json::from_str(&valid_loader_ticket_text()).unwrap();
        value["planned_stages"][4]["status"] = serde_json::json!("run");
        let ticket = write_manifest(&root, "worker-loader-ticket-stage.json", &value.to_string());
        assert!(validate_loader_ticket_manifest(&ticket)
            .unwrap_err()
            .contains("planned_not_run"));
    }

    #[test]
    fn worker_manifest_accepts_valid_generated_raw_rgba() {
        let root = test_root("valid");
        let raw = write_raw(&root, "input.rgba8", 16);
        let manifest = write_manifest(
            &root,
            "transport.json",
            &valid_manifest_text(&root, &raw, 2, 2),
        );

        validate_transport_manifest(&manifest).unwrap();
    }

    #[test]
    fn worker_manifest_rejects_unknown_fields_and_aex_paths() {
        let root = test_root("unknown");
        let raw = write_raw(&root, "input.rgba8", 16);
        let mut value: serde_json::Value =
            serde_json::from_str(&valid_manifest_text(&root, &raw, 2, 2)).unwrap();
        value["extra"] = serde_json::json!(true);
        let manifest = write_manifest(&root, "unknown-field.json", &value.to_string());
        assert!(validate_transport_manifest(&manifest)
            .unwrap_err()
            .contains("JSON"));

        let aex_manifest = write_manifest(
            &root,
            "aex-path.json",
            &valid_manifest_text(&root, &root.join("private.aex.rgba8"), 2, 2),
        );
        assert!(validate_transport_manifest(&aex_manifest)
            .unwrap_err()
            .contains(".aex"));
    }

    #[test]
    fn worker_manifest_rejects_path_traversal_and_sibling_prefixes() {
        let root = test_root("traversal/root");
        let raw = write_raw(&test_root("traversal"), "outside.rgba8", 16);
        let manifest = write_manifest(
            &root,
            "outside.json",
            &valid_manifest_text(&root, &raw, 2, 2),
        );
        assert!(validate_transport_manifest(&manifest)
            .unwrap_err()
            .contains("inside generated_root"));

        let sibling_root = test_root("sibling/root");
        let sibling_raw = write_raw(&test_root("sibling/root-extra"), "input.rgba8", 16);
        let manifest = write_manifest(
            &sibling_root,
            "sibling.json",
            &valid_manifest_text(&sibling_root, &sibling_raw, 2, 2),
        );
        assert!(validate_transport_manifest(&manifest)
            .unwrap_err()
            .contains("inside generated_root"));
    }

    #[test]
    fn worker_manifest_rejects_length_and_declared_byte_mismatch() {
        let root = test_root("bytes");
        let raw = write_raw(&root, "input.rgba8", 12);
        let manifest = write_manifest(
            &root,
            "length.json",
            &valid_manifest_text(&root, &raw, 2, 2),
        );
        assert!(validate_transport_manifest(&manifest)
            .unwrap_err()
            .contains("length"));

        let raw = write_raw(&root, "input-ok.rgba8", 16);
        let mut value: serde_json::Value =
            serde_json::from_str(&valid_manifest_text(&root, &raw, 2, 2)).unwrap();
        value["decoded_bytes"] = serde_json::json!(12);
        let manifest = write_manifest(&root, "decoded.json", &value.to_string());
        assert!(validate_transport_manifest(&manifest)
            .unwrap_err()
            .contains("decoded_bytes"));

        let mut value: serde_json::Value =
            serde_json::from_str(&valid_manifest_text(&root, &raw, 2, 2)).unwrap();
        value["row_stride_bytes"] = serde_json::json!(12);
        let manifest = write_manifest(&root, "stride.json", &value.to_string());
        assert!(validate_transport_manifest(&manifest)
            .unwrap_err()
            .contains("row_stride_bytes"));
    }

    #[test]
    fn worker_manifest_rejects_zero_dimensions() {
        let root = test_root("zero");
        let raw = write_raw(&root, "input.rgba8", 0);
        let manifest = write_manifest(&root, "zero.json", &valid_manifest_text(&root, &raw, 0, 2));

        assert!(validate_transport_manifest(&manifest)
            .unwrap_err()
            .contains("non-zero"));
    }
}
