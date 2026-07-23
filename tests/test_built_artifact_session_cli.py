from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HARNESS_SOURCE = ROOT / "broker" / "crates" / "harness" / "src" / "main.rs"


def test_built_artifact_probes_have_a_supported_session_only_entrypoint():
    source = HARNESS_SOURCE.read_text(encoding="utf-8")
    assert 'session_command == Some("--render-experimental-session")' in source
    assert 'session_command == Some("--render-experimental-session-param")' in source
    assert "parameter.value = value" in source
    assert "render_experimental_image_at_time_with_format" in source
    assert "the deleted one-shot image argv" in source

    migrated = (
        "test_pf_aegp_async_layer_receipt_probe.py",
        "test_pf_aegp_external_cache_roundtrip_probe.py",
        "test_pf_aegp_fast_blur_probe.py",
        "test_pf_aegp_layer_options_probe.py",
        "test_pf_aegp_layer_receipt_probe.py",
        "test_pf_aegp_owned_world_probe.py",
        "test_pf_aegp_platform_world_probe.py",
        "test_pf_aegp_render_options4_tail_probe.py",
        "test_pf_aegp_render_suite5_probe.py",
        "test_pf_convolve_depth_probe.py",
        "test_pf_sampling_probe.py",
        "test_pf_smart_geometry_probe.py",
        "test_pf_transfer_mask_probe.py",
        "test_pf_transfer_rect_probe.py",
        "test_pf_transform_affine_probe.py",
        "test_worker_parameter_discovery.py",
    )
    for name in migrated:
        text = (ROOT / "tests" / name).read_text(encoding="utf-8")
        assert '"--render-image"' not in text
        assert '"--render-image16"' not in text
        assert '"--render-image32"' not in text

    manifest = (ROOT / "tests" / "built_artifact_tests.txt").read_text(
        encoding="utf-8"
    )
    assert "[--render-image" not in manifest
    assert "[--smart-image" not in manifest
