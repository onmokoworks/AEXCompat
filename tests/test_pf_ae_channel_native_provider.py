import json
import os
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
CHANNEL_RUNTIME = ROOT / "minihost/src/worker_pf_ae_channel_runtime.cpp"


def worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target/minihost-timed-layers/Release/aex_render_worker.exe",
        ROOT / "target/minihost-build-v18/Release/aex_render_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def test_native_provider_keeps_plane_metadata_and_never_infers_noncoverage_planes():
    source = (SOURCE.read_text(encoding="utf-8") +
              (SOURCE.parent / "worker_classic_render_runtime.cpp").read_text(encoding="utf-8") +
              CHANNEL_RUNTIME.read_text(encoding="utf-8"))
    for contract in (
        "signed_row_bytes",
        "origin_x",
        "origin_y",
        "downsample_x_num",
        "coordinate_space",
        "sample_receipt",
        "pre_effect_source_pixel",
        "normalized_coverage",
    ):
        assert contract in source
    assert "channel.channel_type = 0x434f5652" in source
    assert "output pixels are never used to infer auxiliary planes" in source
    assert "0x44505448" not in source[source.index("publish_alpha_coverage_provider"):source.index("clear_native_aux_provider")]


def test_native_provider_oracle_normalizes_8_16_float_and_pins_receipt():
    executable = worker()
    assert executable is not None, "build the render worker first"
    completed = subprocess.run(
        [str(executable), "--self-test-pf-ae-channel-native-provider"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert json.loads(completed.stdout) == {
        "pf_ae_channel_native_provider": "passed",
        "coverage_depths": [8, 16, 32],
        "mfr_checkouts": 2048,
        "fabricated_planes": False,
    }


def test_broker_requires_explicit_coverage_contract():
    request = (ROOT / "broker/crates/broker/src/render_request.rs").read_text(encoding="utf-8")
    transport = (ROOT / "broker/crates/broker/src/image_render.rs").read_text(encoding="utf-8")
    assert "alpha_as_coverage_params" in request
    assert '"--alpha-as-coverage-v1"' in transport
    assert "alpha-as-coverage parameter slots are invalid" in transport


def test_coverage_transport_follows_positional_ui_trailers():
    transport = (ROOT / "broker/crates/broker/src/image_render.rs").read_text(encoding="utf-8")
    coverage = transport.index('"--alpha-as-coverage-v1".into()')
    click = transport.index('args_after_plugin.push(format!(\n                    "click:v1|')
    draw = transport.index('RenderUiAction::Draw => args_after_plugin.push("draw:v1".into())')
    assert coverage > click
    assert coverage > draw
