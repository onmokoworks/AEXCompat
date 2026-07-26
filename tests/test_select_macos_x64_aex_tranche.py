import importlib.util
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "select_macos_x64_aex_tranche",
    ROOT / "tools" / "select_macos_x64_aex_tranche.py",
)
assert SPEC and SPEC.loader
SELECT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SELECT)


def entry(name: str, sha: str, size: int, abi: str) -> dict[str, object]:
    export = SELECT.REGISTRATION_EXPORTS[abi]
    return {
        "canonical_path": (
            "C:\\Program Files\\Adobe\\Adobe After Effects 2025\\"
            f"Support Files\\Plug-ins\\Effects\\{name}.aex"
        ),
        "relative_to_root": f"Effects\\{name}.aex",
        "size": size,
        "sha256": sha,
        "export_names": [export],
        "root_category": "adobe_ae_plugins",
        "source_category": "adobe_installed",
        "architecture": "x64",
        "valid_pe": True,
        "product_target_candidate": True,
        "fixture_hint": False,
        "backup_hint": False,
    }


def test_selection_is_balanced_disjoint_and_ordered_by_size_then_sha():
    inventory_entries = [
        entry("v1-excluded", "1" * 64, 10, "v1"),
        entry("v1-large", "3" * 64, 30, "v1"),
        entry("v1-small-high-sha", "4" * 64, 20, "v1"),
        entry("v1-small-low-sha", "2" * 64, 20, "v1"),
        entry("v2-excluded", "5" * 64, 10, "v2"),
        entry("v2-large", "7" * 64, 30, "v2"),
        entry("v2-small", "6" * 64, 20, "v2"),
    ]

    selected = SELECT.select_tranche(
        {"entries": inventory_entries},
        {"1" * 64, "5" * 64},
        per_registration_abi=2,
    )

    assert {item["sha256"] for item in selected}.isdisjoint(
        {"1" * 64, "5" * 64}
    )
    assert [item["sha256"] for item in selected if item["registration_abi"] == "v1"] == [
        "2" * 64,
        "4" * 64,
    ]
    assert [item["sha256"] for item in selected if item["registration_abi"] == "v2"] == [
        "6" * 64,
        "7" * 64,
    ]


def test_selection_rejects_exclusion_absent_from_eligible_inventory():
    inventory = {"entries": [entry("only", "1" * 64, 10, "v1")]}

    with pytest.raises(SELECT.SweepError, match="excluded SHA is absent"):
        SELECT.select_tranche(inventory, {"2" * 64}, per_registration_abi=1)


def test_exclusion_file_is_bound_to_expected_digest(tmp_path):
    source = tmp_path / "prior-shas.txt"
    source.write_text("1" * 64 + "\n", encoding="ascii")
    expected = SELECT.sha256_file(source)

    values, actual = SELECT.load_excluded_shas(source, expected)
    assert values == {"1" * 64}
    assert actual == expected
    with pytest.raises(SELECT.SweepError, match="differs from expected identity"):
        SELECT.load_excluded_shas(source, "0" * 64)


def test_selection_rejects_insufficient_registration_abi():
    inventory = {
        "entries": [
            entry("v1", "1" * 64, 10, "v1"),
            entry("v2", "2" * 64, 10, "v2"),
        ]
    }

    with pytest.raises(SELECT.SweepError, match="only 1 eligible v1"):
        SELECT.select_tranche(inventory, set(), per_registration_abi=2)


def test_eligibility_excludes_audio_prefix_and_non_product_entries():
    audio = entry("Aud_example", "1" * 64, 10, "v1")
    fixture = entry("fixture", "2" * 64, 10, "v1")
    fixture["fixture_hint"] = True

    assert SELECT.is_eligible(audio) is False
    assert SELECT.is_eligible(fixture) is False


def test_ambiguous_registration_exports_fail_closed():
    value = entry("ambiguous", "1" * 64, 10, "v1")
    value["export_names"] = list(SELECT.REGISTRATION_EXPORTS.values())

    with pytest.raises(SELECT.SweepError, match="ambiguously exports"):
        SELECT.registration_abi(value)
