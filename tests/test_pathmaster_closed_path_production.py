import json
import subprocess
from pathlib import Path

from _render_session import HARNESS


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "target" / "sdk-fixtures" / "pathmaster" / "PathMaster.aex"
INPUT = ROOT / "target" / "ae-oracle-colorgrid-input.png"
REQUEST = ROOT / "tools" / "sdk-fixtures" / "pathmaster-hard-edge-request.json"


def test_closed_path_reaches_mask_and_transfer_rect_in_production(tmp_path: Path) -> None:
    request = json.loads(REQUEST.read_text(encoding="utf-8"))
    assignments = {entry["slot"]: entry["value"] for entry in request["assignments"]}
    assert assignments[1] == 1
    assert request["host_context"]["mask_scene"]["masks"][0]["open"] is False

    output = tmp_path / "pathmaster-closed.png"
    completed = subprocess.run(
        [str(HARNESS), "--render-experimental-request", str(FIXTURE), str(INPUT),
         str(output), str(REQUEST)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr
    report = json.loads(completed.stdout)
    assert report["passed"] is True
    assert report["worker_classification"] == "ok"
    assert report["render_path"] == "classic"
    assert report["requested_parameters"][0] == {
        "id": "param_1", "kind": "integer", "slot": 1, "value": 1,
    }
    assert report["pf_path_checkout_calls"] == 1
    assert report["pf_path_checkin_calls"] == 1
    assert report["pf_path_mask_calls"] == 1
    assert report["pf_path_reject_reason"] == 0
    assert report["output_sha256"] == (
        "ed1827e76c2bb35bb6589daf728698013f3863449fa346f7538aeabdb777c81d"
    )
    assert report["guard_bytes_intact"] is True
    assert report["suite_leases_balanced"] is True
    assert report["handle_lifetimes_balanced"] is True
    assert report["world_lifetimes_balanced"] is True
    assert report["pf_path_lifetimes_balanced"] is True
    assert output.is_file()
