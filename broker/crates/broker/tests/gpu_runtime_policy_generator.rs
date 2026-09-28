#![cfg(windows)]

use aexcompat_broker::gpu_runtime_policy_generator::{
    generate_opencl_runtime_policy, privacy_bounded_gpu_policy_report,
};
use aexcompat_broker::image_render::{
    RenderGpuBackend, RenderTiming, prepare_gpu_runtime_policy,
    render_experimental_image_with_auto_opencl_policy,
};
use aexcompat_broker::runtime_module_policy::RuntimeBackend;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

#[test]
#[ignore = "requires exactly one active GPU with a catalog-bound native OpenCL ICD"]
fn real_opencl_policy_is_short_lived_and_catalog_bound() {
    let before = SystemTime::now();
    let policy = generate_opencl_runtime_policy().expect("active OpenCL policy");
    let platform = policy.platform().expect("v2 policy platform");
    assert_eq!(platform.backend, RuntimeBackend::Opencl);
    assert_eq!(policy.modules().len(), 1);
    assert_eq!(policy.modules()[0].backend, RuntimeBackend::Opencl);
    assert!(policy.expires() > before);
    assert!(policy.expires() <= SystemTime::now() + Duration::from_secs(120));
    let shareable = privacy_bounded_gpu_policy_report(&policy).to_string();
    for module in policy.modules() {
        assert!(!shareable.contains(&module.path.to_string_lossy().to_string()));
        assert!(shareable.contains(&module.basename));
    }
}

#[test]
#[ignore = "requires AEXCOMPAT_GPU_PREFLIGHT_AEX and a current Release aex_worker"]
fn real_opencl_policy_preflight_authenticates_loaded_modules() {
    let plugin = std::env::var_os("AEXCOMPAT_GPU_PREFLIGHT_AEX")
        .expect("set AEXCOMPAT_GPU_PREFLIGHT_AEX to one installed AEX");
    let plugin = Path::new(&plugin);
    let digest = format!("{:x}", Sha256::digest(fs::read(plugin).unwrap()));
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root");
    let policy = generate_opencl_runtime_policy().expect("active OpenCL policy");
    let prepared = prepare_gpu_runtime_policy(
        repository,
        plugin,
        &digest,
        RenderGpuBackend::OpenCl,
        policy,
        Vec::new(),
    )
    .expect("OpenCL preflight");
    let report: serde_json::Value = serde_json::from_str(prepared.report_json()).unwrap();
    assert!(
        report["modules"]
            .as_array()
            .is_some_and(|modules| !modules.is_empty())
    );
}

#[test]
#[ignore = "requires AEXCOMPAT_GPU_RENDER_AEX=SDK_Invert_ProcAmp_OpenCL.aex and Release aex_worker"]
fn real_opencl_policy_renders_one_float_frame() {
    let plugin = std::env::var_os("AEXCOMPAT_GPU_RENDER_AEX")
        .expect("set AEXCOMPAT_GPU_RENDER_AEX to the OpenCL SDK fixture");
    let plugin = Path::new(&plugin);
    let digest = format!("{:x}", Sha256::digest(fs::read(plugin).unwrap()));
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root");
    let scratch = std::env::temp_dir().join(format!(
        "aexcompat-opencl-policy-{}-{:032x}",
        std::process::id(),
        rand::random::<u128>()
    ));
    fs::create_dir(&scratch).unwrap();
    let input = scratch.join("input.png");
    let output = scratch.join("output.png");
    image::RgbaImage::from_fn(32, 16, |x, y| {
        image::Rgba([(x * 5) as u8, (y * 9) as u8, 64, 255])
    })
    .save(&input)
    .unwrap();
    let result = render_experimental_image_with_auto_opencl_policy(
        repository,
        plugin,
        &digest,
        &input,
        &output,
        &[],
        RenderTiming::default(),
        None,
        None,
        Vec::new(),
        Vec::new(),
    );
    let report = result.expect("OpenCL frame render");
    let decoded = image::open(&output).expect("decode OpenCL PNG");
    assert_eq!((decoded.width(), decoded.height()), (32, 16));
    assert_eq!(report["render_path"], "smartfx");
    assert_eq!(report["worker_classification"], "ok");
    assert_eq!(report["gpu_render_dispatched"], true);
    assert_eq!(report["opencl_context_used"], true);
    assert_ne!(decoded.to_rgba8().get_pixel(0, 0).0, [0, 0, 64, 255]);
    fs::remove_dir_all(&scratch).unwrap();
}
