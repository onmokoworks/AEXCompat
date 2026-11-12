#![cfg(windows)]
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aexcompat-independent-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, value: &Value) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn invoke(manifest: &Path, repository: &Path) -> (bool, Value) {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_aexcompat-harness"))
        .args(["--headless", "--render-request-batch"])
        .arg(manifest)
        .env("AEXCOMPAT_REPOSITORY_ROOT", repository)
        .output()
        .unwrap();
    let report = serde_json::from_slice(&result.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
    });
    (result.status.success(), report)
}
fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}
fn manifest(plugin: &Path, cases: Vec<Value>) -> Value {
    json!({"schema":"aexcompat.independent_case_batch","schema_version":1,
        "plugin":plugin,"route":"classic8","isolation":"fresh_sequence",
        "frame_deadline_ms":10000,"cases":cases})
}

#[test]
fn classic_request_rejects_smart_effect_before_render_worker() {
    let scratch = Scratch::new();
    let repo = repository();
    let plugin = repo.join("target/minihost-build/worker_independent_smart_fixture.aex");
    let input = scratch.0.join("input.png");
    image::RgbaImage::from_pixel(8, 4, image::Rgba([255u8; 4]))
        .save(&input)
        .unwrap();
    let request = scratch.write(
        "request.json",
        &json!({"schema_version":1,"assignments":[]}),
    );
    let output = scratch.0.join("output.png");
    let value = manifest(
        &plugin,
        vec![json!({"id":"A","input":input,"request":request,"output":output})],
    );
    let (passed, report) = invoke(&scratch.write("manifest.json", &value), &repo);
    assert!(!passed, "{report}");
    assert!(
        report.to_string().contains("no fallback is permitted"),
        "{report}"
    );
    assert!(!output.exists());
}

#[test]
fn duplicate_destinations_and_duplicate_request_keys_fail_before_plugin_load() {
    let scratch = Scratch::new();
    let repo = repository();
    let request = scratch.write(
        "request.json",
        &json!({"schema_version":1,"assignments":[]}),
    );
    let input = scratch.0.join("input.png");
    let output = scratch.0.join("output.png");
    let case = json!({"id":"A","input":input,"request":request,"output":output});
    let mut other = case.clone();
    other["id"] = json!("B");
    let value = manifest(
        &scratch.0.join("nonexistent.aex"),
        vec![case.clone(), other],
    );
    let (passed, report) = invoke(&scratch.write("manifest.json", &value), &repo);
    assert!(!passed);
    assert!(report.to_string().contains("fresh and unique"), "{report}");
    assert!(!output.exists());
    std::fs::write(
        &request,
        br#"{"schema_version":1,"assignments":[],"assignments":[]}"#,
    )
    .unwrap();
    let value = manifest(&scratch.0.join("nonexistent.aex"), vec![case]);
    let (passed, report) = invoke(&scratch.write("manifest.json", &value), &repo);
    assert!(!passed);
    assert!(
        report.to_string().contains("duplicate object key"),
        "{report}"
    );
    assert!(!output.exists());
}

#[test]
fn independent_cli_preflights_all_cases_then_preserves_failure_isolation() {
    let scratch = Scratch::new();
    let repo = repository();
    let plugin = repo.join("target/minihost-build/worker_independent_case_fixture.aex");
    assert!(plugin.is_file());
    let input = scratch.0.join("input.png");
    image::RgbaImage::from_pixel(8, 4, image::Rgba([255u8; 4]))
        .save(&input)
        .unwrap();
    let mut cases = vec![];
    for (index, (frame, assignments)) in [
        (0, json!([{ "slot":1,"value":51 }])),
        (13, json!([])),
        (0, json!([])),
    ]
    .into_iter()
    .enumerate()
    {
        let request = scratch.write(
            &format!("request{index}.json"),
            &json!({"schema_version":1,
            "timing":{"frame":frame,"fps":30,"duration_frames":30},"assignments":assignments}),
        );
        cases.push(
            json!({"id":index.to_string(),"input":input,"request":request,
            "output":scratch.0.join(format!("out{index}.png"))}),
        );
    }
    let value = manifest(&plugin, cases.clone());
    let (passed, report) = invoke(&scratch.write("manifest.json", &value), &repo);
    assert!(!passed, "one render deliberately fails");
    assert_eq!(report["complete"], true, "{report}");
    assert_eq!(report["cases"][0]["evidence_valid"], true, "{report}");
    assert_eq!(report["cases"][1]["render_error"], 7, "{report}");
    assert_eq!(report["cases"][2]["passed"], true, "{report}");
    assert_eq!(
        report["cases"][0]["worker_pid"],
        report["cases"][2]["worker_pid"]
    );
    assert_eq!(
        image::open(scratch.0.join("out0.png"))
            .unwrap()
            .to_rgba8()
            .get_pixel(0, 0)
            .0,
        [1, 51, 1, 255]
    );
    assert!(!scratch.0.join("out1.png").exists());
    assert_eq!(
        image::open(scratch.0.join("out2.png"))
            .unwrap()
            .to_rgba8()
            .get_pixel(0, 0)
            .0,
        [1, 17, 1, 255]
    );

    // A cleanup failure revokes earlier provisional evidence and never dispatches C.
    let mut poisoned_cases = cases.clone();
    for (index, case) in poisoned_cases.iter_mut().enumerate() {
        case["output"] = json!(scratch.0.join(format!("poisoned{index}.png")));
    }
    let poison_request = scratch.write(
        "poison-request.json",
        &json!({"schema_version":1,
        "timing":{"frame":18,"fps":30,"duration_frames":30},"assignments":[]}),
    );
    poisoned_cases[1]["request"] = json!(poison_request);
    let (passed, report) = invoke(
        &scratch.write("manifest.json", &manifest(&plugin, poisoned_cases)),
        &repo,
    );
    assert!(!passed);
    assert_eq!(report["complete"], false, "{report}");
    assert_eq!(
        report["cases"][0]["render_passed_before_close"], true,
        "{report}"
    );
    assert_eq!(report["cases"][0]["passed"], false, "{report}");
    assert_eq!(report["cases"][0]["evidence_valid"], false, "{report}");
    assert_eq!(report["cases"][2]["outcome"], "unattempted", "{report}");
    assert!(!scratch.0.join("poisoned2.png").exists());

    // An invalid later request must publish none of the earlier valid cases.
    for (index, case) in cases.iter_mut().enumerate() {
        case["output"] = json!(scratch.0.join(format!("rejected{index}.png")));
    }
    scratch.write(
        "request2.json",
        &json!({"schema_version":1,"assignments":[{"slot":1,"value":101}]}),
    );
    let (passed, report) = invoke(
        &scratch.write("manifest.json", &manifest(&plugin, cases.clone())),
        &repo,
    );
    assert!(!passed);
    assert!(report.to_string().contains("out of range"), "{report}");
    assert!(!scratch.0.join("rejected0.png").exists());
    scratch.write(
        "request2.json",
        &json!({"schema_version":1,"assignments":[]}),
    );
    let different = scratch.0.join("different.png");
    image::RgbaImage::from_pixel(9, 4, image::Rgba([255u8; 4]))
        .save(&different)
        .unwrap();
    cases[2]["input"] = json!(different);
    let (passed, report) = invoke(
        &scratch.write("manifest.json", &manifest(&plugin, cases)),
        &repo,
    );
    assert!(!passed);
    assert!(
        report.to_string().contains("identical static context"),
        "{report}"
    );
    assert!(!scratch.0.join("rejected0.png").exists());
}
