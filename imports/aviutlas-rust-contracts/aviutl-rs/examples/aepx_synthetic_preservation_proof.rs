//! Synthetic-only AEPX preservation proof.
//!
//! This example promotes the preservation-writer spike into a small reportable
//! artifact without enabling production `.aepx` apply. It reads only checked-in
//! synthetic fixtures, writes only create-new outputs under `target`, never
//! launches After Effects, and never embeds XML bodies, selector values, or edit
//! values in the report.

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs::OpenOptions;
use std::io::{Error as IoError, ErrorKind, Write};
use std::path::{Path, PathBuf};

const REPORT_KIND: &str = "aepx_synthetic_preservation_proof";
const READY_STATUS: &str = "synthetic_preservation_proof_ready";
const ALLOWED_FIXTURES: &[&str] = &[
    "aepx_writer_spike_preservation.aepx",
    "aepx_writer_spike_ambiguous.aepx",
    "aepx_writer_spike_scanner_edges.aepx",
    "aepx_writer_spike_duplicate_id.aepx",
    "aepx_writer_spike_unicode_edges.aepx",
    "aepx_writer_spike_multi_comp.aepx",
    "aepx_writer_spike_crlf_bom.aepx",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyntheticProofRequest {
    schema_version: u32,
    proof_kind: String,
    input_fixture_name: String,
    output_aepx: String,
    operation: Option<SyntheticProofOperation>,
    operations: Option<Vec<SyntheticProofOperation>>,
    options: SyntheticProofOptions,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyntheticProofOperation {
    kind: String,
    selector_kind: String,
    selector_value: String,
    expected_old_value: String,
    new_value: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SyntheticProofOptions {
    synthetic_fixture_only: bool,
    create_new_only: bool,
    preserve_unknown_xml: String,
    report_private_payloads: bool,
}

#[derive(Debug, Serialize)]
pub struct SyntheticPreservationProofReport {
    schema_version: u32,
    report_kind: String,
    publication_status: String,
    status: String,
    synthetic_fixture_only: bool,
    proof_mode: String,
    operation_kind: String,
    selector_kind: Option<String>,
    target_count: u32,
    preservation: PreservationReport,
    io_gate: IoGate,
    write_gate: WriteGate,
    hardening_gate: HardeningGate,
    privacy_gate: PrivacyGate,
    blocked_reasons: Vec<String>,
    notes: Vec<String>,
}

type ProofResult<T> = Result<T, Box<SyntheticPreservationProofReport>>;

#[derive(Debug, Serialize)]
struct PreservationReport {
    unknown_nodes: String,
    unknown_attributes: String,
    xml_declaration: String,
    encoding: String,
    comments: String,
    cdata: String,
    namespace_prefixes: String,
    whitespace: String,
}

#[derive(Debug, Serialize)]
struct IoGate {
    synthetic_xml_body_read_performed: bool,
    xml_body_embedded_in_report: bool,
    private_payloads_embedded_in_report: bool,
    external_process_invoked: bool,
    after_effects_invoked: bool,
}

#[derive(Debug, Serialize)]
struct WriteGate {
    output_write_performed: bool,
    output_write_mode: String,
    output_under_generated_target_root: bool,
    source_overwrite_performed: bool,
    production_apply_enabled: bool,
}

#[derive(Debug, Serialize)]
struct HardeningGate {
    output_path_extension_aepx: bool,
    output_path_has_no_parent_traversal: bool,
    output_path_under_generated_target_root: bool,
    output_parent_canonical_under_generated_target_root: bool,
    replacement_value_xml_safe: bool,
    exact_byte_diff_verified: bool,
    all_operations_resolved_before_write: bool,
    approved_spans_non_overlapping: bool,
    approved_span_count: u32,
}

#[derive(Debug, Clone)]
struct ResolvedReplacement {
    span: std::ops::Range<usize>,
    old_value: String,
    new_value: String,
}

#[derive(Debug, Serialize)]
struct PrivacyGate {
    input_path_embedded: bool,
    output_path_embedded: bool,
    selector_value_embedded: bool,
    expected_old_value_embedded: bool,
    new_value_embedded: bool,
    xml_sentinels_embedded: bool,
    xml_body_embedded: bool,
}

pub fn run_synthetic_preservation_proof_request_json(
    request_json: &str,
) -> Result<String, Box<dyn Error>> {
    let request: SyntheticProofRequest = serde_json::from_str(strip_json_bom(request_json))?;
    let report = run_synthetic_preservation_proof_request(&request);
    Ok(serde_json::to_string_pretty(&report)?)
}

fn run_synthetic_preservation_proof_request(
    request: &SyntheticProofRequest,
) -> SyntheticPreservationProofReport {
    let mut blocked = Vec::new();
    validate_request_boundary(request, &mut blocked);
    let operations = selected_operations(request);
    let (report_operation_kind, report_selector_kind) = report_operation_shape(&operations);
    if !blocked.is_empty() {
        return SyntheticPreservationProofReport::blocked(
            "invalid_request",
            &report_operation_kind,
            report_selector_kind,
            false,
            false,
            blocked,
        );
    }

    let fixture = fixture_path(&request.input_fixture_name);
    let output = PathBuf::from(&request.output_aepx);
    if !output_path_has_allowed_extension(&output) {
        return SyntheticPreservationProofReport::blocked(
            "invalid_request",
            &report_operation_kind,
            report_selector_kind,
            false,
            false,
            vec!["output must have .aepx extension".to_string()],
        );
    }
    if path_has_traversal(&output) {
        return SyntheticPreservationProofReport::blocked(
            "invalid_request",
            &report_operation_kind,
            report_selector_kind,
            false,
            false,
            vec!["output path must not contain traversal components".to_string()],
        );
    }
    if !output_is_under_target_root(&output) {
        return SyntheticPreservationProofReport::blocked(
            "invalid_request",
            &report_operation_kind,
            report_selector_kind,
            false,
            false,
            vec!["output must be under target/aepx-synthetic-preservation-proof".to_string()],
        );
    }
    if output.exists() {
        return SyntheticPreservationProofReport::blocked(
            "output_exists",
            &report_operation_kind,
            report_selector_kind,
            false,
            false,
            vec!["create-new output already exists".to_string()],
        );
    }

    let source = match std::fs::read_to_string(&fixture) {
        Ok(source) => source,
        Err(_) => {
            return SyntheticPreservationProofReport::blocked(
                "target_not_found",
                &report_operation_kind,
                report_selector_kind,
                true,
                false,
                vec!["synthetic fixture could not be read".to_string()],
            );
        }
    };

    if operations.len() == 1 && operations[0].selector_kind == "name" {
        return report_name_selector_probe(&operations[0], &source);
    }

    let replacements = match resolve_replacements(&source, &operations) {
        Ok(replacements) => replacements,
        Err(report) => return *report,
    };
    if replacement_spans_overlap(&replacements) {
        return SyntheticPreservationProofReport::blocked(
            "preservation_failed",
            &report_operation_kind,
            report_selector_kind,
            true,
            false,
            vec!["approved replacement spans overlapped".to_string()],
        );
    }
    let patched = apply_replacements(&source, &replacements);
    if !multi_replacement_preserves_unapproved_bytes(&source, &patched, &replacements) {
        return SyntheticPreservationProofReport::blocked(
            "preservation_failed",
            &report_operation_kind,
            report_selector_kind,
            true,
            false,
            vec!["exact byte-diff preservation proof failed".to_string()],
        );
    }
    let approved_span_count = replacements.len() as u32;
    match write_create_new(&output, &patched) {
        Ok(()) => SyntheticPreservationProofReport::ready(
            &report_operation_kind,
            report_selector_kind.as_deref().unwrap_or("unknown"),
            approved_span_count,
        ),
        Err(ProofWriteError::OutputExists) => SyntheticPreservationProofReport::blocked(
            "output_exists",
            &report_operation_kind,
            report_selector_kind,
            true,
            false,
            vec!["create-new output already exists".to_string()],
        ),
        Err(ProofWriteError::WriteFailed) => SyntheticPreservationProofReport::blocked(
            "write_failed",
            &report_operation_kind,
            report_selector_kind,
            true,
            false,
            vec!["synthetic output write failed".to_string()],
        ),
    }
}

fn report_name_selector_probe(
    operation: &SyntheticProofOperation,
    source: &str,
) -> SyntheticPreservationProofReport {
    let matches = match comp_tag_matches_by_attribute(source, "name", &operation.selector_value) {
        Ok(matches) => matches,
        Err(_) => {
            return SyntheticPreservationProofReport::blocked(
                "parse_error",
                &operation.kind,
                safe_selector_kind(&operation.selector_kind),
                true,
                false,
                vec!["synthetic XML parse failed".to_string()],
            );
        }
    };
    if matches.len() > 1 {
        return SyntheticPreservationProofReport::blocked(
            "ambiguous_target",
            &operation.kind,
            safe_selector_kind(&operation.selector_kind),
            true,
            false,
            vec!["name selector matched more than one synthetic element".to_string()],
        );
    }
    SyntheticPreservationProofReport::blocked(
        "invalid_request",
        &operation.kind,
        safe_selector_kind(&operation.selector_kind),
        true,
        false,
        vec!["name selectors cannot produce preservation proof".to_string()],
    )
}

fn resolve_replacements(
    source: &str,
    operations: &[SyntheticProofOperation],
) -> ProofResult<Vec<ResolvedReplacement>> {
    let mut replacements = Vec::new();
    for operation in operations {
        let selector_kind = operation.selector_kind.as_str();
        if selector_kind != "xml_id" {
            return Err(Box::new(SyntheticPreservationProofReport::blocked(
                "invalid_request",
                &operation.kind,
                safe_selector_kind(selector_kind),
                true,
                false,
                vec!["only xml_id selectors can produce preservation proof".to_string()],
            )));
        }
        let matches = comp_tag_matches_by_attribute(source, "id", &operation.selector_value);
        let matches = match matches {
            Ok(matches) => matches,
            Err(_) => {
                return Err(Box::new(SyntheticPreservationProofReport::blocked(
                    "parse_error",
                    &operation.kind,
                    safe_selector_kind(selector_kind),
                    true,
                    false,
                    vec!["synthetic XML parse failed".to_string()],
                )));
            }
        };
        if matches.is_empty() {
            return Err(Box::new(SyntheticPreservationProofReport::blocked(
                "target_not_found",
                &operation.kind,
                safe_selector_kind(selector_kind),
                true,
                false,
                vec!["target was not found".to_string()],
            )));
        }
        if matches.len() > 1 {
            return Err(Box::new(SyntheticPreservationProofReport::blocked(
                "ambiguous_target",
                &operation.kind,
                safe_selector_kind(selector_kind),
                true,
                false,
                vec!["target selector matched more than one synthetic element".to_string()],
            )));
        }

        let (start, end) = matches[0];
        let tag = &source[start..end];
        let Some((value_start, value_end, old_value)) = attribute_value_span(tag, "name") else {
            return Err(Box::new(SyntheticPreservationProofReport::blocked(
                "target_not_found",
                &operation.kind,
                safe_selector_kind(selector_kind),
                true,
                false,
                vec!["target name attribute was not found".to_string()],
            )));
        };
        if old_value != operation.expected_old_value {
            return Err(Box::new(SyntheticPreservationProofReport::blocked(
                "expected_value_mismatch",
                &operation.kind,
                safe_selector_kind(selector_kind),
                true,
                false,
                vec!["expected old value did not match".to_string()],
            )));
        }
        replacements.push(ResolvedReplacement {
            span: (start + value_start)..(start + value_end),
            old_value,
            new_value: operation.new_value.clone(),
        });
    }
    Ok(replacements)
}

impl SyntheticPreservationProofReport {
    fn ready(operation_kind: &str, selector_kind: &str, approved_span_count: u32) -> Self {
        Self {
            schema_version: 1,
            report_kind: REPORT_KIND.to_string(),
            publication_status: "local-only-synthetic-fixture".to_string(),
            status: READY_STATUS.to_string(),
            synthetic_fixture_only: true,
            proof_mode: "exact_id_span_splice_synthetic_only".to_string(),
            operation_kind: safe_operation_kind(operation_kind),
            selector_kind: safe_selector_kind(selector_kind),
            target_count: approved_span_count,
            preservation: PreservationReport::preserved(),
            io_gate: IoGate {
                synthetic_xml_body_read_performed: true,
                xml_body_embedded_in_report: false,
                private_payloads_embedded_in_report: false,
                external_process_invoked: false,
                after_effects_invoked: false,
            },
            write_gate: WriteGate {
                output_write_performed: true,
                output_write_mode: "create_new_synthetic_target_only".to_string(),
                output_under_generated_target_root: true,
                source_overwrite_performed: false,
                production_apply_enabled: false,
            },
            hardening_gate: HardeningGate::ready(approved_span_count),
            privacy_gate: PrivacyGate::private_payloads_omitted(),
            blocked_reasons: Vec::new(),
            notes: notes(),
        }
    }

    fn blocked(
        status: &str,
        operation_kind: &str,
        selector_kind: Option<String>,
        xml_body_read: bool,
        output_written: bool,
        blocked_reasons: Vec<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            report_kind: REPORT_KIND.to_string(),
            publication_status: "local-only-synthetic-fixture".to_string(),
            status: status.to_string(),
            synthetic_fixture_only: true,
            proof_mode: "exact_id_span_splice_synthetic_only".to_string(),
            operation_kind: safe_operation_kind(operation_kind),
            selector_kind,
            target_count: 0,
            preservation: PreservationReport::not_written(),
            io_gate: IoGate {
                synthetic_xml_body_read_performed: xml_body_read,
                xml_body_embedded_in_report: false,
                private_payloads_embedded_in_report: false,
                external_process_invoked: false,
                after_effects_invoked: false,
            },
            write_gate: WriteGate {
                output_write_performed: output_written,
                output_write_mode: "not_written".to_string(),
                output_under_generated_target_root: false,
                source_overwrite_performed: false,
                production_apply_enabled: false,
            },
            hardening_gate: HardeningGate::blocked(),
            privacy_gate: PrivacyGate::private_payloads_omitted(),
            blocked_reasons,
            notes: notes(),
        }
    }
}

impl PreservationReport {
    fn preserved() -> Self {
        Self {
            unknown_nodes: "preserved".to_string(),
            unknown_attributes: "preserved".to_string(),
            xml_declaration: "preserved".to_string(),
            encoding: "preserved".to_string(),
            comments: "preserved".to_string(),
            cdata: "preserved".to_string(),
            namespace_prefixes: "preserved".to_string(),
            whitespace: "preserved".to_string(),
        }
    }

    fn not_written() -> Self {
        Self {
            unknown_nodes: "not_written".to_string(),
            unknown_attributes: "not_written".to_string(),
            xml_declaration: "not_written".to_string(),
            encoding: "not_written".to_string(),
            comments: "not_written".to_string(),
            cdata: "not_written".to_string(),
            namespace_prefixes: "not_written".to_string(),
            whitespace: "not_written".to_string(),
        }
    }
}

impl PrivacyGate {
    fn private_payloads_omitted() -> Self {
        Self {
            input_path_embedded: false,
            output_path_embedded: false,
            selector_value_embedded: false,
            expected_old_value_embedded: false,
            new_value_embedded: false,
            xml_sentinels_embedded: false,
            xml_body_embedded: false,
        }
    }
}

impl HardeningGate {
    fn ready(approved_span_count: u32) -> Self {
        Self {
            output_path_extension_aepx: true,
            output_path_has_no_parent_traversal: true,
            output_path_under_generated_target_root: true,
            output_parent_canonical_under_generated_target_root: true,
            replacement_value_xml_safe: true,
            exact_byte_diff_verified: true,
            all_operations_resolved_before_write: true,
            approved_spans_non_overlapping: true,
            approved_span_count,
        }
    }

    fn blocked() -> Self {
        Self {
            output_path_extension_aepx: false,
            output_path_has_no_parent_traversal: false,
            output_path_under_generated_target_root: false,
            output_parent_canonical_under_generated_target_root: false,
            replacement_value_xml_safe: false,
            exact_byte_diff_verified: false,
            all_operations_resolved_before_write: false,
            approved_spans_non_overlapping: false,
            approved_span_count: 0,
        }
    }
}

fn validate_request_boundary(request: &SyntheticProofRequest, blocked: &mut Vec<String>) {
    if request.schema_version != 1 {
        blocked.push("schema_version must be 1".to_string());
    }
    if request.proof_kind != REPORT_KIND {
        blocked.push("proof_kind must be aepx_synthetic_preservation_proof".to_string());
    }
    if !ALLOWED_FIXTURES.contains(&request.input_fixture_name.as_str())
        || request.input_fixture_name.contains('/')
        || request.input_fixture_name.contains('\\')
        || request.input_fixture_name.contains("..")
    {
        blocked.push("input fixture must be an allowed synthetic fixture name".to_string());
    }
    if request.operation.is_some() == request.operations.is_some() {
        blocked.push("request must provide exactly one of operation or operations".to_string());
    }
    let operations = selected_operations(request);
    if operations.is_empty() {
        blocked.push("at least one operation is required".to_string());
    }
    if operations.len() > 4 {
        blocked.push("at most four synthetic proof operations are allowed".to_string());
    }
    for operation in operations {
        if operation.kind != "rename_comp" {
            blocked.push("only rename_comp is supported in the synthetic proof".to_string());
        }
        if operation.selector_value.trim().is_empty()
            || operation.expected_old_value.trim().is_empty()
            || operation.new_value.trim().is_empty()
        {
            blocked.push("selector and edit values must be non-empty".to_string());
        }
        if !xml_attribute_replacement_value_is_safe(&operation.new_value) {
            blocked.push("new value must be XML attribute safe without escaping".to_string());
        }
        if request.operations.is_some() && operation.selector_kind != "xml_id" {
            blocked.push("multi-operation proof requires xml_id selectors".to_string());
        }
    }
    if !request.options.synthetic_fixture_only {
        blocked.push("synthetic_fixture_only must be true".to_string());
    }
    if !request.options.create_new_only {
        blocked.push("create_new_only must be true".to_string());
    }
    if request.options.preserve_unknown_xml != "required" {
        blocked.push("preserve_unknown_xml must be required".to_string());
    }
    if request.options.report_private_payloads {
        blocked.push("report_private_payloads must be false".to_string());
    }
}

fn selected_operations(request: &SyntheticProofRequest) -> Vec<SyntheticProofOperation> {
    if let Some(operation) = &request.operation {
        vec![operation.clone()]
    } else {
        request.operations.clone().unwrap_or_default()
    }
}

fn report_operation_shape(operations: &[SyntheticProofOperation]) -> (String, Option<String>) {
    if operations.is_empty() {
        return ("unknown".to_string(), None);
    }
    let operation_kind = if operations
        .iter()
        .all(|operation| operation.kind == operations[0].kind)
    {
        safe_operation_kind(&operations[0].kind)
    } else {
        "mixed".to_string()
    };
    let selector_kind = if operations
        .iter()
        .all(|operation| operation.selector_kind == operations[0].selector_kind)
    {
        safe_selector_kind(&operations[0].selector_kind)
    } else {
        None
    };
    (operation_kind, selector_kind)
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn target_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-synthetic-preservation-proof")
}

fn output_is_under_target_root(path: &Path) -> bool {
    let output = absolute_like(path);
    let root = absolute_like(&target_root());
    output.starts_with(root)
}

fn output_path_has_allowed_extension(path: &Path) -> bool {
    path_has_extension(path, "aepx")
}

fn report_path_has_allowed_extension(path: &Path) -> bool {
    path_has_extension(path, "json")
}

fn path_has_extension(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn path_has_traversal(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    })
}

fn absolute_like(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
    }
}

fn comp_tag_matches_by_attribute(
    source: &str,
    attr_name: &str,
    attr_value: &str,
) -> Result<Vec<(usize, usize)>, ()> {
    let mut cursor = 0usize;
    let mut matches = Vec::new();
    while let Some(relative_start) = source[cursor..].find("<xmp:comp") {
        let start = cursor + relative_start;
        if is_inside_xml_ignored_section(source, start) {
            cursor = start + 1;
            continue;
        }
        if !tag_name_boundary_matches(source, start, "xmp:comp") {
            cursor = start + 1;
            continue;
        }
        let Some(relative_end) = find_tag_end(source, start) else {
            return Err(());
        };
        let end = start + relative_end + 1;
        let tag = &source[start..end];
        if attribute_value(tag, attr_name).as_deref() == Some(attr_value) {
            matches.push((start, end));
        }
        cursor = end;
    }
    Ok(matches)
}

fn is_inside_xml_ignored_section(source: &str, position: usize) -> bool {
    is_inside_delimited_section(source, position, "<!--", "-->")
        || is_inside_delimited_section(source, position, "<![CDATA[", "]]>")
}

fn is_inside_delimited_section(source: &str, position: usize, open: &str, close: &str) -> bool {
    let before = &source[..position];
    match before.rfind(open) {
        Some(open_at) => before
            .rfind(close)
            .is_none_or(|close_at| close_at < open_at),
        None => false,
    }
}

fn tag_name_boundary_matches(source: &str, start: usize, tag_name: &str) -> bool {
    let bytes = source.as_bytes();
    let name_start = start + 1;
    let name_end = name_start + tag_name.len();
    if name_end > bytes.len() || &source[name_start..name_end] != tag_name {
        return false;
    }
    bytes
        .get(name_end)
        .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/'))
}

fn find_tag_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = start;
    let mut quote = None;
    while index < bytes.len() {
        match (quote, bytes[index]) {
            (Some(active), byte) if byte == active => quote = None,
            (None, b'"' | b'\'') => quote = Some(bytes[index]),
            (None, b'>') => return Some(index - start),
            _ => {}
        }
        index += 1;
    }
    None
}

fn attribute_value(tag: &str, expected_name: &str) -> Option<String> {
    attribute_value_span(tag, expected_name).map(|(_, _, value)| value)
}

fn attribute_value_span(tag: &str, expected_name: &str) -> Option<(usize, usize, String)> {
    let bytes = tag.as_bytes();
    let mut index = tag.find(char::is_whitespace).unwrap_or(tag.len());
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] == b'>' || bytes[index] == b'/' {
            return None;
        }
        let name_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && !matches!(bytes[index], b'=' | b'>' | b'/')
        {
            index += 1;
        }
        let name = &tag[name_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'>'
            {
                index += 1;
            }
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || !matches!(bytes[index], b'"' | b'\'') {
            return None;
        }
        let quote = bytes[index];
        index += 1;
        let value_start = index;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        let value_end = index;
        let value = tag[value_start..value_end].to_owned();
        index += 1;
        if name == expected_name {
            return Some((value_start, value_end, value));
        }
    }
    None
}

fn xml_attribute_replacement_value_is_safe(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| !matches!(ch, '<' | '&' | '"' | '\'') && (ch >= ' ' || ch == '\t'))
}

fn replacement_spans_overlap(replacements: &[ResolvedReplacement]) -> bool {
    let mut spans: Vec<_> = replacements
        .iter()
        .map(|replacement| replacement.span.clone())
        .collect();
    spans.sort_by_key(|span| span.start);
    spans.windows(2).any(|pair| pair[1].start < pair[0].end)
}

fn apply_replacements(source: &str, replacements: &[ResolvedReplacement]) -> String {
    let mut patched = source.to_string();
    let mut ordered = replacements.to_vec();
    ordered.sort_by_key(|replacement| std::cmp::Reverse(replacement.span.start));
    for replacement in ordered {
        patched.replace_range(replacement.span, &replacement.new_value);
    }
    patched
}

fn multi_replacement_preserves_unapproved_bytes(
    source: &str,
    patched: &str,
    replacements: &[ResolvedReplacement],
) -> bool {
    let mut ordered = replacements.to_vec();
    ordered.sort_by_key(|replacement| replacement.span.start);
    let mut rebuilt = String::new();
    let mut source_cursor = 0usize;
    for replacement in ordered {
        if replacement.span.start < source_cursor
            || replacement.span.end > source.len()
            || !source.is_char_boundary(replacement.span.start)
            || !source.is_char_boundary(replacement.span.end)
        {
            return false;
        }
        if source.get(replacement.span.clone()) != Some(replacement.old_value.as_str()) {
            return false;
        }
        let Some(prefix) = source.get(source_cursor..replacement.span.start) else {
            return false;
        };
        rebuilt.push_str(prefix);
        rebuilt.push_str(&replacement.new_value);
        source_cursor = replacement.span.end;
    }
    let Some(suffix) = source.get(source_cursor..) else {
        return false;
    };
    rebuilt.push_str(suffix);
    rebuilt == patched
}

#[derive(Debug, PartialEq, Eq)]
enum ProofWriteError {
    OutputExists,
    WriteFailed,
}

fn write_create_new(path: &Path, text: &str) -> Result<(), ProofWriteError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ProofWriteError::WriteFailed)?;
    }
    if !output_parent_canonical_is_under_target_root(path) {
        return Err(ProofWriteError::WriteFailed);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| {
            if err.kind() == ErrorKind::AlreadyExists {
                ProofWriteError::OutputExists
            } else {
                ProofWriteError::WriteFailed
            }
        })?;
    if file.write_all(text.as_bytes()).is_err() {
        let _ = std::fs::remove_file(path);
        return Err(ProofWriteError::WriteFailed);
    }
    Ok(())
}

fn safe_operation_kind(operation_kind: &str) -> String {
    if operation_kind == "rename_comp" {
        "rename_comp".to_string()
    } else {
        "unsupported".to_string()
    }
}

fn safe_selector_kind(selector_kind: &str) -> Option<String> {
    match selector_kind {
        "xml_id" => Some("xml_id".to_string()),
        "name" => Some("name".to_string()),
        _ => None,
    }
}

fn notes() -> Vec<String> {
    vec![
        "Synthetic preservation proof reads checked-in synthetic fixtures only.".to_string(),
        "The proof is not production AEPX apply and does not claim AE round-trip compatibility.".to_string(),
        "The report omits XML bodies, selector values, expected values, new values, sentinels, and paths.".to_string(),
        "After Effects is not launched and no external process is invoked.".to_string(),
        "The proof verifies exact byte-diff preservation outside the approved replacement span.".to_string(),
        "CLI report output must be a create-new JSON file under the synthetic proof target root.".to_string(),
        "Multi-operation proof resolves all spans and rejects overlaps before writing.".to_string(),
    ]
}

fn strip_json_bom(input: &str) -> &str {
    input.strip_prefix('\u{feff}').unwrap_or(input)
}

fn parse_args() -> Result<(PathBuf, PathBuf), String> {
    let mut request = None;
    let mut report = PathBuf::from("target")
        .join("aepx-synthetic-preservation-proof")
        .join("proof.local.json");
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--request" => {
                request = args.next().map(PathBuf::from);
                if request.is_none() {
                    return Err("--request requires a JSON path".to_string());
                }
            }
            "--report" => {
                report = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--report requires a JSON path".to_string())?;
            }
            "--help" | "-h" => {
                return Err("usage: aepx_synthetic_preservation_proof --request request.json [--report target/aepx-synthetic-preservation-proof/proof.local.json]".to_string());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok((
        request.ok_or_else(|| "--request is required".to_string())?,
        report,
    ))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (request_path, report_path) = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let request = std::fs::read_to_string(&request_path)?;
    let report = run_synthetic_preservation_proof_request_json(&request)?;
    write_report_create_new(&report_path, &report)?;
    println!("{}", report_path.display());
    Ok(())
}

fn write_report_create_new(path: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    validate_synthetic_preservation_report_output_path(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !output_parent_canonical_is_under_target_root(path) {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            "report output parent must resolve under target/aepx-synthetic-preservation-proof",
        )
        .into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| {
            if err.kind() == ErrorKind::AlreadyExists {
                IoError::new(ErrorKind::AlreadyExists, "report output already exists")
            } else {
                err
            }
        })?;
    file.write_all(text.as_bytes())?;
    Ok(())
}

fn output_parent_canonical_is_under_target_root(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(root) = std::fs::canonicalize(target_root()) else {
        return false;
    };
    let Ok(parent) = std::fs::canonicalize(parent) else {
        return false;
    };
    parent.starts_with(root)
}

pub fn validate_synthetic_preservation_report_output_path(
    path: &Path,
) -> Result<(), Box<dyn Error>> {
    if !report_path_has_allowed_extension(path) {
        return Err(IoError::new(ErrorKind::InvalidInput, "report output must be .json").into());
    }
    if path_has_traversal(path) {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            "report output path must not contain traversal components",
        )
        .into());
    }
    if !output_is_under_target_root(path) {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            "report output must be under target/aepx-synthetic-preservation-proof",
        )
        .into());
    }
    Ok(())
}
