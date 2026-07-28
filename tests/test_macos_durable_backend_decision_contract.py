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


def test_only_native_one_shot_admission_has_a_deadline_and_falls_through():
    source = MACOS_HARNESS.read_text(encoding="utf-8")
    routing = _function(source, "run_guest_workers")
    worker = _function(source, "run_worker")

    assert "candidate.native.then_some(native_deadline)" in routing
    assert "for candidate in candidates" in routing
    assert "Ok(output) if output.status.success() => return Ok(output)" in routing
    assert "kill_and_reap_or_transfer" in worker
    assert "NATIVE_DEADLINE_REAP_BUDGET" in worker
    assert ".wait()" not in worker


def test_resident_fallback_is_admission_only_and_never_replays_frames():
    source = MACOS_HARNESS.read_text(encoding="utf-8")
    admission = _function(source, "start_resident_worker")
    session = _function(source, "start_resident_session")

    assert "for candidate in candidates" in admission
    assert '"type": "probe"' in admission
    assert "validate_resident_probe" in admission
    assert "launch_resident_candidate(candidate, arguments)" in admission
    assert "start_resident_worker(candidates" in session
    assert "for candidate in candidates" not in session
    assert "run_guest_workers" not in session
