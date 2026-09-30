use aexcompat_broker::host_core::descriptor_manifest::{ManifestPolicy, load};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const BYTES: &[u8] = include_bytes!("../../../../profiles/scattermap/parameter_descriptors.json");
const MASKOFFSET_BYTES: &[u8] =
    include_bytes!("../../../../profiles/maskoffset/parameter_descriptors.json");

/// The manifest is accepted on its structure, not on a digest compiled into
/// the broker (issue #733): an edit that breaks a descriptor contract is what
/// fails, not one that merely changes a value.
#[test]
fn promoted_observation_is_accepted_on_structure_not_a_pinned_digest() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "aexcompat-descriptor-manifest-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("profiles")).unwrap();
    fs::write(root.join("profiles/manifest.json"), BYTES).unwrap();
    let policy = ManifestPolicy {
        path: "profiles/manifest.json",
    };
    let loaded = load(&root, "scattermap", policy).unwrap();
    assert_eq!(loaded.observed_descriptor_count, 7);
    assert_eq!(loaded.profile.descriptors.len(), 5);
    assert_eq!(loaded.profile.descriptors[3].slot, 5);
    assert_eq!(loaded.receipt_id, "scattermap-l2-20260713-001");

    let mut tampered: serde_json::Value = serde_json::from_slice(BYTES).unwrap();

    // Editing a bound is now an ordinary edit: the descriptors describe the
    // plug-in, and the broker no longer holds a compiled-in opinion about what
    // that description must be.
    tampered["descriptors"][0]["maximum"] = serde_json::json!(501);
    fs::write(
        root.join("profiles/manifest.json"),
        serde_json::to_vec_pretty(&tampered).unwrap(),
    )
    .unwrap();
    assert!(load(&root, "scattermap", policy).is_ok());

    // What still fails is a manifest that cannot drive a render: a slot
    // sequence with a hole leaves the host unable to address the parameters.
    tampered["descriptors"][0]["slot"] = serde_json::json!(2);
    fs::write(
        root.join("profiles/manifest.json"),
        serde_json::to_vec_pretty(&tampered).unwrap(),
    )
    .unwrap();
    assert!(load(&root, "scattermap", policy).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn second_fixture_manifest_loads_through_the_generic_core() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "aexcompat-maskoffset-manifest-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("profiles")).unwrap();
    fs::write(root.join("profiles/manifest.json"), MASKOFFSET_BYTES).unwrap();
    let policy = ManifestPolicy {
        path: "profiles/manifest.json",
    };
    let loaded = load(&root, "maskoffset", policy).unwrap();
    assert_eq!(loaded.observed_descriptor_count, 9);
    assert_eq!(loaded.profile.descriptors.len(), 9);
    assert_eq!(
        loaded.profile.descriptors[1].kind,
        aexcompat_broker::host_core::parameter::ValueKind::Color
    );
    assert_eq!(loaded.receipt_id, "maskoffset-l2-20260713-001");
    fs::remove_dir_all(root).unwrap();
}
