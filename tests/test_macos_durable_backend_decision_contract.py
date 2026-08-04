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
    candidates = _function(
        MACOS_HARNESS.read_text(encoding="utf-8"), "guest_worker_candidates"
    )

    native_gate = candidates.index('var_os("AEXCOMPAT_NATIVE_CARRIER")')
    native_worker = candidates.index("x86_64-apple-darwin", native_gate)
    unicorn_release = candidates.index("guest/target/release/aex-guest-worker")
    assert native_gate < native_worker < unicorn_release
    assert 'is_some_and(|value| value == "1")' in candidates




