import os
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCES = (
    ROOT / "minihost" / "src" / "l2_main.cpp",
    ROOT / "minihost" / "src" / "worker_pf_suites.cpp",
    ROOT / "minihost" / "src" / "worker_l2_suite_abi.hpp",
    ROOT / "minihost" / "src" / "worker_pf_suites_internal.hpp",
)


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
    assert "struct PfAeChannelSuite1" in text
    assert "sizeof(PfAeChannelSuite1) == 5 * sizeof(void*)" in text
    for slot, member in enumerate(("count", "indexed", "typed", "checkout", "checkin")):
        assert f"offsetof(PfAeChannelSuite1, {member}) == {slot} * sizeof(void*)" in text
    assert re.search(
        r"PfAeChannelSuite1 g_channel_suite1\{&get_layer_channel_count, &get_layer_channel_indexed,\s*"
        r"&get_layer_channel_typed, &checkout_layer_channel, &checkin_layer_channel\}",
        text,
    )
    assert '*suite = &g_channel_suite1;' in text


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
    assert "kPfInvalidIndex = 513" in text
    assert "kPfUnrecognizedParamType = 514" in text
    assert "kPfBadCallbackParam = 516" in text
    assert text.count("if (found) *found = 0;") == 2


def test_channel_ownership_and_conversion_fail_closed():
    text = source_text()
    assert "kChannelRefMagic" in text
    assert "ref.generation != g_channel_generation" in text
    assert "void** handle=new_handle(bytes)" in text
    assert "void* locked=lock_handle(handle)" in text
    assert "pixels > std::numeric_limits<std::size_t>::max()" in text
    assert "g_live_channel_chunks.find(chunk)" in text
    assert "chunk->data != live.locked_data" in text
    assert "chunk->data_handle != live.handle" in text
    assert "unlock_handle(live.handle); dispose_handle(live.handle);" in text
    assert "g_live_channel_chunks.erase(found)" in text
    assert "reclaim_layer_channels();" in text
    for data_type in ("kDataFloat", "kDataDouble", "kDataLong", "kDataShort",
                      "kDataFixed", "kDataChar", "kDataUByte", "kDataUShort", "kDataUFixed"):
        assert f"case {data_type}" in text


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
