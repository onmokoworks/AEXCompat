import json
import os
import pathlib
import subprocess


ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
RECEIPTS = ROOT / "minihost" / "src" / "worker_render_receipts.cpp"


def _worker() -> pathlib.Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        pathlib.Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)


def test_async_and_render_suites_have_typed_frozen_abi():
    text = SOURCE.read_text(encoding="utf-8")
    assert "struct AegpRenderAsyncManagerSuite1" in text
    assert "sizeof(AegpRenderAsyncManagerSuite1) == 2 * sizeof(void*)" in text
    assert "struct AegpRenderSuite4" in text
    assert "sizeof(AegpRenderSuite4) == 12 * sizeof(void*)" in text
    assert 'std::strcmp(name, "AEGP Render Suite") == 0 && version == 5' in text
    assert "&checkin_frame, &get_receipt_world" in text
    assert "g_render_async_manager_suite1.fill" not in text


def test_receipt_registry_is_bounded_and_invalidates_borrowed_world_first():
    text = RECEIPTS.read_text(encoding="utf-8")
    for marker in (
        "g_receipts.size() < kMaxReceiptCount",
        "g_live_bytes + g_reserved_bytes <= kMaxReceiptBytes - bytes",
        "std::unordered_map<void*, std::unique_ptr<Receipt>> g_receipts",
        "world_registry::unregister_borrowed_view(",
        "receipt = g_receipts.extract(found);",
        "g_live_bytes -= receipt.mapped()->draft->pixels.size();",
    ):
        assert marker in text
    checkin = text[text.index("int32_t checkin(void* handle)"):
                   text.index("bool checkin_if_live(void* handle)")]
    assert checkin.index("g_receipts.extract(found)") < checkin.index(
        "unregister_borrowed_view(")


def test_receipt_and_borrowed_world_handles_are_opaque_and_never_reused():
    text = RECEIPTS.read_text(encoding="utf-8") + SOURCE.read_text(encoding="utf-8")
    for marker in (
        "g_receipt_generation{1}",
        "g_world_generation{1}",
        "assign_handles",
        "receipt_generation << 3",
        "world_generation << 3",
        "receipt_handles.insert(receipt).second",
        "world_handles.insert(world).second",
        "aegp_world_get_type(stale_world, &type) == 0",
    ):
        assert marker in text
    assert "void* key = receipt.get();" not in text
    assert "void* world_token" not in text


def test_async_ready_receipt_runtime_lifecycle():
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the focused runtime test"
    result = subprocess.run(
        [str(worker), "--self-test-aegp-async-receipt"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stderr or result.stdout
    report = json.loads(result.stdout)
    assert report["aegp_async_receipt"] == "passed"
    assert report["created"] == report["checked_in"] == 4
    assert report["live"] == 0
    assert report["live_bytes"] == 0
    assert report["invalid_operations"] >= 4
