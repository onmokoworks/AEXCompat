import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "PF_INPUT_BUFFER_WRITE_RESULT_2026-07-15.json"
WORKER = source_owners.L2_MAIN
PIXEL_BUFFER = ROOT / "minihost" / "src" / "render_pixel_buffer.cpp"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "main.rs"
FIXTURE = ROOT / "instruments" / "pf-input-write-probe" / "pf_input_write_probe.cpp"


def result():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_advertised_input_write_mutates_a_writable_private_source_copy():
    write = result()["advertised_write"]
    assert write["input_write_advertised"] is True
    assert write["input_buffer_writable"] is True
    assert write["render_error"] == 0
    assert write["input_sha256"] != write["output_sha256"]
    assert write["mutated_first_argb_pixel"] == [255, 17, 34, 51]
    assert write["guard_bytes_intact"] is True
    assert write["handle_lifetimes_balanced"] is True
    assert write["world_lifetimes_balanced"] is True


def test_unadvertised_input_write_is_page_faulted_inside_worker():
    denied = result()["unadvertised_write"]
    assert denied["input_write_advertised"] is False
    assert denied["input_buffer_writable"] is False
    assert denied["classification"] == "crashed"
    assert denied["windows_exit_code"] == 0xC0000005
    assert denied["windows_exception"] == "0xC0000005"
    assert denied["failure_stage"] == "render"
    assert denied["last_completed_stage"] == "frame_setup"
    assert denied["broker_failed_safely"] is True


def test_smartfx_input_checkout_honors_the_same_write_permission():
    write = result()["smartfx_advertised_write"]
    assert write["smart_render_supported"] is True
    assert write["input_write_advertised"] is True
    assert write["input_buffer_writable"] is True
    assert write["pre_render_error"] == write["smart_render_error"] == 0
    assert write["input_checkout_request"] == [0, 0, 16, 12]
    assert write["input_sha256"] == write["output_sha256"]
    assert write["mutated_first_argb_pixel"] == [255, 17, 34, 51]
    assert write["result_rect"] == write["max_result_rect"] == [0, 0, 16, 12]
    assert write["guard_bytes_intact"] is True


def test_smartfx_unadvertised_write_is_isolated_after_pre_render():
    denied = result()["smartfx_unadvertised_write"]
    assert denied["input_write_advertised"] is False
    assert denied["input_buffer_writable"] is False
    assert denied["classification"] == "crashed"
    assert denied["windows_exit_code"] == 0xC0000005
    assert denied["failure_stage"] == "smart_render_cpu"
    assert denied["last_completed_stage"] == "smart_pre_render"
    assert denied["broker_failed_safely"] is True


def test_input_write_boundary_is_cleanroom_abi_bound_and_exposed():
    evidence = result()
    worker = source_owners.worker_text()
    pixel_buffer = PIXEL_BUFFER.read_text(encoding="utf-8")
    broker = BROKER.read_text(encoding="utf-8")
    harness = HARNESS.read_text(encoding="utf-8")
    fixture = FIXTURE.read_text(encoding="utf-8")
    assert evidence["abi"]["i_write_input_buffer_flag"] == 2048
    assert "PF_OutFlag_I_WRITE_INPUT_BUFFER" in fixture
    assert "InputPixelBuffer::set_plugin_writable" in pixel_buffer
    assert "PAGE_READONLY" in pixel_buffer and "PAGE_READWRITE" in pixel_buffer
    assert "kOutFlagIWriteInputBuffer = 1u << 11" in worker
    assert "pub fn probe_experimental_input_buffer_write" in broker
    assert "pub fn probe_experimental_smart_input_buffer_write" in broker
    assert 'args[1] == "--probe-experimental-input-buffer-write"' in harness
    assert "Probe input-buffer write access" in harness
    assert "Probe SmartFX input-buffer write access" in harness
