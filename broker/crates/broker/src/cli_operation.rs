const LEGACY_ALIASES: &[(&str, &str)] = &[
    ("l2", "discovery"),
    ("l2-scattermap", "discovery-scattermap"),
    ("render-parameter-request", "classic-parameter-request"),
    ("render-scattermap", "classic-scattermap"),
    ("render-identity-scattermap", "classic-identity-scattermap"),
    (
        "render-horizontal-scattermap",
        "classic-horizontal-scattermap",
    ),
    (
        "render-vertical-no-repeat-scattermap",
        "classic-vertical-no-repeat-scattermap",
    ),
    ("render-mixed-scattermap", "classic-mixed-scattermap"),
    (
        "render-amount-max-scattermap",
        "classic-amount-max-scattermap",
    ),
    ("render-seed-max-scattermap", "classic-seed-max-scattermap"),
    ("render-mix-zero-scattermap", "classic-mix-zero-scattermap"),
    (
        "render-odd-dimensions-scattermap",
        "classic-odd-dimensions-scattermap",
    ),
    (
        "render-padded-stride-scattermap",
        "classic-padded-stride-scattermap",
    ),
    (
        "render-connected-map-scattermap",
        "classic-connected-map-scattermap",
    ),
    (
        "render-inverted-map-scattermap",
        "classic-inverted-map-scattermap",
    ),
    (
        "render-partial-extent-hint-scattermap",
        "classic-partial-extent-hint-scattermap",
    ),
    (
        "render-threaded-default-scattermap",
        "classic-threaded-default-scattermap",
    ),
];

/// Returns the canonical worker-kind CLI operation while retaining the old
/// external spellings as bounded compatibility aliases.
pub fn canonical_operation(operation: &str) -> &str {
    LEGACY_ALIASES
        .iter()
        .find_map(|(legacy, canonical)| (*legacy == operation).then_some(*canonical))
        .unwrap_or(operation)
}

#[cfg(test)]
mod tests {
    use super::canonical_operation;

    #[test]
    fn every_legacy_route_maps_to_its_worker_kind_name() {
        let expected = [
            ("l2", "discovery"),
            ("l2-scattermap", "discovery-scattermap"),
            ("render-parameter-request", "classic-parameter-request"),
            ("render-scattermap", "classic-scattermap"),
            ("render-identity-scattermap", "classic-identity-scattermap"),
            (
                "render-horizontal-scattermap",
                "classic-horizontal-scattermap",
            ),
            (
                "render-vertical-no-repeat-scattermap",
                "classic-vertical-no-repeat-scattermap",
            ),
            ("render-mixed-scattermap", "classic-mixed-scattermap"),
            (
                "render-amount-max-scattermap",
                "classic-amount-max-scattermap",
            ),
            ("render-seed-max-scattermap", "classic-seed-max-scattermap"),
            ("render-mix-zero-scattermap", "classic-mix-zero-scattermap"),
            (
                "render-odd-dimensions-scattermap",
                "classic-odd-dimensions-scattermap",
            ),
            (
                "render-padded-stride-scattermap",
                "classic-padded-stride-scattermap",
            ),
            (
                "render-connected-map-scattermap",
                "classic-connected-map-scattermap",
            ),
            (
                "render-inverted-map-scattermap",
                "classic-inverted-map-scattermap",
            ),
            (
                "render-partial-extent-hint-scattermap",
                "classic-partial-extent-hint-scattermap",
            ),
            (
                "render-threaded-default-scattermap",
                "classic-threaded-default-scattermap",
            ),
        ];
        for (legacy, canonical) in expected {
            assert_eq!(canonical_operation(legacy), canonical, "{legacy}");
            assert_eq!(canonical_operation(canonical), canonical, "{canonical}");
        }
    }

    #[test]
    fn action_names_and_unknown_operations_are_not_reclassified() {
        for operation in [
            "validate-render-request",
            "render-video-batch",
            "smart-parameter-request",
            "smart-scattermap",
            "selftest",
            "render-unknown-operation",
        ] {
            assert_eq!(canonical_operation(operation), operation);
        }
    }
}
