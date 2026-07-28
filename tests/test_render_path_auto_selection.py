from tests import source_owners

import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HARNESS_SOURCE = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"
BROKER_SOURCE = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "target" / "release" / "aexcompat-harness.exe"
FIXTURE = ROOT / "target" / "sdk-fixtures" / "shifter" / "Shifter.aex"
INPUT = ROOT / "target" / "ae-oracle-colorgrid-input.png"


def test_render_path_auto_selection_is_wired_and_explicit_flags_stay_explicit():
    broker = BROKER_SOURCE.read_text(encoding="utf-8")
    # The declaration comes from the AEX itself: out_flags2 bit 10
    # (PF_OutFlag2_SUPPORTS_SMART_RENDER) observed after GLOBAL_SETUP.
    assert "pub const PF_OUTFLAG2_SUPPORTS_SMART_RENDER: u64 = 1 << 10;" in broker
    assert "pub fn smart_render_advertised(out_flags2: u64) -> bool" in broker
    assert (
        'diagnostics["smart_render_advertised"] = '
        "json!(smart_render_advertised(advertised_out_flags2));" in broker
    )

    harness = source_owners.harness_windows_text()
    # The GUI derives its default render path from the inspection diagnostics
    # and keeps the toggle as a manual override (issue #105).
    assert "fn advertised_smart_render(report: &serde_json::Value)" in harness
    assert "self.smart_render = advertised;" in harness
    # The auto CLI route is a separate opt-in; the historical flags keep their
    # explicit Classic/SmartFX semantics so frozen evidence commands do not
    # silently change paths.
    assert '"--render-experimental-auto"' in harness
    assert '"--render-experimental"' in harness
    assert '"--render-experimental-smart"' in harness
    assert 'command == "--render-experimental-auto"' in harness
    assert 'value["render_path_source"]' in harness


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
