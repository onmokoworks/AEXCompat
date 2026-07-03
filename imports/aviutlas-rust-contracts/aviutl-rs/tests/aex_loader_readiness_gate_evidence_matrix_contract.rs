use std::collections::BTreeSet;

const MATRIX: &str =
    include_str!("../../analysis/AEX_LOADER_READINESS_GATE_EVIDENCE_MATRIX_2026-06-03.md");

fn table_rows_after(heading: &str) -> Vec<Vec<&'static str>> {
    let mut rows = Vec::new();
    let mut in_section = false;

    for line in MATRIX.lines() {
        if line == heading {
            in_section = true;
            continue;
        }
        if in_section && line.starts_with("## ") {
            break;
        }
        if !in_section || !line.starts_with('|') || line.contains("---") {
            continue;
        }

        let cells = line
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect::<Vec<_>>();
        if cells.first().is_some_and(|cell| *cell == "Item") {
            continue;
        }
        rows.push(cells);
    }

    rows
}

#[test]
fn evidence_matrix_locks_current_rows_and_classifications() {
    let rows = table_rows_after("## Evidence Matrix");
    let items = rows
        .iter()
        .map(|row| {
            assert_eq!(row.len(), 4, "evidence row shape changed: {row:?}");
            (row[0], row[1])
        })
        .collect::<Vec<_>>();

    assert_eq!(
        items,
        vec![
            ("Final readiness gate report shape", "Merged/Ready"),
            ("Complete no-load evidence status", "Measured"),
            ("Gate remains closed after evidence completion", "Measured"),
            ("Loader preflight no-load invariant", "Measured"),
            ("Provenance chain no-load invariant", "Measured"),
            ("Forbidden-token contamination check", "Measured"),
            ("Fixture identity smoke presence", "Measured"),
            ("Synthetic identity transport", "Implemented but Approx"),
            ("Loader slice review packet relationship", "Measured"),
            (
                "Manual approval receipt boundary",
                "Blocked by Native Oracle",
            ),
            ("Native AEX loading", "Blocked by Native Oracle"),
            (
                "Selector execution and pixel rendering",
                "Blocked by Native Oracle",
            ),
            ("OFX route to AEX", "Blocked by Native Oracle"),
            (
                "Adobe SDK/native ABI implementation",
                "Blocked by Native Oracle",
            ),
        ]
    );

    let allowed = BTreeSet::from([
        "Merged/Ready",
        "Measured",
        "Implemented but Approx",
        "Blocked by Native Oracle",
    ]);
    assert_eq!(
        BTreeSet::from_iter(items.iter().map(|(_, classification)| *classification)),
        allowed
    );
}

#[test]
fn evidence_matrix_keeps_no_load_invariants_explicit() {
    for invariant in [
        "`final_gate_closed=true`",
        "`may_load_aex=false`",
        "`native_load_performed=false`",
        "`selectors_executed=false`",
        "`render_performed=false`",
        "`broker_may_load_plugin=false`",
        "`ofx_route_allowed=false`",
        "`allow_native_aex_load=false`",
        "`allow_worker_plugin_load=false`",
        "`allow_render_png=false`",
        "`allow_ofx_route=false`",
        "`allow_aex_sdk_or_abi_import=false`",
    ] {
        assert!(
            MATRIX.contains(invariant),
            "evidence matrix lost no-load invariant {invariant}"
        );
    }
}

#[test]
fn evidence_matrix_keeps_non_claims_and_blocked_boundary() {
    for non_claim in [
        "It does not prove that an AEX binary can be loaded safely or correctly.",
        "It does not prove selector dispatch, `render_png`, parameter discovery,",
        "It does not prove OFX-to-AEX routing.",
        "It does not replace local operator approval, code review, license review,",
        "It does not make private paths, AEX payloads, binaries, hashes, rendered",
    ] {
        assert!(
            MATRIX.contains(non_claim),
            "evidence matrix lost non-claim {non_claim}"
        );
    }

    for blocked_item in [
        "Any actual `.aex` open/hash/copy/load/execute/describe/render operation.",
        "Any worker plug-in load or broker permission to call native loader APIs.",
        "Any PF selector execution or pixel-buffer exchange with a native effect.",
        "Any AEX render correctness or pixel parity claim.",
        "Any OFX facade route that reaches an AEX loader.",
        "Any import of Adobe SDK headers, native ABI layouts, or native loader symbols.",
        "Any promotion from no-load readiness evidence to implementation approval",
    ] {
        assert!(
            MATRIX.contains(blocked_item),
            "evidence matrix lost blocked boundary {blocked_item}"
        );
    }
}

#[test]
fn evidence_matrix_does_not_promote_native_aex_loading() {
    for promoted_claim in [
        "native AEX loading is approved",
        "native AEX loading is verified",
        "selector dispatch is verified",
        "render_png is verified",
        "pixel parity is verified",
        "OFX-to-AEX routing is approved",
        "Adobe SDK/native ABI implementation is approved",
    ] {
        assert!(
            !MATRIX.contains(promoted_claim),
            "evidence matrix gained premature promotion claim {promoted_claim}"
        );
    }
}
