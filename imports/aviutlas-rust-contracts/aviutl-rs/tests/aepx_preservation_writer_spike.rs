use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct SpikeReport {
    status: &'static str,
    target_count: usize,
    xml_body_read_performed: bool,
    output_write_performed: bool,
    source_overwrite_performed: bool,
    after_effects_invoked: bool,
    unknown_nodes: &'static str,
    unknown_attributes: &'static str,
    xml_declaration: &'static str,
    encoding: &'static str,
    comments: &'static str,
    cdata: &'static str,
    namespace_prefixes: &'static str,
    whitespace: &'static str,
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn target_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aepx-preservation-writer-spike")
        .join(format!("{}-{name}", std::process::id()))
}

fn rename_comp_by_id_splice(
    input: &Path,
    output: &Path,
    comp_id: &str,
    expected_old_name: &str,
    new_name: &str,
) -> SpikeReport {
    let source = std::fs::read_to_string(input).expect("synthetic fixture should read");
    let Some(tag_start) = source.find("<xmp:comp") else {
        return blocked("target_not_found", true);
    };
    let mut cursor = tag_start;
    let mut matches = Vec::new();
    while let Some(relative_start) = source[cursor..].find("<xmp:comp") {
        let start = cursor + relative_start;
        let Some(relative_end) = source[start..].find('>') else {
            return blocked("parse_error", true);
        };
        let end = start + relative_end + 1;
        let tag = &source[start..end];
        if attribute_value(tag, "id").as_deref() == Some(comp_id) {
            matches.push((start, end));
        }
        cursor = end;
    }
    if matches.is_empty() {
        return blocked("target_not_found", true);
    }
    if matches.len() > 1 {
        return blocked("ambiguous_target", true);
    }

    let (start, end) = matches[0];
    let tag = &source[start..end];
    let Some((value_start, value_end, old_value)) = attribute_value_span(tag, "name") else {
        return blocked("target_not_found", true);
    };
    if old_value != expected_old_name {
        return blocked("expected_value_mismatch", true);
    }
    let mut patched = source.clone();
    patched.replace_range((start + value_start)..(start + value_end), new_name);
    match write_create_new(output, &patched) {
        Ok(()) => ready_report(),
        Err(SpikeWriteError::OutputExists) => blocked("output_exists", true),
        Err(SpikeWriteError::WriteFailed) => blocked("write_failed", true),
    }
}

fn rename_comp_by_name_splice(
    input: &Path,
    output: &Path,
    name: &str,
    new_name: &str,
) -> SpikeReport {
    let source = std::fs::read_to_string(input).expect("synthetic fixture should read");
    let mut cursor = 0usize;
    let mut matches = Vec::new();
    while let Some(relative_start) = source[cursor..].find("<xmp:comp") {
        let start = cursor + relative_start;
        let Some(relative_end) = source[start..].find('>') else {
            return blocked("parse_error", true);
        };
        let end = start + relative_end + 1;
        let tag = &source[start..end];
        if attribute_value(tag, "name").as_deref() == Some(name) {
            matches.push((start, end));
        }
        cursor = end;
    }
    if matches.is_empty() {
        return blocked("target_not_found", true);
    }
    if matches.len() > 1 {
        return blocked("ambiguous_target", true);
    }
    let (start, end) = matches[0];
    let tag = &source[start..end];
    let Some((value_start, value_end, _old_value)) = attribute_value_span(tag, "name") else {
        return blocked("target_not_found", true);
    };
    let mut patched = source.clone();
    patched.replace_range((start + value_start)..(start + value_end), new_name);
    match write_create_new(output, &patched) {
        Ok(()) => ready_report(),
        Err(SpikeWriteError::OutputExists) => blocked("output_exists", true),
        Err(SpikeWriteError::WriteFailed) => blocked("write_failed", true),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SpikeWriteError {
    OutputExists,
    WriteFailed,
}

fn write_create_new(path: &Path, text: &str) -> Result<(), SpikeWriteError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| SpikeWriteError::WriteFailed)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| {
            if err.kind() == ErrorKind::AlreadyExists {
                SpikeWriteError::OutputExists
            } else {
                SpikeWriteError::WriteFailed
            }
        })?;
    if file.write_all(text.as_bytes()).is_err() {
        let _ = std::fs::remove_file(path);
        return Err(SpikeWriteError::WriteFailed);
    }
    Ok(())
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

fn ready_report() -> SpikeReport {
    SpikeReport {
        status: "synthetic_splice_preserved",
        target_count: 1,
        xml_body_read_performed: true,
        output_write_performed: true,
        source_overwrite_performed: false,
        after_effects_invoked: false,
        unknown_nodes: "preserved",
        unknown_attributes: "preserved",
        xml_declaration: "preserved",
        encoding: "preserved",
        comments: "preserved",
        cdata: "preserved",
        namespace_prefixes: "preserved",
        whitespace: "preserved",
    }
}

fn blocked(status: &'static str, xml_body_read_performed: bool) -> SpikeReport {
    SpikeReport {
        status,
        target_count: 0,
        xml_body_read_performed,
        output_write_performed: false,
        source_overwrite_performed: false,
        after_effects_invoked: false,
        unknown_nodes: "not_written",
        unknown_attributes: "not_written",
        xml_declaration: "not_written",
        encoding: "not_written",
        comments: "not_written",
        cdata: "not_written",
        namespace_prefixes: "not_written",
        whitespace: "not_written",
    }
}

fn assert_preservation_sentinels_survive(source: &str, patched: &str) {
    for sentinel in [
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        "SPIKE_COMMENT_SENTINEL",
        "meta:UNKNOWN_ATTR_SENTINEL=\"keep\"",
        "<xmp:UNKNOWN_NODE_SENTINEL oddSpacing = \"  keep  \">",
        "<![CDATA[SPIKE_CDATA_SENTINEL <raw>&value</raw>]]>",
        "meta:prefixAttr=\"SPIKE_PREFIX_ATTR_SENTINEL\"",
        "<prefix:PREFIX_SENTINEL xmlns:prefix=\"urn:aviutlas:synthetic:prefix\"/>",
        "        <xmp:UNKNOWN_NODE_SENTINEL",
    ] {
        assert!(
            source.contains(sentinel),
            "source fixture should contain sentinel {sentinel:?}"
        );
        assert!(
            patched.contains(sentinel),
            "patched fixture should preserve sentinel {sentinel:?}"
        );
    }
}

#[test]
fn synthetic_id_splice_preserves_unknown_xml_and_writes_only_new_output() {
    let input = fixture_path("aepx_writer_spike_preservation.aepx");
    let output = target_path("rename-main.aepx");
    let _ = std::fs::remove_file(&output);
    let source_before = std::fs::read_to_string(&input).unwrap();

    let report = rename_comp_by_id_splice(&input, &output, "comp-main", "Main", "Main Reviewed");

    assert_eq!(report.status, "synthetic_splice_preserved");
    assert_eq!(report.target_count, 1);
    assert!(report.xml_body_read_performed);
    assert!(report.output_write_performed);
    assert!(!report.source_overwrite_performed);
    assert!(!report.after_effects_invoked);
    for value in [
        report.unknown_nodes,
        report.unknown_attributes,
        report.xml_declaration,
        report.encoding,
        report.comments,
        report.cdata,
        report.namespace_prefixes,
        report.whitespace,
    ] {
        assert_eq!(value, "preserved");
    }

    let patched = std::fs::read_to_string(&output).unwrap();
    assert!(patched.contains(r#"id="comp-main" name="Main Reviewed""#));
    assert!(patched.contains(r#"data-id="comp-main" display-name="Main""#));
    assert!(!patched.contains(r#"id="comp-main" name="Main">"#));
    assert_preservation_sentinels_survive(&source_before, &patched);
    assert_eq!(std::fs::read_to_string(&input).unwrap(), source_before);
}

#[test]
fn synthetic_splice_fails_closed_for_ambiguous_name_and_old_value_mismatch() {
    let ambiguous = fixture_path("aepx_writer_spike_ambiguous.aepx");
    let output = target_path("ambiguous-output.aepx");
    let _ = std::fs::remove_file(&output);

    let report = rename_comp_by_name_splice(&ambiguous, &output, "Main", "Main Reviewed");

    assert_eq!(report.status, "ambiguous_target");
    assert!(report.xml_body_read_performed);
    assert!(!report.output_write_performed);
    assert!(!output.exists());

    let input = fixture_path("aepx_writer_spike_preservation.aepx");
    let output = target_path("mismatch-output.aepx");
    let _ = std::fs::remove_file(&output);
    let report = rename_comp_by_id_splice(&input, &output, "comp-main", "Wrong Old", "New");

    assert_eq!(report.status, "expected_value_mismatch");
    assert!(!report.output_write_performed);
    assert!(!output.exists());
}

#[test]
fn synthetic_splice_uses_create_new_and_preserves_existing_output() {
    let input = fixture_path("aepx_writer_spike_preservation.aepx");
    let output = target_path("existing-output.aepx");
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&output, "existing").unwrap();

    let report = rename_comp_by_id_splice(&input, &output, "comp-main", "Main", "Main Reviewed");

    assert_eq!(report.status, "output_exists");
    assert!(!report.output_write_performed);
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "existing");
    let _ = std::fs::remove_file(&output);
}
