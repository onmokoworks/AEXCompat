use aexcompat_broker::host_core::descriptor_manifest::{load, ManifestPolicy};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const HASH: &str = "C797EC7C45A603D2C86FB980DC5279E8B075D0E27D466315A2ABFA15BE608C37";
const BYTES: &[u8] = include_bytes!("../../../../profiles/scattermap/parameter_descriptors.json");

#[test]
fn promoted_observation_is_digest_bound_and_tamper_evident() {
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
        sha256: HASH,
    };
    let loaded = load(&root, "scattermap", policy).unwrap();
    assert_eq!(loaded.observed_descriptor_count, 7);
    assert_eq!(loaded.profile.descriptors.len(), 5);
    assert_eq!(loaded.profile.descriptors[3].slot, 5);
    assert_eq!(loaded.receipt_id, "scattermap-l2-20260713-001");

    let mut tampered: serde_json::Value = serde_json::from_slice(BYTES).unwrap();
    fs::write(
        root.join("profiles/manifest.json"),
        serde_json::to_vec(&tampered).unwrap(),
    )
    .unwrap();
    assert!(load(&root, "scattermap", policy).is_ok());

    tampered["descriptors"][0]["maximum"] = serde_json::json!(501);
    fs::write(
        root.join("profiles/manifest.json"),
        serde_json::to_vec_pretty(&tampered).unwrap(),
    )
    .unwrap();
    assert!(load(&root, "scattermap", policy).is_err());
    fs::remove_dir_all(root).unwrap();
}
