import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCES = source_owners.contract_files("pf_ae_adv_item_suite")
PROBE = ROOT / "instruments/pf-ae-adv-item-probe/pf_ae_adv_item_probe.cpp"
BUILD = ROOT / "tools/build-pf-ae-adv-item-probe.ps1"


def test_adv_item_v1_exact_name_version_abi_and_slot_order():
    text = "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)
    assert '{"PF AE Adv Item Suite", 1, &g_adv_item_suite1' in text
    assert "&render_worker_suite_provider_available" in text
    assert "sizeof(PfAdvItemSuite1) == 5 * sizeof(void*)" in text
    slots = ["move_time_step", "move_time_step_active_item", "touch_active_item",
             "force_rerender", "effect_is_active_or_enabled"]
    for index, slot in enumerate(slots):
        assert f"offsetof(PfAdvItemSuite1, {slot}) == {index} * sizeof(void*)" in text


def test_adv_item_callbacks_are_fail_closed_and_headless_policy_is_explicit():
    text = "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)
    for marker in ("direction != 0 && direction != 1", "steps < 0", "INT32_MIN",
                   "INT32_MAX", "active_adv_item_context(in_data)",
                   "active_adv_item_world(world, effect_world)", "!effect_world.data",
                   "entry.world == world", "data != match->data",
                   "bump_render_project_timestamp()", "if (enabled) *enabled = 0",
                   "without dereferencing an untrusted or stale handle"):
        assert marker in text
    assert "g_pf_adv_item_active_time" not in text
    assert "context.active_item_time_valid ? context.active_item_time" in text
    assert "std::memcpy(context.input->data() + kInCurrentTime" not in text


def test_probe_builds_and_fixes_all_five_sdk_slots_and_lease_balance():
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
                    "-File", str(BUILD)], cwd=ROOT, check=True, timeout=180)
    assert (ROOT / "target/pf-ae-adv-item-probe-build/Release/pf_ae_adv_item_probe.aex").is_file()
    text = PROBE.read_text(encoding="utf-8")
    for marker in ("kPFAdvItemSuite", "kPFAdvItemSuiteVersion1", "PF_AdvItemSuite1",
                   "PF_MoveTimeStep", "PF_MoveTimeStepActiveItem", "PF_TouchActiveItem",
                   "PF_ForceRerender", "PF_EffectIsActiveOrEnabled", "invalid_direction",
                   "negative_steps", "null_inputs", "AcquireSuite", "ReleaseSuite",
                   "lease_balanced", "sizeof(PF_AdvItemSuite1) == 5 * sizeof(void*)"):
        assert marker in text
