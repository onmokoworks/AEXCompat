from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_windows_clean_clone_runs_canonical_source_reproducible_gates():
    workflow = (ROOT / ".github/workflows/windows-clean-clone.yml").read_text(
        encoding="utf-8"
    )
    assert "runs-on: windows-latest" in workflow
    assert "components: rustfmt" in workflow
    assert "BASE_SHA: ${{ github.event.pull_request.base.sha || github.event.before }}" in workflow
    assert "git diff --name-only --diff-filter=ACMR $base $env:GITHUB_SHA -- '*.rs'" in workflow
    assert "rustfmt --edition 2024 --check --config skip_children=true @files" in workflow
    assert "cargo fmt --all --check" not in workflow
    assert "cargo check --workspace --locked" in workflow
    assert (
        "cargo check --manifest-path bridges/aviutl2/Cargo.toml --all-targets --locked"
        in workflow
    )
    assert (
        "cargo check --manifest-path bridges/aviutl2-multifilter/Cargo.toml --all-targets --locked"
        in workflow
    )
    # --show-output is load-bearing: libtest captures the output of passing
    # tests, and a test that skips itself passes. Without it a run where the
    # probe suppressed every worker-launching test looks exactly like full
    # coverage (issue #335; the restricted token it named is gone since #731).
    assert "cargo test --workspace --locked -- --show-output" in workflow
    assert "Report sealed-worker skips" in workflow
    assert "cannot launch a sealed worker" in workflow
    assert "no sealed-worker skips" in workflow, (
        "the report must state the no-skip case explicitly, not by staying silent")
    # A hosted runner may be unable to launch a sealed worker at all, so the
    # tests that drive one detect that themselves and report a skip (issue #335).
    # The workflow must not carry a hand-maintained --skip list again: that list
    # did not follow the tests added after it was written, and main went red with
    # 32 failures nobody had opted out of.
    directives = "\n".join(
        line for line in workflow.splitlines() if not line.lstrip().startswith("#"))
    assert "--skip " not in directives
    assert not (ROOT / "broker/crates/broker/tests/secure_launch.rs").exists(), (
        "the sealed-launch integration suite was retired with issue #816"
    )
    assert "uv sync --locked" in workflow
    assert "uv run python -m pytest --collect-only -q --validate-local-artifact-manifest" in workflow
    assert "uv run python -m pytest -q" in workflow
    assert "--run-local-artifact-tests" not in workflow


def test_pre_launch_rejection_tests_keep_running_on_a_launch_limited_host():
    """Rejection paths that never start a process must not carry the #335 guard.

    The guard exists because a hosted runner may be unable to
    initialize a worker. A test that asserts the launch is *refused* before any
    process is created is unaffected by that, so guarding it would delete real
    CI coverage of the refusal. `RenderSession::open` performs every argument
    validation before it reaches the launch, and `secure_launch` fails trusted
    worker staging before it spawns.
    """
    tests_dir = ROOT / "broker/crates/broker/tests"
    unguarded = {
        "render_session.rs": (
            "smart_session_with_an_explicit_gpu_backend_requires_a_policy",
            "open_rejects_two_timed_layers_at_the_same_slot_and_time",
            "open_rejects_two_static_layers_at_the_same_slot",
            "open_rejects_an_out_of_range_alpha_as_coverage_slot",
            "open_rejects_layer_pixels_that_do_not_match_dimensions",
            "open_rejects_a_zero_layer_slot",
            "open_rejects_a_non_empty_world_dump_directory",
            "open_rejects_a_world_dump_directory_outside_the_target_tree",
            "open_rejects_animation_bound_to_an_unknown_slot",
        ),
        # The forced-fallback tests return before RenderSession::open /
        # AudioRenderSession::open, so they never create a process either.
        "render_session_wrapper.rs": (
            "session_infra_failure_fails_closed_without_silent_one_shot",
            "audio_session_failure_fails_closed_without_silent_one_shot",
        ),
    }
    for filename, names in unguarded.items():
        source = (tests_dir / filename).read_text(encoding="utf-8")
        for name in names:
            anchor = source.index(f"fn {name}(")
            head = source[anchor:anchor + 400]
            assert "skip_without_sealed_worker_launch" not in head, (
                f"{filename}::{name} rejects before any process starts; guarding it "
                "removes CI coverage of the refusal")


def test_ae_sdk_workflow_runs_native_parameter_animation_coverage_after_clean_build():
    """#356 must not be reported green because its native fixtures were absent."""
    workflow = (ROOT / ".github/workflows/ae-sdk-tests.yml").read_text(
        encoding="utf-8"
    )
    assert "github.event.repository.private" in workflow
    assert "cmake -S minihost -B target\\minihost-build -G Ninja" in workflow
    assert "build-pf-layer-param-probe" in workflow
    assert "build-pf-param-utils-animation-probe" in workflow
    assert (
        "cargo test -p aexcompat-broker --test parameter_animation --locked -- --show-output"
        in workflow
    )
    assert "target\\minihost-build\\aex_render_worker.exe" in workflow
    assert (
        "target\\pf-layer-param-probe-build\\Release\\pf_layer_param_probe.aex"
        in workflow
    )
    assert (
        "target\\pf-param-utils-animation-probe-build\\Release\\pf_param_utils_animation_probe.aex"
        in workflow
    )
    assert "coverage_source_commit=" in workflow
    assert "Report parameter-animation sealed-worker skips" in workflow
    assert "cannot launch a sealed worker" in workflow


def test_the_sealed_worker_skip_is_opt_in_and_only_ci_opts_in():
    """The skip must never be reachable without an explicit opt-in.

    0xC0000142 is what a hosted runner produces, and it is equally what a
    regression in the launch path itself would produce -- break the staged
    worker copy or its environment and the worker stops loading its system DLLs
    everywhere. The probe cannot tell those apart, so the opt-in is what keeps a
    developer machine failing loudly instead of skipping 56 tests (issue #335).
    The env var still carries the old restricted-token name; the token itself
    went away in #731 and renaming it is left to the CI work in flight.
    """
    probe = (ROOT / "broker/crates/broker/tests/common/mod.rs").read_text(encoding="utf-8")
    assert 'const ALLOW_SKIP_ENV: &str = "AEXCOMPAT_ALLOW_RESTRICTED_TOKEN_SKIP"' in probe
    # The check must gate the probe itself, not merely exist somewhere.
    body = probe[probe.index("fn probe() -> bool {"):]
    gate = body.index("ALLOW_SKIP_ENV")
    suppress = body.index("STATUS_DLL_INIT_FAILED")
    assert gate < suppress, "the opt-in must be consulted before the skip decision"

    video_batch = (ROOT / "tests/test_render_video_batch_cli.py").read_text(encoding="utf-8")
    assert 'os.environ.get("AEXCOMPAT_ALLOW_RESTRICTED_TOKEN_SKIP") == "1"' in video_batch

    # Exactly the two CI workflows opt in; nothing else may.
    opted_in = set()
    for workflow in sorted((ROOT / ".github/workflows").glob("*.yml")):
        text = "\n".join(
            line for line in workflow.read_text(encoding="utf-8").splitlines()
            if not line.lstrip().startswith("#"))
        if 'AEXCOMPAT_ALLOW_RESTRICTED_TOKEN_SKIP: "1"' in text:
            opted_in.add(workflow.name)
    assert opted_in == {"windows-clean-clone.yml", "ae-sdk-tests.yml"}, opted_in
