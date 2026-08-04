"""Source-contract tests for the cluster session (issue #405).

Fixes the fail-closed invariants of docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md
across the three components:

- broker: the cluster-manifest bounds and validation, the sealed-root
  staging of the manifest, the swap/discovery session API, and the
  declared-set module audit validated at close — while the one-shot audit
  keeps its fixed native-aligned 512-module cap.
- minihost worker: strict manifest parsing, closure pins with the
  sealed-root LoadLibrary flags, the dedicated swap-failure exit code, the
  discovery session launch mode, and swap epochs in the module audit.
- aviutl2-multifilter bridge: the cluster-fallback diagnostics that never
  round a session failure into a plain success.

Assertions target stable symbols (function/constant/flag names), not
formatting.
"""
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
BROKER = ROOT / "broker" / "crates" / "broker" / "src"
MINIHOST = ROOT / "minihost" / "src"

MANIFEST = (BROKER / "cluster_manifest.rs").read_text(encoding="utf-8")
DISPATCH = (BROKER / "secure_image_dispatch.rs").read_text(encoding="utf-8")
SESSION = source_owners.RENDER_SESSION_SOURCE.read_text(encoding="utf-8")
AUDIT = (BROKER / "worker_module_audit.rs").read_text(encoding="utf-8")

WORKER = source_owners.worker_text()
WORKER_MANIFEST = (MINIHOST / "worker_cluster_manifest.cpp").read_text(
    encoding="utf-8"
)
WORKER_MANIFEST_H = (MINIHOST / "worker_cluster_manifest.hpp").read_text(
    encoding="utf-8"
)
WORKER_RENDER_SESSION = (MINIHOST / "worker_render_session.cpp").read_text(
    encoding="utf-8"
)
WORKER_MODULE_AUDIT = (MINIHOST / "runtime_module_audit.cpp").read_text(
    encoding="utf-8"
)
CLI_DISPATCH = (MINIHOST / "l2_cli_dispatch.cpp").read_text(encoding="utf-8")
L2_MAIN = source_owners.l2_translation_unit_text()
CMAKE = (MINIHOST.parent / "CMakeLists.txt").read_text(encoding="utf-8")

BRIDGE = "\n".join(
    path.read_text(encoding="utf-8")
    for path in (ROOT / "bridges" / "aviutl2-multifilter" / "src").glob("*.rs")
)


# --- broker: cluster-manifest-v1 ------------------------------------------








# --- broker: sessions ------------------------------------------------------


def test_render_session_speaks_swap_plugin():
    assert "pub fn swap_plugin" in SESSION
    assert "pub enum SwapOutcome" in SESSION
    assert '\\"type\\":\\"swap_plugin\\"' in SESSION
    assert '"swap_done"' in SESSION
    # A GLOBAL_SETUP failure stays plugin-local; the session continues.
    assert "PluginError" in SESSION


def test_discovery_session_speaks_inspect_with_a_bounded_report():
    assert "pub struct DiscoverySession" in SESSION
    assert "pub fn inspect_plugin" in SESSION
    assert '"--discovery-session-v1"' in SESSION
    # inspect_done may carry up to 4 MiB (design §4); every other session
    # flavor stays at 64 KiB.
    assert "MAX_DISCOVERY_MESSAGE_BYTES: usize = 4 * 1024 * 1024" in SESSION
    assert "MAX_MESSAGE_BYTES: usize = 64 * 1024" in SESSION
    # The structured parameter-local failure shape (error_kind + optional
    # report), not a free-form payload.
    assert "error_kind" in SESSION
    assert "entrypoint_unresolved" in SESSION
    assert "selector_error" in SESSION








# --- minihost worker -------------------------------------------------------












# --- bridge (aviutl2-multifilter) ------------------------------------------




