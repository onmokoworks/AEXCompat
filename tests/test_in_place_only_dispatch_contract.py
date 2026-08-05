from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_multifilter_has_no_staged_discovery_or_render_switch():
    production = "\n".join(
        path.read_text(encoding="utf-8")
        for path in (
            ROOT / "bridges/aviutl2-multifilter/src/discovery_inspection.rs",
            ROOT / "bridges/aviutl2-multifilter/src/discovery_cache.rs",
            ROOT / "bridges/aviutl2-multifilter/src/runtime.rs",
        )
    )
    assert "AEXCOMPAT_MULTIFILTER_STAGED_DISCOVERY" not in production
    assert "AEXCOMPAT_MULTIFILTER_STAGED_RENDER" not in production
    assert "prepare_discovery_in_place(plugin, dependency, build)" in production
    assert "dependency_search_dirs = roots.clone()" in production


def test_secure_image_dispatch_no_longer_selects_staging_from_empty_search_dirs():
    dispatch = (
        ROOT / "broker/crates/broker/src/secure_image_dispatch.rs"
    ).read_text(encoding="utf-8")
    assert "if !input.dependency_search_dirs.is_empty()" not in dispatch
    assert dispatch.count("joined_dependency_search_dirs(&input.dependency_search_dirs)?") >= 2
    assert "SealedLoadTree::create(main, dependencies)" not in dispatch
    assert "SealedLoadTree::create_with_resources(main, dependencies, resources)" not in dispatch


def test_resident_sessions_expose_only_in_place_opening():
    render = (ROOT / "broker/crates/broker/src/render_session.rs").read_text(
        encoding="utf-8"
    )
    discovery = (
        ROOT / "broker/crates/broker/src/render_session/discovery.rs"
    ).read_text(encoding="utf-8")
    assert "an in-place session requires dependency search directories" in render
    assert "pub fn open_in_place(" in discovery
    assert "pub struct DiscoverySessionOpenRequest" not in discovery
    assert "pub fn open(" not in discovery


def test_system_gpu_probes_do_not_create_dummy_sealed_trees():
    for relative in (
        "broker/crates/broker/src/cuda_compute_probe.rs",
        "broker/crates/broker/src/opencl_runtime_probe.rs",
    ):
        source = (ROOT / relative).read_text(encoding="utf-8")
        assert "probe_guard_tree" not in source
        assert "secure_launch_without_plugin" in source


def test_cluster_transport_is_v2_only_in_broker_and_worker():
    production = "\n".join(
        (ROOT / relative).read_text(encoding="utf-8")
        for relative in (
            "broker/crates/broker/src/secure_image_dispatch.rs",
            "minihost/src/l2_cli_dispatch.cpp",
            "minihost/src/worker_invocation_orchestration.cpp",
            "minihost/src/worker_cluster_manifest.cpp",
            "broker/crates/dummy-workers/src/bin/session_protocol_worker.rs",
        )
    )
    assert "--cluster-manifest-v1" not in production
    assert "cluster-manifest-v2" in production
    assert "schema != \"cluster-manifest-v2\"" in production
