import os
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost/src/l2_main.cpp"
COMPONENT = ROOT / "minihost/src/pf_cache_on_load_suite.cpp"
SDK_ROOT = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
SDK_HEADER = Path(SDK_ROOT) / "Examples" / "Headers" / "AE_CacheOnLoadSuite.h" if SDK_ROOT else None


def _sdk_header() -> Path:
    if SDK_HEADER is None or not SDK_HEADER.is_file():
        pytest.skip("set AFTER_EFFECTS_SDK_ROOT to a valid After Effects SDK root")
    return SDK_HEADER


def test_cache_on_load_v1_exact_name_version_and_one_slot_abi():
    text = SOURCE.read_text(encoding="utf-8")
    component = COMPONENT.read_text(encoding="utf-8")
    header = _sdk_header().read_text(encoding="utf-8")
    assert '#define kPFCacheOnLoadSuite\t\t\t"PF Cache On Load Suite"' in header
    assert "kPFCacheOnLoadSuiteVersion1\t1" in header
    assert 'std::strcmp(name, "PF Cache On Load Suite") == 0 && version == 1' in text
    assert "sizeof(PfCacheOnLoadSuite1) == sizeof(void*)" in component
    assert "offsetof(PfCacheOnLoadSuite1, set_no_cache_on_load) == 0" in component


def test_cache_on_load_policy_is_bounded_and_effect_owned():
    component = COMPONENT.read_text(encoding="utf-8")
    assert "effect_ref != owned_effect_ref" in component
    assert "effect_available != 0 && effect_available != 1" in component
    assert "g_no_cache_on_load.store(effect_available != 0" in component
    assert "no persistent startup plug-in cache" in component


def test_cache_on_load_sdk_function_shape_is_frozen():
    header = _sdk_header().read_text(encoding="utf-8")
    assert "typedef struct PF_CacheOnLoadSuite1" in header
    assert "(*PF_SetNoCacheOnLoad)" in header
    assert "PF_ProgPtr" in header
    assert "effectAvailable" in header
