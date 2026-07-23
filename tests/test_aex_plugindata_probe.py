"""Machine-portable self-tests for the live PluginData registration probe.

The probe's live half (loading a real .aex and calling its entrypoint) needs
real Adobe plug-ins, so these tests only cover the portable halves: the
worker-rule mirror (``worker_verdict``) and the text/name validators, fed with
synthetic registrations modelled on observed real plug-ins (issue #326).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

TOOLS_ROOT = Path(__file__).resolve().parents[1] / "tools"
if str(TOOLS_ROOT) not in sys.path:
    sys.path.insert(0, str(TOOLS_ROOT))

import aex_plugindata_probe as probe


def _registration(**overrides):
    base = {
        "name": "Demo",
        "match_name": "ADBE Demo",
        "category": "Sample",
        "entrypoint": "FilterMain",
        "kind_code": "eFKT",
        "api_major": 13,
        "api_minor": 29,
        "reserved_info": 0,
        "support_url": None,
    }
    base.update(overrides)
    return base


def _result(registrations, **overrides):
    base = {
        "plugin": "Demo.aex",
        "path": "Demo.aex",
        "status": "called",
        "export_variant": "v1",
        "entry_result": 0,
        "entrypoint_exported": True,
        "registrations": registrations,
    }
    base.update(overrides)
    return base


def test_arithmetic_like_registration_fails_api_version_and_reserved():
    # Arithmetic.aex registers api 13.29 with reserved 0 (observed).
    verdict = probe.worker_verdict(_result([_registration()]))
    assert not verdict["accepted"]
    assert set(verdict["reasons"]) == {"api_version", "reserved_info"}


def test_cycore_like_registration_passes_current_worker_rules():
    # CycoreFXHD/Threshold.aex registers api 13.28 with reserved 8 (observed),
    # the only combination the current worker accepts.
    verdict = probe.worker_verdict(
        _result([_registration(api_minor=28, reserved_info=8, entrypoint="MainEntry")])
    )
    assert verdict["accepted"]
    assert verdict["reasons"] == []


def test_multiple_callbacks_are_rejected():
    # Effects/Threshold.aex registers twice (EffectMainExtra/2, observed).
    verdict = probe.worker_verdict(_result([_registration(), _registration()]))
    assert not verdict["accepted"]
    assert "callback_count" in verdict["reasons"]


def test_zero_callbacks_are_rejected():
    verdict = probe.worker_verdict(_result([]))
    assert not verdict["accepted"]
    assert "callback_count" in verdict["reasons"]


def test_reserved_values_seen_in_the_wild_are_each_scored():
    # Observed reserved values: 0 (155), 1 (24), 8 (31), 9 (5).
    for reserved in (0, 1, 9):
        verdict = probe.worker_verdict(
            _result([_registration(api_minor=28, reserved_info=reserved)])
        )
        assert verdict["reasons"] == ["reserved_info"]
    verdict = probe.worker_verdict(_result([_registration(api_minor=28, reserved_info=8)]))
    assert verdict["accepted"]


def test_api_version_boundary_matches_worker_cap():
    # The worker accepts api <= 13.28 only.
    assert probe.worker_verdict(
        _result([_registration(api_major=12, api_minor=99, reserved_info=8)])
    )["accepted"]
    assert probe.worker_verdict(
        _result([_registration(api_minor=28, reserved_info=8)])
    )["accepted"]
    assert "api_version" in probe.worker_verdict(
        _result([_registration(api_minor=29, reserved_info=8)])
    )["reasons"]
    assert "api_version" in probe.worker_verdict(
        _result([_registration(api_major=14, api_minor=0, reserved_info=8)])
    )["reasons"]


def test_non_effect_kind_is_rejected():
    verdict = probe.worker_verdict(
        _result([_registration(kind_code="AEgx", api_minor=28, reserved_info=8)])
    )
    assert "kind" in verdict["reasons"]


def test_unexported_entrypoint_symbol_is_rejected():
    verdict = probe.worker_verdict(
        _result([_registration(api_minor=28, reserved_info=8)], entrypoint_exported=False)
    )
    assert verdict["reasons"] == ["entrypoint_not_exported"]


def test_status_short_circuits():
    for status, reason in (
        ("load_error", "load_error"),
        ("no_export", "no_export"),
        ("entry_error", "entry_error"),
    ):
        verdict = probe.worker_verdict(_result([], status=status, entry_result=None))
        assert verdict == {"accepted": False, "reasons": [reason]}


def test_export_name_validation():
    assert probe._valid_export_name("EffectMain")
    assert probe._valid_export_name("EffectMainExtra2")
    assert probe._valid_export_name("_entry")
    assert not probe._valid_export_name("")
    assert not probe._valid_export_name(None)
    assert not probe._valid_export_name("9Main")
    assert not probe._valid_export_name("Has Space")
    assert not probe._valid_export_name("x" * 128)


def test_sweep_names_reads_plugin_rows(tmp_path):
    sweep = tmp_path / "sweep.json"
    sweep.write_text(
        json.dumps({"plugins": [{"plugin": "A.aex"}, {"plugin": "B.aex"}]}),
        encoding="utf-8",
    )
    assert probe._sweep_plugin_names(sweep) == {"A.aex", "B.aex"}
