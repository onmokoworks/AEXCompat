import json
import os
import subprocess
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
RENDER_SOURCE = ROOT / "minihost" / "src" / "render_subsystem.cpp"
WORLD_SAFETY_SOURCE = ROOT / "minihost" / "src" / "worker_world_safety.cpp"
WORLD_SAFETY_HEADER = ROOT / "minihost" / "src" / "worker_world_safety.hpp"
WORLD_REGISTRY_SOURCE = ROOT / "minihost" / "src" / "worker_world_registry.cpp"
WORLD_REGISTRY_HEADER = ROOT / "minihost" / "src" / "worker_world_registry.hpp"
WORLD_TRANSFORM_RUNTIME = ROOT / "minihost" / "src" / "worker_pf_world_transform_runtime.cpp"
EXTERNAL_RENDER_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_external_render_runtime.cpp"


def _worker() -> Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def test_dispatch_registry_is_tls_scoped_and_fail_closed():
    text = WORLD_SAFETY_SOURCE.read_text(encoding="utf-8")
    header = WORLD_SAFETY_HEADER.read_text(encoding="utf-8")
    main = SOURCE.read_text(encoding="utf-8")
    registry = WORLD_REGISTRY_SOURCE.read_text(encoding="utf-8")
    assert "thread_local std::vector<std::vector<DispatchWorldFormat>>" in text
    assert "class DispatchWorldFormatScope" in header
    assert "entry->world == world" in text
    assert "entry.data == data && entry.rowbytes == rowbytes" in text
    assert "unique->pixel_format != entry.pixel_format" in text
    assert "OwnedWorldResolution::rejected" in text
    assert "const auto exact = g_worlds.find" in registry
    assert "candidate.second.pixels != data" in registry
    assert "OwnedWorldResolution::rejected" in registry
    assert "std::abs(rowbytes) / width" not in text + main + registry


def test_pf_owned_world_registry_is_a_genuine_bounded_component():
    header = WORLD_REGISTRY_HEADER.read_text(encoding="utf-8")
    source = WORLD_REGISTRY_SOURCE.read_text(encoding="utf-8")
    main = SOURCE.read_text(encoding="utf-8")
    cmake = (ROOT / "minihost" / "CMakeLists.txt").read_text(encoding="utf-8")
    assert cmake.count("src/worker_world_registry.cpp") == 1
    assert '#include "worker_world_registry.cpp"' not in main
    assert "std::unordered_map<void*, OwnedWorld> g_worlds" in source
    assert "constexpr std::size_t kMaxWorldCount = 64" in source
    assert "constexpr uint64_t kMaxWorldBytes = 256ULL * 1024 * 1024" in source
    assert "g_live_bytes > kMaxWorldBytes - size" in source
    assert "found == g_worlds.end()" in source
    assert "snapshot_owned_world" in header + source
    assert "configure_gpu_fallback_bridge" in header + source
    assert "configure_host_world_fallback" in source
    assert "g_owned_worlds" not in main
    assert "int32_t __cdecl new_world(" not in main
    assert "configure_host_world_fallback" not in main
    assert "g_aegp_world_views" not in main
    assert "g_platform_worlds" not in main
    assert "std::unordered_map<void**, AegpWorldView> g_aegp_views" in source
    assert "std::unordered_map<void*, PlatformWorldEntry> g_platform_worlds" in source
    assert "g_async_receipts" not in source


def test_pf_world_transform_uses_shared_sdk_argb64_and_argb128_fourcc():
    runtime = WORLD_TRANSFORM_RUNTIME.read_text(encoding="utf-8")
    suites = (ROOT / "minihost" / "src" / "worker_pf_suites.cpp").read_text(
        encoding="utf-8"
    )
    for source in (runtime, suites):
        assert '#include "worker_world_registry.hpp"' in source
        assert "world_registry::kPixelFormatArgb64" in source
        assert "world_registry::kPixelFormatArgb128" in source
        assert "1650946658" not in source
        assert "1650946659" not in source
    assert "kPixelFormatArgb64 = 909206881" in WORLD_REGISTRY_HEADER.read_text(
        encoding="utf-8"
    )
    assert "kPixelFormatArgb128 = 842229089" in WORLD_REGISTRY_HEADER.read_text(
        encoding="utf-8"
    )


def test_owned_snapshot_and_aegp_backing_lock_boundaries_are_explicit():
    source = WORLD_REGISTRY_SOURCE.read_text(encoding="utf-8")
    snapshot = source[source.index("bool snapshot_owned_world("):
                      source.index("int32_t aegp_world_type_from_format(")]
    assert snapshot.index("std::lock_guard<std::mutex> lock(g_mutex)") < snapshot.index(
        "std::memcpy(&descriptor, world, sizeof(descriptor))")
    assert snapshot.index("std::memcpy(&descriptor, world, sizeof(descriptor))") < snapshot.index(
        "descriptor.data != found->second.pixels")
    blur = source[source.index("int32_t __cdecl aegp_world_fast_blur("):
                  source.index("int32_t __cdecl aegp_world_new_platform(")]
    assert blur.index("snapshot_aegp_view(handle, view, world)") < blur.index(
        "pixels_lock(view.platform_backing->pixels_mutex)")
    assert "registry mutex while waiting for mutable pixel access" in blur
    assert "g_live_owned_aegp_backings.fetch_sub(1)" in source
    assert "g_live_owned_aegp_backings.load() >= kMaxOwnedAegpWorlds" in source


def test_aegp_world_and_platform_ownership_live_in_registry_not_l2():
    header = WORLD_REGISTRY_HEADER.read_text(encoding="utf-8")
    source = WORLD_REGISTRY_SOURCE.read_text(encoding="utf-8")
    main = SOURCE.read_text(encoding="utf-8")
    for callback in (
        "aegp_world_new_owned", "aegp_world_dispose", "aegp_world_get_type",
        "aegp_world_get_size", "aegp_world_get_rowbytes",
        "aegp_world_get_base_addr8", "aegp_world_fill_pf_world",
        "aegp_world_fast_blur", "aegp_world_new_platform",
        "aegp_world_dispose_platform", "aegp_world_reference_platform",
    ):
        assert f"int32_t __cdecl {callback}(" in source
        assert f"int32_t __cdecl {callback}(" not in main
    for api in (
        "register_borrowed_view", "unregister_borrowed_view",
        "snapshot_aegp_world", "snapshot_platform_world",
        "adopt_platform_world", "aegp_lifetimes_balanced",
    ):
        assert api in header + source
    receipts = (ROOT / "minihost" / "src" / "worker_render_receipts.cpp").read_text(
        encoding="utf-8")
    assert "ReceiptDraft" in main
    external = EXTERNAL_RENDER_RUNTIME.read_text(encoding="utf-8")
    assert "struct ExternalRenderedFrame" in external
    assert "g_receipts" not in main
    assert "g_receipts" in receipts
    assert "std::vector<ExternalRenderedFrame> g_cache" in external
    assert "ReceiptDraft" not in source
    assert "ExternalRenderedFrame" not in source


def test_effect_world_abi_and_bounds_live_in_world_safety_component():
    header = WORLD_SAFETY_HEADER.read_text(encoding="utf-8")
    source = WORLD_SAFETY_SOURCE.read_text(encoding="utf-8")
    assert "sizeof(LocalEffectWorld) == kEffectWorldSize" in header
    for offset in ("world_flags) == 16", "data) == 24", "rowbytes) == 32",
                   "extent_hint) == 44", "pix_aspect_ratio) == 88"):
        assert offset in header
    for bound in ("width <= 4096", "height <= 4096", "16'777'216",
                  "rowbytes >= width * pixel_bytes", "rowbytes <= 4096 * 16"):
        assert bound in source
    assert "pixel_bytes == 4 ? (flags & 1) == 0 : (flags & 1) != 0" in source


def test_classic_and_smart_dispatch_register_host_worlds_and_resizes():
    smart_dispatch = (ROOT / "minihost" / "src" / "worker_smart_dispatch.cpp").read_text(
        encoding="utf-8"
    )
    text = (SOURCE.read_text(encoding="utf-8") +
            (SOURCE.parent / "worker_classic_render_runtime.cpp").read_text(encoding="utf-8") +
            WORLD_TRANSFORM_RUNTIME.read_text(encoding="utf-8") + smart_dispatch)
    render = RENDER_SOURCE.read_text(encoding="utf-8")
    assert text.count("DispatchWorldFormatScope dispatch_worlds;") >= 3
    assert text.count("register_world(output_world.data(), dispatch_pixel_format)") >= 1
    assert smart_dispatch.count("register_world(request.output_world->data(),") >= 2
    # Connected-map storage moved into the render subsystem; L2 retains only
    # the per-dispatch registration of that owned world.
    assert "bool prepare_connected_map_world" in render
    assert "prepare_world_layout(map.world" in render
    assert "register_world(map_world.world.data(), kPixelFormatArgb32)" in text
    assert "register_world(world.data(), dispatch_pixel_format)" in text
    assert "register_world(request.output_world->data()," in smart_dispatch
    assert "world_registry::kPixelFormatGpuBgra128" in smart_dispatch


def test_composite16_runtime_provenance_and_concurrency_matrix():
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the runtime test"
    completed = subprocess.run(
        [worker, "--self-test-world-transform-composite"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert json.loads(completed.stdout) == {"world_transform_composite_rect": "passed"}


def test_pf_world_registry_rejects_double_dispose_and_oversized_allocations():
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the runtime test"
    completed = subprocess.run(
        [worker, "--self-test-pf-world-registry"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert json.loads(completed.stdout) == {
        "pf_world_registry": "passed",
        "double_dispose_rejected": True,
        "allocation_limit_rejected": True,
        "owned_snapshot_atomic": True,
        "concurrent_snapshot_dispose": True,
        "live_count": 0,
        "live_bytes": 0,
    }
