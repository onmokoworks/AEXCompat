from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "minihost/src/worker_host_suite_wiring.cpp").read_text(
    encoding="utf-8"
)


def test_bib_suite_is_cataloged_as_a_single_resolver_slot():
    assert '{"AEFX Text BIB Suite", 1, nullptr, &provide_bib_suite}' in SOURCE
    assert "std::array<void*, 1> suite" in SOURCE
    assert 'state.suite[0] = reinterpret_cast<void*>(state.resolver)' in SOURCE


def test_bib_provider_is_fail_closed_and_does_not_load_arbitrary_paths():
    assert 'GetModuleHandleW(L"BIB.dll")' in SOURCE
    assert "LoadLibrary" not in SOURCE
    assert 'GetProcAddress(bib, "BIBInitialize4")' in SOURCE
    for procedure in (
        "BIBRegisterProcAddress",
        "BIBReportError",
        "BIBUnregisterInterface",
        "BIBGetUnregisterCountAddr",
        "BIBIsMultiThreaded",
    ):
        assert procedure in SOURCE
    assert "if (!state.resolver) return nullptr;" in SOURCE
