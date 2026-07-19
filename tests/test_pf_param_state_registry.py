import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"
STATE_SOURCE = ROOT / "minihost" / "src" / "worker_pf_state_runtime.cpp"
SELFTEST_SOURCE = ROOT / "minihost" / "src" / "worker_parameter_selftests.cpp"


def source_text():
    return "\n".join(path.read_text(encoding="utf-8") for path in
                     (STATE_SOURCE, SOURCE, SELFTEST_SOURCE))


def _workers():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target/minihost-build-v18/Release/aex_render_worker.exe",
        ROOT / "target/minihost-build-v18/aex_render_worker.exe",
        ROOT / "target/minihost-build-vs2022/Release/aex_render_worker.exe",
        ROOT / "target/minihost-build-vs2022/aex_render_worker.exe",
    ]
    return [path for path in candidates if path and path.is_file()]


def test_pf_state_is_an_opaque_bounded_registry_token():
    source = source_text()
    for marker in (
        "BCryptGenRandom(nullptr, token.data()",
        "BCRYPT_USE_SYSTEM_PREFERRED_RNG",
        "kMaxPfStateRegistryEntries = 4096",
        "g_pf_state_registry_mutex",
        "canonical_snapshot",
        "g_pf_state_effect_generation",
        "g_pf_state_effect_live",
        "purge_pf_state_registry_locked(nullptr)",
        "reset_effect_lifetime(true)",
        "invoke_global_setdown(entry",
    ):
        assert marker in source
    state_section = source.split("using PfStateToken", 1)[1].split(
        "int32_t __cdecl is_identical_param_checkout", 1
    )[0]
    assert "param_state_hash" not in state_section
    assert "std::memcmp(first, second, sizeof(PfState))" not in state_section


def test_pf_state_comparison_fails_closed_and_preserves_outputs():
    source = source_text()
    comparison = source.split("int32_t __cdecl are_param_states_identical", 1)[1].split(
        "int32_t __cdecl get_current_param_state_obsolete", 1
    )[0]
    assert "g_pf_state_registry.find(first_token)" in comparison
    assert "g_pf_state_registry.find(second_token)" in comparison
    assert "left->second.owner != owner" in comparison
    assert "left->second.generation != g_pf_state_effect_generation" in comparison
    assert "!g_pf_state_effect_live" in comparison
    assert comparison.index("return kPfBadCallbackParam") < comparison.index("*same =")
    for adversary in ("PfState zero{}, random{}", "bit_flip", "&g_layer"):
        assert adversary in source


def test_pf_state_registry_native_adversarial_self_test():
    workers = _workers()
    assert workers, "build a VS2022 render worker before running the native test"
    for worker in workers:
        completed = subprocess.run(
            [str(worker), "--self-test-pf-param-utils-suite"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert completed.stdout.strip() == '{"pf_param_utils_suite3":"passed"}'
