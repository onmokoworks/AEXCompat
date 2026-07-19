import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "PF_NOP_RENDER_PASSTHROUGH_RESULT_2026-07-15.json"
WORKER = source_owners.L2_MAIN
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "main.rs"
FIXTURE = ROOT / "instruments" / "pf-nop-render-probe" / "pf_nop_render_probe.cpp"


def result():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_nop_render_preserves_sequence_but_skips_all_frame_selectors():
    render = result()["classic_argb8"]
    assert render["nop_render_advertised"] is True
    assert render["selector_order"] == [
        "global_setup", "params_setup", "sequence_setup",
        "sequence_setdown", "global_setdown",
    ]
    assert render["frame_setup_dispatched"] is False
    assert render["render_selector_dispatched"] is False
    assert render["frame_setdown_dispatched"] is False
    assert render["render_performed"] is False
    assert render["render_error"] == 0


def test_nop_render_is_an_exact_guarded_source_passthrough():
    render = result()["classic_argb8"]
    assert render["input_sha256"] == render["output_sha256"]
    assert render["guard_bytes_intact"] is True
    assert render["handle_lifetimes_balanced"] is True
    assert render["world_lifetimes_balanced"] is True
    assert render["suite_leases_balanced"] is True
    assert render["global_setdown_error"] == 0


def test_smartfx_nop_render_skips_pre_render_and_smart_render():
    render = result()["smartfx_argb8"]
    assert render["smart_render_supported"] is True
    assert render["nop_render_advertised"] is True
    assert render["selector_order"] == [
        "global_setup", "params_setup", "sequence_setup",
        "sequence_setdown", "global_setdown",
    ]
    assert render["frame_setup_dispatched"] is False
    assert render["smart_pre_render_dispatched"] is False
    assert render["smart_render_selector_dispatched"] is False
    assert render["frame_setdown_dispatched"] is False
    assert render["render_performed"] is False
    assert render["pre_render_error"] == render["smart_render_error"] == 0
    assert render["result_rect"] == render["max_result_rect"] == [0, 0, 16, 12]
    assert render["input_sha256"] == render["output_sha256"]
    assert render["guard_bytes_intact"] is True


def test_nop_render_is_cleanroom_abi_bound_and_exposed():
    evidence = result()
    worker = source_owners.worker_text()
    broker = BROKER.read_text(encoding="utf-8")
    harness = HARNESS.read_text(encoding="utf-8")
    fixture = FIXTURE.read_text(encoding="utf-8")
    assert evidence["abi"]["nop_render_flag"] == 262144
    assert "PF_OutFlag_NOP_RENDER" in fixture
    assert "case PF_Cmd_RENDER:" in fixture
    assert "case PF_Cmd_SMART_PRE_RENDER:" in fixture
    assert "case PF_Cmd_SMART_RENDER:" in fixture
    assert "constexpr uint32_t kOutFlagNopRender = 1u << 18;" in worker
    assert "nop_render_advertised" in worker
    assert "pub fn probe_experimental_nop_render" in broker
    assert "pub fn probe_experimental_smart_nop_render" in broker
    assert 'args[1] == "--probe-experimental-nop-render"' in harness
    assert "Probe NOP_RENDER passthrough" in harness
    assert "Probe SmartFX NOP_RENDER passthrough" in harness
