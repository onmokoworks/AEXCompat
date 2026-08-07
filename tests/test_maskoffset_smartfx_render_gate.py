import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "MASKOFFSET_SMARTFX_RENDER_GATE_RESULT_2026-07-18.json"


def test_current_evidence_is_redacted_and_truthful():
    data = json.loads(RESULT.read_text(encoding="utf-8-sig"))
    assert data["oracle"] is False
