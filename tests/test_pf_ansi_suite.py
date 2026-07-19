import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCES = (
    ROOT / "minihost" / "src" / "l2_main.cpp",
    ROOT / "minihost" / "src" / "worker_l2_suite_abi.hpp",
)


def source_text():
    return "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)


def test_pf_ansi_suite_v1_wires_all_19_sdk_slots_without_gaps():
    text = source_text()
    expected = [
        "atan", "atan2", "ceil", "cos", "exp", "fabs", "floor", "fmod",
        "hypot", "log", "log10", "pow", "sin", "sqrt", "tan", "sprintf",
        "strcpy", "asin", "acos",
    ]
    assignments = re.findall(
        r"g_ansi_suite1\[(\d+)\] = reinterpret_cast<void\*>\(&ansi_(\w+)\);",
        text,
    )
    assert assignments == [(str(index), name) for index, name in enumerate(expected)]
    assert "std::array<void*, 19> g_ansi_suite1{}" in text


def test_pf_ansi_numeric_callbacks_use_a_finite_fail_closed_policy():
    text = source_text()
    assert "if (!std::isfinite(value)) return 0.0;" in text
    assert "if (!std::isfinite(left) || !std::isfinite(right)) return 0.0;" in text
    assert "return std::isfinite(result) ? result : 0.0;" in text
    for guard in (
        "if (divisor == 0.0) return 0.0;",
        "if (!(value > 0.0)) return 0.0;",
        "if (value < 0.0) return 0.0;",
        "if (value < -1.0 || value > 1.0) return 0.0;",
    ):
        assert guard in text


def test_pf_ansi_string_callbacks_remain_null_and_length_guarded():
    text = source_text()
    for guard in (
        "if (!destination || !format || strnlen_s(format, 256) == 256) return -1;",
        "required >= 0 && required <= 4096",
        "if (!destination || !source) return nullptr;",
        "const std::size_t length = strnlen_s(source, 4096);",
    ):
        assert guard in text
