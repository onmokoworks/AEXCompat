import json
import subprocess
from pathlib import Path

from PIL import Image

from _render_session import HARNESS, assert_artifact_fresh, run_session_render

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments" / "pf-host-catalog-param-probe" / "pf_host_catalog_param_probe.cpp"
SCRIPT = ROOT / "tools" / "build-pf-host-catalog-param-probe.ps1"
WORKER = ROOT / "target" / "minihost-build" / "aex_worker.exe"
PROBE = (ROOT / "target" / "pf-host-catalog-param-probe-build" / "Release"
         / "pf_host_catalog_param_probe.aex")


def test_probe_builds_against_path_data_suite1():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
                   cwd=ROOT, check=True, timeout=180)
    assert PROBE.is_file()


def test_discovery_and_render_register_the_same_parameter_table(tmp_path):
    # The probe registers a BUTTON when its GLOBAL_SETUP is offered PF Path
    # Data Suite v1 and a CHECKBOX otherwise, and renders what its own process
    # registered. Discovery and rendering run in separate workers, so this only
    # passes when both offer the plug-in one suite catalog (issue #1764): with
    # the catalogs apart, discovery reported a checkbox, the session launch
    # sent it an integer, and the render worker, holding a button, exited 3
    # before frame 0.
    assert WORKER.is_file() and PROBE.is_file()
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    inspected = subprocess.run(
        [str(HARNESS), "--headless", "--inspect-experimental", str(PROBE)],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        timeout=60,
    )
    assert inspected.returncode == 0, inspected.stdout + inspected.stderr
    parameters = json.loads(inspected.stdout)
    assert [(item["slot"], item["name"]) for item in parameters] == [(1, "Catalog")]
    # The host offers PF Path Data Suite v1 on every route (issue #1764), so
    # the plug-in registers its button. Whether AE does so at every selector
    # is inferred from Sapphire's behaviour there, not from an AE capture.
    assert parameters[0]["kind"] == "button"

    rgba_input = tmp_path / "input.rgba"
    rgba_input.write_bytes(bytes([1, 2, 3, 255]) * (8 * 4))
    output = tmp_path / "output.rgba"
    report = run_session_render(tmp_path, PROBE, rgba_input, output, width=8, height=4)
    assert report["status"] == "render_completed", report
    # (red, green) = (registered a button, offered the suite) in the render
    # worker's own process.
    assert output.read_bytes() == bytes([255, 255, 128, 255]) * (8 * 4)


def test_params_only_discovery_and_render_register_the_same_parameter_table(tmp_path):
    # The same agreement through the one-shot `--l2-params-only` discovery
    # route, which `--render-experimental-session-param` inspects with. It
    # takes a different worker branch from the discovery session above, so
    # each route is held to the one catalog separately.
    assert WORKER.is_file() and PROBE.is_file()
    assert_artifact_fresh(PROBE, SOURCE, WORKER, HARNESS)
    session_input = tmp_path / "input.png"
    Image.new("RGBA", (8, 4), (1, 2, 3, 255)).save(session_input)
    session_output = tmp_path / "output.png"
    completed = subprocess.run(
        [str(HARNESS), "--render-experimental-session-param", str(PROBE),
         str(session_input), str(session_output), "argb8", "classic", "0", "1", "1",
         "1", "0"],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        timeout=60,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    assert json.loads(completed.stdout)["passed"] is True
    pixels = Image.open(session_output).convert("RGBA").tobytes()
    assert pixels == bytes([255, 255, 128, 255]) * (8 * 4)
