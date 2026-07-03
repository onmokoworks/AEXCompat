//! Contract-first AEPX patch request validator.
//!
//! This example validates the safe request boundary for future `.aepx`
//! structural patching. It deliberately does not parse XML, rewrite projects,
//! launch After Effects, or read private project payloads.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

const ALLOWED_MODES: &[&str] = &["inspect_metadata", "noop_validate", "dry_run", "apply"];
const ALLOWED_EDIT_KINDS: &[&str] = &[
    "rename_comp",
    "rename_layer",
    "set_comment",
    "set_marker",
    "replace_text_source",
    "relink_asset_path",
];
const ALLOWED_TARGET_KINDS: &[&str] =
    &["project", "comp", "layer", "marker", "text_source", "asset"];

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AepxPatchRequest {
    pub schema_version: u32,
    pub operation: String,
    pub input_aepx: String,
    #[serde(default)]
    pub output_aepx: Option<String>,
    #[serde(default)]
    pub patch_id: Option<String>,
    pub publication_status: String,
    #[serde(default)]
    pub operations: Vec<PatchOperation>,
    pub options: PatchOptions,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatchOperation {
    pub id: String,
    pub kind: String,
    pub target: PatchTarget,
    #[serde(default)]
    pub expected_old_value: Option<Value>,
    #[serde(default)]
    pub new_value: Option<Value>,
    #[serde(default)]
    pub user_supplied: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatchTarget {
    pub kind: String,
    pub selector: PatchSelector,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatchSelector {
    #[serde(default)]
    pub xml_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub comp_name: Option<String>,
    #[serde(default)]
    pub layer_name: Option<String>,
    #[serde(default)]
    pub asset_path: Option<String>,
    #[serde(default)]
    pub marker_index: Option<u64>,
}

impl PatchSelector {
    fn is_empty(&self) -> bool {
        self.xml_id.is_none()
            && self.name.is_none()
            && self.comp_name.is_none()
            && self.layer_name.is_none()
            && self.asset_path.is_none()
            && self.marker_index.is_none()
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatchOptions {
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub allow_ambiguous_selector: bool,
    pub preserve_unknown_xml: String,
    pub normalization_mode: String,
    #[serde(default)]
    pub max_file_bytes: Option<u64>,
    #[serde(default)]
    pub max_operations: Option<usize>,
    #[serde(default)]
    pub report_private_payloads: bool,
}

#[derive(Debug, Serialize)]
pub struct AepxPatchReport {
    pub schema_version: u32,
    pub status: String,
    pub aepx_patch_report_binding: AepxPatchReportBinding,
    pub input_aepx: Option<String>,
    pub output_aepx: Option<String>,
    pub metadata: MetadataReport,
    pub operations: Vec<OperationReport>,
    pub preservation: PreservationReport,
    pub io_gate: IoGate,
    pub write_gate: WriteGate,
    pub warnings: Vec<String>,
    pub unsupported: Vec<String>,
    pub elapsed_ms: u128,
}

#[derive(Debug, Serialize)]
pub struct AepxPatchReportBinding {
    pub algorithm: String,
    pub checksum_hex: String,
    pub checksum_scope: String,
    pub fields: Vec<String>,
    pub payloads_embedded: bool,
    pub covers_xml_or_private_patch_payloads: bool,
    pub cryptographic_digest: BindingDigest,
}

#[derive(Debug, Serialize)]
pub struct BindingDigest {
    pub algorithm: String,
    pub digest_hex: String,
    pub digest_scope: String,
    pub payloads_embedded: bool,
    pub covers_xml_or_private_patch_payloads: bool,
}

#[derive(Debug, Serialize)]
pub struct MetadataReport {
    pub input_exists: bool,
    pub input_is_file: bool,
    pub observed_file_bytes: Option<u64>,
    pub normalized_input_aepx: Option<String>,
    pub normalized_output_aepx: Option<String>,
    pub xml_body_read: bool,
}

#[derive(Debug, Serialize)]
pub struct OperationReport {
    pub id: String,
    pub status: String,
    pub target_count: u32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PreservationReport {
    pub unknown_nodes: String,
    pub unknown_attributes: String,
    pub xml_declaration: String,
    pub encoding: String,
    pub comments: String,
    pub cdata: String,
    pub namespace_prefixes: String,
    pub whitespace: String,
}

#[derive(Debug, Serialize)]
pub struct IoGate {
    pub xml_body_read_performed: bool,
    pub xml_body_write_performed: bool,
    pub xml_body_embedded_in_report: bool,
    pub private_patch_payloads_embedded_in_report: bool,
    pub external_process_invoked: bool,
    pub after_effects_invoked: bool,
}

impl IoGate {
    fn metadata_only() -> Self {
        Self {
            xml_body_read_performed: false,
            xml_body_write_performed: false,
            xml_body_embedded_in_report: false,
            private_patch_payloads_embedded_in_report: false,
            external_process_invoked: false,
            after_effects_invoked: false,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WriteGate {
    pub patch_application_status: String,
    pub xml_writer_status: String,
    pub user_approval_status: String,
    pub output_write_performed: bool,
    pub source_overwrite_performed: bool,
}

impl WriteGate {
    fn no_write() -> Self {
        Self {
            patch_application_status: "not_applied".to_owned(),
            xml_writer_status: "not_implemented".to_owned(),
            user_approval_status: "required_before_write".to_owned(),
            output_write_performed: false,
            source_overwrite_performed: false,
        }
    }
}

impl AepxPatchReport {
    fn new(status: &str, elapsed_ms: u128) -> Self {
        let mut report = Self {
            schema_version: 1,
            status: status.to_owned(),
            aepx_patch_report_binding: AepxPatchReportBinding::empty(),
            input_aepx: None,
            output_aepx: None,
            metadata: MetadataReport::empty(),
            operations: Vec::new(),
            preservation: PreservationReport {
                unknown_nodes: "not_written".to_owned(),
                unknown_attributes: "not_written".to_owned(),
                xml_declaration: "not_written".to_owned(),
                encoding: "not_written".to_owned(),
                comments: "not_written".to_owned(),
                cdata: "not_written".to_owned(),
                namespace_prefixes: "not_written".to_owned(),
                whitespace: "not_written".to_owned(),
            },
            io_gate: IoGate::metadata_only(),
            write_gate: WriteGate::no_write(),
            warnings: Vec::new(),
            unsupported: Vec::new(),
            elapsed_ms,
        };
        report.refresh_binding();
        report
    }

    fn new_with_metadata(status: &str, elapsed_ms: u128, metadata: MetadataReport) -> Self {
        let mut report = Self::new(status, elapsed_ms);
        report.metadata = metadata;
        report.refresh_binding();
        report
    }

    fn invalid(message: impl Into<String>, elapsed_ms: u128) -> Self {
        let mut report = Self::new("invalid_request", elapsed_ms);
        report.warnings.push(message.into());
        report.refresh_binding();
        report
    }

    fn invalid_with_metadata(
        message: impl Into<String>,
        elapsed_ms: u128,
        metadata: MetadataReport,
    ) -> Self {
        let mut report = Self::new_with_metadata("invalid_request", elapsed_ms, metadata);
        report.warnings.push(message.into());
        report.refresh_binding();
        report
    }

    fn refresh_binding(&mut self) {
        self.aepx_patch_report_binding = AepxPatchReportBinding::from_report(self);
    }
}

impl AepxPatchReportBinding {
    fn empty() -> Self {
        Self {
            algorithm: "fnv1a64-v1-noncryptographic".to_owned(),
            checksum_hex: "0000000000000000".to_owned(),
            checksum_scope: "metadata-only AEPX patch report binding; excludes XML body, selectors, expected/new values, and private patch payloads".to_owned(),
            fields: Vec::new(),
            payloads_embedded: false,
            covers_xml_or_private_patch_payloads: false,
            cryptographic_digest: BindingDigest {
                algorithm: "sha256-v1".to_owned(),
                digest_hex: "0".repeat(64),
                digest_scope: "metadata-only AEPX patch report digest; excludes XML body, selectors, expected/new values, and private patch payloads".to_owned(),
                payloads_embedded: false,
                covers_xml_or_private_patch_payloads: false,
            },
        }
    }

    fn from_report(report: &AepxPatchReport) -> Self {
        let fields = vec![
            "schema_version",
            "status",
            "input_aepx",
            "output_aepx",
            "metadata.input_exists",
            "metadata.input_is_file",
            "metadata.observed_file_bytes",
            "metadata.normalized_input_aepx",
            "metadata.normalized_output_aepx",
            "metadata.xml_body_read",
            "operation_count",
            "operation_statuses",
            "operation_target_counts",
            "preservation.unknown_nodes",
            "preservation.unknown_attributes",
            "preservation.xml_declaration",
            "preservation.encoding",
            "preservation.comments",
            "preservation.cdata",
            "preservation.namespace_prefixes",
            "preservation.whitespace",
            "io_gate.xml_body_read_performed",
            "io_gate.xml_body_write_performed",
            "io_gate.xml_body_embedded_in_report",
            "io_gate.private_patch_payloads_embedded_in_report",
            "io_gate.external_process_invoked",
            "io_gate.after_effects_invoked",
            "write_gate.patch_application_status",
            "write_gate.xml_writer_status",
            "write_gate.user_approval_status",
            "write_gate.output_write_performed",
            "write_gate.source_overwrite_performed",
            "warning_count",
            "unsupported_count",
        ];
        let operation_statuses = report
            .operations
            .iter()
            .map(|operation| operation.status.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let operation_target_counts = report
            .operations
            .iter()
            .map(|operation| operation.target_count.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut canonical = String::new();
        push_binding_field(
            &mut canonical,
            "schema_version",
            &report.schema_version.to_string(),
        );
        push_binding_field(&mut canonical, "status", &report.status);
        push_binding_field(
            &mut canonical,
            "input_aepx",
            report.input_aepx.as_deref().unwrap_or(""),
        );
        push_binding_field(
            &mut canonical,
            "output_aepx",
            report.output_aepx.as_deref().unwrap_or(""),
        );
        push_binding_field(
            &mut canonical,
            "metadata.input_exists",
            bool_text(report.metadata.input_exists),
        );
        push_binding_field(
            &mut canonical,
            "metadata.input_is_file",
            bool_text(report.metadata.input_is_file),
        );
        push_binding_field(
            &mut canonical,
            "metadata.observed_file_bytes",
            &report
                .metadata
                .observed_file_bytes
                .map(|value| value.to_string())
                .unwrap_or_default(),
        );
        push_binding_field(
            &mut canonical,
            "metadata.normalized_input_aepx",
            report
                .metadata
                .normalized_input_aepx
                .as_deref()
                .unwrap_or(""),
        );
        push_binding_field(
            &mut canonical,
            "metadata.normalized_output_aepx",
            report
                .metadata
                .normalized_output_aepx
                .as_deref()
                .unwrap_or(""),
        );
        push_binding_field(
            &mut canonical,
            "metadata.xml_body_read",
            bool_text(report.metadata.xml_body_read),
        );
        push_binding_field(
            &mut canonical,
            "operation_count",
            &report.operations.len().to_string(),
        );
        push_binding_field(&mut canonical, "operation_statuses", &operation_statuses);
        push_binding_field(
            &mut canonical,
            "operation_target_counts",
            &operation_target_counts,
        );
        push_binding_field(
            &mut canonical,
            "preservation.unknown_nodes",
            &report.preservation.unknown_nodes,
        );
        push_binding_field(
            &mut canonical,
            "preservation.unknown_attributes",
            &report.preservation.unknown_attributes,
        );
        push_binding_field(
            &mut canonical,
            "preservation.xml_declaration",
            &report.preservation.xml_declaration,
        );
        push_binding_field(
            &mut canonical,
            "preservation.encoding",
            &report.preservation.encoding,
        );
        push_binding_field(
            &mut canonical,
            "preservation.comments",
            &report.preservation.comments,
        );
        push_binding_field(
            &mut canonical,
            "preservation.cdata",
            &report.preservation.cdata,
        );
        push_binding_field(
            &mut canonical,
            "preservation.namespace_prefixes",
            &report.preservation.namespace_prefixes,
        );
        push_binding_field(
            &mut canonical,
            "preservation.whitespace",
            &report.preservation.whitespace,
        );
        push_binding_field(
            &mut canonical,
            "io_gate.xml_body_read_performed",
            bool_text(report.io_gate.xml_body_read_performed),
        );
        push_binding_field(
            &mut canonical,
            "io_gate.xml_body_write_performed",
            bool_text(report.io_gate.xml_body_write_performed),
        );
        push_binding_field(
            &mut canonical,
            "io_gate.xml_body_embedded_in_report",
            bool_text(report.io_gate.xml_body_embedded_in_report),
        );
        push_binding_field(
            &mut canonical,
            "io_gate.private_patch_payloads_embedded_in_report",
            bool_text(report.io_gate.private_patch_payloads_embedded_in_report),
        );
        push_binding_field(
            &mut canonical,
            "io_gate.external_process_invoked",
            bool_text(report.io_gate.external_process_invoked),
        );
        push_binding_field(
            &mut canonical,
            "io_gate.after_effects_invoked",
            bool_text(report.io_gate.after_effects_invoked),
        );
        push_binding_field(
            &mut canonical,
            "write_gate.patch_application_status",
            &report.write_gate.patch_application_status,
        );
        push_binding_field(
            &mut canonical,
            "write_gate.xml_writer_status",
            &report.write_gate.xml_writer_status,
        );
        push_binding_field(
            &mut canonical,
            "write_gate.user_approval_status",
            &report.write_gate.user_approval_status,
        );
        push_binding_field(
            &mut canonical,
            "write_gate.output_write_performed",
            bool_text(report.write_gate.output_write_performed),
        );
        push_binding_field(
            &mut canonical,
            "write_gate.source_overwrite_performed",
            bool_text(report.write_gate.source_overwrite_performed),
        );
        push_binding_field(
            &mut canonical,
            "warning_count",
            &report.warnings.len().to_string(),
        );
        push_binding_field(
            &mut canonical,
            "unsupported_count",
            &report.unsupported.len().to_string(),
        );

        Self {
            algorithm: "fnv1a64-v1-noncryptographic".to_owned(),
            checksum_hex: fnv1a64_hex(&canonical),
            checksum_scope: "metadata-only AEPX patch report binding; excludes XML body, selectors, expected/new values, and private patch payloads".to_owned(),
            fields: fields.into_iter().map(str::to_string).collect(),
            payloads_embedded: false,
            covers_xml_or_private_patch_payloads: false,
            cryptographic_digest: BindingDigest {
                algorithm: "sha256-v1".to_owned(),
                digest_hex: sha256_hex(&canonical),
                digest_scope: "metadata-only AEPX patch report digest; excludes XML body, selectors, expected/new values, and private patch payloads".to_owned(),
                payloads_embedded: false,
                covers_xml_or_private_patch_payloads: false,
            },
        }
    }
}

impl MetadataReport {
    fn empty() -> Self {
        Self {
            input_exists: false,
            input_is_file: false,
            observed_file_bytes: None,
            normalized_input_aepx: None,
            normalized_output_aepx: None,
            xml_body_read: false,
        }
    }

    fn from_request_paths(input_aepx: &str, output_aepx: Option<&str>) -> Self {
        let input_path = Path::new(input_aepx);
        let input_metadata = input_path.metadata().ok();
        let input_is_file = input_metadata
            .as_ref()
            .is_some_and(std::fs::Metadata::is_file);
        Self {
            input_exists: input_metadata.is_some(),
            input_is_file,
            observed_file_bytes: input_metadata
                .as_ref()
                .filter(|metadata| metadata.is_file())
                .map(std::fs::Metadata::len),
            normalized_input_aepx: Some(normalize_path(input_aepx)),
            normalized_output_aepx: output_aepx.map(normalize_path),
            xml_body_read: false,
        }
    }
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn push_binding_field(text: &mut String, key: &str, value: &str) {
    text.push_str(key);
    text.push('=');
    text.push_str(value);
    text.push('\n');
}

fn fnv1a64_hex(input: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn sha256_hex(input: &str) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = input.as_bytes().to_vec();
    let bit_len = (message.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let offset = i * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    h.iter().map(|word| format!("{word:08x}")).collect()
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cli = Cli::parse(&args)?;
    let request_text = std::fs::read_to_string(&cli.request)
        .with_context(|| format!("failed to read request {}", cli.request.display()))?;
    let report = run_aepx_patch_request_text(&request_text, Some(&cli.request))?;
    write_report(&cli.report, &report)?;
    Ok(())
}

#[derive(Debug)]
struct Cli {
    request: PathBuf,
    report: PathBuf,
}

impl Cli {
    fn parse(args: &[String]) -> anyhow::Result<Self> {
        let mut request = None;
        let mut report = None;
        let mut i = 1usize;
        while i < args.len() {
            match args[i].as_str() {
                "--request" => {
                    i += 1;
                    request = args.get(i).map(PathBuf::from);
                }
                "--report" => {
                    i += 1;
                    report = args.get(i).map(PathBuf::from);
                }
                "--help" | "-h" => print_usage_and_exit(),
                value => bail!("unknown argument {value}"),
            }
            i += 1;
        }
        Ok(Self {
            request: request.context("--request <path> is required")?,
            report: report.context("--report <path> is required")?,
        })
    }
}

fn print_usage_and_exit() -> ! {
    eprintln!("usage: aepx_patch_probe --request <request.json> --report <report.json>");
    std::process::exit(2);
}

pub fn run_aepx_patch_request_text(
    request_text: &str,
    _request_path: Option<&Path>,
) -> anyhow::Result<AepxPatchReport> {
    let started = Instant::now();
    let request: AepxPatchRequest = match serde_json::from_str(request_text) {
        Ok(request) => request,
        Err(err) => {
            return Ok(AepxPatchReport::invalid(
                format!("request JSON should parse: {err}"),
                started.elapsed().as_millis(),
            ));
        }
    };
    Ok(run_aepx_patch_request(
        &request,
        started.elapsed().as_millis(),
    ))
}

pub fn run_aepx_patch_request(request: &AepxPatchRequest, elapsed_ms: u128) -> AepxPatchReport {
    let mut report = validate_request(request, elapsed_ms);
    if report.status != "dry_run_ok" {
        report.refresh_binding();
        return report;
    }

    let mut report = match request.operation.as_str() {
        "inspect_metadata" => {
            if report.metadata.input_is_file {
                report.status = "ok".to_owned();
                report
                    .warnings
                    .push("metadata-only contract probe; XML body not read".to_owned());
            } else {
                report.status = "source_not_found".to_owned();
                report
                    .warnings
                    .push("input_aepx not found or is not a file; XML body not read".to_owned());
            }
            report
        }
        "noop_validate" | "dry_run" => report,
        "apply" => {
            report.status = "unsupported_operation".to_owned();
            report
                .unsupported
                .push("AEPX XML patch writing is not implemented in v0".to_owned());
            report
        }
        _ => AepxPatchReport::invalid("unsupported operation", elapsed_ms),
    };
    report.refresh_binding();
    report
}

fn validate_request(request: &AepxPatchRequest, elapsed_ms: u128) -> AepxPatchReport {
    let metadata =
        MetadataReport::from_request_paths(&request.input_aepx, request.output_aepx.as_deref());
    if request.schema_version != 1 {
        return AepxPatchReport::invalid_with_metadata(
            "schema_version must be 1",
            elapsed_ms,
            metadata,
        );
    }
    if !ALLOWED_MODES.contains(&request.operation.as_str()) {
        return AepxPatchReport::invalid_with_metadata(
            format!("unsupported operation {}", request.operation),
            elapsed_ms,
            metadata,
        );
    }
    if !Path::new(&request.input_aepx).is_absolute() {
        return AepxPatchReport::invalid_with_metadata(
            "input_aepx must be absolute",
            elapsed_ms,
            metadata,
        );
    }
    if !request.input_aepx.to_ascii_lowercase().ends_with(".aepx") {
        return AepxPatchReport::invalid_with_metadata(
            "input_aepx must end with .aepx",
            elapsed_ms,
            metadata,
        );
    }
    if request.publication_status == "unknown" {
        return AepxPatchReport::invalid_with_metadata(
            "publication_status unknown is fail-closed",
            elapsed_ms,
            metadata,
        );
    }
    if request.options.overwrite {
        return AepxPatchReport::invalid_with_metadata(
            "overwrite is rejected in v0",
            elapsed_ms,
            metadata,
        );
    }
    if request.options.allow_ambiguous_selector {
        return AepxPatchReport::invalid_with_metadata(
            "ambiguous selectors are rejected in v0",
            elapsed_ms,
            metadata,
        );
    }
    if request.options.preserve_unknown_xml != "required" {
        return AepxPatchReport::invalid_with_metadata(
            "preserve_unknown_xml must be required",
            elapsed_ms,
            metadata,
        );
    }
    if request.options.normalization_mode != "none" {
        return AepxPatchReport::invalid_with_metadata(
            "normalization_mode must be none",
            elapsed_ms,
            metadata,
        );
    }
    if request.options.report_private_payloads {
        return AepxPatchReport::invalid_with_metadata(
            "report_private_payloads true is invalid",
            elapsed_ms,
            metadata,
        );
    }
    if let Some(max_file_bytes) = request.options.max_file_bytes {
        if metadata
            .observed_file_bytes
            .is_some_and(|observed_file_bytes| observed_file_bytes > max_file_bytes)
        {
            return AepxPatchReport::invalid_with_metadata(
                "input_aepx exceeds max_file_bytes",
                elapsed_ms,
                metadata,
            );
        }
    }
    let max_operations = request.options.max_operations.unwrap_or(100);
    if request.operations.len() > max_operations {
        return AepxPatchReport::invalid_with_metadata(
            "operations exceed max_operations",
            elapsed_ms,
            metadata,
        );
    }

    for operation in &request.operations {
        if !ALLOWED_EDIT_KINDS.contains(&operation.kind.as_str()) {
            let mut report =
                AepxPatchReport::new_with_metadata("unsupported_operation", elapsed_ms, metadata);
            report
                .unsupported
                .push(format!("unsupported edit operation {}", operation.kind));
            return report;
        }
        if !ALLOWED_TARGET_KINDS.contains(&operation.target.kind.as_str()) {
            return AepxPatchReport::invalid_with_metadata(
                format!("unsupported target kind {}", operation.target.kind),
                elapsed_ms,
                metadata,
            );
        }
        if operation.target.selector.is_empty() {
            return AepxPatchReport::invalid_with_metadata(
                "operation target.selector must be a non-empty object",
                elapsed_ms,
                metadata,
            );
        }
        if let Err(message) = validate_expected_old_value_guard(operation) {
            return AepxPatchReport::invalid_with_metadata(message, elapsed_ms, metadata);
        }
        if matches!(
            operation.kind.as_str(),
            "set_comment" | "set_marker" | "replace_text_source" | "relink_asset_path"
        ) && operation.user_supplied != Some(true)
        {
            return AepxPatchReport::invalid_with_metadata(
                format!("{} requires user_supplied true", operation.kind),
                elapsed_ms,
                metadata,
            );
        }
        if let Err(message) = validate_sensitive_operation_shape(operation) {
            return AepxPatchReport::invalid_with_metadata(message, elapsed_ms, metadata);
        }
        if is_ambiguous_selector(operation) {
            let mut report =
                AepxPatchReport::new_with_metadata("ambiguous_target", elapsed_ms, metadata);
            report
                .warnings
                .push(format!("operation {} selector is ambiguous", operation.id));
            return report;
        }
    }

    if request.operation == "apply" && request.output_aepx.is_none() {
        return AepxPatchReport::invalid_with_metadata(
            "output_aepx is required for apply",
            elapsed_ms,
            metadata,
        );
    }
    if let Some(output_aepx) = request.output_aepx.as_deref() {
        if !Path::new(output_aepx).is_absolute() {
            return AepxPatchReport::invalid_with_metadata(
                "output_aepx must be absolute",
                elapsed_ms,
                metadata,
            );
        }
        if !output_aepx.to_ascii_lowercase().ends_with(".aepx") {
            return AepxPatchReport::invalid_with_metadata(
                "output_aepx must end with .aepx",
                elapsed_ms,
                metadata,
            );
        }
        if normalized_path_eq(&request.input_aepx, output_aepx) {
            let mut report =
                AepxPatchReport::new_with_metadata("output_same_as_source", elapsed_ms, metadata);
            report.input_aepx = Some(request.input_aepx.clone());
            report.output_aepx = Some(output_aepx.to_owned());
            report.warnings.push(
                "input_aepx and output_aepx must be distinct, even for non-apply modes".to_owned(),
            );
            return report;
        }
        if Path::new(output_aepx).exists() {
            let mut report =
                AepxPatchReport::new_with_metadata("output_exists", elapsed_ms, metadata);
            report.input_aepx = Some(request.input_aepx.clone());
            report.output_aepx = Some(output_aepx.to_owned());
            report
                .warnings
                .push("output_aepx already exists; dry-run refuses overwrite plans".to_owned());
            return report;
        }
    }

    let mut report = AepxPatchReport::new_with_metadata("dry_run_ok", elapsed_ms, metadata);
    report.input_aepx = Some(request.input_aepx.clone());
    report.output_aepx = request.output_aepx.clone();
    report.operations = request
        .operations
        .iter()
        .map(|operation| OperationReport {
            id: operation.id.clone(),
            status: "planned".to_owned(),
            target_count: 0,
            warnings: Vec::new(),
        })
        .collect();
    report
}

fn validate_expected_old_value_guard(operation: &PatchOperation) -> Result<(), String> {
    let Some(expected_old_value) = operation.expected_old_value.as_ref() else {
        return Err(format!(
            "{} requires expected_old_value guard",
            operation.kind
        ));
    };
    if expected_old_value.is_null() {
        return Err(format!(
            "{} requires expected_old_value guard",
            operation.kind
        ));
    }

    match operation.kind.as_str() {
        "rename_comp"
        | "rename_layer"
        | "set_comment"
        | "replace_text_source"
        | "relink_asset_path" => {
            if !expected_old_value.is_string() {
                return Err(format!(
                    "{} expected_old_value must be a string",
                    operation.kind
                ));
            }
        }
        "set_marker" => {
            if !expected_old_value.is_object() {
                return Err("set_marker expected_old_value must be an object".to_owned());
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_sensitive_operation_shape(operation: &PatchOperation) -> Result<(), String> {
    match operation.kind.as_str() {
        "rename_comp" | "rename_layer" | "set_comment" => {
            if operation
                .new_value
                .as_ref()
                .and_then(Value::as_str)
                .is_none_or(|value| value.is_empty())
            {
                return Err(format!(
                    "{} new_value must be a non-empty string",
                    operation.kind
                ));
            }
        }
        "set_marker" => {
            if operation.target.kind != "marker" {
                return Err("set_marker target.kind must be marker".to_owned());
            }
            validate_marker_object(
                operation.expected_old_value.as_ref(),
                "set_marker expected_old_value",
                true,
            )?;
            validate_marker_object(operation.new_value.as_ref(), "set_marker new_value", false)?;
        }
        "replace_text_source" => {
            if operation.target.kind != "text_source" {
                return Err("replace_text_source target.kind must be text_source".to_owned());
            }
            if !operation
                .expected_old_value
                .as_ref()
                .is_some_and(Value::is_string)
            {
                return Err("replace_text_source expected_old_value must be a string".to_owned());
            }
            if !operation.new_value.as_ref().is_some_and(Value::is_string) {
                return Err("replace_text_source new_value must be a string".to_owned());
            }
        }
        "relink_asset_path" => {
            if operation.target.kind != "asset" {
                return Err("relink_asset_path target.kind must be asset".to_owned());
            }
            if operation.target.selector.asset_path.is_none() {
                return Err("relink_asset_path requires selector.asset_path".to_owned());
            }
            let asset_path = operation
                .target
                .selector
                .asset_path
                .as_deref()
                .filter(|path| !path.is_empty())
                .ok_or_else(|| {
                    "relink_asset_path selector.asset_path must be a non-empty string".to_owned()
                })?;
            if operation
                .expected_old_value
                .as_ref()
                .is_none_or(|value| value.as_str() != Some(asset_path))
            {
                return Err(
                    "relink_asset_path expected_old_value must match selector.asset_path"
                        .to_owned(),
                );
            }
            if operation
                .new_value
                .as_ref()
                .and_then(Value::as_str)
                .is_none_or(|value| value.is_empty())
            {
                return Err("relink_asset_path new_value must be a non-empty string".to_owned());
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_marker_object(
    value: Option<&Value>,
    label: &str,
    allow_empty_comment: bool,
) -> Result<(), String> {
    let Some(marker) = value.and_then(Value::as_object) else {
        return Err(format!("{label} must be an object"));
    };
    for key in marker.keys() {
        if !matches!(key.as_str(), "time_seconds" | "comment") {
            return Err(format!("{label} field {key} is unsupported"));
        }
    }
    if !marker
        .get("time_seconds")
        .and_then(Value::as_f64)
        .is_some_and(|time| time >= 0.0)
    {
        return Err(format!(
            "{label}.time_seconds must be a non-negative number"
        ));
    }
    let comment_is_valid = marker
        .get("comment")
        .and_then(Value::as_str)
        .is_some_and(|comment| allow_empty_comment || !comment.is_empty());
    if !comment_is_valid {
        return Err(if allow_empty_comment {
            format!("{label}.comment must be a string")
        } else {
            format!("{label}.comment must be a non-empty string")
        });
    }
    Ok(())
}

fn is_ambiguous_selector(operation: &PatchOperation) -> bool {
    let selector = &operation.target.selector;
    if selector.xml_id.is_some() {
        return false;
    }

    match operation.target.kind.as_str() {
        "layer" | "text_source" => {
            (selector.name.is_some() || selector.layer_name.is_some())
                && selector.comp_name.is_none()
        }
        "marker" => selector.marker_index.is_some() && selector.comp_name.is_none(),
        "asset" => selector.asset_path.is_none(),
        _ => false,
    }
}

fn normalized_path_eq(left: &str, right: &str) -> bool {
    normalize_path(left) == normalize_path(right)
}

fn normalize_path(path: &str) -> String {
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
            value => parts.push(value.to_ascii_lowercase()),
        }
    }
    if prefix.is_empty() {
        parts.join("/")
    } else if parts.is_empty() {
        format!("{prefix}/")
    } else {
        format!("{prefix}/{}", parts.join("/"))
    }
}

fn write_report(path: &Path, report: &AepxPatchReport) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(report).context("report should serialize")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("failed to create new report {}", path.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(mode: &str) -> AepxPatchRequest {
        AepxPatchRequest {
            schema_version: 1,
            operation: mode.to_owned(),
            input_aepx: "D:/AviUtlas/local/source.aepx".to_owned(),
            output_aepx: Some("D:/AviUtlas/local/output.aepx".to_owned()),
            patch_id: Some("test".to_owned()),
            publication_status: "local-only".to_owned(),
            operations: vec![PatchOperation {
                id: "op_001".to_owned(),
                kind: "rename_comp".to_owned(),
                target: PatchTarget {
                    kind: "comp".to_owned(),
                    selector: PatchSelector {
                        xml_id: None,
                        name: Some("Main".to_owned()),
                        comp_name: None,
                        layer_name: None,
                        asset_path: None,
                        marker_index: None,
                    },
                },
                expected_old_value: Some(serde_json::json!("Main")),
                new_value: Some(serde_json::json!("Main Patched")),
                user_supplied: Some(true),
            }],
            options: PatchOptions {
                overwrite: false,
                allow_ambiguous_selector: false,
                preserve_unknown_xml: "required".to_owned(),
                normalization_mode: "none".to_owned(),
                max_file_bytes: Some(10_485_760),
                max_operations: Some(10),
                report_private_payloads: false,
            },
        }
    }

    #[test]
    fn dry_run_plans_without_xml_io() {
        let report = run_aepx_patch_request(&request("dry_run"), 0);
        assert_eq!(report.status, "dry_run_ok");
        assert_eq!(
            report.aepx_patch_report_binding.algorithm,
            "fnv1a64-v1-noncryptographic"
        );
        assert_eq!(report.aepx_patch_report_binding.checksum_hex.len(), 16);
        assert_eq!(
            report
                .aepx_patch_report_binding
                .cryptographic_digest
                .algorithm,
            "sha256-v1"
        );
        assert_eq!(
            report
                .aepx_patch_report_binding
                .cryptographic_digest
                .digest_hex
                .len(),
            64
        );
        assert_eq!(
            report.metadata.normalized_input_aepx.as_deref(),
            Some("d:/aviutlas/local/source.aepx")
        );
        assert_eq!(
            report.metadata.normalized_output_aepx.as_deref(),
            Some("d:/aviutlas/local/output.aepx")
        );
        assert!(!report.metadata.xml_body_read);
        assert_eq!(report.preservation.unknown_nodes, "not_written");
        assert_eq!(report.operations[0].status, "planned");
    }

    #[test]
    fn inspect_metadata_reports_metadata_without_xml_body() {
        let path = std::env::temp_dir().join(format!(
            "aepx_patch_probe_metadata_{}.aepx",
            std::process::id()
        ));
        std::fs::File::create(&path).expect("create empty aepx fixture");

        let mut request = request("inspect_metadata");
        request.input_aepx = path.display().to_string();
        request.output_aepx = None;
        let report = run_aepx_patch_request(&request, 0);

        let _ = std::fs::remove_file(&path);
        assert_eq!(report.status, "ok");
        assert!(report.metadata.input_exists);
        assert!(report.metadata.input_is_file);
        assert_eq!(report.metadata.observed_file_bytes, Some(0));
        assert!(report.metadata.normalized_input_aepx.is_some());
        assert_eq!(report.metadata.normalized_output_aepx, None);
        assert!(!report.metadata.xml_body_read);
    }

    #[test]
    fn apply_is_still_unsupported_after_validation() {
        let report = run_aepx_patch_request(&request("apply"), 0);
        assert_eq!(report.status, "unsupported_operation");
        assert_eq!(
            report.metadata.normalized_input_aepx.as_deref(),
            Some("d:/aviutlas/local/source.aepx")
        );
        assert_eq!(
            report.metadata.normalized_output_aepx.as_deref(),
            Some("d:/aviutlas/local/output.aepx")
        );
        assert!(!report.metadata.xml_body_read);
        assert!(report
            .unsupported
            .iter()
            .any(|item| item.contains("not implemented")));
    }

    #[test]
    fn unknown_publication_status_fails_closed() {
        let mut request = request("dry_run");
        request.publication_status = "unknown".to_owned();
        let report = run_aepx_patch_request(&request, 0);
        assert_eq!(report.status, "invalid_request");
    }

    #[test]
    fn same_apply_output_reports_output_same_as_source() {
        let mut request = request("apply");
        request.output_aepx = Some("d:/aviutlas/local/./SOURCE.aepx".to_owned());
        let report = run_aepx_patch_request(&request, 0);
        assert_eq!(report.status, "output_same_as_source");
        assert_eq!(
            report.metadata.normalized_input_aepx,
            report.metadata.normalized_output_aepx
        );
    }

    #[test]
    fn report_writer_uses_create_new_and_preserves_existing_report() {
        let path = std::env::temp_dir().join(format!(
            "aepx_patch_probe_report_create_new_{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        write_report(&path, &AepxPatchReport::new("dry_run_ok", 0)).unwrap();
        let err = write_report(&path, &AepxPatchReport::new("invalid_request", 0)).unwrap_err();

        let report = std::fs::read_to_string(&path).unwrap();
        assert!(report.contains("\"status\": \"dry_run_ok\""));
        assert!(err.to_string().contains("failed to create new report"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sha256_helper_matches_known_vector() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
