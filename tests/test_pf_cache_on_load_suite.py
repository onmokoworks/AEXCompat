import os
from pathlib import Path

import pytest
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
COMPONENT = ROOT / "minihost/src/pf_cache_on_load_suite.cpp"
SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
SDK_HEADER = Path(SDK_ROOT) / "Examples" / "Headers" / "AE_CacheOnLoadSuite.h" if SDK_ROOT else None


def _sdk_header() -> Path:
    if SDK_HEADER is None or not SDK_HEADER.is_file():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    return SDK_HEADER






def test_cache_on_load_sdk_function_shape_is_frozen():
    header = _sdk_header().read_text(encoding="utf-8")
    assert "typedef struct PF_CacheOnLoadSuite1" in header
    assert "(*PF_SetNoCacheOnLoad)" in header
    assert "PF_ProgPtr" in header
    assert "effectAvailable" in header
