import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "run-maskoffset-smartfx-render-gate.ps1"
RESULT = ROOT / "analysis" / "MASKOFFSET_SMARTFX_RENDER_GATE_RESULT_2026-07-18.json"
EXPECTED = "bf419f44e915901bac882e9b9e3411b8407df7f4e3a4a1c2719314bdfbb74b5f"


def test_gate_is_broker_only_and_invokes_cli_twice():
    script = SCRIPT.read_text(encoding="utf-8-sig")
    assert "smart-parameter-request" in script
    assert "foreach ($receiptRelative in $receiptRelatives)" in script
    assert "Start-Process" not in script
    assert "--smart-mask-request" not in script
    assert "aex_smart_worker.exe --" not in script


def test_gate_authenticates_broker_worker_request_and_approval():
    script = SCRIPT.read_text(encoding="utf-8-sig").lower()
    assert "1d98ec9a0b6f2b76223ef7729017f6be792e482748733ab16036a3533877a234" in script
    assert "60094903544c2e15b6d26b3b41b1c4ea96b565afd514dc7167c943ccd70cb4dd" in script
    assert "d92568e9f880ec9d06ceccb03de0ebb62d3dd00b0bdb9e3978c5fd687b7e26d7" in script
    assert "maskoffset-smartfx-20260713-001" in script


def test_receipt_validation_covers_required_fields():
    script = SCRIPT.read_text(encoding="utf-8-sig")
    for marker in ("plugin_authenticated", "request_authenticated", "output_exact", "selectors_success", "guards_intact", "secure_route", "ownership_valid"):
        assert marker in script
    assert "sealed_load_tree_restricted_token" in script
    assert "normal_token_fallback" in script
    assert EXPECTED in script.lower()


def test_current_evidence_is_redacted_and_truthful():
    data = json.loads(RESULT.read_text(encoding="utf-8-sig"))
    assert data["oracle"] is False
    serialized = RESULT.read_text(encoding="utf-8-sig")
    assert not re.search(r"[A-Za-z]:[\\/]", serialized)
    assert "source_path" not in serialized
