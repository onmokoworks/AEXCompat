import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments/pf-adv-time-probe/pf_adv_time_probe.cpp"
RC = ROOT / "instruments/pf-adv-time-probe/pf_adv_time_probe.rc"
BUILD = ROOT / "tools/build-pf-adv-time-probe.ps1"


def test_pf_adv_time_probe_release_build():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(BUILD)],
                   cwd=ROOT, check=True, timeout=180)
    assert (ROOT / "target/pf-adv-time-probe-build/Release/pf_adv_time_probe.aex").is_file()


def test_probe_covers_v4_slots_edges_guards_and_raw_observation():
    source = SOURCE.read_text(encoding="utf-8")
    for marker in ("kPFAdvTimeSuite", "kPFAdvTimeSuiteVersion4", "PF_AdvTimeSuite4",
                   "PF_FormatTimeActiveItem", "PF_FormatTime(", "PF_FormatTimePlus",
                   "PF_GetTimeDisplayPref", "PF_TimeCountFrames", "active_positive",
                   "active_negative_duration", "active_scale_zero", "world_duration",
                   "plus_comp_duration", "display_pref", "exact", "partial_excluded",
                   "partial_included", "invalid_scale_zero", "overflow", "null_inputs",
                   "active_null_output", "guard_intact", "raw_output", "lease_balanced",
                   "AcquireSuite", "ReleaseSuite", "__try", "GetExceptionInformation",
                   "AEXCOMPAT_PF_ADV_TIME_V4", "OutputDebugStringA"):
        assert marker in source
    assert 'observe(in, nullptr, "global_setup")' in source
    assert 'observe(in, output, "render")' in source
    assert "PF Time" not in source
    assert "PF Adv Time v4 Probe" in RC.read_text(encoding="utf-8")


def test_lease_buffer_arithmetic_and_c_abi_are_hardened():
    source = SOURCE.read_text(encoding="utf-8")
    for marker in ("bool owns", "owns = true", "if (owns && !release_count)",
                   "acquired_null_suite", "lease.release()", "output->width < 0",
                   "output->height < 0", "output->rowbytes < 0",
                   "numeric_limits<std::size_t>::max)()", "kMaxImageCapacity",
                   "rowbytes * height", "effect_main_impl", "catch (const std::bad_alloc&)",
                   "PF_Err_OUT_OF_MEMORY", "catch (...)", "PF_Err_INTERNAL_STRUCT_DAMAGED"):
        assert marker in source
    assert source.index("owns = true") < source.index("suite = static_cast")
    assert source.index("width * sizeof(PF_Pixel8) > rowbytes") < source.index("for (std::size_t y=0")


def test_render_writes_atomic_fixed_temp_sidecar_without_changing_render_result():
    source = SOURCE.read_text(encoding="utf-8")
    for marker in ("write_atomic_sidecar", "aexcompat-pf-adv-time-v4.json",
                   "GetCurrentProcessId", "GetCurrentThreadId", "CreateFileA",
                   "WriteFile", "FlushFileBuffers", "CloseHandle", "MoveFileExA",
                   "MOVEFILE_REPLACE_EXISTING", "MOVEFILE_WRITE_THROUGH", "DeleteFileA",
                   "AEXCOMPAT_PF_ADV_TIME_V4_SIDECAR", "win32_error", "resolve_temp",
                   "create_temp", "write_temp", "flush_temp", "close_temp", "rename_temp"):
        assert marker in source
    render = source[source.index("PF_Err render("):source.index("PF_Err effect_main_impl(")]
    assert 'observe(in, output, "render")' in render
    assert "emit_sidecar_status(write_atomic_sidecar(json))" in render
    assert "return PF_Err_INTERNAL_STRUCT_DAMAGED" not in render
    assert render.index("write_atomic_sidecar(json)") < render.index("if (!output || !output->data")


def test_fixture_records_host_results_without_expected_values():
    source = SOURCE.read_text(encoding="utf-8")
    for forbidden in ("expected_count", "expected_text", "host_expected", "assert("):
        assert forbidden not in source
    assert "raw_bytes" in source
    assert "raw_output" in source
