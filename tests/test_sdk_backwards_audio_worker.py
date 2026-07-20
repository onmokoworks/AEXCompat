"""Real-AEX audio behavioral self-test for issue #251.

Drives the built render worker directly through the one-shot ``--render-audio``
path against the SDK_Backwards fixture (SDK audio example, reverses the audio and
adds its default tone). This crosses the real AEX boundary for audio: it proves
the worker loads a real audio effect, drives AUDIO_SETUP/RENDER/SETDOWN, and
returns a contract-complete report. It also pins the #257 regression: the
one-shot audio report must carry ``module_audit`` so the broker's secure dispatch
accepts it.

The resident ``--render-audio-session-v1`` transport (named section + inherited
pipes + generation protocol) is exercised from Rust via ``AudioRenderSession``
(broker/crates/broker/tests/render_session.rs and render_session_wrapper.rs); it
is impractical to set up from Python, so this self-test covers the one-shot path.

Registered in ``built_artifact_tests.txt``; run with
``--run-built-artifact-tests`` after building the worker and the fixture
(``tools/build-sdk-backwards.ps1``).
"""

import hashlib
import json
import math
import struct
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target/minihost-build/aex_render_worker.exe"
AEX = ROOT / "target/sdk-fixtures/sdk-backwards/SDK_Backwards.aex"


def test_real_sdk_backwards_audio_one_shot_renders_and_audits(tmp_path):
    sha = hashlib.sha256(AEX.read_bytes()).hexdigest()
    count = 512
    samples = [math.sin(index * 0.05) * 0.5 for index in range(count)]
    input_bytes = b"".join(struct.pack("<f", value) for value in samples)
    input_path = tmp_path / "input.f32"
    output_path = tmp_path / "output.f32"
    input_path.write_bytes(input_bytes)

    completed = subprocess.run(
        [
            str(WORKER),
            "--render-audio",
            str(AEX),
            sha,
            "v2|",
            str(input_path),
            str(output_path),
            str(count),
            "44100",
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=60,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)

    assert report["status"] == "render_completed"
    assert report["sample_rate"] == 44100
    assert report["channels"] == 1
    assert report["sample_format"] == "float32"
    assert report["audio_setup_error"] == 0
    assert report["audio_render_error"] == 0
    assert report["audio_setdown_error"] == 0
    assert report["setup_range_valid"] is True
    assert report["guard_bytes_intact"] is True
    assert report["samples_finite"] is True
    assert report["audio_lifetimes_balanced"] is True
    assert report["invalid_audio_operations"] == 0
    assert report["output_created"] is True
    assert 0 < report["output_samples"] <= count

    # The one-shot audio report omitted module_audit entirely before #257, so
    # the broker's secure dispatch failed "secure worker module audit is
    # missing". Pin that the field is now emitted and well-formed. A direct
    # (non-broker) launch has no sealed context, so the audit is "not_required"
    # with empty snapshots; the enforced "passed" case under the secure dispatch
    # is covered by the render_session_wrapper.rs audio A/B.
    audit = report["module_audit"]
    assert audit["schema"] == 1
    assert audit["status"] == "not_required"
    assert audit["unknown_count"] == 0
    assert "observed_union" in audit and "plugin" in audit["observed_union"]

    output_bytes = output_path.read_bytes()
    assert len(output_bytes) == report["output_samples"] * 4
    # Non-vacuous: SDK_Backwards actually transformed the audio.
    assert output_bytes != input_bytes
