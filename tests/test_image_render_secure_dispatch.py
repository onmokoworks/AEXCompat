from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
SESSION = SOURCE.parent / "render_session.rs"


def render_function() -> str:
    source = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    start = source.index("fn render_with_artifact(")
    return source[start:source.index("\n#[cfg(test)]", start + 1)]


def session_open() -> str:
    """`RenderSession::open`, which is where the render launch is now built.

    #365 deleted the one-shot argv transport, so the plug-in identity, the GPU
    policy binding, and the launch argv that `render_with_artifact` used to
    assemble all live here. The slice ends at the next `pub fn` so it covers the
    whole of `open` and nothing after it.
    """
    session = source_owners.RENDER_SESSION_SOURCE.read_text(encoding="utf-8")
    start = session.index(
        "    pub fn open(request: SessionOpenRequest<'_>) -> io::Result<RenderSession> {"
    )
    return session[start:session.index("\n    pub fn ", start + 1)]




def test_plugin_identity_is_strictly_decoded_and_size_bound():
    source = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    session = source_owners.RENDER_SESSION_SOURCE.read_text(encoding="utf-8")
    assert "value.len() != 64" in source
    assert "byte.is_ascii_hexdigit()" in source
    # Both session launches (image and audio) pin the plug-in by decoded digest
    # and on-disk size. This used to be asserted on render_with_artifact's
    # one-shot dispatch, which #365 deleted.
    assert session.count("expected_sha256: decode_sha256_hex(request.plugin_sha256)?") == 2
    assert session.count("expected_size: fs::metadata(request.plugin_path)?.len()") == 2




def test_a_gpu_session_requires_a_policy_and_never_retries_on_cpu():
    """The session binds the GPU dispatch to an authenticated policy at open.

    The deleted one-shot ran a GPU preflight and, on Auto, retried on CPU while
    recording gpu_attempt/gpu_fallback_used. A session cannot retry mid-flight,
    so that collapses to open time: Auto without a policy IS a CPU session, Auto
    with a policy is a GPU session with no retry, and an explicit GPU backend
    without a policy fails closed before any transport work.
    """
    source = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    body = session_open()

    assert "pub struct GpuRuntimePolicyInput<'a>" in source
    assert (
        "render_experimental_image_with_approved_dependencies_and_gpu_runtime_policy"
        in source
    )
    # gpu_capable is a depth/smart property; the Auto fold and the fail-closed
    # check both hang off it.
    assert (
        "let gpu_capable = request.smart && request.pixel_format == RenderPixelFormat::Argb32f"
        in body
    )
    assert "&& request.gpu_backend == RenderGpuBackend::Auto" in body
    assert "&& request.gpu_runtime_policy.is_none()" in body
    assert "RenderGpuBackend::Cpu" in body
    assert "let gpu_attempt = gpu_capable && runtime_backend(effective_backend).is_some();" in body
    assert "if gpu_attempt && request.gpu_runtime_policy.is_none() {" in body
    assert (
        "GPU render requires a session-bound authenticated runtime module policy report"
        in body
    )
    # The authorization manifest is attached only when GPU is actually attempted,
    # which is what keeps a policy inert below float32 and on classic.
    assert "let _runtime_authorization = if gpu_attempt {" in body
    # No CPU retry exists to carry the GPU trailer into.
    assert "gpu_fallback_used" not in body
    assert "cpu_fallback_args_after_plugin" not in body


def test_gpu_backend_mapping_is_explicit_and_auto_preflights_as_cuda():
    source = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    start = source.index("fn runtime_backend(")
    end = source.index("\n}\n", start) + 2
    mapping = source[start:end]
    assert "RenderGpuBackend::Auto | RenderGpuBackend::Cuda" in mapping
    assert "Some(RuntimeBackend::Cuda)" in mapping
    assert "RenderGpuBackend::OpenCl => Some(RuntimeBackend::Opencl)" in mapping
    assert "RenderGpuBackend::DirectX => Some(RuntimeBackend::Directx)" in mapping
    assert "RenderGpuBackend::Cpu => None" in mapping


def test_gpu_policy_render_routes_through_the_session_and_a_preflight_producer():
    source = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    # The wrapper carries the policy into the session instead of hard-coding None.
    assert "gpu_runtime_policy: Option<GpuRuntimePolicyInput<'a>>," in source
    assert "gpu_runtime_policy: request.gpu_runtime_policy," in source
    # The policy input is produced by a GPU module-audit preflight worker run that
    # emits the classified module report the render re-authenticates.
    assert "pub fn prepare_gpu_runtime_policy(" in source
    assert '"--gpu-module-report-v1"' in source
    assert "authenticate_gpu_worker_report(" in source


def test_all_other_image_routes_use_secure_dispatch_without_isolated_fallback():
    source = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    assert "windows_process::run_isolated" not in source
    assert "run_isolated(" not in source
    # 32, not 33: the one-shot `--render-audio` dispatch went with #365. The rest
    # are the diagnostic request routes, none of which renders an image.
    assert source.count("dispatch_approved_image(") == 32
    assert source.count("WorkerKind::L2,") == 24
    assert source.count("WorkerKind::Render,") == 6
    # Two smart request routes plus the GPU module-audit preflight (#290), which
    # dispatches the smart worker to emit the classified module report the GPU
    # policy producer authenticates.
    assert source.count("WorkerKind::Smart,") == 3


def test_shared_dispatch_preserves_cli_order_and_empty_dependency_approval():
    source = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    start = source.index("fn dispatch_approved_image(")
    end = source.index("\npub(crate) fn decode_sha256_hex", start)
    helper = source[start:end]
    assert "expected_sha256: decode_sha256_hex(approved_sha256)?" in helper
    assert "expected_size: fs::metadata(plugin_path)?.len()" in helper
    assert "dependencies: vec![]" in helper
    assert helper.index("args_before_plugin,") < helper.index("args_after_plugin,")


def test_gpu_render_manifests_carry_the_preflight_session_identity():
    """A render manifest must embed the identity its report was authenticated
    against. Minting a fresh identity per render lets a prepared report authorize
    a manifest from another session, defeating the anti-replay binding."""
    source = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    session = source_owners.RENDER_SESSION_SOURCE.read_text(encoding="utf-8")

    assert (
        "pub(crate) fn prepare_runtime_authorization_transport_with_identity(" in source
    )
    assert "runtime module session identity must be nonzero" in source

    # The session is the only render path since #365, and it encodes the
    # manifest with the authenticated identity rather than minting one.
    assert "prepare_runtime_authorization_transport_with_identity(" in session
    assert "policy_input.session_identity," in session
    assert "prepare_runtime_authorization_transport(" not in session
    assert "prepare_runtime_authorization_transport_with_identity(" not in render_function()

    # The minting constructor stays reserved for the preflight (which originates
    # the session identity) and the params-inspect path (which ignores it).
    assert source.count("prepare_runtime_authorization_transport(repository") == 2


def test_gpu_preflight_seals_the_same_dependencies_as_the_render():
    """The preflight loads the staged plug-in natively, so a plug-in importing an
    approved helper DLL only resolves if the preflight seals the render's
    dependency artifacts alongside the authorization manifest."""
    source = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    start = source.index("pub fn prepare_gpu_runtime_policy(")
    body = source[start : source.index("\n}\n", start) + 2]

    assert "dependencies: Vec<ApprovedImageArtifact>," in body
    assert "let mut preflight_dependencies = dependencies;" in body
    assert "preflight_dependencies.push(authorization.artifact.clone());" in body
    assert "preflight_dependencies," in body
    # The manifest must no longer be the only sealed artifact.
    assert "vec![authorization.artifact.clone()]" not in body




def test_a_policy_below_float32_or_on_classic_does_not_exclude_a_render():
    """An inert runtime module policy must not make `open` reject a render.

    `gpu_capable` is false for Argb8/Argb16 and for classic at every depth, so
    those shapes neither fold the backend nor attach the authorization manifest
    (#337, #340). Carrying a policy is therefore a no-op for them, and `open`
    must not turn it into a rejection. This used to be asserted against the
    session-eligibility gate as well; #365 deleted the gate, so `open` is the
    only place the claim can be broken now.
    """
    session = source_owners.RENDER_SESSION_SOURCE.read_text(encoding="utf-8")
    assert "if !request.smart && request.gpu_runtime_policy.is_some()" not in session
    assert (
        "let gpu_capable = request.smart && request.pixel_format == RenderPixelFormat::Argb32f"
        in session
    )
    # Nothing else may reject on the mere presence of a policy.
    body = session_open()
    assert "request.gpu_runtime_policy.is_some()" not in body, (
        "open must gate on gpu_attempt, not on a policy being present")
