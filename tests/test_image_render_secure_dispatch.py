from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"


def render_function() -> str:
    source = SOURCE.read_text(encoding="utf-8")
    start = source.index("fn render_with_artifact(")
    return source[start:source.index("\n#[cfg(test)]", start + 1)]


def test_render_workers_admit_the_local_build_through_dispatch_only():
    source = SOURCE.read_text(encoding="utf-8")
    dispatch = (SOURCE.parent / "secure_image_dispatch.rs").read_text(encoding="utf-8")

    # Frozen worker trust constants are retired; render/smart/L2 image dispatch
    # admits the locally built worker at dispatch time instead.
    assert "WORKER_TRUST" not in source
    assert 'include!("generated_' not in source
    for name in (
        "generated_l2_worker_trust.rs",
        "generated_render_worker_trust.rs",
        "generated_smart_worker_trust.rs",
    ):
        assert not (SOURCE.parent / name).exists()

    # Admission happens exactly once per launch, inside dispatch_secure_image
    # and its render-session variant, and the staged copy must still match the
    # admitted bytes before launch.
    assert dispatch.count("admit_local_worker(") == 3  # definition + one-shot + session
    assert "pub fn dispatch_secure_image_session(" in dispatch
    assert "local worker binary is missing or unreadable" in dispatch
    assert "local worker binary is empty" in dispatch
    assert "Sha256::digest(fs::read(&worker" not in render_function()


def test_plugin_identity_is_strictly_decoded_and_size_bound():
    source = SOURCE.read_text(encoding="utf-8")
    body = render_function()
    assert "value.len() != 64" in source
    assert "byte.is_ascii_hexdigit()" in source
    assert "expected_sha256: decode_sha256_hex(plugin_sha256)?" in body
    assert "expected_size: fs::metadata(plugin_path)?.len()" in body


def test_gpu_initial_dispatch_is_policy_bound_and_cpu_retry_remains_policy_free():
    body = render_function()
    assert "dispatch_secure_gpu_image(" in body
    assert "authenticate_gpu_worker_report(" in body
    assert "dispatch_secure_image(initial_dispatch)?" in body
    # Auto SmartFX may use one secure CPU dispatch for a GPU preflight
    # fallback and another for a worker-reported GPU failure retry.
    assert body.count("dispatch_secure_image(SecureImageDispatch") == 2
    assert "args_before_plugin[0] = image_worker_command(" in body
    assert "RenderGpuBackend::Cpu," in body
    assert "run_isolated" not in body
    assert "args_before_plugin: &args_before_plugin" in body
    assert "args_after_plugin: &args_after_plugin" in body
    assert "let mut args_before_plugin = vec![command.into()]" in body
    assert "let mut args_after_plugin = vec![\n        plugin_sha256.to_ascii_lowercase()" in body


def test_gpu_policy_is_required_only_for_an_actual_gpu_initial_attempt():
    source = SOURCE.read_text(encoding="utf-8")
    body = render_function()
    assert "pub struct GpuRuntimePolicyInput<'a>" in source
    assert (
        "render_experimental_image_with_approved_dependencies_and_gpu_runtime_policy"
        in source
    )
    assert "let gpu_initial_attempt = smart" in body
    assert "pixel_format == RenderPixelFormat::Argb32f" in body
    assert "secondaries.is_empty()" in body
    assert "timed_secondaries.is_empty()" in body
    assert "audio.is_none()" in body
    assert "runtime_backend(gpu_backend).is_some()" in body
    assert "GPU render requires a session-bound authenticated runtime module policy report" in body
    fallback = body[body.index("let worker_report = if gpu_attempt_failed") :]
    assert "dispatch_secure_gpu_image(" not in fallback
    assert "authenticate_gpu_worker_report(" not in fallback


def test_gpu_backend_mapping_is_explicit_and_auto_preflights_as_cuda():
    source = SOURCE.read_text(encoding="utf-8")
    start = source.index("fn runtime_backend(")
    end = source.index("\n}\n", start) + 2
    mapping = source[start:end]
    assert "RenderGpuBackend::Auto | RenderGpuBackend::Cuda" in mapping
    assert "Some(RuntimeBackend::Cuda)" in mapping
    assert "RenderGpuBackend::OpenCl => Some(RuntimeBackend::Opencl)" in mapping
    assert "RenderGpuBackend::DirectX => Some(RuntimeBackend::Directx)" in mapping
    assert "RenderGpuBackend::Cpu => None" in mapping


def test_gpu_policy_render_routes_through_the_session_and_a_preflight_producer():
    source = SOURCE.read_text(encoding="utf-8")
    body = render_function()
    gate = body[body.index("let session_eligible ="):body.index("if session_eligible")]
    # A GPU single-image render (Argb32f + a GPU backend) with an authenticated
    # runtime-module policy is session-eligible (#290); policy-less GPU stays out.
    assert "runtime_backend(gpu_backend).is_some()" in gate
    assert "gpu_runtime_policy.is_some()" in gate
    assert "gpu_runtime_policy.is_none()" in gate
    # The wrapper carries the policy into the session instead of hard-coding None.
    assert "gpu_runtime_policy: Option<GpuRuntimePolicyInput<'a>>," in source
    assert "gpu_runtime_policy: request.gpu_runtime_policy," in source
    # The policy input is produced by a GPU module-audit preflight worker run that
    # emits the classified module report the render re-authenticates.
    assert "pub fn prepare_gpu_runtime_policy(" in source
    assert '"--gpu-module-report-v1"' in source
    assert "authenticate_gpu_worker_report(" in source


def test_all_other_image_routes_use_secure_dispatch_without_isolated_fallback():
    source = SOURCE.read_text(encoding="utf-8")
    assert "windows_process::run_isolated" not in source
    assert "run_isolated(" not in source
    assert source.count("dispatch_approved_image(") == 33
    assert source.count("WorkerKind::L2,") == 24
    assert source.count("WorkerKind::Render,") == 7
    # Two smart render routes plus the GPU module-audit preflight (#290), which
    # dispatches the smart worker to emit the classified module report the GPU
    # policy producer authenticates.
    assert source.count("WorkerKind::Smart,") == 3


def test_shared_dispatch_preserves_cli_order_and_empty_dependency_approval():
    source = SOURCE.read_text(encoding="utf-8")
    start = source.index("fn dispatch_approved_image(")
    end = source.index("\npub(crate) fn decode_sha256_hex", start)
    helper = source[start:end]
    assert "expected_sha256: decode_sha256_hex(approved_sha256)?" in helper
    assert "expected_size: fs::metadata(plugin_path)?.len()" in helper
    assert "dependencies: vec![]" in helper
    assert helper.index("args_before_plugin,") < helper.index("args_after_plugin,")
    assert 'let args_before_plugin = vec!["--render-audio".into()]' in source
    assert "let args_after_plugin = vec![\n        actual.to_ascii_lowercase()," in source
