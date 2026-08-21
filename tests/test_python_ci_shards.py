import importlib.util
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "tools" / "run-python-ci-tests.py"


def load_runner():
    spec = importlib.util.spec_from_file_location("python_ci_shards", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def test_python_test_shards_are_deterministic_complete_and_disjoint() -> None:
    runner = load_runner()
    files = tuple(
        ROOT / "tests" / name for name in ("test_c.py", "test_a.py", "test_b.py")
    )

    first = runner.test_shards(files)
    second = runner.test_shards(files)

    assert first == second
    assert first == ((files[1], files[0]), (files[2],))
    assert set(first[0]).isdisjoint(first[1])
    assert set(first[0]) | set(first[1]) == set(files)


def test_python_test_inventory_includes_nested_test_files(tmp_path) -> None:
    runner = load_runner()
    runner.TESTS = tmp_path
    root_test = tmp_path / "test_root.py"
    nested_test = tmp_path / "nested" / "test_nested.py"
    ignored = tmp_path / "nested" / "helper.py"
    nested_test.parent.mkdir()
    for path in (root_test, nested_test, ignored):
        path.write_text("", encoding="utf-8")

    assert runner.python_test_files() == (nested_test, root_test)


def test_python_test_shards_fail_closed_on_invalid_inventory() -> None:
    runner = load_runner()
    test_file = ROOT / "tests" / "test_example.py"

    with pytest.raises(SystemExit, match="no Python test files"):
        runner.test_shards(())
    with pytest.raises(SystemExit, match="duplicate Python test files"):
        runner.test_shards((test_file, test_file))
    with pytest.raises(SystemExit, match="shard is empty"):
        runner.test_shards((test_file,))


def test_run_shard_executes_only_the_selected_files_and_propagates_exit(
    monkeypatch,
) -> None:
    runner = load_runner()
    files = tuple(
        ROOT / "tests" / name
        for name in ("test_a.py", "test_b.py", "test_c.py", "test_d.py")
    )
    monkeypatch.setattr(runner, "python_test_files", lambda: files)
    calls = []

    class Result:
        returncode = 17

    monkeypatch.setattr(
        runner.subprocess,
        "run",
        lambda *args, **kwargs: calls.append((args, kwargs)) or Result(),
    )

    with pytest.raises(SystemExit) as failure:
        runner.run_shard(1, ["-q", "--run-built-artifact-tests"])

    assert failure.value.code == 17
    command = calls[0][0][0]
    test_b = str(Path("tests") / "test_b.py")
    test_d = str(Path("tests") / "test_d.py")
    assert command[:6] == [
        runner.sys.executable,
        "-m",
        "pytest",
        "-q",
        "--run-built-artifact-tests",
        test_b,
    ]
    assert command[6:] == [test_d]
    assert calls[0][1]["cwd"] == ROOT


def test_run_shard_rejects_an_unknown_shard(monkeypatch) -> None:
    runner = load_runner()
    files = tuple(ROOT / "tests" / f"test_{name}.py" for name in ("a", "b"))
    monkeypatch.setattr(runner, "python_test_files", lambda: files)

    with pytest.raises(SystemExit, match="invalid Python test shard"):
        runner.run_shard(2, [])
