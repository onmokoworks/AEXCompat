import os
from pathlib import Path

import pytest

from _render_session import assert_artifact_fresh


SESSION_PROBE_TESTS = (
    "test_pf_aegp_async_layer_receipt_probe.py",
    "test_pf_aegp_external_cache_roundtrip_probe.py",
    "test_pf_aegp_fast_blur_probe.py",
    "test_pf_aegp_layer_options_probe.py",
    "test_pf_aegp_layer_receipt_probe.py",
    "test_pf_aegp_owned_world_probe.py",
    "test_pf_aegp_platform_world_probe.py",
    "test_pf_aegp_render_options4_tail_probe.py",
    "test_pf_convolve_depth_probe.py",
    "test_pf_sampling_probe.py",
    "test_pf_smart_geometry_probe.py",
    "test_pf_transfer_mask_probe.py",
    "test_pf_transfer_rect_probe.py",
    "test_pf_transform_affine_probe.py",
    "test_worker_parameter_discovery.py",
)


@pytest.mark.parametrize("filename", SESSION_PROBE_TESTS)
def test_session_probe_callers_guard_artifact_provenance(filename):
    source = Path(__file__).with_name(filename).read_text(encoding="utf-8")
    assert "assert_artifact_fresh" in source
    assert "SOURCE, WORKER, HARNESS" in source


def test_stale_artifact_is_rejected_with_rebuild_diagnostic(tmp_path):
    artifact = tmp_path / "probe.aex"
    runtime = tmp_path / "aex_render_worker.exe"
    artifact.write_bytes(b"old probe")
    runtime.write_bytes(b"new worker")
    os.utime(artifact, ns=(1_700_000_000_000_000_000, 1_700_000_000_000_000_000))
    os.utime(runtime, ns=(1_700_000_001_000_000_000, 1_700_000_001_000_000_000))

    with pytest.raises(AssertionError) as error:
        assert_artifact_fresh(artifact, runtime)

    message = str(error.value)
    assert "stale built artifact" in message
    assert str(artifact) in message
    assert str(runtime) in message
    assert "rebuild the artifact" in message


def test_artifact_at_least_as_new_as_runtime_inputs_is_accepted(tmp_path):
    artifact = tmp_path / "probe.aex"
    source = tmp_path / "probe.cpp"
    artifact.write_bytes(b"new probe")
    source.write_bytes(b"old source")
    os.utime(source, ns=(1_700_000_000_000_000_000, 1_700_000_000_000_000_000))
    os.utime(artifact, ns=(1_700_000_001_000_000_000, 1_700_000_001_000_000_000))

    assert_artifact_fresh(artifact, source)
