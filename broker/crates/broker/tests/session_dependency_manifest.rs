use aexcompat_broker::secure_image_dispatch::ApprovedImageArtifact;
use aexcompat_broker::session_dependency_manifest::{MAX_SESSION_DEPENDENCIES, parse_and_validate};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture {
    root: PathBuf,
    main: ApprovedImageArtifact,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-session-manifest-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let main_path = root.join("main.plugin");
        fs::write(&main_path, b"main").unwrap();
        Self {
            root,
            main: ApprovedImageArtifact {
                path: main_path,
                expected_sha256: Sha256::digest(b"main").into(),
                expected_size: 4,
            },
        }
    }

    fn dependency(&self, name: &str, bytes: &[u8]) -> Value {
        let path = self.root.join(name);
        fs::write(&path, bytes).unwrap();
        json!({
            "path": path,
            "basename": name,
            "sha256": format!("{:x}", Sha256::digest(bytes)),
            "size": bytes.len()
        })
    }

    fn parse(&self, dependencies: Vec<Value>) -> std::io::Result<Vec<ApprovedImageArtifact>> {
        let json = serde_json::to_vec(&json!({
            "schema_version": 1,
            "dependencies": dependencies
        }))
        .unwrap();
        parse_and_validate(&json, &self.main)
            .map(|manifest| manifest.into_approved_image_artifacts())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn converts_strict_json_to_dispatch_artifacts() {
    let fixture = Fixture::new();
    let approved = fixture
        .parse(vec![fixture.dependency("helper.dll", b"helper")])
        .unwrap();
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].path, fixture.root.join("helper.dll"));
    assert_eq!(
        approved[0].expected_sha256,
        Sha256::digest(b"helper").as_slice()
    );
    assert_eq!(approved[0].expected_size, 6);
}

#[test]
fn rejects_unknown_fields_and_more_than_64_dependencies() {
    let fixture = Fixture::new();
    let unknown = serde_json::to_vec(&json!({
        "schema_version": 1, "dependencies": [], "extra": true
    }))
    .unwrap();
    assert!(parse_and_validate(&unknown, &fixture.main).is_err());

    let dependencies = (0..=MAX_SESSION_DEPENDENCIES)
        .map(|index| fixture.dependency(&format!("d{index}.dll"), b"x"))
        .collect();
    assert!(fixture.parse(dependencies).is_err());
}

#[test]
fn rejects_duplicates_case_collisions_and_main_collision() {
    let fixture = Fixture::new();
    let first = fixture.dependency("helper.dll", b"one");
    let duplicate = first.clone();
    assert!(fixture.parse(vec![first, duplicate]).is_err());

    let lower = fixture.dependency("case.dll", b"one");
    let upper = fixture.dependency("CASE.DLL", b"two");
    assert!(fixture.parse(vec![lower, upper]).is_err());

    let main_collision = fixture.dependency("MAIN.PLUGIN", b"other");
    assert!(fixture.parse(vec![main_collision]).is_err());
}

#[test]
fn rejects_unsafe_names_bad_hash_zero_or_mismatched_size() {
    let fixture = Fixture::new();
    for name in ["CON.dll", "trail.dll.", "bad:name.dll"] {
        let path = fixture.root.join("safe.dll");
        fs::write(&path, b"x").unwrap();
        let dependency = json!({"path":path,"basename":name,"sha256":"ab".repeat(32),"size":1});
        assert!(fixture.parse(vec![dependency]).is_err(), "accepted {name}");
    }

    let mut bad_hash = fixture.dependency("hash.dll", b"x");
    bad_hash["sha256"] = json!("not-a-hash");
    assert!(fixture.parse(vec![bad_hash]).is_err());
    let mut zero = fixture.dependency("zero.dll", b"x");
    zero["size"] = json!(0);
    assert!(fixture.parse(vec![zero]).is_err());
    let mut wrong_size = fixture.dependency("size.dll", b"x");
    wrong_size["size"] = json!(2);
    assert!(fixture.parse(vec![wrong_size]).is_err());
}

#[test]
fn rejects_relative_paths() {
    let fixture = Fixture::new();
    let dependency = json!({
        "path":"relative.dll", "basename":"relative.dll", "sha256":"ab".repeat(32), "size":1
    });
    assert!(fixture.parse(vec![dependency]).is_err());
}

#[test]
fn rejects_changed_or_mismatched_main_plugin_identity() {
    let mut fixture = Fixture::new();
    fixture.main.expected_size = 5;
    assert!(fixture.parse(Vec::new()).is_err());

    fixture.main.expected_size = 4;
    fixture.main.expected_sha256 = [0; 32];
    assert!(fixture.parse(Vec::new()).is_err());
}

#[test]
fn rejects_hardlinks() {
    let fixture = Fixture::new();
    let original = fixture.root.join("original.dll");
    let linked = fixture.root.join("linked.dll");
    fs::write(&original, b"linked").unwrap();
    fs::hard_link(&original, &linked).unwrap();
    let dependency = json!({
        "path": original, "basename":"original.dll", "sha256":"ab".repeat(32), "size":6
    });
    assert!(fixture.parse(vec![dependency]).is_err());
}

#[test]
fn rejects_symlink_or_reparse_sources() {
    let fixture = Fixture::new();
    let target = fixture.root.join("target.dll");
    let link = fixture.root.join("link.dll");
    fs::write(&target, b"target").unwrap();
    if create_file_link(&target, &link).is_err() {
        return;
    }
    let dependency = json!({
        "path": link, "basename":"link.dll", "sha256":"ab".repeat(32), "size":6
    });
    assert!(fixture.parse(vec![dependency]).is_err());
}

#[cfg(windows)]
fn create_file_link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(unix)]
fn create_file_link(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(any(windows, unix)))]
fn create_file_link(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "unsupported",
    ))
}
