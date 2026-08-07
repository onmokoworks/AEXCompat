import hashlib
import json
import os
from pathlib import Path, PureWindowsPath


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "AEGP_ASYNC_RECEIPT_RUNTIME_RESULT_2026-07-16.json"


def _load():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()



def _installed_sdk_file(record):
    candidates = [Path(record["path"])]
    sdk_root = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
    if sdk_root:
        parts = PureWindowsPath(record["path"]).parts
        if "Examples" in parts:
            candidates.append(Path(sdk_root).joinpath(*parts[parts.index("Examples"):]))
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    raise AssertionError(
        "SDK source was not found at the recorded path or under AFTER_EFFECTS_SDK_ROOT: "
        + record["path"]
    )


def test_installed_sdk_abi_sources_match_recorded_provenance():
    evidence = _load()["abi_source"]

    for key in ("current_header", "legacy_header"):
        record = evidence[key]
        path = _installed_sdk_file(record)
        assert path.stat().st_size == record["size_bytes"]
        assert _sha256(path) == record["sha256"]

    assert evidence["legacy_header"]["observations"][0].endswith("numeric acquisition version 5.")
