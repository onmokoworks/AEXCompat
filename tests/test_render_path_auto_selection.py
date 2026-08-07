import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "broker" / "target" / "release" / "aexcompat-harness.exe"
FIXTURE = ROOT / "target" / "sdk-fixtures" / "shifter" / "Shifter.aex"
INPUT = ROOT / "target" / "ae-oracle-colorgrid-input.png"

def test_auto_route_follows_advertised_smart_render(tmp_path: Path) -> None:
    auto_output = tmp_path / "shifter-auto.png"
    completed = subprocess.run(
        [str(HARNESS), "--render-experimental-auto", str(FIXTURE),
         str(INPUT), str(auto_output)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert completed.returncode == 0, completed.stderr
    report = json.loads(completed.stdout)
    # SDK Shifter implements both paths and advertises SUPPORTS_SMART_RENDER,
    # so the auto route must pick SmartFX and record why.
    assert report["passed"] is True
    assert report["render_path"] == "smartfx"
    assert report["render_path_source"] == "advertised_out_flags2"
    assert report["smart_render_advertised"] is True
    assert auto_output.is_file()

    explicit_output = tmp_path / "shifter-classic.png"
    completed = subprocess.run(
        [str(HARNESS), "--render-experimental", str(FIXTURE),
         str(INPUT), str(explicit_output)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert completed.returncode == 0, completed.stderr
    report = json.loads(completed.stdout)
    # The explicit Classic flag must stay Classic even though the fixture
    # advertises SmartFX support.
    assert report["render_path"] == "classic"
    assert "render_path_source" not in report
    assert explicit_output.is_file()
