from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
PF_SUITES = ROOT / "minihost" / "src" / "worker_pf_suites.cpp"
SOURCE = source_owners.L2_SOURCE
SAMPLING_RUNTIME = ROOT / "minihost" / "src" / "worker_pf_sampling_runtime.cpp"


def test_legacy_sampling_callbacks_are_wired_at_the_frozen_offsets():
    source = "\n".join(path.read_text(encoding="utf-8") for path in (SOURCE, PF_SUITES, SAMPLING_RUNTIME))
    assert "constexpr std::size_t kUtilsBeginSampling = 0" in source
    assert "constexpr std::size_t kUtilsSubpixelSample = 8" in source
    assert "constexpr std::size_t kUtilsAreaSample = 16" in source
    assert "constexpr std::size_t kUtilsEndSampling = 32" in source
    assert "write(utils, kUtilsBeginSampling, &begin_sampling8)" in source
    assert "write(utils, kUtilsAreaSample, &area_sample8)" in source
    assert "write(utils, kUtilsEndSampling, &end_sampling8)" in source


def test_legacy_sampling_lifecycle_is_bounded_and_balanced():
    source = "\n".join(path.read_text(encoding="utf-8") for path in (SOURCE, PF_SUITES, SAMPLING_RUNTIME))
    for marker in (
        "struct LegacySamplingSession",
        "g_legacy_sampling_sessions.emplace",
        "resolve_world(source_world, 4",
        "found->second.quality != quality",
        "found->second.mode_flags != mode_flags",
        "found->second.thread_id != GetCurrentThreadId()",
        "g_legacy_sampling_sessions.erase(found)",
    ):
        assert marker in source
