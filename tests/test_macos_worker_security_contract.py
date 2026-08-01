from pathlib import Path
import plistlib


ROOT = Path(__file__).resolve().parents[1]


def source(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def test_macos_controller_keeps_process_and_resource_bounds_explicit():
    controller = source("broker/crates/harness/src/macos_worker_controller.rs")
    for contract in (
        '.current_dir(&self.root)',
        ".env_clear()",
        ".process_group(0)",
        "RLIMIT_CPU",
        "RLIMIT_NOFILE",
        "RLIMIT_FSIZE",
        "proc_pid_rusage",
        "proc_listchildpids",
        "MAX_RESIDENT_BYTES",
        "MAX_STDOUT_BYTES",
        "MAX_STDERR_BYTES",
        "MAX_SESSION_FILES",
        "MAX_SESSION_BYTES",
        "SIGTERM",
        "SIGKILL",
        "macos_worker_residual_process",
    ):
        assert contract in controller


def test_macos_tiers_deadlines_and_protocol_bound_are_not_common_schema_changes():
    macos = source("broker/crates/harness/src/macos.rs")
    controller = source("broker/crates/harness/src/macos_worker_controller.rs")
    assert "apple_silicon_unicorn_guest" in controller
    assert "apple_silicon_native_carrier_trusted_only" in controller
    assert "MAX_RESIDENT_PROTOCOL_BYTES: usize = 1024 * 1024" in macos
    assert "RESIDENT_RENDER_DEADLINE: Duration = Duration::from_secs(30)" in macos
    assert "RESIDENT_CLOSE_DEADLINE: Duration = Duration::from_secs(2)" in macos
    assert 'var_os("AEXCOMPAT_NATIVE_CARRIER")' in macos
    assert 'var_os("AEXCOMPAT_NATIVE_CARRIER_TRUSTED")' in macos


def test_hardened_runtime_entitlements_are_minimal_and_distinct():
    arm = plistlib.loads((ROOT / "tools/macos/arm64-unicorn.entitlements").read_bytes())
    native = plistlib.loads(
        (ROOT / "tools/macos/x86_64-native-carrier.entitlements").read_bytes()
    )
    assert arm == {
        "com.apple.security.cs.allow-jit": True,
        "com.apple.security.cs.allow-unsigned-executable-memory": True,
    }
    assert native == {"com.apple.security.cs.allow-unsigned-executable-memory": True}
    forbidden = {
        "com.apple.security.get-task-allow",
        "com.apple.security.cs.disable-library-validation",
        "com.apple.security.cs.disable-executable-page-protection",
        "com.apple.security.cs.allow-dyld-environment-variables",
    }
    assert forbidden.isdisjoint(arm)
    assert forbidden.isdisjoint(native)


def test_guest_execution_admission_is_fail_closed():
    pe = source("guest/crates/aex-guest-worker/src/pe.rs")
    unicorn = source("guest/crates/aex-guest-worker/src/x64/imports.rs")
    native = source("guest/crates/aex-guest-worker/src/native_x64/imports.rs")
    native_engine = source("guest/crates/aex-guest-worker/src/native_x64.rs")
    assert "WritableExecutableSection" in pe
    assert "LegacyZero" not in unicorn
    assert "UnsupportedLegacyImport" in unicorn
    assert "native_import_is_implemented" in native
    assert "_dyld_image_count" in native_engine
    assert "native guest loaded unexpected Mach-O images" in native_engine
