from pathlib import Path
import source_owners


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


def test_bib_provider_is_fail_closed_and_loads_only_from_the_sealed_dir():
    assert 'GetModuleHandleW(L"BIB.dll")' in SOURCE
    # Closures that never link BIB statically (Scribble, issue #362 selector
    # families) get exactly one bounded load attempt: the admitted plug-in's
    # own directory joined with the fixed name "BIB.dll", resolved with
    # LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32. No
    # PATH, CWD, or caller-controlled component is ever involved.
    assert 'std::filesystem::path(g_plugin_file_path).parent_path() / L"BIB.dll"' in SOURCE
    assert "LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR" in SOURCE
    assert 'LoadLibraryExW(L"BIB.dll"' not in SOURCE
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


def test_pica_components_init_after_bib_with_bounded_loads_and_seh_guards():
    # The DVA Bravo initializer registers the PICA component interfaces (ACE
    # etc.) into BIB; ae_sweetpea hosts the SP-suite plugins. The real host
    # drives both at process start (issue #362: ProfileToProfile resolves
    # ACEInterface2 through the resolver; Particle_Playground needs the SP
    # suite family). Both run once per process, after BIB is up, outside its
    # mutex, SEH-guarded, and load only from the admitted plug-in directory.
    assert "ensure_pica_components_initialized" in SOURCE
    assert 'GetModuleHandleW(L"dvabravoinitializer.dll")' in SOURCE
    assert 'GetModuleHandleW(L"ae_sweetpea.dll")' in SOURCE
    assert 'L"dvabravoinitializer.dll"' in SOURCE
    assert "?SetBIBProcAddress@dvabravoinitializer@@YAXP6APEAXPEBD00@Z@Z" in SOURCE
    assert "?InitBravoComponents@dvabravoinitializer@@YAP6APEAXPEBD00@ZP6AX0@Z@Z" in SOURCE
    assert "?SPInit@ae_sweetpea@@YAHPEAUSPHostProcs@@PEBUSPPlatformFileSpecification@@H@Z" in SOURCE
    assert "?SPStartupPlugins@ae_sweetpea@@YAHXZ" in SOURCE
    assert "sp_init(nullptr, nullptr, 0)" in SOURCE
    assert "bravo_init_seh_filter" in SOURCE
    assert "LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR" in SOURCE

L2_SOURCE = source_owners.l2_translation_unit_text()


def test_case_id_rejection_runs_global_setdown_before_bib_termination():
    old = "    if (dispatch.case_id_rejected) return session.finish(2);"
    new = "    if (dispatch.case_id_rejected) {\n      dispose_arbitrary_defaults(entry, input, output);\n      if (global_error == 0)\n        invoke_global_setdown(entry, input.data(), output.data());\n      return session.finish(2);\n    }"
    assert old not in L2_SOURCE
    assert L2_SOURCE.count(new) == 2
