import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "instruments/pf-batch-sampling-oracle/pf_batch_sampling_oracle.cpp"
RESOURCE = ROOT / "instruments/pf-batch-sampling-oracle/pf_batch_sampling_oracle.rc"
BUILD = ROOT / "tools/build-pf-batch-sampling-oracle.ps1"
RUNNER = ROOT / "tools/ae-batch-sampling-oracle-run.jsx"


def test_probe_uses_only_public_batch_suite_entry_points_safely():
    source = SOURCE.read_text(encoding="utf-8")
    for member in ("begin_sampling", "end_sampling", "get_batch_func", "get_batch_func16"):
        assert f"suite->{member}" in source
    assert "batch_pointer_invoked\\\":false" in source
    assert "reinterpret_cast<PF_Batch" not in source
    assert "if (begin_err == PF_Err_NONE)" in source
    assert "ChangedBytes(before, after_begin)" in source
    assert "ChangedBytes(after_begin, after_end)" in source


def test_pipl_and_runner_register_the_same_effect():
    resource = RESOURCE.read_text(encoding="utf-8")
    runner = RUNNER.read_text(encoding="utf-8")
    assert "AEXCompat PF Batch Sampling V1" in resource
    assert 'addProperty("AEXCompat PF Batch Sampling V1")' in runner
    assert '"EffectMain\\0\\0"' in resource
    assert '32, 0x0, "\\x1DPF Batch Sampling Suite Probe\\0\\0"' in resource
    assert '32, 0x0, "\\x1EAEXCompat PF Batch Sampling V1\\0"' in resource


def test_probe_builds_against_installed_public_sdk():
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(BUILD)],
        cwd=ROOT, check=True, timeout=180,
    )
    artifact = ROOT / "target/pf-batch-sampling-oracle-build/Release/pf_batch_sampling_oracle.aex"
    assert artifact.stat().st_size > 0
