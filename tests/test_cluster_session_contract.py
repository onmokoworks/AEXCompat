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


def test_cluster_manifest_enforces_the_documented_bounds():
    """Design §2.1: exceeding any bound must fail the cluster session at
    build time (fail-closed to the per-plugin path), so the caps live in the
    validator, not in a comment."""
    assert "MAX_CLUSTER_PLUGINS: usize = 256" in MANIFEST
    assert "MAX_CLUSTER_MODULE_BOUND: u32 = 4096" in MANIFEST
    assert "MAX_CLUSTER_MANIFEST_BYTES: usize = 4 * 1024 * 1024" in MANIFEST
    assert "MAX_CLUSTER_PAYLOAD_BYTES: usize = 16384" in MANIFEST
    # The bounds are enforced, not merely declared.
    assert "cluster plugin count is outside 1..=256" in MANIFEST
    assert "cluster module bound is outside 1..=4096" in MANIFEST
    assert "cluster manifest body is outside 1 byte..4 MiB" in MANIFEST


def test_cluster_manifest_reuses_the_dependency_manifest_rules():
    """Basename and SHA-256 rules must be exactly those of
    session_dependency_manifest (design §2.1); a second, looser ruleset would
    widen the launch-time trust decision."""
    assert "validate_windows_basename" in MANIFEST
    assert "decode_sha256" in MANIFEST
    assert MANIFEST.count("deny_unknown_fields") >= 3
    assert '"cluster-manifest-v1"' in MANIFEST


def test_cluster_manifest_is_staged_inside_the_sealed_root():
    """Design §2.3: the worker pins the manifest's parent directory against
    the sealed root, so the broker must stage the document into the sealed
    tree and pass the staged path — never a broker temp path."""
    assert 'CLUSTER_MANIFEST_SEALED_BASENAME: &str = "cluster-manifest-v1.json"' in MANIFEST
    assert "CLUSTER_MANIFEST_SEALED_BASENAME" in DISPATCH
    assert "SealedLoadTree::create_cluster" in DISPATCH
    assert '"--cluster-manifest-v1"' in DISPATCH
    # The argv path is the staged copy inside the tree root, not the
    # broker-owned staging source.
    staged = DISPATCH[DISPATCH.index("let staged_manifest"):]
    assert "tree" in staged.split(";")[0]


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


def test_cluster_audit_is_validated_at_close_against_the_declared_set():
    """Design §5: the fixed one-shot cap is replaced by the launch-manifest
    declaration (plugins ∪ dependencies, module_bound), validated when the
    session closes — for both the render and the discovery session."""
    assert "pub struct ClusterAuditDeclaration" in AUDIT
    assert "pub fn validate_cluster_worker_audit" in AUDIT
    assert "epochs" in AUDIT
    assert "secure worker module audit carries an undeclared plugin-class module" in AUDIT
    # Render close and discovery close both run the cluster validator.
    assert SESSION.count("validate_cluster_worker_audit") >= 2


def test_one_shot_audit_keeps_the_fixed_cap():
    """The declared-set model applies to cluster sessions only; the one-shot
    validator keeps the bounded native producer capacity after #478."""
    assert "pub const MAX_AUDITED_MODULES: usize = 512;" in AUDIT
    assert "pub fn validate_required_worker_audit" in AUDIT


def test_broker_validates_driverstore_in_both_audit_models():
    """The OS DriverStore category (issue #362) rides every validator rule
    the other location categories ride: module-count, cross-category
    duplicate rejection, and the observed-union subset, on both the one-shot
    and the cluster path. `#[serde(default)]` keeps reports from pre-#362
    workers (which have no such field) valid."""
    assert "driverstore: Vec<String>" in AUDIT
    assert "absent in reports from pre-#362 workers" in AUDIT
    # Duplicate-rejection chains and module counts in both validators.
    assert AUDIT.count(".chain(&snapshot.driverstore)") == 2
    assert AUDIT.count("+ snapshot.driverstore.len()") == 2
    # Union subset on the one-shot path (terminal snapshots) and the cluster
    # path (terminal and epoch snapshots alike).
    compact = "".join(AUDIT.split())
    assert compact.count(
        "require_subset(&audit.post_load.driverstore,&audit.observed_union.driverstore,"
    ) == 1
    assert compact.count(
        "require_subset(&audit.pre_unload.driverstore,&audit.observed_union.driverstore,"
    ) == 1
    assert compact.count(
        "require_subset(&snapshot.driverstore,&audit.observed_union.driverstore)?;"
    ) == 1
    # The limit-exceeded diagnostics disclose the category like the others.
    assert '"driverstore": snapshot.driverstore.len(),' in AUDIT
    assert '"driverstore": samples(&snapshot.driverstore),' in AUDIT


# --- minihost worker -------------------------------------------------------


def test_worker_manifest_is_strictly_parsed_and_bounded():
    assert CMAKE.count("src/worker_cluster_manifest.cpp") == 1
    assert '#include "strict_json.hpp"' in WORKER_MANIFEST
    assert "json_exact_keys" in WORKER_MANIFEST
    assert '"cluster-manifest-v1"' in WORKER_MANIFEST
    assert "kMaxPlugins = 256" in WORKER_MANIFEST_H


def test_closure_pins_hold_dependencies_with_the_sealed_root_flags():
    """Design §3: dependencies are pinned with an explicit LoadLibraryExW per
    manifest entry using the same flags as the current admission path, and
    the pins live for the session so a plug-in FreeLibrary cannot unload the
    shared closure."""
    assert "class ClosurePins" in WORKER_MANIFEST_H
    assert "LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR" in WORKER_MANIFEST
    assert "LOAD_LIBRARY_SEARCH_SYSTEM32" in WORKER_MANIFEST
    assert "ClosurePins::release" in WORKER_MANIFEST


def test_swap_failure_has_a_dedicated_exit_code():
    """Design §7: a swap failure (quiesce/unload/audit) is diagnosed apart
    from a protocol violation (23) and an invariant failure (24)."""
    assert "return finish_session(25);" in L2_MAIN
    assert "return session.finish_integrated_report(25);" in L2_MAIN
    assert '"swap_plugin"' in WORKER_RENDER_SESSION
    assert "swap_done" in WORKER_RENDER_SESSION
    assert "swap_failure" in WORKER_RENDER_SESSION


def test_discovery_session_is_a_launch_mode_with_inspect_messages():
    assert 'L"--discovery-session-v1"' in CLI_DISPATCH
    assert "discovery_session_mode" in CLI_DISPATCH
    # The inspect exchange lives somewhere in the worker implementation
    # (today the discovery loop in l2_main.cpp); resolve through the owner
    # manifest so a TU extraction cannot break the contract.
    assert '"inspect_plugin"' in WORKER
    assert "inspect_done" in WORKER


def test_module_audit_records_swap_epochs_under_the_declared_bound():
    """Design §5: the audit gains per-swap epochs, and the fixed
    kMaxAuditedModules is replaced by the launch manifest's module_bound in
    cluster sessions only — the fixed cap stays the default otherwise."""
    assert "epochs" in WORKER_MODULE_AUDIT
    assert "configure_module_audit_cluster" in WORKER_MODULE_AUDIT
    assert "kMaxAuditedModules = 512" in WORKER_MODULE_AUDIT
    assert "audit_module_bound" in WORKER_MODULE_AUDIT


# --- bridge (aviutl2-multifilter) ------------------------------------------


def test_bridge_clusters_only_on_a_normalized_closure_identity():
    """Clustering keys on the closure content (sorted basename:sha256 pairs),
    never on paths or folders, so an identity match really means one shared
    closure."""
    assert "fn closure_identity_of" in BRIDGE
    assert "expected_sha256" in BRIDGE
    assert "to_lowercase()" in BRIDGE
    assert "fn plan_tasks" in BRIDGE
    assert "DiscoverySession" in BRIDGE


def test_bridge_never_rounds_a_cluster_failure_into_a_success():
    """Design §6: the member the session died on is recorded as a structured
    failure, and every member re-inspected per-plugin carries the fallback
    note — a fallback can never look like an ordinary success."""
    assert "cluster_fallback" in BRIDGE
    assert '"cluster_session_invalidated"' in BRIDGE
    assert '"invalidated"' in BRIDGE
    assert '"one_shot_fallback"' in BRIDGE
    assert "at_member" in BRIDGE
    assert "fn fallback_members" in BRIDGE
    # The fallback actually re-inspects through the per-plugin path.
    fallback = BRIDGE[BRIDGE.index("fn fallback_members"):]
    assert "finish_one_shot" in fallback[: fallback.index("\nfn ") if "\nfn " in fallback else len(fallback)]
    # A close-time audit rejection redoes session-inspected members per-plugin.
    assert "session_clean" in BRIDGE
