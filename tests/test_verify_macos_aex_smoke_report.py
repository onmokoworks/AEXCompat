import importlib.util
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import zlib

import pytest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "verify_macos_aex_smoke_report",
    ROOT / "tools" / "verify_macos_aex_smoke_report.py",
)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def png(width: int = 2, height: int = 1) -> bytes:
    def chunk(name: bytes, payload: bytes) -> bytes:
        body = name + payload
        return struct.pack(">I", len(payload)) + body + struct.pack(">I", zlib.crc32(body))

    pixels = b"\x00" + b"\x00\x00\x00\xff" * width
    return (
        MODULE.PNG_SIGNATURE
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(pixels))
        + chunk(b"IEND", b"")
    )


def valid_report() -> dict:
    return {
        "schema_version": 1,
        "error": None,
        "gpu": {
            "requested_backend": "cpu",
            "pre_render": {"attempted": True, "completed": True, "error": 0},
            "render": {"attempted": True, "completed": True, "error": 0},
            "cleanup_complete": True,
        },
        "suite_requests": ["PF Handle Suite v2"],
        "unsupported_suite_calls": [],
        "dropped_unsupported_suite_calls": 0,
    }


def write_fixture(tmp_path: Path, report: dict | str | bytes):
    report_path = tmp_path / "diagnostic.json"
    if isinstance(report, dict):
        report_path.write_text(json.dumps(report), encoding="utf-8")
    elif isinstance(report, str):
        report_path.write_text(report, encoding="utf-8")
    else:
        report_path.write_bytes(report)
    output_path = tmp_path / "output.png"
    output_path.write_bytes(png())
    return report_path, output_path


def test_validates_successful_bounded_render_and_png(tmp_path):
    report_path, output_path = write_fixture(tmp_path, valid_report())
    result = MODULE.validate(report_path, output_path)
    assert result["status"] == "verified"
    assert (result["width"], result["height"]) == (2, 1)
    assert result["suite_request_count"] == 1


def test_accepts_classic_render_without_a_pre_render_selector(tmp_path):
    report = valid_report()
    report["render_error"] = 0
    report["gpu"]["pre_render"] = {
        "attempted": False,
        "completed": False,
    }
    report_path, output_path = write_fixture(tmp_path, report)
    assert MODULE.validate(report_path, output_path)["status"] == "verified"


@pytest.mark.parametrize(
    ("mutation", "message"),
    [
        (lambda value: value.update(schema_version=2), "schema_version"),
        (lambda value: value.update(error="selector failed"), "render error"),
        (
            lambda value: value["gpu"].update(requested_backend="opencl"),
            "correctness path",
        ),
        (
            lambda value: value["gpu"]["render"].update(completed=False),
            "render did not complete",
        ),
        (lambda value: value["gpu"].update(cleanup_complete=False), "cleanup"),
        (
            lambda value: value.update(unsupported_suite_calls=[{"slot": 7}]),
            "unsupported suite",
        ),
        (
            lambda value: value.update(dropped_unsupported_suite_calls=1),
            "dropped unsupported",
        ),
    ],
)
def test_rejects_failed_or_ambiguous_diagnostics(tmp_path, mutation, message):
    report = valid_report()
    mutation(report)
    report_path, output_path = write_fixture(tmp_path, report)
    with pytest.raises(SystemExit, match=message):
        MODULE.validate(report_path, output_path)


def test_rejects_invalid_json_and_non_png_output(tmp_path):
    report_path, output_path = write_fixture(tmp_path, "not-json")
    with pytest.raises(SystemExit, match="UTF-8 JSON"):
        MODULE.validate(report_path, output_path)

    report_path.write_text(json.dumps(valid_report()), encoding="utf-8")
    output_path.write_bytes(b"not a png")
    with pytest.raises(SystemExit, match="PNG contract"):
        MODULE.validate(report_path, output_path)

    output_path.write_bytes(
        MODULE.PNG_SIGNATURE + struct.pack(">I", 13) + b"IHDR" + struct.pack(">II", 2, 1)
    )
    with pytest.raises(SystemExit, match="PNG chunk"):
        MODULE.validate(report_path, output_path)


def test_rejects_duplicate_diagnostic_keys_at_any_depth(tmp_path):
    report = json.dumps(valid_report())
    report = report.replace(
        '"unsupported_suite_calls": []',
        '"unsupported_suite_calls": [{"slot": 7}], "unsupported_suite_calls": []',
    )
    report_path, output_path = write_fixture(tmp_path, report)
    with pytest.raises(SystemExit, match="duplicate JSON key: unsupported_suite_calls"):
        MODULE.validate(report_path, output_path)

    nested = json.dumps(valid_report()).replace(
        '"requested_backend": "cpu"',
        '"requested_backend": "opencl", "requested_backend": "cpu"',
    )
    report_path.write_text(nested, encoding="utf-8")
    with pytest.raises(SystemExit, match="duplicate JSON key: requested_backend"):
        MODULE.validate(report_path, output_path)


def test_rejects_png_with_bad_crc_or_truncated_idat(tmp_path):
    report_path, output_path = write_fixture(tmp_path, valid_report())
    payload = bytearray(output_path.read_bytes())
    payload[29] ^= 1
    output_path.write_bytes(payload)
    with pytest.raises(SystemExit, match="chunk CRC"):
        MODULE.validate(report_path, output_path)

    output_path.write_bytes(png()[:-12])
    with pytest.raises(SystemExit, match="required PNG chunks"):
        MODULE.validate(report_path, output_path)


def test_rejects_report_larger_than_the_trace_envelope(tmp_path):
    report_path, output_path = write_fixture(tmp_path, valid_report())
    with report_path.open("ab") as stream:
        stream.truncate(MODULE.MAX_REPORT_BYTES + 1)
    with pytest.raises(SystemExit, match="bounded contract"):
        MODULE.validate(report_path, output_path)


def test_package_verifier_executes_mounted_arm64_worker_and_propagates_inputs(tmp_path):
    shell = shutil.which("sh")
    if shell is None:
        pytest.skip("POSIX shell is unavailable")
    fake_bin = tmp_path / "bin"
    payload = tmp_path / "payload"
    fake_bin.mkdir()
    (payload / "arm64").mkdir(parents=True)
    worker = payload / "arm64" / "aex-guest-worker"
    worker.write_text(
        """#!/bin/sh
set -eu
if [ "${1:-}" = "--help" ]; then exit 2; fi
test "$1" = render-trace-png
test "$2" = "$EXPECTED_AEX"
test "$3" = "$EXPECTED_INPUT"
cp "$FAKE_OUTPUT_PNG" "$4"
printf '%s\\n' '{"schema_version":1,"render_error":0,"gpu":{"requested_backend":"cpu","pre_render":{"attempted":true,"completed":true,"error":0},"render":{"attempted":true,"completed":true,"error":0},"cleanup_complete":true},"suite_requests":[],"unsupported_suite_calls":[],"dropped_unsupported_suite_calls":0}'
""",
        encoding="utf-8",
    )
    worker.chmod(0o755)
    worker_payload = worker.read_bytes()
    manifest = {
        "schema": "aexcompat-macos-carriers-v1",
        "distribution_tier": "local-adhoc",
        "workers": [
            {
                "architecture": "arm64",
                "backend": "unicorn",
                "path": "arm64/aex-guest-worker",
                "sha256": hashlib.sha256(worker_payload).hexdigest(),
                "size": len(worker_payload),
            }
        ],
    }
    (payload / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")

    (fake_bin / "uname").write_text("#!/bin/sh\necho Darwin\n", encoding="utf-8")
    (fake_bin / "file").write_text(
        '#!/bin/sh\necho "$1: Mach-O 64-bit executable arm64"\n', encoding="utf-8"
    )
    (fake_bin / "codesign").write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    (fake_bin / "hdiutil").write_text(
        """#!/bin/sh
set -eu
case "$1" in
  verify|detach) exit 0 ;;
  attach)
    while [ "$#" -gt 0 ]; do
      if [ "$1" = "-mountpoint" ]; then
        shift
        mkdir -p "$1"
        cp -R "$FAKE_PAYLOAD"/. "$1"/
        exit 0
      fi
      shift
    done
    ;;
esac
exit 2
""",
        encoding="utf-8",
    )
    for command in fake_bin.iterdir():
        command.chmod(0o755)

    artifact = tmp_path / "carriers.dmg"
    artifact.write_bytes(b"fake-dmg")
    aex = tmp_path / "effect.aex"
    input_png = tmp_path / "input.png"
    output_png = tmp_path / "worker-output.png"
    aex.write_bytes(b"MZ")
    input_png.write_bytes(png())
    output_png.write_bytes(png())
    environment = os.environ.copy()
    environment.update(
        {
            "PATH": f"{fake_bin}:{environment.get('PATH', '')}",
            "FAKE_PAYLOAD": str(payload),
            "FAKE_OUTPUT_PNG": str(output_png),
            "EXPECTED_AEX": str(aex),
            "EXPECTED_INPUT": str(input_png),
        }
    )
    verifier = ROOT / "tools" / "verify-macos-aex-carrier-package.sh"
    result = subprocess.run(
        [shell, str(verifier), str(artifact), str(aex), str(input_png)],
        text=True,
        capture_output=True,
        env=environment,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    assert '"status": "verified"' in result.stdout
    assert "arm64 Unicorn render, diagnostics, and cleanup verified" in result.stdout


def test_package_verifier_rejects_unpaired_smoke_inputs_before_mount(tmp_path):
    shell = shutil.which("sh")
    if shell is None:
        pytest.skip("POSIX shell is unavailable")
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    for name, body in {
        "uname": "#!/bin/sh\necho Darwin\n",
        "hdiutil": "#!/bin/sh\nexit 99\n",
    }.items():
        command = fake_bin / name
        command.write_text(body, encoding="utf-8")
        command.chmod(0o755)
    artifact = tmp_path / "carriers.dmg"
    artifact.write_bytes(b"fake")
    aex = tmp_path / "effect.aex"
    aex.write_bytes(b"MZ")
    environment = os.environ.copy()
    environment["PATH"] = f"{fake_bin}:{environment.get('PATH', '')}"
    result = subprocess.run(
        [
            shell,
            str(ROOT / "tools" / "verify-macos-aex-carrier-package.sh"),
            str(artifact),
            str(aex),
        ],
        text=True,
        capture_output=True,
        env=environment,
        check=False,
    )
    assert result.returncode == 2
    assert "requires both" in result.stderr


def test_local_prepare_rejects_missing_smoke_files_before_build(tmp_path):
    shell = shutil.which("sh")
    if shell is None:
        pytest.skip("POSIX shell is unavailable")
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    uname = fake_bin / "uname"
    uname.write_text("#!/bin/sh\necho Darwin\n", encoding="utf-8")
    uname.chmod(0o755)
    output = tmp_path / "must-not-exist.dmg"
    environment = os.environ.copy()
    environment.update(
        {
            "PATH": f"{fake_bin}:{environment.get('PATH', '')}",
            "AEXCOMPAT_SMOKE_AEX": str(tmp_path / "missing.aex"),
            "AEXCOMPAT_SMOKE_INPUT_PNG": str(tmp_path / "missing.png"),
        }
    )
    result = subprocess.run(
        [
            shell,
            str(ROOT / "tools" / "prepare-local-macos-aex-carriers.sh"),
            str(output),
        ],
        text=True,
        capture_output=True,
        env=environment,
        check=False,
    )
    assert result.returncode == 2
    assert "does not exist" in result.stderr
    assert not output.exists()
