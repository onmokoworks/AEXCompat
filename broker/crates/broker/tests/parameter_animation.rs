use aexcompat_broker::image_render::{
    parameter_animation_sidecar_json, AnimationInterpolation, AnimationTime, AnimationValue,
    ParameterAnimation, ParameterAnimationKey,
};
use serde_json::Value;

/// Broker-driven one-shot regression for issue #141: the worker's native
/// sidecar loader pins the sidecar parent to current_path()/target/
/// image-transport, so a one-shot launch whose cwd is the staging root (the
/// pre-fix behavior) rejects every `--parameter-animation-v1` dispatch with
/// parse error 3 before rendering. Driving the real render worker through the
/// full `dispatch_secure_image` pipeline proves the launch cwd matches the
/// broker-owned transport directory. Gated on the locally built worker and the
/// pf_param_utils_animation_probe fixture, like the other real-worker gates.
#[cfg(windows)]
mod windows_real_worker {
    use aexcompat_broker::image_render::{
        render_experimental_image_with_parameter_animation, AnimationInterpolation, AnimationTime,
        AnimationValue, InteractiveParameter, ParameterAnimation, ParameterAnimationKey,
        RenderTiming,
    };
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct RemoveOnDrop(Vec<PathBuf>);

    impl Drop for RemoveOnDrop {
        fn drop(&mut self) {
            for path in &self.0 {
                let _ = fs::remove_file(path);
            }
        }
    }

    fn scalar_key(time: (i32, u32), interpolation: AnimationInterpolation, value: f64) -> ParameterAnimationKey {
        ParameterAnimationKey {
            time: AnimationTime {
                value: time.0,
                scale: time.1,
            },
            interpolation,
            value: AnimationValue::Scalar { value },
        }
    }

    #[test]
    fn one_shot_dispatch_delivers_the_animation_sidecar_to_the_real_worker() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let worker = repository.join("target/minihost-build/aex_render_worker.exe");
        let plugin = repository.join(
            "target/pf-param-utils-animation-probe-build/Release/pf_param_utils_animation_probe.aex",
        );
        if !worker.exists() || !plugin.exists() {
            return;
        }
        let hash = format!("{:X}", Sha256::digest(fs::read(&plugin).unwrap()));
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let input = std::env::temp_dir().join(format!("aexcompat-oneshot-animation-input-{nonce}.png"));
        let output = std::env::temp_dir().join(format!("aexcompat-oneshot-animation-output-{nonce}.png"));
        let _cleanup = RemoveOnDrop(vec![input.clone(), output.clone()]);
        image::RgbaImage::from_pixel(7, 5, image::Rgba([255, 0, 0, 0]))
            .save(&input)
            .unwrap();

        // The probe declares exactly one float parameter ("Animated", slot 1)
        // and validates a three-key timeline at times 0, 12, 24 on scale 24
        // through PF_ParamUtilsSuite3; on success it fills the output with
        // opaque green (0, 211, 0) and encodes any failure mask in the red and
        // blue channels.
        let parameters = [InteractiveParameter {
            slot: 1,
            name: "Animated".into(),
            kind: "float".into(),
            minimum: -100.0,
            maximum: 100.0,
            value: 10.0,
            choices: Vec::new(),
            color: [0; 4],
            components: [0.0; 3],
            component_count: 0,
            layer_path: None,
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }];
        let animations = [ParameterAnimation {
            slot: 1,
            keys: vec![
                scalar_key((0, 24), AnimationInterpolation::Linear, 10.0),
                scalar_key((12, 24), AnimationInterpolation::Linear, 20.0),
                scalar_key((24, 24), AnimationInterpolation::Hold, 30.0),
            ],
        }];
        let report = render_experimental_image_with_parameter_animation(
            repository,
            &plugin,
            &hash,
            &input,
            &output,
            &parameters,
            &animations,
            RenderTiming {
                current_time: 12,
                time_step: 1,
                total_time: 24,
                time_scale: 1,
            },
        )
        .expect("broker one-shot dispatch must deliver the animation sidecar to the worker");
        assert_eq!(report["passed"], true, "report: {report}");
        assert_eq!(report["worker_classification"], "ok", "report: {report}");
        let rendered = image::open(&output).unwrap().into_rgba8();
        assert_eq!((rendered.width(), rendered.height()), (7, 5));
        assert!(
            rendered.pixels().all(|pixel| pixel.0 == [0, 211, 0, 255]),
            "probe reported keyframe failures: report={report}, pixel={:?}",
            rendered.pixels().next()
        );
    }
}

fn scalar(time: (i32, u32), value: f64) -> ParameterAnimationKey {
    ParameterAnimationKey {
        time: AnimationTime {
            value: time.0,
            scale: time.1,
        },
        interpolation: AnimationInterpolation::Linear,
        value: AnimationValue::Scalar { value },
    }
}

#[test]
fn sidecar_is_versioned_and_keeps_typed_values_out_of_cli_payloads() {
    let bytes = parameter_animation_sidecar_json(&[ParameterAnimation {
        slot: 2,
        keys: vec![
            scalar((0, 30), 1.0),
            ParameterAnimationKey {
                time: AnimationTime {
                    value: 1,
                    scale: 30,
                },
                interpolation: AnimationInterpolation::Hold,
                value: AnimationValue::Color {
                    value: [255, 10, 20, 30],
                },
            },
        ],
    }])
    .unwrap();
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["parameters"][0]["slot"], 2);
    assert_eq!(
        document["parameters"][0]["keys"][1]["value"]["type"],
        "color"
    );
}

#[test]
fn rational_order_is_strict_and_duplicate_slots_are_rejected() {
    let duplicate_time = ParameterAnimation {
        slot: 1,
        keys: vec![scalar((1, 2), 0.0), scalar((2, 4), 1.0)],
    };
    assert!(parameter_animation_sidecar_json(&[duplicate_time]).is_err());

    let one = ParameterAnimation {
        slot: 7,
        keys: vec![scalar((0, 1), 0.0)],
    };
    assert!(parameter_animation_sidecar_json(&[one.clone(), one]).is_err());
}

#[test]
fn zero_scale_nonfinite_and_invalid_components_fail_closed() {
    let cases = [
        ParameterAnimationKey {
            time: AnimationTime { value: 0, scale: 0 },
            interpolation: AnimationInterpolation::Hold,
            value: AnimationValue::Scalar { value: 0.0 },
        },
        scalar((0, 1), f64::NAN),
        ParameterAnimationKey {
            time: AnimationTime { value: 0, scale: 1 },
            interpolation: AnimationInterpolation::Linear,
            value: AnimationValue::Components { value: vec![] },
        },
    ];
    for key in cases {
        assert!(parameter_animation_sidecar_json(&[ParameterAnimation {
            slot: 1,
            keys: vec![key],
        }])
        .is_err());
    }
}

#[test]
fn per_parameter_and_total_key_limits_are_strict() {
    let too_many = ParameterAnimation {
        slot: 1,
        keys: (0..257).map(|time| scalar((time, 1), 0.0)).collect(),
    };
    assert!(parameter_animation_sidecar_json(&[too_many]).is_err());

    let animations = (1..=17)
        .map(|slot| ParameterAnimation {
            slot,
            keys: (0..256).map(|time| scalar((time, 1), 0.0)).collect(),
        })
        .collect::<Vec<_>>();
    assert!(parameter_animation_sidecar_json(&animations).is_err());
}

#[test]
fn arbitrary_keys_are_versioned_and_byte_bounded() {
    let animation = |value| ParameterAnimation {
        slot: 3,
        keys: vec![ParameterAnimationKey {
            time: AnimationTime { value: 0, scale: 1 },
            interpolation: AnimationInterpolation::Linear,
            value: AnimationValue::Arbitrary { value },
        }],
    };
    let bytes = parameter_animation_sidecar_json(&[animation(vec![0x43, 0x47, 1, 2])]).unwrap();
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(
        document["parameters"][0]["keys"][0]["value"]["type"],
        "arbitrary"
    );
    assert!(parameter_animation_sidecar_json(&[animation(vec![])]).is_err());
    assert!(parameter_animation_sidecar_json(&[animation(vec![0; 64 * 1024 + 1])]).is_err());
}
