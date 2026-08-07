import os

import pytest

from _render_session import assert_artifact_fresh


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
    assert str(artifact) in message
    assert str(runtime) in message


def test_artifact_at_least_as_new_as_runtime_inputs_is_accepted(tmp_path):
    artifact = tmp_path / "probe.aex"
    source = tmp_path / "probe.cpp"
    artifact.write_bytes(b"new probe")
    source.write_bytes(b"old source")
    os.utime(source, ns=(1_700_000_000_000_000_000, 1_700_000_000_000_000_000))
    os.utime(artifact, ns=(1_700_000_001_000_000_000, 1_700_000_001_000_000_000))

    assert_artifact_fresh(artifact, source)
