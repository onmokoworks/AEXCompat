from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"


def source() -> str:
    return SOURCE.read_text(encoding="utf-8")


def test_adv_time_v4_has_exact_public_name_version_and_five_slots():
    text = source()
    assert 'std::strcmp(name, "PF AE Adv Time Suite") == 0 && version == 4' in text
    assert "std::array<void*, 5> g_adv_time_suite4" in text
    callbacks = [
        "adv_time_format_active",
        "adv_time_format",
        "adv_time_format_plus",
        "adv_time_get_display_pref",
        "adv_time_count_frames",
    ]
    positions = [text.index(f"reinterpret_cast<void*>(&{name})") for name in callbacks]
    assert positions == sorted(positions)


def test_headless_display_policy_is_explicit_and_does_not_claim_ae_preferences():
    text = source()
    assert "constexpr int64_t kHeadlessFramesPerSecond = 30" in text
    assert "std::memset(pref, 0, sizeof(*pref))" in text
    assert "pref->framemax = static_cast<int32_t>(kHeadlessFramesPerSecond)" in text
    assert "pref->nondrop30 = 1" in text
    assert "*starting_frame = 0" in text
    assert "The headless host has no AE UI preference state" in text


def test_formatting_and_frame_count_fail_closed_at_abi_boundaries():
    text = source()
    assert "constexpr std::size_t kPfMaxTimeBufferSize = 32" in text
    assert "if (buffer) buffer[0] = '\\0'" in text
    assert "if (!buffer || scale == 0) return 4" in text
    assert "static_cast<int64_t>(value) * kHeadlessFramesPerSecond" in text
    assert "remainder < 0" in text
    assert "written) >= kPfMaxTimeBufferSize" in text
    assert "if (frame_count) *frame_count = 0" in text
    assert "!start || !step || !frame_count" in text
    assert "step->value <= 0" in text
    assert "static_cast<uint64_t>(start->scale)" in text
    assert "count < INT32_MIN || count > INT32_MAX" in text
