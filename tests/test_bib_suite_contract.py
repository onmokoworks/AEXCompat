from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "minihost/src/worker_host_suite_wiring.cpp").read_text(
    encoding="utf-8"
)


def test_bib_suite_is_cataloged_as_a_single_resolver_slot():
    assert '{"AEFX Text BIB Suite", 1, nullptr, &provide_bib_suite}' in SOURCE
    assert "std::array<void*, 1> suite" in SOURCE
    # suite[0] mirrors BIBGetGetProcAddress: an entry thunk that returns the
    # current resolver, not the resolver itself. Plug-ins call suite[0] with
    # arbitrary register state, so putting the resolver in the slot enters it
    # with garbage arguments and dereferences wild pointers (#362 VR family).
    assert "const void* bib_resolver_entry() noexcept;" in SOURCE
    assert "state.suite[0] = reinterpret_cast<void*>(&bib_resolver_entry);" in SOURCE
    assert "state.suite[0] = reinterpret_cast<void*>(state.resolver)" not in SOURCE


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

L2_SOURCE = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")


def test_case_id_rejection_runs_global_setdown_before_bib_termination():
    old = "    if (dispatch.case_id_rejected) return session.finish(2);"
    new = "    if (dispatch.case_id_rejected) {\n      dispose_arbitrary_defaults(entry, input, output);\n      if (global_error == 0)\n        invoke_global_setdown(entry, input.data(), output.data());\n      return session.finish(2);\n    }"
    assert old not in L2_SOURCE
    assert L2_SOURCE.count(new) == 2
