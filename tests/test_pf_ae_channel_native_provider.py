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
    session = (ROOT / "broker/crates/broker/src/render_session.rs").read_text(encoding="utf-8")
    assert "alpha_as_coverage_params" in request
    # The launch argv is built by the session since #365 deleted the one-shot
    # transport; the validation message stays where the shared helpers live.
    assert '"--alpha-as-coverage-v1".to_owned()' in session
    assert "alpha-as-coverage parameter slots are invalid" in session
    assert "alpha_as_coverage_params" in transport


def test_coverage_transport_follows_the_positional_trailers():
    """The named auxiliary pairs must sit behind every positional trailer.

    The worker strips `--flag value` pairs off argv's tail before it decodes the
    positional session contract, so a named transport pushed *ahead* of a
    positional trailer would shift where the contract lands. Before #365 the
    trailer this had to follow was the one-shot's positional UI field; the
    session sends its UI action per frame in the v:2 message instead, and its
    positional tail is the layer trailer plus the mask/spatial/render/audio
    trailers.
    """
    session = (ROOT / "broker/crates/broker/src/render_session.rs").read_text(encoding="utf-8")
    coverage = session.index('"--alpha-as-coverage-v1".to_owned()')
    for positional in (
        "args_after_plugin.push(mask.clone());",
        "args_after_plugin.push(spatial.clone());",
        "args_after_plugin.push(render_environment.clone());",
        "args_after_plugin.push(audio.clone());",
    ):
        assert session.index(positional) < coverage, positional
    # The click/draw grammar the UI trailer carried still lives in
    # encode_ui_field, which now feeds the per-frame `ui_action` attribute.
    transport = (ROOT / "broker/crates/broker/src/image_render.rs").read_text(encoding="utf-8")
    assert "\"click:v1|" in transport
    assert '"draw:v1".into()' in transport
    assert '"ui_action".into()' in session
