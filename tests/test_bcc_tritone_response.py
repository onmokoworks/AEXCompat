"""Bounded BCC Tritone lookup response for two grayscale layouts."""

import hashlib
import json
import os
import secrets
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path

import pytest
from PIL import Image

from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH, argb


EXPECTED_PLUGIN = Path(
    r"C:\Program Files\Adobe\Common\Plug-ins\7.0\MediaCore"
    r"\BorisFX\Continuum\BCCTritone.aex"
)
EXPECTED_NAME = "BCCTritone.aex"
EXPECTED_VENDOR = "BorisFX"
EXPECTED_PRODUCT = "Continuum"
EXPECTED_PLUGIN_SIZE = 33792
EXPECTED_PLUGIN_SHA256 = (
    "07fba7c2c51adc43528f03a112859f0f20a3556de01c1ddcda84c465d0a1a1ab"
)
EXPECTED_PATH_SHA256 = (
    "7038c4a4026d985e9b74e3c0d8dfc32b0ee7d15a68496e34c38703375de29176"
)
EXPECTED_INSPECTION_SHA256 = (
    "75f0e1f92a62ef230f1ddfd41d42c7cebcfd7e9fc98d852f5b95aed27cad6bde"
)

COLORS = {
    8: [255, 255, 0, 0],
    10: [255, 0, 255, 0],
    11: [255, 0, 0, 255],
}

INTEGER_SLOTS = {9, 13, 14, 16, 19, 72}

CORE_SCHEMA = {
    6: {
        "name": "Host Layer",
        "kind": "layer",
        "minimum": 0,
        "maximum": 0,
        "value": 0,
        "choices": [],
    },
    8: {
        "name": "Black Color",
        "kind": "color",
        "minimum": 0,
        "maximum": 0,
        "value": 0,
        "choices": [],
    },
    9: {
        "name": "Use Midpoint Color",
        "kind": "integer",
        "minimum": 0,
        "maximum": 1,
        "value": 1,
        "choices": [],
    },
    10: {
        "name": "Midpoint Color",
        "kind": "color",
        "minimum": 0,
        "maximum": 0,
        "value": 0,
        "choices": [],
    },
    11: {
        "name": "White Color",
        "kind": "color",
        "minimum": 0,
        "maximum": 0,
        "value": 0,
        "choices": [],
    },
    12: {
        "name": "Midpoint",
        "kind": "float",
        "minimum": 0,
        "maximum": 255,
        "value": 128,
        "choices": [],
    },
    13: {
        "name": "Input Channel",
        "kind": "integer",
        "minimum": 1,
        "maximum": 8,
        "value": 1,
        "choices": [
            "Luma",
            "Red",
            "Green",
            "Blue",
            "Luma Inverse",
            "Red Inverse",
            "Green Inverse",
            "Blue Inverse",
        ],
    },
    14: {
        "name": "Output Channels",
        "kind": "integer",
        "minimum": 1,
        "maximum": 8,
        "value": 1,
        "choices": [
            "RGB",
            "Red",
            "Green",
            "Blue",
            "Red and Green",
            "Red and Blue",
            "Green and Blue",
            "Difference",
        ],
    },
    15: {
        "name": "Repeats",
        "kind": "float",
        "minimum": 0,
        "maximum": 10,
        "value": 1,
        "choices": [],
    },
    16: {
        "name": "Repeat Mode",
        "kind": "integer",
        "minimum": 1,
        "maximum": 2,
        "value": 1,
        "choices": ["Back and Forth", "Jump"],
    },
    17: {
        "name": "Mix with Original",
        "kind": "float",
        "minimum": 0,
        "maximum": 100,
        "value": 0,
        "choices": [],
    },
    19: {
        "name": "PixelChooser",
        "kind": "integer",
        "minimum": 1,
        "maximum": 4,
        "value": 1,
        "choices": ["Off", "On", "Mask Unchosen Pixels", "View Matte Source"],
    },
    72: {
        "name": "Render Legacy PixelChooser",
        "kind": "integer",
        "minimum": 0,
        "maximum": 1,
        "value": 0,
        "choices": [],
    },
}


def _sha256_file(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _utc_now():
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _length_prefixed_digest(values):
    digest = hashlib.sha256()
    for value in values:
        encoded = os.fsencode(value)
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
    return digest.hexdigest()


def _receipt_id(receipt):
    return _canonical_sha256(
        {key: value for key, value in receipt.items() if key != "receipt_id"}
    )


def portable_payload(value, artifact_root):
    if isinstance(value, dict):
        return {key: portable_payload(item, artifact_root) for key, item in value.items()}
    if isinstance(value, list):
        return [portable_payload(item, artifact_root) for item in value]
    if isinstance(value, str):
        return value.replace(str(artifact_root), "<artifacts>").replace(
            artifact_root.as_posix(), "<artifacts>"
        ).replace("\\", "/")
    return value


def _write_process_evidence(
    tmp_path,
    stem,
    *,
    command,
    argv_shape,
    pid,
    sequence,
    run_nonce,
    started_utc,
    finished_utc,
    duration_ns,
    returncode,
    stdout,
    stderr,
    parsed,
    timed_out=False,
):
    (tmp_path / f"{stem}.stdout.bin").write_bytes(stdout)
    (tmp_path / f"{stem}.stderr.bin").write_bytes(stderr)
    execution_identity = None
    if isinstance(parsed, dict):
        diagnostics = parsed.get("diagnostics", parsed.get("worker_diagnostics"))
        if isinstance(diagnostics, dict):
            execution_identity = diagnostics.get("execution_identity")
    receipt = {
        "schema_version": 2,
        "run_nonce": run_nonce,
        "sequence": sequence,
        "operation": stem,
        "argv_shape": argv_shape,
        "command_digest": _length_prefixed_digest(
            [str(Path(command[0]).resolve(strict=True)), str(ROOT.resolve()), *command[1:]]
        ),
        "pid": pid,
        "started_utc": started_utc,
        "finished_utc": finished_utc,
        "duration_ns": duration_ns,
        "returncode": returncode,
        "timed_out": timed_out,
        "stdout_bytes": len(stdout),
        "stdout_sha256": hashlib.sha256(stdout).hexdigest(),
        "stderr_bytes": len(stderr),
        "stderr_sha256": hashlib.sha256(stderr).hexdigest(),
        "parsed_json_sha256": _canonical_sha256(parsed) if parsed is not None else None,
        "portable_json_sha256": (
            _canonical_sha256(portable_payload(parsed, tmp_path))
            if parsed is not None else None
        ),
        "execution_identity_sha256": (
            _canonical_sha256(execution_identity)
            if execution_identity is not None
            else None
        ),
    }
    receipt["receipt_id"] = _receipt_id(receipt)
    (tmp_path / f"{stem}.process.json").write_text(
        json.dumps(receipt, indent=2, sort_keys=True),
        encoding="utf-8",
    )
    return receipt


def validate_process_receipts(receipts, expected_count):
    assert len(receipts) == expected_count
    assert [receipt["sequence"] for receipt in receipts] == list(
        range(1, expected_count + 1)
    )
    assert len({receipt["run_nonce"] for receipt in receipts}) == expected_count
    assert len({receipt["receipt_id"] for receipt in receipts}) == expected_count
    for receipt in receipts:
        assert receipt["schema_version"] == 2
        assert receipt["receipt_id"] == _receipt_id(receipt)
        assert len(receipt["run_nonce"]) == 32
        assert receipt["pid"] > 0
        started = datetime.fromisoformat(receipt["started_utc"].replace("Z", "+00:00"))
        finished = datetime.fromisoformat(receipt["finished_utc"].replace("Z", "+00:00"))
        assert started <= finished
        assert receipt["duration_ns"] > 0
        assert receipt["returncode"] == 0
        assert receipt["timed_out"] is False
        for key in (
            "command_digest",
            "stdout_sha256",
            "stderr_sha256",
            "parsed_json_sha256",
            "portable_json_sha256",
            "execution_identity_sha256",
        ):
            assert len(receipt[key]) == 64
            int(receipt[key], 16)


def _canonical_sha256(value):
    encoded = json.dumps(
        value,
        ensure_ascii=False,
        allow_nan=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def source_a_pixels():
    """Every luma occurs once per row in a diagonally shifted layout."""
    return bytes(
        component
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for value in ((x + 37 * y) % 256,)
        for component in (value, value, value, 255)
    )


def source_b_pixels():
    """The same luma histogram in an unrelated spatial permutation."""
    return bytes(
        component
        for y in range(HEIGHT)
        for x in range(WIDTH)
        for value in ((73 * x + 29 * y + 11) % 256,)
        for component in (value, value, value, 255)
    )


def tritone_pixel(value):
    if value <= 128:
        green = 255 * value // 128
        return 255 - green, green, 0, 255
    blue = 255 * (value - 128) // 127
    return 0, 255 - blue, blue, 255


def duotone_pixel(value):
    return 255 - value, 0, value, 255


def apply_mapping(mapper, source):
    return bytes(
        component
        for value in source[0::4]
        for component in mapper(value)
    )


def mapping_by_input(source, output):
    mapping = {}
    for offset in range(0, len(source), 4):
        assert source[offset] == source[offset + 1] == source[offset + 2]
        value = source[offset]
        rgba = tuple(output[offset:offset + 4])
        if value in mapping:
            assert mapping[value] == rgba
        else:
            mapping[value] = rgba
    assert sorted(mapping) == list(range(256))
    return [mapping[value] for value in range(256)]


def assert_monotonic(values, *, increasing):
    pairs = zip(values, values[1:])
    if increasing:
        assert all(left <= right for left, right in pairs)
    else:
        assert all(left >= right for left, right in pairs)


def assert_tritone_response(
    tritone_a,
    repeat_a,
    tritone_b,
    duotone_a,
    mix100_a,
):
    source_a = source_a_pixels()
    source_b = source_b_pixels()
    expected_size = WIDTH * HEIGHT * 4
    outputs = (tritone_a, repeat_a, tritone_b, duotone_a, mix100_a)
    assert source_a != source_b
    assert sorted(source_a[0::4]) == sorted(source_b[0::4])
    for output in outputs:
        assert len(output) == expected_size
        assert output[3::4] == bytes([255]) * (WIDTH * HEIGHT)

    assert tritone_a == repeat_a
    assert tritone_a != source_a
    assert tritone_b != source_b
    assert tritone_a != tritone_b
    assert duotone_a != source_a
    assert tritone_a != duotone_a
    assert mix100_a == source_a

    tri_a = mapping_by_input(source_a, tritone_a)
    tri_b = mapping_by_input(source_b, tritone_b)
    duo = mapping_by_input(source_a, duotone_a)
    assert tri_a == tri_b
    assert len(set(tri_a)) == len(set(duo)) == 256

    assert tri_a[0] == (255, 0, 0, 255)
    assert tri_a[128] == (0, 255, 0, 255)
    assert tri_a[-1][0] == 0
    assert tri_a[-1][1] <= 1
    assert tri_a[-1][2] >= 253
    assert all(pixel[2] == 0 for pixel in tri_a[:129])
    assert all(pixel[0] == 0 for pixel in tri_a[128:])
    assert_monotonic([pixel[0] for pixel in tri_a[:129]], increasing=False)
    assert_monotonic([pixel[1] for pixel in tri_a[:129]], increasing=True)
    assert_monotonic([pixel[1] for pixel in tri_a[128:]], increasing=False)
    assert_monotonic([pixel[2] for pixel in tri_a[128:]], increasing=True)

    assert duo[0] == (255, 0, 0, 255)
    assert duo[-1][0] == 0
    assert duo[-1][2] >= 254
    assert all(pixel[1] == 0 for pixel in duo)
    assert duo[128][0] > 0 and duo[128][2] > 0
    assert_monotonic([pixel[0] for pixel in duo], increasing=False)
    assert_monotonic([pixel[2] for pixel in duo], increasing=True)

    return {
        "repeat_exact": tritone_a == repeat_a,
        "layout_lookup_equal": tri_a == tri_b,
        "mix100_exact": mix100_a == source_a,
        "tritone_a_rgba_sha256": hashlib.sha256(tritone_a).hexdigest(),
        "tritone_b_rgba_sha256": hashlib.sha256(tritone_b).hexdigest(),
        "duotone_a_rgba_sha256": hashlib.sha256(duotone_a).hexdigest(),
        "source_a_rgba_sha256": hashlib.sha256(source_a).hexdigest(),
        "source_b_rgba_sha256": hashlib.sha256(source_b).hexdigest(),
    }


def _synthetic_outputs():
    source_a = source_a_pixels()
    source_b = source_b_pixels()
    tritone_a = apply_mapping(tritone_pixel, source_a)
    return (
        tritone_a,
        tritone_a,
        apply_mapping(tritone_pixel, source_b),
        apply_mapping(duotone_pixel, source_a),
        source_a,
    )


def test_tritone_validator_accepts_two_layouts_and_repeat():
    metrics = assert_tritone_response(*_synthetic_outputs())
    assert metrics["repeat_exact"] is True
    assert metrics["layout_lookup_equal"] is True
    assert metrics["mix100_exact"] is True


def _swap_red_blue(pixels):
    changed = bytearray(pixels)
    changed[0::4], changed[2::4] = changed[2::4], changed[0::4]
    return bytes(changed)


@pytest.mark.parametrize(
    "fault",
    (
        "copy",
        "lost_midpoint",
        "repeat_drift",
        "wrong_b",
        "layout_sensitive",
        "channel_swap",
        "flat",
        "nonmonotonic",
        "x_only_gradient",
        "alpha",
        "mix_changed",
        "truncated",
    ),
)
def test_tritone_validator_rejects_corruption(fault):
    tritone_a, repeat_a, tritone_b, duotone_a, mix100_a = _synthetic_outputs()
    assert_tritone_response(
        tritone_a,
        repeat_a,
        tritone_b,
        duotone_a,
        mix100_a,
    )

    if fault == "copy":
        tritone_a = repeat_a = source_a_pixels()
    elif fault == "lost_midpoint":
        tritone_a = repeat_a = duotone_a
    elif fault == "repeat_drift":
        repeat_a = bytearray(repeat_a)
        repeat_a[0] ^= 1
    elif fault == "wrong_b":
        tritone_b = tritone_a
    elif fault == "layout_sensitive":
        tritone_b = bytes(
            component
            for _y in range(HEIGHT)
            for x in range(WIDTH)
            for component in tritone_pixel(x)
        )
    elif fault == "channel_swap":
        tritone_a = repeat_a = _swap_red_blue(tritone_a)
    elif fault == "flat":
        tritone_a = repeat_a = tritone_a[:4] * (WIDTH * HEIGHT)
    elif fault == "nonmonotonic":
        tritone_a = bytearray(tritone_a)
        for offset, value in enumerate(source_a_pixels()[0::4]):
            if value == 64:
                tritone_a[offset * 4] = 200
        repeat_a = bytes(tritone_a)
    elif fault == "x_only_gradient":
        tritone_a = bytes(
            component
            for _y in range(HEIGHT)
            for x in range(WIDTH)
            for component in tritone_pixel(x)
        )
        repeat_a = tritone_a
    elif fault == "alpha":
        tritone_a = bytearray(tritone_a)
        tritone_a[3] = 0
        repeat_a = bytes(tritone_a)
    elif fault == "mix_changed":
        mix100_a = tritone_a
    else:
        duotone_a = duotone_a[:-4]

    with pytest.raises(AssertionError):
        assert_tritone_response(
            bytes(tritone_a),
            bytes(repeat_a),
            bytes(tritone_b),
            bytes(duotone_a),
            bytes(mix100_a),
        )


@pytest.mark.parametrize("fault", ("tamper", "nonce", "duplicate", "time"))
def test_process_receipt_validator_rejects_corruption(fault):
    def receipt(sequence, nonce, pid):
        value = {
            "schema_version": 2,
            "run_nonce": nonce,
            "sequence": sequence,
            "operation": f"run-{sequence}",
            "argv_shape": ["<harness>", "--headless", "<plugin>"],
            "command_digest": "11" * 32,
            "pid": pid,
            "started_utc": "2026-09-20T00:00:00Z",
            "finished_utc": "2026-09-20T00:00:01Z",
            "duration_ns": 1,
            "returncode": 0,
            "timed_out": False,
            "stdout_bytes": 1,
            "stdout_sha256": "22" * 32,
            "stderr_bytes": 0,
            "stderr_sha256": "33" * 32,
            "parsed_json_sha256": "44" * 32,
            "portable_json_sha256": "44" * 32,
            "execution_identity_sha256": "55" * 32,
        }
        value["receipt_id"] = _receipt_id(value)
        return value

    receipts = [receipt(1, "aa" * 16, 101), receipt(2, "bb" * 16, 102)]
    validate_process_receipts(receipts, 2)
    if fault == "tamper":
        receipts[0]["stdout_sha256"] = "66" * 32
    elif fault == "nonce":
        receipts[1]["run_nonce"] = receipts[0]["run_nonce"]
        receipts[1]["receipt_id"] = _receipt_id(receipts[1])
    elif fault == "duplicate":
        receipts[1] = dict(receipts[0])
    else:
        receipts[1]["started_utc"] = "2026-09-20T00:00:02Z"
        receipts[1]["receipt_id"] = _receipt_id(receipts[1])
    with pytest.raises(AssertionError):
        validate_process_receipts(receipts, 2)


def _assert_inspection(parameters):
    assert len(parameters) == 95
    assert len({parameter["slot"] for parameter in parameters}) == 95
    assert {parameter["slot"] for parameter in parameters} == set(range(1, 96))
    assert _canonical_sha256(parameters) == EXPECTED_INSPECTION_SHA256
    by_slot = {parameter["slot"]: parameter for parameter in parameters}
    for slot, expected in CORE_SCHEMA.items():
        parameter = by_slot[slot]
        for key, value in expected.items():
            assert parameter[key] == value
        assert parameter["enabled"] is True
        assert parameter["visible"] is True
        assert parameter["supervised"] is True
    return by_slot


def _request(source, use_midpoint, mix):
    specifications = {
        8: ("color", COLORS[8]),
        9: ("integer", use_midpoint),
        10: ("color", COLORS[10]),
        11: ("color", COLORS[11]),
        12: ("float", 128),
        13: ("integer", 1),
        14: ("integer", 1),
        15: ("float", 1),
        16: ("integer", 1),
        17: ("float", mix),
        19: ("integer", 1),
        72: ("integer", 0),
    }
    assignments = [{"slot": 6, "layer": str(source)}]
    receipts = []
    for slot, (kind, value) in sorted(specifications.items()):
        if kind == "color":
            assignments.append({"slot": slot, "color": value})
            alpha, red, green, blue = value
            receipt_value = {
                "alpha": alpha,
                "red": red,
                "green": green,
                "blue": blue,
            }
        else:
            assignments.append({"slot": slot, "value": value})
            receipt_value = value
        receipts.append(
            {
                "id": f"param_{slot}",
                "kind": kind,
                "slot": slot,
                "value": receipt_value,
            }
        )
    assert {receipt["slot"] for receipt in receipts if receipt["kind"] == "integer"} == (
        INTEGER_SLOTS
    )
    return assignments, receipts


def _assert_execution_identity(diagnostics, build_identity):
    identity = diagnostics["execution_identity"]
    assert identity["schema_version"] == 1
    assert identity["worker"] == {
        "binding": "broker_authenticated_pinned_stage",
        "sha256": build_identity["worker_sha256"],
        "size_bytes": build_identity["worker_size_bytes"],
    }
    assert identity["plugin_images"] == [
        {
            "plugin_index": 0,
            "basename": EXPECTED_NAME,
            "sha256": EXPECTED_PLUGIN_SHA256,
            "size_bytes": EXPECTED_PLUGIN_SIZE,
            "binding_status": "same_file_identity_matches_loaded_module",
        }
    ]


def _assert_report(report, source_pixels, output, expected_receipts, build_identity):
    assert report["passed"] is True
    assert report["schema_version"] == 1
    assert report["stage"] == "interactive_image_render"
    assert report["plugin_id"] == "experimental-timed-layers"
    assert report["worker_classification"] == "ok"
    assert report["render_path"] == "smartfx"
    assert report["pixel_format"] == "argb8"
    assert report["output_transport"] == "rgba8_png"
    assert report["width"] == report["input_width"] == WIDTH
    assert report["height"] == report["input_height"] == HEIGHT
    assert report["full_resolution_dimensions"] == [WIDTH, HEIGHT]
    assert report["in_data_dimensions"] == [WIDTH, HEIGHT]
    assert report["current_time"] == 0
    assert report["time_step"] == report["local_time_step"] == 1
    assert report["time_scale"] == 30
    assert report["total_time"] == 300
    assert report["field"] == 0
    assert report["quality"] == 1
    assert report["downsample_x"] == report["downsample_y"] == [1, 1]
    assert report["input_sha256"] == hashlib.sha256(argb(source_pixels)).hexdigest()
    assert report["output_pixels_valid"] is True
    assert Path(report["output_png"]).resolve(strict=True) == output.resolve(
        strict=True
    )
    assert report["output_raw"] is None
    assert report["output_origin"] == [0, 0]
    assert report["pre_effect_source_origin"] == [0, 0]
    assert report["input_checkout_result_rect"] == [0, 0, WIDTH, HEIGHT]
    assert report["result_rect"] == [0, 0, WIDTH, HEIGHT]
    assert report["max_result_rect"] == [0, 0, WIDTH, HEIGHT]
    assert report["result_rects_valid"] is True
    assert report["result_within_request"] is True
    assert report["spatial_contract_ok"] is True
    assert report["output_origin_contract_ok"] is True
    assert report["param_checkouts_balanced"] is True
    assert report["smart_render_selector_dispatched"] is True
    assert report["pre_render_error"] == 0
    assert report["smart_render_error"] == 0
    assert report["smart_render_selector_error"] == 0
    assert report["secondary_layers"] == [
        {
            "slot": 6,
            "width": WIDTH,
            "height": HEIGHT,
        }
    ]

    assert report["in_data_num_params"] == 96
    assert len(report["parameter_metadata"]) == 95
    assert [item["index"] for item in report["parameter_metadata"]] == list(
        range(1, 96)
    )
    assert report["parameter_count_contract_ok"] is False
    assert report["host_contract_warning"] is True
    assert report["guard_bytes_intact"] is True
    assert report["handle_lifetimes_balanced"] is True
    assert report["world_lifetimes_balanced"] is True
    assert report["suite_leases_balanced"] is True
    assert report["suite_lease_warning"] is False
    assert report["live_suite_leases"] == ""
    assert report["suite_acquires"] == report["suite_releases"]
    assert report["requested_parameters"] == expected_receipts
    assert len({receipt["slot"] for receipt in expected_receipts}) == len(
        expected_receipts
    )
    assert len({receipt["id"] for receipt in expected_receipts}) == len(
        expected_receipts
    )
    assert 6 not in {receipt["slot"] for receipt in expected_receipts}

    diagnostics = report["worker_diagnostics"]
    _assert_execution_identity(diagnostics, build_identity)
    assert diagnostics["classification"] == "ok"
    assert diagnostics["exit_code"] == 0
    assert diagnostics["active_stage"] is None
    assert diagnostics["failure_stage"] is None
    assert diagnostics["first_failure_stage"] is None
    assert diagnostics["load_failure"] is None
    assert diagnostics["unsupported_suite_calls"] == []
    assert diagnostics["unsupported_suite_calls_truncated"] is False
    assert diagnostics["callback_denials"] == []
    assert diagnostics["callback_denials_truncated"] is False
    assert diagnostics["module_audit_warning"] == (
        "secure worker module audit did not pass"
    )
    missing_suites = [
        {"name": "VDS App Suite", "version": 1},
        {"name": "AEGP Camera Suite", "version": 2},
    ]
    assert diagnostics["missing_suites"] == missing_suites
    assert diagnostics["suite_acquire_failures"] == missing_suites
    assert diagnostics["missing_suites_truncated"] is False
    assert diagnostics["suite_acquire_failures_truncated"] is False
    assert diagnostics.get("worker_freshness_warning") is None
    assert diagnostics["stderr_truncated"] is False
    assert diagnostics["last_completed_stage"] == "global_setdown"


def test_real_bcc_tritone_response(tmp_path):
    configured = os.environ.get("AEXCOMPAT_TEST_BCC_TRITONE")
    if not configured:
        pytest.skip(
            "set AEXCOMPAT_TEST_BCC_TRITONE to the exact installed BCCTritone.aex"
        )
    assert os.name == "nt"

    plugin_path = Path(configured).resolve(strict=True)
    expected_path = EXPECTED_PLUGIN.resolve(strict=True)
    assert plugin_path == expected_path
    assert plugin_path.is_file()
    assert plugin_path.name == EXPECTED_NAME
    assert plugin_path.parent.name == EXPECTED_PRODUCT
    assert plugin_path.parent.parent.name == EXPECTED_VENDOR
    assert plugin_path.stat().st_size == EXPECTED_PLUGIN_SIZE
    assert _sha256_file(plugin_path) == EXPECTED_PLUGIN_SHA256
    plugin = str(plugin_path)

    harness = ROOT / "broker/target/release/aexcompat-harness.exe"
    worker = ROOT / "target/minihost-build/aex_worker.exe"
    assert harness.is_file() and harness.stat().st_size > 0
    assert worker.is_file() and worker.stat().st_size > 0

    identity = {
        "schema_version": 1,
        "plugin": {
            "name": plugin_path.name,
            "size_bytes": plugin_path.stat().st_size,
            "path_sha256": hashlib.sha256(
                str(plugin_path).lower().encode("utf-8")
            ).hexdigest(),
            "sha256": _sha256_file(plugin_path),
        },
        "build": {
            "harness_sha256": _sha256_file(harness),
            "harness_size_bytes": harness.stat().st_size,
            "worker_sha256": _sha256_file(worker),
            "worker_size_bytes": worker.stat().st_size,
        },
    }
    (tmp_path / "preflight-evidence.json").write_text(
        json.dumps(identity, indent=2, sort_keys=True), encoding="utf-8"
    )
    assert identity["plugin"] == {
        "name": EXPECTED_NAME,
        "size_bytes": EXPECTED_PLUGIN_SIZE,
        "path_sha256": EXPECTED_PATH_SHA256,
        "sha256": EXPECTED_PLUGIN_SHA256,
    }

    process_receipts = []

    def argv_shape(command):
        result = []
        for index, value in enumerate(map(str, command)):
            path = Path(value)
            if index == 0:
                result.append("<harness>")
            elif path == plugin_path:
                result.append("<plugin>")
            elif path.is_absolute():
                try:
                    relative = path.relative_to(tmp_path)
                except ValueError:
                    result.append("<absolute-path>")
                else:
                    result.append(f"<artifact:{relative.as_posix()}>")
            else:
                result.append(value)
        return result

    def run(stem, *args, timeout_seconds=90):
        command = [str(harness), "--headless", *map(str, args)]
        run_nonce = secrets.token_hex(16)
        started_utc = _utc_now()
        started_ns = time.perf_counter_ns()
        process = subprocess.Popen(
            command,
            cwd=ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        timed_out = False
        try:
            stdout, stderr = process.communicate(timeout=timeout_seconds)
        except subprocess.TimeoutExpired:
            timed_out = True
            process.kill()
            stdout, stderr = process.communicate()
        finished_ns = time.perf_counter_ns()
        finished_utc = _utc_now()
        parsed = None
        parse_error = None
        if not timed_out and process.returncode == 0:
            try:
                parsed = json.loads(stdout)
            except (ValueError, UnicodeError) as error:
                parse_error = error
        receipt = _write_process_evidence(
            tmp_path,
            stem,
            command=command,
            argv_shape=argv_shape(command),
            pid=process.pid,
            sequence=len(process_receipts) + 1,
            run_nonce=run_nonce,
            started_utc=started_utc,
            finished_utc=finished_utc,
            duration_ns=finished_ns - started_ns,
            returncode=process.returncode,
            stdout=stdout,
            stderr=stderr,
            parsed=parsed,
            timed_out=timed_out,
        )
        process_receipts.append(receipt)
        if timed_out:
            pytest.fail(f"{stem} timed out after {timeout_seconds} seconds")
        assert process.returncode == 0, stderr.decode(
            "utf-8", errors="replace"
        )
        assert parse_error is None, f"{stem} emitted invalid JSON: {parse_error}"
        return parsed

    inspection = run(
        "inspection",
        "--inspect-experimental-report",
        plugin,
        timeout_seconds=None,
    )
    (tmp_path / "inspection.json").write_text(
        json.dumps(inspection, indent=2, sort_keys=True), encoding="utf-8"
    )
    _assert_inspection(inspection["parameters"])
    _assert_execution_identity(
        inspection["diagnostics"], identity["build"]
    )

    source_pixels_by_label = {
        "a": source_a_pixels(),
        "b": source_b_pixels(),
    }
    sources = {}
    for label, pixels in source_pixels_by_label.items():
        source = tmp_path / f"source-{label}.png"
        Image.frombytes("RGBA", (WIDTH, HEIGHT), pixels).save(source)
        assert source.is_file() and source.stat().st_size > 0
        with Image.open(source) as image:
            assert image.format == "PNG"
            assert image.mode == "RGBA"
            image.load()
            assert image.size == (WIDTH, HEIGHT)
            assert image.tobytes() == pixels
        sources[label] = source

    cases = (
        ("tritone_a", "a", 1, 0),
        ("repeat_a", "a", 1, 0),
        ("tritone_b", "b", 1, 0),
        ("duotone_a", "a", 0, 0),
        ("mix100_a", "a", 1, 100),
    )
    outputs = {}
    for label, source_label, use_midpoint, mix in cases:
        assignments, expected_receipts = _request(
            sources[source_label], use_midpoint, mix
        )
        request = tmp_path / f"{label}.request.json"
        output = tmp_path / f"{label}.png"
        request.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "timing": {
                        "frame": 0,
                        "fps": 30,
                        "duration_frames": 300,
                    },
                    "assignments": assignments,
                },
                sort_keys=True,
            ),
            encoding="utf-8",
        )
        report = run(
            f"{label}.report",
            "--render-experimental-smart-request",
            plugin,
            sources[source_label],
            output,
            request,
        )
        (tmp_path / f"{label}.report.json").write_text(
            json.dumps(report, indent=2, sort_keys=True), encoding="utf-8"
        )
        _assert_report(
            report,
            source_pixels_by_label[source_label],
            output,
            expected_receipts,
            identity["build"],
        )

        assert output.is_file() and output.stat().st_size > 0
        with Image.open(output) as image:
            assert image.format == "PNG"
            assert image.mode == "RGBA"
            image.load()
            assert image.size == (WIDTH, HEIGHT)
            pixels = image.tobytes()
        assert hashlib.sha256(argb(pixels)).hexdigest() == report["output_sha256"]
        outputs[label] = pixels

    postflight_identity = {
        "schema_version": 1,
        "plugin": {
            "name": plugin_path.name,
            "size_bytes": plugin_path.stat().st_size,
            "path_sha256": hashlib.sha256(
                str(plugin_path).lower().encode("utf-8")
            ).hexdigest(),
            "sha256": _sha256_file(plugin_path),
        },
        "build": {
            "harness_sha256": _sha256_file(harness),
            "harness_size_bytes": harness.stat().st_size,
            "worker_sha256": _sha256_file(worker),
            "worker_size_bytes": worker.stat().st_size,
        },
    }
    (tmp_path / "postflight-evidence.json").write_text(
        json.dumps(postflight_identity, indent=2, sort_keys=True), encoding="utf-8"
    )
    assert postflight_identity == identity
    validate_process_receipts(process_receipts, 6)

    metrics = assert_tritone_response(
        outputs["tritone_a"],
        outputs["repeat_a"],
        outputs["tritone_b"],
        outputs["duotone_a"],
        outputs["mix100_a"],
    )
    (tmp_path / "metrics.json").write_text(
        json.dumps(metrics, indent=2, sort_keys=True), encoding="utf-8"
    )
