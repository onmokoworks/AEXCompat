from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "minihost" / "src" / "worker_report.cpp").read_text(
    encoding="utf-8"
)
INSPECTION = (
    ROOT
    / "broker"
    / "crates"
    / "broker"
    / "src"
    / "image_render"
    / "inspection_and_probes.rs"
).read_text(encoding="utf-8")


def test_all_worker_report_double_fields_use_the_finite_json_number_boundary():
    assert "#include <cmath>" in SOURCE
    assert "if (std::isfinite(value))" in SOURCE
    assert 'else o << "null";' in SOURCE
    for field in (
        "p.valid_min",
        "p.valid_max",
        "p.slider_min",
        "p.slider_max",
        "p.default_value",
        "p.current_value",
        "p.default_components[i]",
        "p.current_components[i]",
    ):
        assert f"number(o, {field})" in SOURCE


def test_inspection_contract_treats_null_nonfinite_metadata_as_unavailable():
    assert 'row.get("valid_min")' in INSPECTION
    assert 'row.get("valid_max")' in INSPECTION
    assert ".and_then(Value::as_f64)" in INSPECTION
    assert ".unwrap_or(default)" in INSPECTION
    assert (
        'row.get("default").and_then(Value::as_f64).unwrap_or(0.0)'
        in INSPECTION
    )
