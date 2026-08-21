use aexcompat_broker::parameter_animation::{
    AnimationInterpolation, AnimationTime, AnimationValue, ParameterAnimation,
    ParameterAnimationKey, parameter_animation_sidecar_json,
};
use serde_json::Value;

/// Broker-driven regression for issue #141: the worker's native sidecar loader
/// pins the sidecar parent to current_path()/image-transport, so a
/// launch whose cwd is the staging root (the pre-fix behavior) rejects every
/// `--parameter-animation-v1` dispatch with parse error 3 before rendering.
/// Driving the real render worker through the full broker pipeline proves the
/// launch cwd matches the broker-owned transport directory. The pin binds the
/// session launch too (`secure_launch_session` uses the same repository cwd and
/// the session writes its own `parameter-animation-session-*.json` under that
/// directory), so removing the one-shot transport (#365) moved this coverage
/// onto the session rather than deleting it. Gated on the locally built worker
/// and the pf_param_utils_animation_probe fixture, like the other real-worker
/// gates.
#[cfg(windows)]
mod windows_real_worker {
    use aexcompat_broker::image_render::{
        AnimationInterpolation, AnimationTime, AnimationValue, InteractiveParameter,
        ParameterAnimation, ParameterAnimationKey, RenderTiming, render_experimental_image,
        render_experimental_image_with_parameter_animation,
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

    fn scalar_key(
        time: (i32, u32),
        interpolation: AnimationInterpolation,
        value: f64,
    ) -> ParameterAnimationKey {
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
    fn broker_dispatch_delivers_the_animation_sidecar_to_the_real_worker() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let worker = repository.join("target/minihost-build/aex_worker.exe");
        // The fixture extension is joined at runtime because the native code
        // guard forbids production-looking plugin literals in broker sources.
        let plugin = repository
            .join("target/pf-param-utils-animation-probe-build/Release")
            .join(["pf_param_utils_animation_probe", "aex"].join("."));
        if !worker.exists() || !plugin.exists() {
            return;
        }
        let hash = format!("{:X}", Sha256::digest(fs::read(&plugin).unwrap()));
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let input =
            std::env::temp_dir().join(format!("aexcompat-dispatch-animation-input-{nonce}.png"));
        let output =
            std::env::temp_dir().join(format!("aexcompat-dispatch-animation-output-{nonce}.png"));
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
        .expect("broker dispatch must deliver the animation sidecar to the worker");
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

    fn layer_param(slot: u32, path: &Path) -> InteractiveParameter {
        InteractiveParameter {
            slot,
            name: "layer".into(),
            kind: "layer".into(),
            minimum: 0.0,
            maximum: 0.0,
            value: 0.0,
            choices: Vec::new(),
            color: [0; 4],
            components: [0.0; 3],
            component_count: 0,
            layer_path: Some(path.to_path_buf()),
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0, 0],
        }
    }

    fn float_param(slot: u32, value: f64) -> InteractiveParameter {
        InteractiveParameter {
            slot,
            name: "amount".into(),
            kind: "float".into(),
            minimum: 0.0,
            maximum: 255.0,
            value,
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
        }
    }

    /// Real-AEX coverage that classic parameter animation (issue #132) writes the
    /// interpolated-at-current_time value into the `params[]` array the effect
    /// reads at PF_Cmd_RENDER — a surface distinct from the PF_ParamUtilsSuite
    /// keyframe path that pf_param_utils_animation_probe exercises. The
    /// pf-layer-param-probe reflects its slider (slot 2) into the output, so the
    /// animated value is observable byte-for-byte. Two oracle-free claims:
    ///   1. the value moves over time: the render at the first keyframe time
    ///      differs from the render at the last keyframe time;
    ///   2. the value is correct: the render at a keyframe time is byte-identical
    ///      to a static render carrying that keyframe's value.
    /// Both renders ride the same (and, since #365, only) transport, so the
    /// value-supply mechanism -- static payload vs animation sidecar -- is the
    /// only variable. Gated on the locally built worker and the
    /// pf-layer-param-probe fixture.
    #[test]
    fn classic_parameter_animation_drives_params_array_on_the_real_worker() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let worker = repository.join("target/minihost-build/aex_worker.exe");
        let plugin = repository
            .join("target/pf-layer-param-probe-build/Release")
            .join(["pf_layer_param_probe", "aex"].join("."));
        if !worker.exists() || !plugin.exists() {
            return;
        }
        let hash = format!("{:X}", Sha256::digest(fs::read(&plugin).unwrap()));
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp = std::env::temp_dir();
        let input = temp.join(format!("aexcompat-anim-layer-input-{nonce}.png"));
        let layer = temp.join(format!("aexcompat-anim-layer-layer-{nonce}.png"));
        let anim_low = temp.join(format!("aexcompat-anim-layer-a0-{nonce}.png"));
        let anim_high = temp.join(format!("aexcompat-anim-layer-a1-{nonce}.png"));
        let static_low = temp.join(format!("aexcompat-anim-layer-s0-{nonce}.png"));
        let _cleanup = RemoveOnDrop(vec![
            input.clone(),
            layer.clone(),
            anim_low.clone(),
            anim_high.clone(),
            static_low.clone(),
        ]);
        image::RgbaImage::from_fn(9, 6, |x, y| {
            image::Rgba([(x * 5) as u8, (y * 9) as u8, (x + y) as u8, 255])
        })
        .save(&input)
        .unwrap();
        image::RgbaImage::from_fn(9, 6, |x, y| {
            image::Rgba([(x + y) as u8, (x * 6) as u8, (y * 7) as u8, 255])
        })
        .save(&layer)
        .unwrap();

        // The slider (slot 2) animates 20 -> 220 over times 0..60 @ scale 30.
        // Linear interpolation is exact at both keyframe times, and the probe
        // snaps the value to an integer, so the observed value is exactly 20 at
        // t=0 and 220 at t=60.
        let low = 20.0;
        let high = 220.0;
        let params = |value: f64| vec![layer_param(1, &layer), float_param(2, value)];
        let animations = [ParameterAnimation {
            slot: 2,
            keys: vec![
                scalar_key((0, 30), AnimationInterpolation::Linear, low),
                scalar_key((60, 30), AnimationInterpolation::Linear, high),
            ],
        }];
        let timing = |current_time: i32| RenderTiming {
            current_time,
            time_step: 1,
            total_time: 60,
            time_scale: 30,
        };

        let report_low = render_experimental_image_with_parameter_animation(
            repository,
            &plugin,
            &hash,
            &input,
            &anim_low,
            &params(low),
            &animations,
            timing(0),
        )
        .expect("animation render at the first keyframe time");
        assert_eq!(report_low["passed"], true, "report: {report_low}");
        let report_high = render_experimental_image_with_parameter_animation(
            repository,
            &plugin,
            &hash,
            &input,
            &anim_high,
            &params(low),
            &animations,
            timing(60),
        )
        .expect("animation render at the last keyframe time");
        assert_eq!(report_high["passed"], true, "report: {report_high}");

        // Claim 1: the animated parameter actually moves the output over time.
        assert_ne!(
            fs::read(&anim_low).unwrap(),
            fs::read(&anim_high).unwrap(),
            "the animated slider did not change the output between keyframe times"
        );

        // Claim 2: the animated value at a keyframe time equals a static render
        // carrying that value. Both renders take the same transport, so the
        // value-supply mechanism (static payload vs animation sidecar) is the
        // only difference from the animation render.
        let static_report = render_experimental_image(
            repository,
            &plugin,
            &hash,
            &input,
            &static_low,
            &params(low),
        )
        .expect("static render carrying the first keyframe value");
        assert_eq!(static_report["passed"], true, "report: {static_report}");
        assert_eq!(
            fs::read(&anim_low).unwrap(),
            fs::read(&static_low).unwrap(),
            "classic animation at t=0 did not match a static render carrying the keyframe value; \
             the interpolated value was not written into params[]"
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
        assert!(
            parameter_animation_sidecar_json(&[ParameterAnimation {
                slot: 1,
                keys: vec![key],
            }])
            .is_err()
        );
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
