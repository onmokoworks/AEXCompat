//! Metadata-only AEX static classifier.
//!
//! This example reads the local static inventory JSON and emits a conservative
//! catalog. By default it does not open, hash, load, or execute `.aex` files.
//! With `--inspect-pe`, it may read PE headers, export names, and resource
//! directory names only; it still never loads or executes plug-in code.
//! With `--inspect-pipl-payload`, it performs a bounded semantic scan of PiPL
//! resource contents for already-expected strings without emitting raw bytes.
//! With `--inspect-adjacent-source`, it may read adjacent `build.rs` text for
//! PiPL declarations, still without reading private binary payloads.

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Inventory {
    schema_version: u32,
    publication_status: String,
    safety_notes: Vec<String>,
    #[serde(default)]
    aex_candidates: Vec<InventoryAexCandidate>,
}

#[derive(Debug, Deserialize)]
struct InventoryAexCandidate {
    path: String,
    bytes: u64,
    inferred_class: String,
    fixture_status: String,
}

#[derive(Debug, Serialize)]
struct Catalog {
    schema_version: u32,
    catalog_id: String,
    publication_status: String,
    status: String,
    generated_by: String,
    input: CatalogInput,
    notes: Vec<String>,
    effects: Vec<EffectRecord>,
}

#[derive(Debug, Serialize)]
struct CatalogInput {
    schema_version: u32,
    publication_status: String,
    safety_notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EffectRecord {
    schema_version: u32,
    effect_id: String,
    path: String,
    size: u64,
    source: EffectSource,
    identity: EffectIdentity,
    classification: EffectClassification,
    params: Vec<EffectParam>,
    frame: FrameContract,
    host_surfaces: HostSurfaces,
    execution: ExecutionContract,
    risk: RiskContract,
    last_probe: Option<LastProbe>,
    pe_machine: Option<String>,
    exports: Vec<String>,
    resource_summary: String,
    pipl_resource_status: String,
    pipl_resource_entries: Vec<PiplResourceEntry>,
    pipl_content_scan: PiplContentScan,
    pipl_kind: Option<String>,
    pipl_name: Option<String>,
    pipl_category: Option<String>,
    pipl_match_name: Option<String>,
    entrypoint: Option<EntryPoint>,
    adjacent_source_tree: Option<String>,
    adjacent_license: Option<String>,
    fixture_status: String,
    inferred_class: String,
    plugin_class: String,
    status: String,
    confidence: String,
    evidence: Vec<String>,
    blocked_reasons: Vec<String>,
    deferred_features: Vec<String>,
    recommended_next_action: String,
    render_capable: bool,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EffectSource {
    kind: String,
    path: String,
    path_publication: String,
    source_tree: Option<String>,
    source_license: String,
    binary_publication: String,
    origin: String,
}

#[derive(Debug, Serialize)]
struct EffectIdentity {
    display_name: Option<String>,
    match_name: Option<String>,
    category: Option<String>,
    vendor: Option<String>,
    version: Option<String>,
    entrypoint: Option<EntryPoint>,
}

#[derive(Debug, Serialize)]
struct EffectClassification {
    plugin_class: String,
    confidence: String,
    evidence: Vec<String>,
    blocked_reasons: Vec<String>,
    deferred_features: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EffectParam {}

#[derive(Debug, Serialize)]
struct FrameContract {
    pixel_formats: Vec<String>,
    preferred_pixel_format: String,
    alpha: String,
    color_management: String,
    max_width: u32,
    max_height: u32,
    transport: Vec<String>,
    time_model: TimeModel,
}

#[derive(Debug, Serialize)]
struct TimeModel {
    mode: String,
    frame_index: u32,
    time_seconds: f32,
}

#[derive(Debug, Serialize)]
struct HostSurfaces {
    aex_worker: HostSurface,
    aviutlas_external_effect: HostSurface,
    ofx_facade: HostSurface,
}

#[derive(Debug, Serialize)]
struct HostSurface {
    supported: bool,
    status: String,
}

#[derive(Debug, Serialize)]
struct ExecutionContract {
    load_status: String,
    allowlist_status: String,
    worker_required: bool,
    broker_may_load_plugin: bool,
    selectors: Vec<ExecutionSelector>,
    required_suites: Vec<String>,
    unsupported: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ExecutionSelector {
    name: String,
    status: String,
    evidence: String,
}

#[derive(Debug, Serialize)]
struct RiskContract {
    cleanroom: String,
    license: String,
    publication: String,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LastProbe {}

#[derive(Debug, Clone, Serialize)]
struct EntryPoint {
    name: String,
    source: String,
    verified_from_binary: bool,
}

#[derive(Debug)]
struct Classification {
    plugin_class: &'static str,
    status: &'static str,
    confidence: &'static str,
    evidence: Vec<String>,
    blocked_reasons: Vec<String>,
    deferred_features: Vec<String>,
    recommended_next_action: &'static str,
    entrypoint: Option<EntryPoint>,
    adjacent_source_tree: Option<String>,
    adjacent_license: Option<String>,
    pipl_kind: Option<String>,
    pipl_name: Option<String>,
    pipl_category: Option<String>,
    pipl_match_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ClassifierOptions {
    pub inspect_pe: bool,
    pub inspect_adjacent_source: bool,
    pub inspect_pipl_payload: bool,
}

#[derive(Debug, Default)]
struct PeInspection {
    pe_machine: Option<String>,
    exports: Vec<String>,
    resource_summary: String,
    pipl_resource_status: String,
    pipl_resource_entries: Vec<PiplResourceEntry>,
    pipl_content_scan: PiplContentScan,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PiplResourceEntry {
    resource_id: Option<u32>,
    resource_name: Option<String>,
    language_id: Option<u32>,
    language_name: Option<String>,
    data_size: u32,
    code_page: u32,
    contents_read: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PiplContentScan {
    status: String,
    scan_mode: String,
    bytes_limit: u32,
    bytes_read: u32,
    truncated: bool,
    contents_emitted: bool,
    matched_fields: Vec<PiplContentMatch>,
    unmatched_expected_fields: Vec<String>,
    notes: Vec<String>,
}

impl Default for PiplContentScan {
    fn default() -> Self {
        pipl_content_scan_not_requested()
    }
}

#[derive(Debug, Clone, Serialize)]
struct PiplContentMatch {
    field: String,
    value: String,
    source: String,
}

#[derive(Debug, Clone)]
struct ExpectedPiplField {
    field: String,
    value: String,
}

#[derive(Debug, Clone)]
struct PiplResourceDataEntry {
    public: PiplResourceEntry,
    data_offset: Option<usize>,
}

#[derive(Debug, Clone)]
struct ResourceDirectoryEntry {
    identifier: ResourceIdentifier,
    data_or_subdir: u32,
}

#[derive(Debug, Clone)]
enum ResourceIdentifier {
    Id(u32),
    Name(String),
}

#[derive(Debug, Default)]
struct AdjacentSourceInspection {
    identity_matches_candidate: bool,
    pipl_kind: Option<String>,
    pipl_name: Option<String>,
    pipl_category: Option<String>,
    pipl_match_name: Option<String>,
    entrypoint: Option<EntryPoint>,
    supports_smart_render: bool,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_data_ptr: u32,
    raw_data_size: u32,
}

pub fn classify_inventory_json(input: &str) -> Result<String, Box<dyn Error>> {
    classify_inventory_json_with_options(input, ClassifierOptions::default())
}

pub fn classify_inventory_json_with_options(
    input: &str,
    options: ClassifierOptions,
) -> Result<String, Box<dyn Error>> {
    let inventory: Inventory = serde_json::from_str(input)?;
    let effects = inventory
        .aex_candidates
        .iter()
        .map(|candidate| classify_candidate(candidate, options))
        .collect();

    let binary_access_note = if options.inspect_pipl_payload {
        "PE inspection performs a bounded semantic PiPL content scan; no raw bytes are emitted and no .aex code is loaded or executed."
    } else if options.inspect_pe {
        "PE inspection reads headers/export names/resource names only; no .aex code is loaded or executed."
    } else {
        "No .aex file was opened, hashed, loaded, or executed."
    };
    let mut notes = vec![
        "Classifications are inferred from inventory path/extension/metadata only.".to_string(),
        binary_access_note.to_string(),
        if options.inspect_pipl_payload {
            "PiPL content scan matches only already-expected bounded strings; no raw PiPL bytes, hashes, or unknown properties are emitted.".to_string()
        } else if options.inspect_pe {
            "PiPL payloads are not parsed; resource directory names are metadata only.".to_string()
        } else {
            "PE headers, exports, resources, and PiPL fields are not inspected in this slice."
                .to_string()
        },
        "Records are not render-capable until a reviewed worker/probe path verifies support."
            .to_string(),
    ];
    if options.inspect_adjacent_source {
        notes.push(
            "Adjacent source inspection reads build.rs PiPL declarations only; it does not read or execute plug-in binaries."
                .to_string(),
        );
    }

    let catalog = Catalog {
        schema_version: 1,
        catalog_id: "external-effects-local".to_string(),
        publication_status: "local-only".to_string(),
        status: "metadata-only-static-inference".to_string(),
        generated_by: "aex_static_classifier".to_string(),
        input: CatalogInput {
            schema_version: inventory.schema_version,
            publication_status: inventory.publication_status,
            safety_notes: inventory.safety_notes,
        },
        notes,
        effects,
    };

    Ok(serde_json::to_string_pretty(&catalog)?)
}

fn classify_candidate(
    candidate: &InventoryAexCandidate,
    options: ClassifierOptions,
) -> EffectRecord {
    let mut class = infer_classification(candidate);
    let adjacent_source = if options.inspect_adjacent_source {
        inspect_adjacent_source_candidate(candidate, &class)
    } else {
        AdjacentSourceInspection::default()
    };
    apply_adjacent_source_inspection(&mut class, &adjacent_source);
    let expected_pipl_fields = expected_pipl_fields(&class);
    let pe = if options.inspect_pe {
        inspect_pe_candidate(
            &candidate.path,
            options.inspect_pipl_payload,
            &expected_pipl_fields,
        )
    } else {
        PeInspection {
            resource_summary: "not-inspected".to_string(),
            pipl_resource_status: "not-inspected".to_string(),
            pipl_content_scan: pipl_content_scan_not_requested(),
            ..PeInspection::default()
        }
    };
    let effect_id = effect_id(&candidate.path);
    let source_tree = class.adjacent_source_tree.clone();
    let source_license = class
        .adjacent_license
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let entrypoint = class.entrypoint.clone();
    let mut evidence = class.evidence.clone();
    evidence.extend(adjacent_source.evidence.clone());
    evidence.extend(pe.evidence.clone());
    let blocked_reasons = class.blocked_reasons.clone();
    let deferred_features = class.deferred_features.clone();
    let unsupported = unsupported_features_for(&class);
    let binary_access_note = if options.inspect_pe {
        "PE header/export/resource directory metadata may be read without loading code"
    } else {
        "No .aex binary was opened, hashed, loaded, or executed"
    };
    let selectors = entrypoint
        .as_ref()
        .map(|entrypoint| {
            vec![ExecutionSelector {
                name: entrypoint.name.clone(),
                status: "not_run".to_string(),
                evidence: entrypoint.source.clone(),
            }]
        })
        .unwrap_or_default();
    EffectRecord {
        schema_version: 1,
        effect_id: effect_id.clone(),
        path: candidate.path.clone(),
        size: candidate.bytes,
        source: EffectSource {
            kind: "aex".to_string(),
            path: candidate.path.clone(),
            path_publication: "local-only".to_string(),
            source_tree: source_tree.clone(),
            source_license: source_license.clone(),
            binary_publication: "blocked-until-review".to_string(),
            origin: candidate.fixture_status.clone(),
        },
        identity: EffectIdentity {
            display_name: class
                .pipl_name
                .clone()
                .or_else(|| Some(file_stem(&candidate.path))),
            match_name: class.pipl_match_name.clone(),
            category: class.pipl_category.clone(),
            vendor: None,
            version: None,
            entrypoint: entrypoint.clone(),
        },
        classification: EffectClassification {
            plugin_class: class.plugin_class.to_string(),
            confidence: class.confidence.to_string(),
            evidence: evidence.clone(),
            blocked_reasons: blocked_reasons.clone(),
            deferred_features: deferred_features.clone(),
        },
        params: Vec::new(),
        frame: default_frame_contract(),
        host_surfaces: HostSurfaces {
            aex_worker: HostSurface {
                supported: false,
                status: if class.status == "candidate_for_contract_probe" {
                    "contract-only"
                } else {
                    "blocked"
                }
                .to_string(),
            },
            aviutlas_external_effect: HostSurface {
                supported: false,
                status: "future".to_string(),
            },
            ofx_facade: HostSurface {
                supported: false,
                status: "future".to_string(),
            },
        },
        execution: ExecutionContract {
            load_status: "not_loaded".to_string(),
            allowlist_status: if class.status == "candidate_for_contract_probe" {
                "required"
            } else {
                "denied"
            }
            .to_string(),
            worker_required: true,
            broker_may_load_plugin: false,
            selectors,
            required_suites: Vec::new(),
            unsupported,
        },
        risk: RiskContract {
            cleanroom: "local-only".to_string(),
            license: if source_license == "unknown" {
                "unknown"
            } else {
                "candidate-needs-review"
            }
            .to_string(),
            publication: "do-not-publish-binary".to_string(),
            notes: vec![
                "No binary payloads in classifier output".to_string(),
                binary_access_note.to_string(),
                "Source/license fields are static metadata and need review before allowlisting"
                    .to_string(),
            ],
        },
        last_probe: None,
        pe_machine: pe.pe_machine,
        exports: pe.exports,
        resource_summary: pe.resource_summary,
        pipl_resource_status: pe.pipl_resource_status,
        pipl_resource_entries: pe.pipl_resource_entries,
        pipl_content_scan: pe.pipl_content_scan,
        pipl_kind: class.pipl_kind.clone(),
        pipl_name: class.pipl_name.clone(),
        pipl_category: class.pipl_category.clone(),
        pipl_match_name: class.pipl_match_name.clone(),
        entrypoint,
        adjacent_source_tree: source_tree,
        adjacent_license: class.adjacent_license,
        fixture_status: candidate.fixture_status.clone(),
        inferred_class: candidate.inferred_class.clone(),
        plugin_class: class.plugin_class.to_string(),
        status: class.status.to_string(),
        confidence: class.confidence.to_string(),
        evidence,
        blocked_reasons,
        deferred_features,
        recommended_next_action: class.recommended_next_action.to_string(),
        render_capable: false,
        notes: vec![
            "static inference only".to_string(),
            "not render-capable".to_string(),
            format!("inventory fixture_status: {}", candidate.fixture_status),
        ],
    }
}

fn inspect_pe_candidate(
    path: &str,
    inspect_pipl_payload: bool,
    expected_pipl_fields: &[ExpectedPiplField],
) -> PeInspection {
    match fs::read(path) {
        Ok(bytes) => match inspect_pe_bytes(&bytes, inspect_pipl_payload, expected_pipl_fields) {
            Ok(mut inspection) => {
                inspection
                    .evidence
                    .push("read-only PE metadata inspection completed".to_string());
                inspection
            }
            Err(error) => PeInspection {
                resource_summary: format!("pe-inspect-error:{error}"),
                pipl_content_scan: pipl_content_scan_not_requested(),
                evidence: vec![format!("read-only PE metadata inspection failed: {error}")],
                ..PeInspection::default()
            },
        },
        Err(error) => PeInspection {
            resource_summary: format!("pe-inspect-error:{error}"),
            pipl_content_scan: pipl_content_scan_not_requested(),
            evidence: vec![format!("read-only PE metadata inspection failed: {error}")],
            ..PeInspection::default()
        },
    }
}

fn inspect_adjacent_source_candidate(
    candidate: &InventoryAexCandidate,
    class: &Classification,
) -> AdjacentSourceInspection {
    let source_tree = class
        .adjacent_source_tree
        .clone()
        .or_else(|| source_tree_guess(&candidate.path));
    let Some(source_tree) = source_tree else {
        return AdjacentSourceInspection::default();
    };
    let Some(build_rs) = find_build_rs(&source_tree) else {
        return AdjacentSourceInspection {
            evidence: vec![format!(
                "adjacent source inspection: build.rs not found under {source_tree}"
            )],
            ..AdjacentSourceInspection::default()
        };
    };
    match fs::read_to_string(&build_rs) {
        Ok(source) => parse_build_rs_metadata(&source, &build_rs, &candidate.path),
        Err(error) => AdjacentSourceInspection {
            evidence: vec![format!(
                "adjacent source inspection failed for {}: {error}",
                build_rs.display()
            )],
            ..AdjacentSourceInspection::default()
        },
    }
}

fn apply_adjacent_source_inspection(
    class: &mut Classification,
    inspection: &AdjacentSourceInspection,
) {
    if inspection.pipl_kind.is_none()
        && inspection.pipl_name.is_none()
        && inspection.entrypoint.is_none()
    {
        return;
    }
    if !inspection.identity_matches_candidate {
        return;
    }
    class.confidence = "observed-adjacent-source";
    if let Some(kind) = &inspection.pipl_kind {
        class.pipl_kind = Some(kind.clone());
        match kind.as_str() {
            "AEEffect" => {
                if inspection.entrypoint.is_some() && !class.status.starts_with("defer_") {
                    class.plugin_class = "classic-effect-candidate";
                    class.status = "candidate_for_contract_probe";
                    class.recommended_next_action =
                        "allowlist only after local source/license review; probe legacy path only";
                    class.blocked_reasons.retain(|reason| {
                        !reason.contains("inventory metadata is insufficient")
                            && !reason.contains("confirm adjacent source")
                    });
                }
            }
            "AEGP" => {
                class.plugin_class = "aegp";
                class.status = "blocked_aegp";
                class.recommended_next_action = "keep out of render fixtures";
                if !class
                    .blocked_reasons
                    .iter()
                    .any(|reason| reason.contains("AEGP"))
                {
                    class
                        .blocked_reasons
                        .push("AEGP is outside v0 image render support".to_string());
                }
                if !class
                    .deferred_features
                    .iter()
                    .any(|feature| feature == "AEGP")
                {
                    class.deferred_features.push("AEGP".to_string());
                }
            }
            _ => {}
        }
    }
    if let Some(name) = &inspection.pipl_name {
        class.pipl_name = Some(name.clone());
    }
    if let Some(category) = &inspection.pipl_category {
        class.pipl_category = Some(category.clone());
    }
    if let Some(match_name) = &inspection.pipl_match_name {
        class.pipl_match_name = Some(match_name.clone());
    }
    if let Some(entrypoint) = &inspection.entrypoint {
        class.entrypoint = Some(entrypoint.clone());
    }
    if inspection.supports_smart_render
        && !class
            .deferred_features
            .iter()
            .any(|feature| feature == "SmartFX")
    {
        class.deferred_features.push("SmartFX".to_string());
    }
}

fn expected_pipl_fields(class: &Classification) -> Vec<ExpectedPiplField> {
    let mut fields = Vec::new();
    push_expected_field(&mut fields, "pipl_name", class.pipl_name.as_deref());
    push_expected_field(&mut fields, "pipl_category", class.pipl_category.as_deref());
    push_expected_field(
        &mut fields,
        "pipl_match_name",
        class.pipl_match_name.as_deref(),
    );
    push_expected_field(
        &mut fields,
        "entrypoint",
        class.entrypoint.as_ref().map(|entry| entry.name.as_str()),
    );
    fields
}

fn push_expected_field(fields: &mut Vec<ExpectedPiplField>, field: &str, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };
    let value = value.trim();
    if value.is_empty() || value.len() > 255 {
        return;
    }
    if fields
        .iter()
        .any(|existing| existing.field == field && existing.value == value)
    {
        return;
    }
    fields.push(ExpectedPiplField {
        field: field.to_string(),
        value: value.to_string(),
    });
}

fn find_build_rs(source_tree: &str) -> Option<PathBuf> {
    let root = Path::new(source_tree);
    [root.join("build.rs"), root.join("rust").join("build.rs")]
        .into_iter()
        .find(|path| path.is_file())
}

fn parse_build_rs_metadata(
    source: &str,
    path: &Path,
    candidate_path: &str,
) -> AdjacentSourceInspection {
    let pipl_kind = if source.contains("Property::Kind(PIPLType::AEEffect)") {
        Some("AEEffect".to_string())
    } else if source.contains("Property::Kind(PIPLType::AEGP)") {
        Some("AEGP".to_string())
    } else {
        None
    };
    let pipl_name = extract_property_string(source, "Property::Name");
    let pipl_category = extract_property_string(source, "Property::Category");
    let pipl_match_name = extract_property_string(source, "Property::AE_Effect_Match_Name");
    let entrypoint =
        extract_property_string(source, "Property::CodeWin64X86").map(|name| EntryPoint {
            name,
            source: "adjacent-build-rs".to_string(),
            verified_from_binary: false,
        });
    let supports_smart_render = source.contains("SupportsSmartRender");

    let identity_matches_candidate = pipl_name
        .as_ref()
        .map(|name| normalized_identity(name) == normalized_identity(&file_stem(candidate_path)))
        .unwrap_or(true);

    let mut evidence = vec![format!("adjacent build.rs inspected: {}", path.display())];
    if let Some(kind) = &pipl_kind {
        evidence.push(format!("adjacent build.rs PiPL kind: {kind}"));
    }
    if let Some(entrypoint) = &entrypoint {
        evidence.push(format!(
            "adjacent build.rs CodeWin64X86 entrypoint: {}",
            entrypoint.name
        ));
    }
    if supports_smart_render {
        evidence.push("adjacent build.rs declares SupportsSmartRender".to_string());
    }
    if !identity_matches_candidate {
        evidence.push(format!(
            "adjacent build.rs identity mismatch: pipl_name={} candidate_file={}",
            pipl_name.as_deref().unwrap_or("unknown"),
            file_stem(candidate_path)
        ));
    }

    AdjacentSourceInspection {
        identity_matches_candidate,
        pipl_kind,
        pipl_name,
        pipl_category,
        pipl_match_name,
        entrypoint,
        supports_smart_render,
        evidence,
    }
}

fn extract_property_string(source: &str, property: &str) -> Option<String> {
    let needle = format!("{property}(\"");
    let start = source.find(&needle)? + needle.len();
    let tail = &source[start..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}

fn normalized_identity(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn inspect_pe_bytes(
    bytes: &[u8],
    inspect_pipl_payload: bool,
    expected_pipl_fields: &[ExpectedPiplField],
) -> Result<PeInspection, String> {
    if bytes.len() < 0x40 || &bytes[0..2] != b"MZ" {
        return Err("missing-mz-header".to_string());
    }
    let pe_offset = read_u32(bytes, 0x3c)? as usize;
    if pe_offset.checked_add(24).ok_or("pe-offset-overflow")? > bytes.len() {
        return Err("pe-header-out-of-bounds".to_string());
    }
    if &bytes[pe_offset..pe_offset + 4] != b"PE\0\0" {
        return Err("missing-pe-signature".to_string());
    }

    let machine_raw = read_u16(bytes, pe_offset + 4)?;
    let section_count = read_u16(bytes, pe_offset + 6)? as usize;
    let optional_size = read_u16(bytes, pe_offset + 20)? as usize;
    let optional_offset = pe_offset + 24;
    let section_offset = optional_offset
        .checked_add(optional_size)
        .ok_or("section-offset-overflow")?;
    if section_offset > bytes.len() {
        return Err("optional-header-out-of-bounds".to_string());
    }

    let optional_magic = read_u16(bytes, optional_offset)?;
    let data_directory_offset = match optional_magic {
        0x10b => optional_offset + 96,
        0x20b => optional_offset + 112,
        other => return Err(format!("unknown-optional-header:0x{other:04x}")),
    };
    let number_of_rva_and_sizes_offset = data_directory_offset
        .checked_sub(4)
        .ok_or("directory-count-offset-underflow")?;
    let directory_count = read_u32(bytes, number_of_rva_and_sizes_offset)?;
    let sections = parse_sections(bytes, section_offset, section_count)?;

    let mut inspection = PeInspection {
        pe_machine: Some(machine_name(machine_raw).to_string()),
        exports: Vec::new(),
        resource_summary: "absent".to_string(),
        pipl_resource_status: "absent".to_string(),
        pipl_resource_entries: Vec::new(),
        pipl_content_scan: pipl_content_scan_not_requested(),
        evidence: vec![format!("PE machine: {}", machine_name(machine_raw))],
    };

    if directory_count > 0 {
        let export_rva = read_u32(bytes, data_directory_offset)?;
        let export_size = read_u32(bytes, data_directory_offset + 4)?;
        inspection.exports = parse_export_names(bytes, &sections, export_rva, export_size)?;
        if !inspection.exports.is_empty() {
            inspection.evidence.push(format!(
                "PE export names observed: {}",
                inspection.exports.len()
            ));
        }
    }

    if directory_count > 2 {
        let resource_directory_offset = data_directory_offset + 16;
        let resource_rva = read_u32(bytes, resource_directory_offset)?;
        let resource_size = read_u32(bytes, resource_directory_offset + 4)?;
        let resource_inspection = inspect_resource_directory(
            bytes,
            &sections,
            resource_rva,
            resource_size,
            inspect_pipl_payload,
            expected_pipl_fields,
        )?;
        inspection.resource_summary = resource_inspection.resource_summary;
        inspection.pipl_resource_status = resource_inspection.pipl_resource_status;
        inspection.pipl_resource_entries = resource_inspection.pipl_resource_entries;
        inspection.pipl_content_scan = resource_inspection.pipl_content_scan;
        if inspection
            .resource_summary
            .to_ascii_lowercase()
            .contains("pipl")
        {
            inspection
                .evidence
                .push("PE resource directory includes PiPL type name".to_string());
        }
        if !inspection.pipl_resource_entries.is_empty() {
            inspection.evidence.push(format!(
                "PE PiPL resource metadata entries observed: {}",
                inspection.pipl_resource_entries.len()
            ));
        }
        if inspect_pipl_payload {
            inspection.evidence.push(format!(
                "bounded PiPL content scan status: {}",
                inspection.pipl_content_scan.status
            ));
        }
    }

    Ok(inspection)
}

#[derive(Debug, Default)]
struct ResourceInspection {
    resource_summary: String,
    pipl_resource_status: String,
    pipl_resource_entries: Vec<PiplResourceEntry>,
    pipl_content_scan: PiplContentScan,
}

fn parse_sections(
    bytes: &[u8],
    section_offset: usize,
    section_count: usize,
) -> Result<Vec<Section>, String> {
    let mut sections = Vec::new();
    for index in 0..section_count.min(96) {
        let offset = section_offset
            .checked_add(index * 40)
            .ok_or("section-entry-overflow")?;
        if offset.checked_add(40).ok_or("section-entry-overflow")? > bytes.len() {
            return Err("section-table-out-of-bounds".to_string());
        }
        sections.push(Section {
            virtual_size: read_u32(bytes, offset + 8)?,
            virtual_address: read_u32(bytes, offset + 12)?,
            raw_data_size: read_u32(bytes, offset + 16)?,
            raw_data_ptr: read_u32(bytes, offset + 20)?,
        });
    }
    Ok(sections)
}

fn parse_export_names(
    bytes: &[u8],
    sections: &[Section],
    export_rva: u32,
    export_size: u32,
) -> Result<Vec<String>, String> {
    if export_rva == 0 || export_size == 0 {
        return Ok(Vec::new());
    }
    let export_offset = match rva_to_offset(sections, export_rva) {
        Some(offset) => offset,
        None => return Ok(Vec::new()),
    };
    if export_offset
        .checked_add(40)
        .ok_or("export-directory-overflow")?
        > bytes.len()
    {
        return Ok(Vec::new());
    }

    let name_count = read_u32(bytes, export_offset + 24)?.min(64);
    let names_rva = read_u32(bytes, export_offset + 32)?;
    let names_offset = match rva_to_offset(sections, names_rva) {
        Some(offset) => offset,
        None => return Ok(Vec::new()),
    };

    let mut names = Vec::new();
    for index in 0..name_count as usize {
        let name_rva_offset = names_offset
            .checked_add(index * 4)
            .ok_or("export-name-table-overflow")?;
        if name_rva_offset
            .checked_add(4)
            .ok_or("export-name-table-overflow")?
            > bytes.len()
        {
            break;
        }
        let name_rva = read_u32(bytes, name_rva_offset)?;
        if let Some(name_offset) = rva_to_offset(sections, name_rva) {
            if let Some(name) = read_c_string(bytes, name_offset, 256) {
                names.push(name);
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

fn inspect_resource_directory(
    bytes: &[u8],
    sections: &[Section],
    resource_rva: u32,
    resource_size: u32,
    inspect_pipl_payload: bool,
    expected_pipl_fields: &[ExpectedPiplField],
) -> Result<ResourceInspection, String> {
    if resource_rva == 0 || resource_size == 0 {
        return Ok(ResourceInspection {
            resource_summary: "absent".to_string(),
            pipl_resource_status: "absent".to_string(),
            pipl_content_scan: pipl_content_scan_not_requested(),
            ..ResourceInspection::default()
        });
    }
    let resource_base = match rva_to_offset(sections, resource_rva) {
        Some(offset) => offset,
        None => {
            return Ok(ResourceInspection {
                resource_summary: "present-unmapped".to_string(),
                pipl_resource_status: "present_unmapped".to_string(),
                pipl_content_scan: pipl_content_scan_not_requested(),
                ..ResourceInspection::default()
            });
        }
    };
    if resource_base
        .checked_add(16)
        .ok_or("resource-directory-overflow")?
        > bytes.len()
    {
        return Ok(ResourceInspection {
            resource_summary: "present-truncated".to_string(),
            pipl_resource_status: "present_truncated".to_string(),
            pipl_content_scan: pipl_content_scan_not_requested(),
            ..ResourceInspection::default()
        });
    }
    let top_entries = parse_resource_directory_entries(bytes, resource_base, 0, 32)?;
    let mut types = Vec::new();
    for entry in &top_entries {
        types.push(resource_identifier_label(&entry.identifier));
    }
    types.sort();
    types.dedup();

    let pipl_data_entries =
        parse_pipl_resource_entries(bytes, sections, resource_base, &top_entries)?;
    let pipl_entries = pipl_data_entries
        .iter()
        .map(|entry| entry.public.clone())
        .collect::<Vec<_>>();
    let pipl_resource_status = if pipl_data_entries.is_empty() {
        if types.iter().any(|item| item.eq_ignore_ascii_case("pipl")) {
            "present_type_only"
        } else {
            "absent"
        }
    } else {
        "present_metadata_only"
    };
    let pipl_content_scan = if inspect_pipl_payload {
        scan_pipl_content(bytes, &pipl_data_entries, expected_pipl_fields)
    } else {
        pipl_content_scan_not_requested()
    };
    let resource_summary = if types.is_empty() {
        format!("present; top_level_entries={}", top_entries.len())
    } else {
        format!(
            "present; top_level_entries={}; types={}",
            top_entries.len(),
            types.join(",")
        )
    };
    Ok(ResourceInspection {
        resource_summary,
        pipl_resource_status: pipl_resource_status.to_string(),
        pipl_resource_entries: pipl_entries,
        pipl_content_scan,
    })
}

fn parse_pipl_resource_entries(
    bytes: &[u8],
    sections: &[Section],
    resource_base: usize,
    top_entries: &[ResourceDirectoryEntry],
) -> Result<Vec<PiplResourceDataEntry>, String> {
    let Some(pipl_type_entry) = top_entries.iter().find(|entry| {
        matches!(&entry.identifier, ResourceIdentifier::Name(name) if name.eq_ignore_ascii_case("pipl"))
    }) else {
        return Ok(Vec::new());
    };
    if pipl_type_entry.data_or_subdir & 0x8000_0000 == 0 {
        return Ok(Vec::new());
    }
    let type_dir = pipl_type_entry.data_or_subdir & 0x7fff_ffff;
    let name_entries = parse_resource_directory_entries(bytes, resource_base, type_dir, 64)?;
    let mut pipl_entries = Vec::new();
    for name_entry in name_entries {
        if name_entry.data_or_subdir & 0x8000_0000 == 0 {
            if let Some((data_size, code_page, data_offset)) = parse_resource_data_entry(
                bytes,
                sections,
                resource_base,
                name_entry.data_or_subdir,
            )? {
                pipl_entries.push(pipl_resource_entry(
                    &name_entry.identifier,
                    None,
                    data_size,
                    code_page,
                    data_offset,
                ));
            }
            continue;
        }
        let language_dir = name_entry.data_or_subdir & 0x7fff_ffff;
        for language_entry in
            parse_resource_directory_entries(bytes, resource_base, language_dir, 64)?
        {
            if language_entry.data_or_subdir & 0x8000_0000 != 0 {
                continue;
            }
            if let Some((data_size, code_page, data_offset)) = parse_resource_data_entry(
                bytes,
                sections,
                resource_base,
                language_entry.data_or_subdir,
            )? {
                pipl_entries.push(pipl_resource_entry(
                    &name_entry.identifier,
                    Some(&language_entry.identifier),
                    data_size,
                    code_page,
                    data_offset,
                ));
            }
        }
    }
    Ok(pipl_entries)
}

fn parse_resource_directory_entries(
    bytes: &[u8],
    resource_base: usize,
    directory_relative_offset: u32,
    limit: usize,
) -> Result<Vec<ResourceDirectoryEntry>, String> {
    let directory = resource_base
        .checked_add(directory_relative_offset as usize)
        .ok_or("resource-directory-offset-overflow")?;
    if directory
        .checked_add(16)
        .ok_or("resource-directory-overflow")?
        > bytes.len()
    {
        return Ok(Vec::new());
    }
    let named = read_u16(bytes, directory + 12)? as usize;
    let ids = read_u16(bytes, directory + 14)? as usize;
    let total = named.saturating_add(ids).min(limit);
    let mut entries = Vec::new();
    for index in 0..total {
        let entry_offset = directory
            .checked_add(16 + index * 8)
            .ok_or("resource-entry-overflow")?;
        if entry_offset
            .checked_add(8)
            .ok_or("resource-entry-overflow")?
            > bytes.len()
        {
            break;
        }
        let name_or_id = read_u32(bytes, entry_offset)?;
        let data_or_subdir = read_u32(bytes, entry_offset + 4)?;
        entries.push(ResourceDirectoryEntry {
            identifier: parse_resource_identifier(bytes, resource_base, name_or_id),
            data_or_subdir,
        });
    }
    Ok(entries)
}

fn parse_resource_data_entry(
    bytes: &[u8],
    sections: &[Section],
    resource_base: usize,
    data_entry_relative_offset: u32,
) -> Result<Option<(u32, u32, Option<usize>)>, String> {
    let entry_offset = resource_base
        .checked_add(data_entry_relative_offset as usize)
        .ok_or("resource-data-entry-overflow")?;
    if entry_offset
        .checked_add(16)
        .ok_or("resource-data-entry-overflow")?
        > bytes.len()
    {
        return Ok(None);
    }
    let data_rva = read_u32(bytes, entry_offset)?;
    let data_size = read_u32(bytes, entry_offset + 4)?;
    let code_page = read_u32(bytes, entry_offset + 8)?;
    let data_offset = if data_rva == 0 {
        None
    } else if let Some(offset) = rva_to_offset(sections, data_rva) {
        Some(offset)
    } else {
        return Ok(None);
    };
    Ok(Some((data_size, code_page, data_offset)))
}

fn pipl_resource_entry(
    resource_identifier: &ResourceIdentifier,
    language_identifier: Option<&ResourceIdentifier>,
    data_size: u32,
    code_page: u32,
    data_offset: Option<usize>,
) -> PiplResourceDataEntry {
    let (resource_id, resource_name) = resource_identifier_parts(resource_identifier);
    let (language_id, language_name) = language_identifier
        .map(resource_identifier_parts)
        .unwrap_or((None, None));
    PiplResourceDataEntry {
        public: PiplResourceEntry {
            resource_id,
            resource_name,
            language_id,
            language_name,
            data_size,
            code_page,
            contents_read: false,
        },
        data_offset,
    }
}

const PIPL_CONTENT_SCAN_BYTES_LIMIT: u32 = 16 * 1024;
const PIPL_CONTENT_SCAN_RESOURCE_LIMIT: usize = 4;

fn pipl_content_scan_not_requested() -> PiplContentScan {
    PiplContentScan {
        status: "not_requested".to_string(),
        scan_mode: "disabled".to_string(),
        bytes_limit: 0,
        bytes_read: 0,
        truncated: false,
        contents_emitted: false,
        matched_fields: Vec::new(),
        unmatched_expected_fields: Vec::new(),
        notes: vec![
            "PiPL content scan was not requested.".to_string(),
            "No PiPL resource contents were read.".to_string(),
        ],
    }
}

fn scan_pipl_content(
    bytes: &[u8],
    entries: &[PiplResourceDataEntry],
    expected_fields: &[ExpectedPiplField],
) -> PiplContentScan {
    if entries.is_empty() {
        return PiplContentScan {
            status: "no_pipl_resource".to_string(),
            scan_mode: "bounded-expected-string-match-only".to_string(),
            bytes_limit: PIPL_CONTENT_SCAN_BYTES_LIMIT,
            bytes_read: 0,
            truncated: false,
            contents_emitted: false,
            matched_fields: Vec::new(),
            unmatched_expected_fields: expected_fields
                .iter()
                .map(|field| field.field.clone())
                .collect(),
            notes: pipl_content_scan_notes(),
        };
    }
    if expected_fields.is_empty() {
        return PiplContentScan {
            status: "no_expected_fields".to_string(),
            scan_mode: "bounded-expected-string-match-only".to_string(),
            bytes_limit: PIPL_CONTENT_SCAN_BYTES_LIMIT,
            bytes_read: 0,
            truncated: false,
            contents_emitted: false,
            matched_fields: Vec::new(),
            unmatched_expected_fields: Vec::new(),
            notes: pipl_content_scan_notes(),
        };
    }

    let mut scanned = Vec::new();
    let mut bytes_read = 0u32;
    let mut oversize = false;
    let mut truncated = false;
    for entry in entries.iter().take(PIPL_CONTENT_SCAN_RESOURCE_LIMIT) {
        if entry.public.data_size > PIPL_CONTENT_SCAN_BYTES_LIMIT {
            oversize = true;
            continue;
        }
        let Some(offset) = entry.data_offset else {
            truncated = true;
            continue;
        };
        let size = entry.public.data_size as usize;
        let Some(end) = offset.checked_add(size) else {
            truncated = true;
            continue;
        };
        if end > bytes.len() {
            truncated = true;
            continue;
        }
        scanned.push(&bytes[offset..end]);
        bytes_read = bytes_read.saturating_add(entry.public.data_size);
    }

    let mut matched_fields = Vec::new();
    let mut unmatched_expected_fields = Vec::new();
    for expected in expected_fields {
        if scanned
            .iter()
            .any(|payload| semantic_value_is_present(payload, &expected.value))
        {
            matched_fields.push(PiplContentMatch {
                field: expected.field.clone(),
                value: expected.value.clone(),
                source: "bounded-pipl-content-scan".to_string(),
            });
        } else {
            unmatched_expected_fields.push(expected.field.clone());
        }
    }

    let status = if oversize {
        "present_content_oversize"
    } else if truncated {
        "present_content_truncated"
    } else if matched_fields.len() == expected_fields.len() {
        "semantic_matches"
    } else if matched_fields.is_empty() {
        "no_semantic_matches"
    } else {
        "partial_semantic_matches"
    };
    PiplContentScan {
        status: status.to_string(),
        scan_mode: "bounded-expected-string-match-only".to_string(),
        bytes_limit: PIPL_CONTENT_SCAN_BYTES_LIMIT,
        bytes_read,
        truncated: truncated || oversize,
        contents_emitted: false,
        matched_fields,
        unmatched_expected_fields,
        notes: pipl_content_scan_notes(),
    }
}

fn semantic_value_is_present(payload: &[u8], value: &str) -> bool {
    let ascii = value.as_bytes();
    if !ascii.is_empty() && payload.windows(ascii.len()).any(|window| window == ascii) {
        return true;
    }
    let utf16 = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    !utf16.is_empty() && payload.windows(utf16.len()).any(|window| window == utf16)
}

fn pipl_content_scan_notes() -> Vec<String> {
    vec![
        "Only already-expected bounded semantic strings are matched.".to_string(),
        "Raw PiPL bytes, hashes, unknown properties, and decoded layouts are not emitted."
            .to_string(),
        "This scan does not approve loading, describe, render, or OFX routing.".to_string(),
    ]
}

fn parse_resource_identifier(
    bytes: &[u8],
    resource_base: usize,
    name_or_id: u32,
) -> ResourceIdentifier {
    if name_or_id & 0x8000_0000 != 0 {
        let name_offset = resource_base + (name_or_id & 0x7fff_ffff) as usize;
        read_utf16_resource_name(bytes, name_offset)
            .map(ResourceIdentifier::Name)
            .unwrap_or_else(|| ResourceIdentifier::Name("unreadable-name".to_string()))
    } else {
        ResourceIdentifier::Id(name_or_id & 0xffff)
    }
}

fn resource_identifier_parts(identifier: &ResourceIdentifier) -> (Option<u32>, Option<String>) {
    match identifier {
        ResourceIdentifier::Id(id) => (Some(*id), None),
        ResourceIdentifier::Name(name) => (None, Some(name.clone())),
    }
}

fn resource_identifier_label(identifier: &ResourceIdentifier) -> String {
    match identifier {
        ResourceIdentifier::Id(id) => format!("id:{id}"),
        ResourceIdentifier::Name(name) => name.clone(),
    }
}

fn rva_to_offset(sections: &[Section], rva: u32) -> Option<usize> {
    for section in sections {
        let span = section.virtual_size.max(section.raw_data_size).max(1);
        let end = section.virtual_address.checked_add(span)?;
        if rva >= section.virtual_address && rva < end {
            let relative = rva.checked_sub(section.virtual_address)?;
            let offset = section.raw_data_ptr.checked_add(relative)?;
            return Some(offset as usize);
        }
    }
    None
}

fn read_c_string(bytes: &[u8], offset: usize, max_len: usize) -> Option<String> {
    if offset >= bytes.len() {
        return None;
    }
    let end_limit = offset.saturating_add(max_len).min(bytes.len());
    let end = bytes[offset..end_limit]
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| offset + relative)?;
    let raw = &bytes[offset..end];
    if raw
        .iter()
        .all(|byte| byte.is_ascii_graphic() || *byte == b'_')
    {
        std::str::from_utf8(raw).ok().map(str::to_string)
    } else {
        None
    }
}

fn read_utf16_resource_name(bytes: &[u8], offset: usize) -> Option<String> {
    let len = read_u16(bytes, offset).ok()? as usize;
    let chars_offset = offset.checked_add(2)?;
    let bytes_len = len.checked_mul(2)?;
    if chars_offset.checked_add(bytes_len)? > bytes.len() || len > 64 {
        return None;
    }
    let mut units = Vec::with_capacity(len);
    for index in 0..len {
        units.push(read_u16(bytes, chars_offset + index * 2).ok()?);
    }
    String::from_utf16(&units).ok()
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    if offset.checked_add(2).ok_or("u16-offset-overflow")? > bytes.len() {
        return Err("unexpected-eof".to_string());
    }
    Ok(u16::from_le_bytes([bytes[offset], bytes[offset + 1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    if offset.checked_add(4).ok_or("u32-offset-overflow")? > bytes.len() {
        return Err("unexpected-eof".to_string());
    }
    Ok(u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

fn machine_name(machine: u16) -> &'static str {
    match machine {
        0x014c => "x86",
        0x8664 => "x86_64",
        0x01c0 => "arm",
        0xaa64 => "arm64",
        _ => "unknown",
    }
}

fn default_frame_contract() -> FrameContract {
    FrameContract {
        pixel_formats: vec!["rgba8".to_string()],
        preferred_pixel_format: "rgba8".to_string(),
        alpha: "unknown".to_string(),
        color_management: "none-claimed".to_string(),
        max_width: 4096,
        max_height: 4096,
        transport: vec!["png-v0".to_string()],
        time_model: TimeModel {
            mode: "single-frame".to_string(),
            frame_index: 0,
            time_seconds: 0.0,
        },
    }
}

fn unsupported_features_for(class: &Classification) -> Vec<String> {
    let mut unsupported = class.deferred_features.clone();
    if class.plugin_class == "aegp" {
        unsupported.push("AEGP".to_string());
    }
    if class.plugin_class == "blocked" {
        unsupported.push("heavy/specialized effect".to_string());
    }
    if class.plugin_class == "unknown" {
        unsupported.push("unverified plugin class".to_string());
    }
    unsupported.sort();
    unsupported.dedup();
    unsupported
}

fn infer_classification(candidate: &InventoryAexCandidate) -> Classification {
    let stem = file_stem(&candidate.path).to_ascii_lowercase();
    let path_lower = candidate.path.to_ascii_lowercase();
    let inferred = candidate.inferred_class.as_str();
    let mut evidence = vec![
        format!("inventory path: {}", candidate.path),
        format!("inventory inferred_class: {}", candidate.inferred_class),
        format!("inventory fixture_status: {}", candidate.fixture_status),
    ];

    match stem.as_str() {
        "adaptivefilter" => {
            evidence.push("runbook adjacent-source whitelist: AEEffect EffectMain".to_string());
            evidence.push("runbook adjacent-source whitelist: SupportsSmartRender".to_string());
            classic_adjacent(
                "AdaptiveFilter",
                "Filter",
                "ONMK_AdaptiveFilter",
                source_tree_from_marker(&candidate.path, "AdaptiveFilterRust"),
                vec!["SmartFX".to_string()],
            )
            .with_evidence(evidence)
        }
        "medianpro" => {
            evidence.push("runbook adjacent-source whitelist: AEEffect EffectMain".to_string());
            evidence.push("runbook adjacent-source whitelist: SupportsSmartRender".to_string());
            classic_adjacent(
                "MedianPro",
                "Filter",
                "ONMK_MedianPro",
                source_tree_from_marker(&candidate.path, "MedianProRust"),
                vec!["SmartFX".to_string()],
            )
            .with_evidence(evidence)
        }
        "patharray" => {
            evidence.push(
                "runbook adjacent-source whitelist: geometry/mask-dependent AEEffect".to_string(),
            );
            let mut classification = classic_adjacent(
                "PathArray",
                "Geometry",
                "ONMK_PathArray",
                source_tree_from_marker(&candidate.path, "PathArrayRust"),
                vec![
                    "SmartFX".to_string(),
                    "geometry/mask-dependent behavior".to_string(),
                ],
            )
            .with_evidence(evidence);
            classification.status = "defer_heavy_dependency";
            classification.blocked_reasons.push(
                "geometry/mask-dependent static candidate; not a v0 image fixture".to_string(),
            );
            classification.recommended_next_action =
                "defer until worker reports geometry/mask support explicitly";
            classification
        }
        "aetimelinesyncaegp" | "exeditremoteaegp" => {
            evidence.push("runbook adjacent-source whitelist: AEGP/controller plugin".to_string());
            Classification {
                plugin_class: "aegp",
                status: "blocked_aegp",
                confidence: "inferred",
                evidence,
                blocked_reasons: vec![
                    "AEGP/controller plugins are not v0 image render fixtures".to_string()
                ],
                deferred_features: vec![
                    "AEGP".to_string(),
                    "host project/timeline APIs".to_string(),
                ],
                recommended_next_action: "keep as controller reference; do not send to image probe",
                entrypoint: Some(EntryPoint {
                    name: "EntryPointFunc".to_string(),
                    source: "runbook-adjacent-source-whitelist".to_string(),
                    verified_from_binary: false,
                }),
                adjacent_source_tree: source_tree_from_aegp_path(&candidate.path),
                adjacent_license: None,
                pipl_kind: Some("AEGP".to_string()),
                pipl_name: Some(file_stem(&candidate.path)),
                pipl_category: None,
                pipl_match_name: None,
            }
        }
        "cmykmisreg"
        | "distortchroma"
        | "maskoffset"
        | "minimaxmap"
        | "refractiondispersion"
        | "scattermap"
        | "frameslice" => {
            evidence.push("runbook first-queue candidate from inventory/source naming".to_string());
            Classification {
                plugin_class: "classic-effect-candidate",
                status: "classified_from_inventory",
                confidence: "inferred",
                evidence,
                blocked_reasons: Vec::new(),
                deferred_features: Vec::new(),
                recommended_next_action: "confirm adjacent source/PiPL metadata before image probe",
                entrypoint: None,
                adjacent_source_tree: source_tree_guess(&candidate.path),
                adjacent_license: None,
                pipl_kind: None,
                pipl_name: Some(file_stem(&candidate.path)),
                pipl_category: None,
                pipl_match_name: None,
            }
        }
        _ if inferred == "aegp" || stem.ends_with("aegp") => Classification {
            plugin_class: "aegp",
            status: "blocked_aegp",
            confidence: "inferred",
            evidence,
            blocked_reasons: vec!["AEGP is outside v0 image render support".to_string()],
            deferred_features: vec!["AEGP".to_string()],
            recommended_next_action: "keep out of render fixtures",
            entrypoint: None,
            adjacent_source_tree: None,
            adjacent_license: None,
            pipl_kind: Some("AEGP".to_string()),
            pipl_name: Some(file_stem(&candidate.path)),
            pipl_category: None,
            pipl_match_name: None,
        },
        _ if inferred == "heavy-or-specialized-effect" || looks_heavy(&path_lower) => {
            Classification {
                plugin_class: "blocked",
                status: "defer_heavy_dependency",
                confidence: "inferred",
                evidence,
                blocked_reasons: vec![
                    "heavy/specialized candidate deferred by static runbook".to_string()
                ],
                deferred_features: vec!["GPU/model/project-dependent behavior possible".to_string()],
                recommended_next_action: "defer until sandbox and reporting pipeline are stable",
                entrypoint: None,
                adjacent_source_tree: None,
                adjacent_license: None,
                pipl_kind: None,
                pipl_name: Some(file_stem(&candidate.path)),
                pipl_category: None,
                pipl_match_name: None,
            }
        }
        _ if inferred == "likely-classic-effect" => Classification {
            plugin_class: "classic-effect-candidate",
            status: "classified_from_inventory",
            confidence: "inferred",
            evidence,
            blocked_reasons: Vec::new(),
            deferred_features: Vec::new(),
            recommended_next_action: "confirm adjacent source/PiPL metadata before image probe",
            entrypoint: None,
            adjacent_source_tree: source_tree_guess(&candidate.path),
            adjacent_license: None,
            pipl_kind: None,
            pipl_name: Some(file_stem(&candidate.path)),
            pipl_category: None,
            pipl_match_name: None,
        },
        _ => Classification {
            plugin_class: "unknown",
            status: "unknown_needs_resource_scan",
            confidence: "unverified",
            evidence,
            blocked_reasons: vec!["inventory metadata is insufficient for effect class".to_string()],
            deferred_features: Vec::new(),
            recommended_next_action: "perform reviewed read-only PE/PiPL resource scan",
            entrypoint: None,
            adjacent_source_tree: None,
            adjacent_license: None,
            pipl_kind: None,
            pipl_name: Some(file_stem(&candidate.path)),
            pipl_category: None,
            pipl_match_name: None,
        },
    }
}

impl Classification {
    fn with_evidence(mut self, evidence: Vec<String>) -> Self {
        self.evidence = evidence;
        self
    }
}

fn classic_adjacent(
    pipl_name: &str,
    pipl_category: &str,
    pipl_match_name: &str,
    adjacent_source_tree: Option<String>,
    deferred_features: Vec<String>,
) -> Classification {
    Classification {
        plugin_class: "classic-effect-candidate",
        status: "candidate_for_contract_probe",
        confidence: "inferred",
        evidence: Vec::new(),
        blocked_reasons: Vec::new(),
        deferred_features,
        recommended_next_action:
            "allowlist only after local source/license review; probe legacy path only",
        entrypoint: Some(EntryPoint {
            name: "EffectMain".to_string(),
            source: "runbook-adjacent-source-whitelist".to_string(),
            verified_from_binary: false,
        }),
        adjacent_source_tree,
        adjacent_license: None,
        pipl_kind: Some("AEEffect".to_string()),
        pipl_name: Some(pipl_name.to_string()),
        pipl_category: Some(pipl_category.to_string()),
        pipl_match_name: Some(pipl_match_name.to_string()),
    }
}

fn looks_heavy(path_lower: &str) -> bool {
    ["onnx", "depth", "flow", "particle", "flare", "tuiimage"]
        .iter()
        .any(|needle| path_lower.contains(needle))
}

fn file_stem(path: &str) -> String {
    let file_name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .split('.')
        .next()
        .unwrap_or(path);
    file_name.to_string()
}

fn effect_id(path: &str) -> String {
    let raw = file_stem(path).to_ascii_lowercase();
    let mut id = String::new();
    let mut previous_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            id.push(ch);
            previous_dash = false;
        } else if !previous_dash {
            id.push('-');
            previous_dash = true;
        }
    }
    let id = id.trim_matches('-');
    if id.is_empty() {
        "unknown-local".to_string()
    } else {
        format!("{id}-local")
    }
}

fn source_tree_from_marker(path: &str, marker: &str) -> Option<String> {
    let index = path.find(marker)?;
    Some(path[..index + marker.len()].to_string())
}

fn source_tree_from_aegp_path(path: &str) -> Option<String> {
    for marker in ["ae-timeline-sync", "exedit-ae-remote"] {
        if let Some(index) = path.find(marker) {
            return Some(path[..index + marker.len()].to_string());
        }
    }
    None
}

fn source_tree_guess(path: &str) -> Option<String> {
    let marker = "target\\release\\";
    path.find(marker).map(|index| path[..index].to_string())
}

fn default_input_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate should live under repository root")
        .join("analysis")
        .join("AE_AEX_AEP_STATIC_INVENTORY_2026-05-31.json")
}

fn default_output_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-static-classifier")
        .join("catalog.local.json")
}

fn parse_args() -> Result<(PathBuf, PathBuf, bool, bool, bool), String> {
    let mut input = default_input_path();
    let mut output = default_output_path();
    let mut inspect_pe = false;
    let mut inspect_adjacent_source = false;
    let mut inspect_pipl_payload = false;
    let mut positional = Vec::<PathBuf>::new();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => {
                input = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--input requires a path".to_string())?;
            }
            "--output" => {
                output = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| "--output requires a path".to_string())?;
            }
            "--inspect-pe" => {
                inspect_pe = true;
            }
            "--inspect-pipl-payload" => {
                inspect_pipl_payload = true;
            }
            "--inspect-adjacent-source" => {
                inspect_adjacent_source = true;
            }
            "--help" | "-h" => {
                return Err(
                    "usage: aex_static_classifier [input.json output.json] [--input inventory.json] [--output catalog.json] [--inspect-pe] [--inspect-pipl-payload] [--inspect-adjacent-source]"
                        .to_string(),
                );
            }
            other if other.starts_with("--") => return Err(format!("unknown argument: {other}")),
            other => positional.push(PathBuf::from(other)),
        }
    }

    match positional.as_slice() {
        [] => {}
        [input_path] => input = input_path.clone(),
        [input_path, output_path] => {
            input = input_path.clone();
            output = output_path.clone();
        }
        _ => return Err("expected at most two positional paths: input output".to_string()),
    }

    if inspect_pipl_payload && !inspect_pe {
        return Err("--inspect-pipl-payload requires --inspect-pe".to_string());
    }

    Ok((
        input,
        output,
        inspect_pe,
        inspect_adjacent_source,
        inspect_pipl_payload,
    ))
}

fn main() -> Result<(), Box<dyn Error>> {
    let (input_path, output_path, inspect_pe, inspect_adjacent_source, inspect_pipl_payload) =
        match parse_args() {
            Ok(paths) => paths,
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(2);
            }
        };

    let input = std::fs::read_to_string(&input_path)?;
    let catalog = classify_inventory_json_with_options(
        &input,
        ClassifierOptions {
            inspect_pe,
            inspect_adjacent_source,
            inspect_pipl_payload,
        },
    )?;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output_path, catalog)?;
    println!("{}", output_path.display());
    Ok(())
}
