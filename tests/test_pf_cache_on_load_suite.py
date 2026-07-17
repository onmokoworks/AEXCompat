from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost/src/l2_main.cpp"
SDK_HEADER = Path(r"C:\Program Files\Adobe\AfterEffectsSDK\Examples\Headers\AE_CacheOnLoadSuite.h")


def test_cache_on_load_v1_exact_name_version_and_one_slot_abi():
    text = SOURCE.read_text(encoding="utf-8")
    header = SDK_HEADER.read_text(encoding="utf-8")
    assert '#define kPFCacheOnLoadSuite\t\t\t"PF Cache On Load Suite"' in header
    assert "kPFCacheOnLoadSuiteVersion1\t1" in header
    assert 'std::strcmp(name, "PF Cache On Load Suite") == 0 && version == 1' in text
    assert "sizeof(PfCacheOnLoadSuite1) == sizeof(void*)" in text
    assert "offsetof(PfCacheOnLoadSuite1, set_no_cache_on_load) == 0" in text


def test_cache_on_load_policy_is_bounded_and_effect_owned():
    text = SOURCE.read_text(encoding="utf-8")
    assert "effect_ref != &g_effect" in text
    assert "effect_available != 0 && effect_available != 1" in text
    assert "g_no_cache_on_load.store(effect_available != 0" in text
    assert "no persistent startup plug-in cache" in text


def test_cache_on_load_sdk_function_shape_is_frozen():
    header = SDK_HEADER.read_text(encoding="utf-8")
    assert "typedef struct PF_CacheOnLoadSuite1" in header
    assert "(*PF_SetNoCacheOnLoad)" in header
    assert "PF_ProgPtr" in header
    assert "effectAvailable" in header
