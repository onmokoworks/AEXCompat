#![cfg(windows)]
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
fn invoke(repo: &Path, args: &[&Path]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_aexcompat-harness"))
        .arg("--headless")
        .args(args)
        .env("AEXCOMPAT_REPOSITORY_ROOT", repo)
        .output()
        .unwrap()
}
fn require_ok(result: &std::process::Output) {
    assert!(
        result.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn inspect_typed_and_batch_preserve_every_large_parameter() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let scratch = Scratch(std::env::temp_dir().join(format!("aex-many-{}", std::process::id())));
    std::fs::create_dir(&scratch.0).unwrap();
    let input = scratch.0.join("input.png");
    image::RgbaImage::from_pixel(3, 2, image::Rgba([1u8, 2, 3, 255]))
        .save(&input)
        .unwrap();
    for count in [1025u32, 1623, 4096] {
        let plugin = repo.join(format!(
            "target/minihost-build/worker_many_parameters_{count}.aex"
        ));
        let inspected = invoke(&repo, &[Path::new("--inspect-experimental"), &plugin]);
        require_ok(&inspected);
        let inspection: Value = serde_json::from_slice(&inspected.stdout).unwrap();
        assert_eq!(inspection.as_array().unwrap().len(), count as usize);
        let mut cases = Vec::new();
        for (index, all) in [true, false, true].into_iter().enumerate() {
            let assignments = if all {
                (1..=count)
                    .map(|slot| json!({"slot":slot,"value":slot%251}))
                    .collect::<Vec<_>>()
            } else {
                vec![json!({"slot":count,"value":123})]
            };
            let request = scratch.0.join(format!("{count}-{index}.json"));
            let document =
                serde_json::to_vec(&json!({"schema_version":1,"assignments":assignments})).unwrap();
            if all {
                assert!(document.len() > 16384);
            }
            std::fs::write(&request, document).unwrap();
            let single = scratch.0.join(format!("single-{count}-{index}.png"));
            let rendered = invoke(
                &repo,
                &[
                    Path::new("--render-experimental-request"),
                    &plugin,
                    &input,
                    &single,
                    &request,
                ],
            );
            require_ok(&rendered);
            let sum = (1..=count)
                .map(|slot| {
                    slot * if all {
                        slot % 251
                    } else if slot == count {
                        123
                    } else {
                        17
                    }
                })
                .sum::<u32>();
            let expected = [sum as u8, (sum >> 8) as u8, (sum >> 16) as u8, 255];
            assert!(
                image::open(&single)
                    .unwrap()
                    .to_rgba8()
                    .pixels()
                    .all(|p| p.0 == expected)
            );
            cases.push(
                json!({"id":index.to_string(),"input":input,"request":request,
                "output":scratch.0.join(format!("batch-{count}-{index}.png"))}),
            );
        }
        let manifest = scratch.0.join("manifest.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec(&json!({"schema":"aexcompat.independent_case_batch",
            "schema_version":1,"plugin":plugin,"route":"classic8","isolation":"fresh_sequence",
            "frame_deadline_ms":10000,"cases":cases}))
            .unwrap(),
        )
        .unwrap();
        let batch = invoke(&repo, &[Path::new("--render-request-batch"), &manifest]);
        require_ok(&batch);
        let report: Value = serde_json::from_slice(&batch.stdout).unwrap();
        assert_eq!(report["complete"], true);
        for index in 0..3 {
            assert_eq!(report["cases"][index]["evidence_valid"], true, "{report}");
            let single = image::open(scratch.0.join(format!("single-{count}-{index}.png")))
                .unwrap()
                .to_rgba8();
            let batch = image::open(scratch.0.join(format!("batch-{count}-{index}.png")))
                .unwrap()
                .to_rgba8();
            assert_eq!(single, batch);
        }
    }
    let plugin = repo.join("target/minihost-build/worker_many_parameters_4097.aex");
    let rejected = invoke(&repo, &[Path::new("--inspect-experimental"), &plugin]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("inspection"));
}
