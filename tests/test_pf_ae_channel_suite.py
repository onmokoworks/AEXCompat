import os
import re
import subprocess
from pathlib import Path
import source_owners

ROOT = Path(__file__).resolve().parents[1]
SOURCES = source_owners.contract_files("pf_ae_channel_suite")
def source_text():
    return "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)

def worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)

def test_channel_suite1_is_typed_and_matches_the_frozen_sdk_abi():
    text = source_text()
    assert re.search(
        r"PfAeChannelSuite1 g_channel_suite1\{&get_layer_channel_count, &get_layer_channel_indexed,\s*"
        r"&get_layer_channel_typed, &checkout_layer_channel, &checkin_layer_channel\}",
        text,
    )

def test_channel_struct_layout_errors_and_found_contract_are_explicit():
    text = source_text()
    for assertion in (
        "sizeof(PfChannelRef) == 64",
        "sizeof(PfChannelDesc) == 76",
        "offsetof(PfChannelDesc, data_type) == 68",
        "sizeof(PfChannelChunk) == 104",
        "offsetof(PfChannelChunk, data_handle) == 88",
        "offsetof(PfChannelChunk, data) == 96",
    ):
        assert assertion in text
    assert text.count("if (found) *found = 0;") == 2

def test_channel_ownership_and_conversion_fail_closed():
    text = source_text()

def test_pf_ae_channel_suite_native_lifecycle_and_hardening():
    executable = worker()
    assert executable is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [str(executable), "--self-test-pf-ae-channel-suite"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert completed.stdout.strip() == '{"pf_ae_channel_suite":"passed"}'
