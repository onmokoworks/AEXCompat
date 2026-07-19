from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MAIN = source_owners.L2_MAIN.read_text(encoding="utf-8")
HEADER = (ROOT / "minihost/src/worker_host_suite_router.hpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost/src/worker_host_suite_router.cpp").read_text(
    encoding="utf-8"
)
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")
CATALOG = (ROOT / "minihost/src/worker_host_suite_catalog.cpp").read_text(encoding="utf-8")


def test_host_suite_router_owns_registry_acquire_and_release_boundary():
    assert CMAKE.count("src/worker_host_suite_router.cpp") == 1
    assert "struct ProviderCatalog" in HEADER
    assert "int32_t acquire_host_suite" in HEADER
    assert "int32_t release_host_suite" in HEADER
    assert "suite_registry().acquire" in SOURCE
    assert "suite_registry().release" in SOURCE
    acquire = MAIN[MAIN.rindex("int32_t __cdecl acquire_suite"):]
    acquire = acquire[: acquire.index("int32_t __cdecl release_suite")]
    assert "acquire_catalog_suite(" in acquire
    assert "acquire_host_suite(" in CATALOG
    assert "suite_registry().acquire" not in acquire


def test_provider_priority_and_terminal_errors_are_explicit():
    provider_loop = SOURCE[SOURCE.index("SuiteResolveResult resolve_catalog"):]
    assert "index < catalog.provider_count" in provider_loop
    assert "result != SuiteResolveResult::not_found" in provider_loop
    assert provider_loop.index("provider.resolve") < provider_loop.index(
        "catalog.fallback"
    )
    assert "SuiteResolveResult::rejected_bad_param" in SOURCE


def test_component_pf_and_aegp_families_are_injected_without_legacy_route():
    acquire = MAIN[MAIN.index("const StaticSuite component_suites[]"):]
    for suite in (
        "AE Plugin Helper Suite",
        "PF Cache On Load Suite",
        "PF AE Adv Time Suite",
        "AEGP Memory Suite",
        "AEGP Utility Suite",
    ):
        assert suite in acquire
    assert "resolve_scene_suite_provider" in acquire
    assert "resolve_static_provider" in CATALOG
    assert "providers[0] = configuration.scene_provider" in CATALOG
    assert "providers[1] = {&resolve_static_provider" in CATALOG
    assert "resolve_legacy_host_suite" not in MAIN
    assert "configure_host_suite_catalog" in acquire
    assert "ProviderCatalog provider_catalog" in CATALOG


def test_static_provider_matches_exact_name_and_version_only():
    assert "candidate.version == version" in SOURCE
    assert "std::strcmp(candidate.name, name) == 0" in SOURCE
    assert "return *suite ? SuiteResolveResult::acquired" in SOURCE


def test_factory_and_availability_hooks_preserve_conditional_suite_routes():
    assert "candidate.available(candidate.availability_context)" in SOURCE
    assert "candidate.factory(candidate.factory_context)" in SOURCE
    assert "return SuiteResolveResult::not_found" in SOURCE
    for family in (
        "AEGP World Suite",
        "AEGP Render Options Suite",
        "AEGP Layer Render Options Suite",
        "AEGP Render Suite",
        "AEGP Layer Mask Suite",
        "AEGP Stream Suite",
        "AEGP Keyframe Suite",
    ):
        assert family in MAIN[MAIN.index("int32_t __cdecl acquire_suite"):]
    assert "render_options4_provider_available" in MAIN
    assert "render_suite2_provider_available" in MAIN
    assert "mask_suite_provider_available" in MAIN


def test_l2_has_no_legacy_name_version_routing_chain():
    routing = MAIN[MAIN.index("SuiteResolveResult resolve_scene_suite_provider(") :]
    routing = routing[: routing.index("int32_t __cdecl release_suite(")]
    assert "resolve_legacy_host_suite" not in routing
    assert "std::strcmp(name" not in routing
    assert "configure_host_suite_catalog" in routing
    assert "ProviderCatalog provider_catalog" in CATALOG
