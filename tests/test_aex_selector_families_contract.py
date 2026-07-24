from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PARSER = (ROOT / "minihost/src/aex_string_table_impl.cpp").read_text(encoding="utf-8")
L2 = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
WIRING = (ROOT / "minihost/src/worker_host_suite_wiring.cpp").read_text(
    encoding="utf-8"
)
UTILITY_HPP = (ROOT / "minihost/src/worker_aegp_utility_suite.hpp").read_text(
    encoding="utf-8"
)
COLOR = (ROOT / "minihost/src/worker_color_settings_runtime.cpp").read_text(
    encoding="utf-8"
)
COMPUTE = (ROOT / "minihost/src/worker_compute_cache_suite.cpp").read_text(
    encoding="utf-8"
)
BOOTSTRAP_CPP = (ROOT / "minihost/src/worker_effect_bootstrap.cpp").read_text(
    encoding="utf-8"
)


def test_string_table_values_are_freeform_but_structure_stays_fail_closed():
    # Entry values carry empty text, embedded newlines, and UTF-8 in bundled
    # effects (Colorama, Lumetri, OCIO); only key paths stay structural.
    assert "// Entry values are free-form text" in PARSER
    assert "ascii_text(candidate.substr(prefix.size(), equals - prefix.size()))" in PARSER
    # LStr digit structure is still validated.
    assert "byte < '0' || byte > '9'" in PARSER


def test_string_table_groups_select_the_about_version_group():
    # One image carries match-name, shared-library, and sibling-effect LStr
    # groups; the runtime lookup serves the unique group whose id 0 is the
    # about-version string.
    assert 'id0->second.find(", v%")' in PARSER
    assert "has_about_version_id0" in PARSER
    # Same-group duplicate ids and ambiguous multi-group images stay invalid.
    assert "return invalid_table();" in PARSER


def test_lookup_returns_empty_string_for_missing_id_in_valid_table_only():
    assert "LoadString semantics for a valid table" in L2
    assert "ParseStatus::Valid" in L2
    assert "kEmptyString" in L2


def test_legacy_support_init_calls_u_birth_once_seh_guarded():
    assert "initialize_legacy_support_libraries" in L2
    assert 'GetModuleHandleW(L"U.dll")' in L2
    assert 'GetProcAddress(u_module, "U_Birth")' in L2
    assert "u_birth_seh_filter" in L2
    assert "stage:legacy_support_init" in L2


def test_utility_suite_versions_3_and_11_are_cataloged():
    assert '{"AEGP Utility Suite", 3, &g_utility_suite1}' in WIRING
    assert '{"AEGP Utility Suite", 11, &g_utility_suite5}' in WIRING
    # Suite1 (v3): RegisterWithAEGP at slot 7, GetMainHWND at slot 8, 9 slots.
    assert "sizeof(UtilitySuite1) == 9 * sizeof(void*)" in UTILITY_HPP
    # Suite5 (v11): RegisterWithAEGP at slot 8, GetMainHWND at slot 9, 31 slots.
    assert "sizeof(UtilitySuite5) == 31 * sizeof(void*)" in UTILITY_HPP


def test_color_settings_suite_v6_is_served_and_ocio_queries_accept_id_zero():
    assert '{"PF Color Settings Suite", 6, nullptr, &provide_color_settings7}' in WIRING
    assert "ocio_query_id_accepted" in COLOR
    assert "plugin_id == 0 || plugin_id == 1" in COLOR


def test_compute_cache_suite_is_cataloged_and_purged_on_cluster_reset():
    assert '{"AEGP Compute Cache", 1, nullptr, &provide_compute_cache1}' in WIRING
    assert "aexcompat::compute_cache::purge_registry();" in L2
    assert "kErrNotInCacheOrComputePending = 22" in COMPUTE
    assert "delete_compute_value" in COMPUTE


def test_legacy_app_callback_is_wired_at_utils_offset_200():
    assert "host_app_callback" in L2
    assert "// Legacy application-specific callback `app` (issue #362" in BOOTSTRAP_CPP
    assert "200};" in BOOTSTRAP_CPP
