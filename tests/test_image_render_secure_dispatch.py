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
    # The GPU initial dispatch is the only one that binds the policy args; a
    # captured manifest failure degrades it to the policy-free snapshots before
    # the dispatch is built, so the GPU trailer never reaches a CPU command.
    assert body.count("&args_after_plugin") == 1
    assert "args_after_plugin: if manifest_fallback_error.is_some() {" in body
    # Both Auto CPU retries (the caught preflight-error fallback and the
    # worker-reported GPU failure retry) dispatch the policy-free snapshots, so
    # neither carries the runtime-module authorization trailer or its manifest
    # dependency into a CPU render that loads no GPU DLL.
    assert body.count("args_after_plugin: &cpu_fallback_args_after_plugin") == 2
    assert body.count("dependencies: cpu_fallback_dependencies.clone()") == 2
    # The snapshots must be taken before the trailer is appended, or they would
    # capture the GPU authorization they exist to exclude.
    trailer = 'args_after_plugin.push("--runtime-module-authorization-v1".to_owned());'
    assert body.count(trailer) == 1
    assert (
        body.index("let cpu_fallback_args_after_plugin = args_after_plugin.clone();")
        < body.index(trailer)
    )
    assert (
        body.index("let cpu_fallback_dependencies = dependencies.clone();")
        < body.index(trailer)
    )
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


def test_gpu_render_manifests_carry_the_preflight_session_identity():
    """A render manifest must embed the identity its report was authenticated
    against. Minting a fresh identity per render lets a prepared report authorize
    a manifest from another session, defeating the anti-replay binding."""
    source = SOURCE.read_text(encoding="utf-8")
    session = (SOURCE.parent / "render_session.rs").read_text(encoding="utf-8")

    assert (
        "pub(crate) fn prepare_runtime_authorization_transport_with_identity(" in source
    )
    assert "runtime module session identity must be nonzero" in source

    # Both render paths encode the manifest with the authenticated identity, and
    # neither reaches for the identity-minting constructor.
    render = render_function()
    for body in (render, session):
        assert "prepare_runtime_authorization_transport_with_identity(" in body
        assert "policy_input.session_identity," in body
        assert "prepare_runtime_authorization_transport(" not in body

    # The minting constructor stays reserved for the preflight (which originates
    # the session identity) and the params-inspect path (which ignores it).
    assert source.count("prepare_runtime_authorization_transport(repository") == 2


def test_gpu_preflight_seals_the_same_dependencies_as_the_render():
    """The preflight loads the staged plug-in natively, so a plug-in importing an
    approved helper DLL only resolves if the preflight seals the render's
    dependency artifacts alongside the authorization manifest."""
    source = SOURCE.read_text(encoding="utf-8")
    start = source.index("pub fn prepare_gpu_runtime_policy(")
    body = source[start : source.index("\n}\n", start) + 2]

    assert "dependencies: Vec<ApprovedImageArtifact>," in body
    assert "let mut preflight_dependencies = dependencies;" in body
    assert "preflight_dependencies.push(authorization.artifact.clone());" in body
    assert "preflight_dependencies," in body
    # The manifest must no longer be the only sealed artifact.
    assert "vec![authorization.artifact.clone()]" not in body


def test_smart_sessions_carry_static_context_trailers():
    """A host context must not force a render onto the one-shot transport (#331).

    The broker builds the mask/spatial/render trailers from `host_context` and pushes
    them onto the session's positional tail; the worker's smart session command has to
    peel them in the same order the classic session command does, or the ten-slot
    session contract does not resolve and the command is rejected outright.
    """
    source = SOURCE.read_text(encoding="utf-8")
    session = (SOURCE.parent / "render_session.rs").read_text(encoding="utf-8")
    dispatch = (
        ROOT / "minihost" / "src" / "l2_cli_dispatch.cpp"
    ).read_text(encoding="utf-8")

    # The gate no longer excludes a static host context.
    gate = render_function()[
        render_function().index("let session_eligible ="): render_function().index(
            "if session_eligible"
        )
    ]
    assert "host_context.is_none()" not in gate
    # The session open no longer refuses smart requests that carry the trailers.
    assert "smart sessions do not carry static context trailers yet" not in session
    # The broker still pushes all three, in the one-shot order.
    push = session[session.index("if let Some(mask) = &request.mask_trailer"):]
    assert push.index("request.mask_trailer") < push.index("request.spatial_trailer")
    assert push.index("request.spatial_trailer") < push.index(
        "request.render_environment_trailer"
    )

    # Both session commands peel the three trailers ahead of the layer trailer, so
    # the ten-slot core lands at the same place on either route. `>= 11` is the
    # session arity guard (10 slots + the trailer under test) and distinguishes
    # these from the one-shot peels, which share the expressions but guard on
    # `>= 14`. Asserted per branch and in order: counting file-wide would pass a
    # smart branch that peeled mask before render, which shifts where the core
    # lands, or one that re-based image_argc on effective_argc ahead of the
    # (shared) layer line -- both re-break #331 while keeping every count at 2.
    peels = [
        "mode.image_render_environment = ",
        "mode.image_spatial_context = ",
        "mode.image_mask_context = ",
        "mode.session_layers = ",
        "const int session_core_argc = ",
    ]
    branches = {
        "classic": 'if (equals(command, L"--render-session-v1") || session16 || session32) {',
        "smart": 'if (equals(command, L"--smart-session-v1") || session16 || session32) {',
    }
    for name, opener in branches.items():
        start = dispatch.index(opener)
        branch = dispatch[start : dispatch.index("return WorkerMode{};", start)]
        at = -1
        for peel in peels:
            found = branch.find(peel, at + 1)
            assert found > at, f"{name}: {peel} missing or out of order"
            at = found
        # The chain must thread through the *_argc fields, never restart from
        # effective_argc after the first peel.
        assert "mode.image_argc = mode.image_trailer_argc -" in branch, name
        assert "mode.image_argc = effective_argc" not in branch, name
        assert (
            "const int session_core_argc = mode.image_argc - (mode.session_layers ? 1 : 0);"
            in branch
        ), name


def test_a_policy_below_float32_or_on_classic_does_not_exclude_a_render():
    """An inert runtime module policy must not exclude an eligible session.

    The one-shot's gpu_initial_attempt requires float32, so it never reads the policy
    for Argb8/Argb16 and renders through --smart-image/--smart-image16. The session's
    gpu_capable is false for the same reason, so it neither folds the backend nor
    attaches the authorization manifest. Classic Auto sessions likewise never attempt
    GPU, so carrying a policy must not make RenderSession::open reject a shape that
    both routes handle identically (#337, #340).
    """

    def flat(text):
        # rustfmt decides where these conditions wrap, so compare on collapsed
        # whitespace rather than pinning a particular line break.
        return " ".join(text.split())

    body = render_function()
    gate = flat(body[body.index("let session_eligible ="): body.index("if session_eligible")])

    # Non-layered: Auto is admitted when there is no policy OR the depth makes one
    # inert. The float32 GPU arm still requires an authenticated policy.
    assert (
        "(gpu_backend == RenderGpuBackend::Auto && (gpu_runtime_policy.is_none() "
        "|| pixel_format != RenderPixelFormat::Argb32f))" in gate
    )
    assert (
        "(pixel_format == RenderPixelFormat::Argb32f && runtime_backend(gpu_backend).is_some() "
        "&& gpu_runtime_policy.is_some())" in gate
    )
    # Layered (issue #353): Argb32f is admitted under Auto *without* a policy --
    # that was the last real one-shot gap, and the A/B in
    # render_session_wrapper.rs::smart_timed_multilayer_matches_the_one_shot_transport
    # measures the equivalence. With a policy it stays excluded; see below.
    layered_start = gate.index("Smart layered: Auto only, at every depth")
    layered = gate[layered_start : gate.index("} else {", layered_start)]
    assert "gpu_backend == RenderGpuBackend::Auto" in layered
    # Layered float32 + policy must stay EXCLUDED. Dropping the depth bound
    # removed the clause that had been keeping it out, and for that shape the
    # polarity of the divergence flips: the session opens the real GPU session
    # (its CPU fold only fires when the policy is absent) while the one-shot
    # skips its GPU preflight for layered renders and stays on the CPU. That is
    # a different divergence from the accepted one and has no A/B behind it.
    assert (
        "gpu_backend == RenderGpuBackend::Auto && (pixel_format "
        "!= RenderPixelFormat::Argb32f || gpu_runtime_policy.is_none())" in flat(layered)
    ), "layered float32 + policy must be excluded, not merely unadmitted"

    # Classic Auto is already admitted by the image-render gate. RenderSession::open
    # must accept the same policy-carrying request and only attach the manifest when
    # it actually attempts GPU -- both are what make the policy inert.
    session = (SOURCE.parent / "render_session.rs").read_text(encoding="utf-8")
    assert "if !request.smart && request.gpu_runtime_policy.is_some()" not in session
    assert (
        "let gpu_capable = request.smart && request.pixel_format == RenderPixelFormat::Argb32f"
        in session
    )


def test_no_corpus_fixture_is_both_layered_and_gpu_declaring():
    """The layered gate's Argb32f admission rests on this, so pin it.

    The worker turns gpu_negotiation on for float32 whenever the plug-in
    advertises GPU support -- layers do not enter into it
    (worker_smart_setup.cpp reads bit 25 of out_flags2). A plug-in that is both
    layered and GPU-declaring would therefore take the device on the one-shot
    layered command while a policy-less Auto session folds to CPU, which is the
    one divergence issue #353 accepts as session-canonical.

    "Layered" is really a property of the *request* (the broker builds
    secondaries from caller-supplied parameters with a layer_path, and never
    cross-checks them against the plug-in's parameter table). The harness ties
    the two together -- assign_layer_paths refuses a slot the inspection did not
    report as a layer -- so scanning sources is a proxy for "a shipped caller can
    produce a layered request against this plug-in", not the invariant itself.

    The detector matches bit 25 the way the worker does, by value as well as by
    macro name, so a probe spelling it numerically cannot slip past. It is
    checked against a known-positive spelling first, because no fixture in
    instruments/ sets the bit at all and a silently non-matching detector would
    make the whole test vacuous.
    """
    root = SOURCE.parents[4]
    assert (root / "instruments").is_dir() and (root / "broker").is_dir(), (
        f"expected {root} to be the repository root")

    def declares_gpu(text: str) -> bool:
        # PF_OutFlag2_SUPPORTS_GPU_RENDER_F32 == 1 << 25 == 0x2000000 == 33554432.
        return any(marker in text for marker in (
            "PF_OutFlag2_SUPPORTS_GPU_RENDER_F32",
            "1 << 25", "1L << 25", "1u << 25", "1UL << 25",
            "0x2000000", "0x02000000", "33554432",
        ))

    assert declares_gpu("out_flags2 |= PF_OutFlag2_SUPPORTS_GPU_RENDER_F32;")
    assert declares_gpu("out_flags2 |= (1L << 25);")
    assert not declares_gpu("out_flags2 |= PF_OutFlag2_SUPPORTS_SMART_RENDER;")

    layered, gpu_declaring = set(), set()
    for source in sorted((root / "instruments").rglob("*.cpp")):
        text = source.read_text(encoding="utf-8")
        if "PF_ADD_LAYER" in text or "PF_Param_LAYER" in text:
            layered.add(source.name)
        if declares_gpu(text):
            gpu_declaring.add(source.name)
    assert layered, "expected the layered probes to still exist"
    both = layered & gpu_declaring
    assert not both, (
        "these fixtures are both layered and GPU-declaring, so the one-shot "
        "would negotiate GPU where the session folds to CPU (issue #353): "
        f"{sorted(both)}")
