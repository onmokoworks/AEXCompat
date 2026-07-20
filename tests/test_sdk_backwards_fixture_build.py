import hashlib
import json
import pathlib
import subprocess


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "build-sdk-backwards.ps1"
PROPS = ROOT / "tools" / "sdk-fixtures" / "sdk-backwards-v143.props"
OUTPUT = ROOT / "target" / "sdk-fixtures" / "sdk-backwards" / "SDK_Backwards.aex"
RESULT = ROOT / "target" / "sdk-fixtures" / "sdk-backwards" / "build-result.json"


def test_sdk_backwards_build_contract_is_read_only_and_v143():
    script = SCRIPT.read_text(encoding="utf-8")
    props = PROPS.read_text(encoding="utf-8")
    assert "PlatformToolset=v143" in script
    assert "sdk_source_unchanged" in script
    # The PiPL is generated into target/ and injected, keeping the SDK tree
    # read-only (the vcxproj otherwise writes SDK_BackwardsPiPL.rc in-place).
    assert "CustomBuild Update" in props
    assert "ExcludedFromBuild>true" in props
    assert "ResourceCompile Remove" in props
    assert "AEXCompatSdkBackwardsPiPLRc" in props


def test_official_sdk_backwards_builds_unchanged_and_records_hash():
    completed = subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(SCRIPT)],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        timeout=300,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    assert OUTPUT.is_file()
    result = json.loads(RESULT.read_text(encoding="utf-8-sig"))
    digest = hashlib.sha256(OUTPUT.read_bytes()).hexdigest()
    assert result["status"] == "built"
    assert result["platform_toolset"] == "v143"
    assert result["configuration"] == "Release|x64"
    assert result["artifact_sha256"] == digest
    assert result["artifact_size"] == OUTPUT.stat().st_size
    assert result["sdk_source_unchanged"] is True
    assert "SDK_Backwards.aex SHA-256: " + digest in completed.stdout
