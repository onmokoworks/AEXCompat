from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MACOS_HARNESS = ROOT / "broker/crates/harness/src/macos.rs"


def _function(source: str, name: str) -> str:
    start = source.index(f"fn {name}(")
    opening = source.index("{", start)
    depth = 0
    for index in range(opening, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[start : index + 1]
    raise AssertionError(f"unterminated function {name}")


def test_unicorn_is_default_and_native_carrier_is_explicitly_opt_in():
    source = MACOS_HARNESS.read_text(encoding="utf-8")
    candidates = _function(source, "guest_worker_candidates")
    native_gate = _function(source, "native_carrier_opted_in")

    assert 'var_os("AEXCOMPAT_NATIVE_CARRIER")' in native_gate
    assert 'var_os("AEXCOMPAT_NATIVE_CARRIER_TRUSTED")' in native_gate
    assert "native_carrier_opted_in_values" in native_gate
    native_worker = candidates.index("x86_64-apple-darwin")
    unicorn_release = candidates.index("guest/target/release/aex-guest-worker")
    assert native_worker < unicorn_release
    assert "if native_carrier_opted_in()?" in candidates

