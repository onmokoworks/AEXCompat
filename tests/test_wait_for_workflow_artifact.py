import importlib.util
import io
import json
from pathlib import Path
from urllib.error import HTTPError

import pytest


SCRIPT = Path(__file__).resolve().parents[1] / "tools" / "wait-for-workflow-artifact.py"


def load_waiter():
    spec = importlib.util.spec_from_file_location("workflow_artifact_waiter", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def payload(*artifacts):
    return {"artifacts": list(artifacts)}


def artifact(*, run_id=42, name="archive", size=123, expired=False):
    return {
        "name": name,
        "size_in_bytes": size,
        "expired": expired,
        "workflow_run": {"id": run_id},
    }


class Response(io.BytesIO):
    def __init__(self, body):
        super().__init__(json.dumps(body).encode("utf-8"))


def test_waits_for_only_nonempty_artifact_from_current_run():
    waiter = load_waiter()
    responses = [
        payload(artifact(run_id=41)),
        payload(artifact(size=0), artifact(expired=True)),
        payload(artifact(name="other")),
        payload(artifact()),
    ]
    urls = []
    sleeps = []

    def opener(request, *, timeout):
        urls.append(request.full_url)
        assert request.get_header("Authorization") == "Bearer secret"
        assert timeout == 15
        return Response(responses.pop(0))

    waiter.wait_for_artifact(
        "owner/repo", 42, "archive", "secret", opener=opener,
        clock=lambda: len(sleeps) * 5, sleep=sleeps.append,
    )
    assert len(urls) == 4
    assert all("/actions/runs/42/artifacts?" in url for url in urls)
    assert all("name=archive" in url for url in urls)
    assert sleeps == [5, 5, 5]


def test_timeout_is_bounded_when_archive_never_arrives():
    waiter = load_waiter()
    elapsed = 0
    calls = 0

    def opener(request, *, timeout):
        nonlocal calls
        calls += 1
        return Response(payload())

    def sleep(seconds):
        nonlocal elapsed
        elapsed += seconds

    with pytest.raises(TimeoutError, match="did not become available"):
        waiter.wait_for_artifact(
            "owner/repo", 42, "archive", "secret", timeout_seconds=12,
            opener=opener, clock=lambda: elapsed, sleep=sleep,
        )
    assert elapsed == 12
    assert calls == 4


def test_invalid_response_fails_closed():
    waiter = load_waiter()
    with pytest.raises(ValueError, match="invalid workflow artifact response"):
        waiter.wait_for_artifact(
            "owner/repo", 42, "archive", "secret",
            opener=lambda request, *, timeout: Response({"total_count": 1}),
        )


def test_authorization_error_is_not_retried():
    waiter = load_waiter()

    def forbidden(request, *, timeout):
        raise HTTPError(request.full_url, 403, "forbidden", {}, None)

    with pytest.raises(HTTPError) as error:
        waiter.wait_for_artifact(
            "owner/repo", 42, "archive", "secret", opener=forbidden,
            sleep=lambda seconds: pytest.fail("authorization failure was retried"),
        )
    assert error.value.code == 403


def test_transient_server_error_is_retried():
    waiter = load_waiter()
    attempts = 0

    def flaky(request, *, timeout):
        nonlocal attempts
        attempts += 1
        if attempts == 1:
            raise HTTPError(request.full_url, 503, "unavailable", {}, None)
        return Response(payload(artifact()))

    waiter.wait_for_artifact(
        "owner/repo", 42, "archive", "secret", opener=flaky,
        sleep=lambda seconds: None,
    )
    assert attempts == 2


def test_failed_producer_stops_waiting_without_archive_timeout():
    waiter = load_waiter()
    requested = []

    def opener(request, *, timeout):
        requested.append(request.full_url)
        if "/jobs?" in request.full_url:
            return Response({"jobs": [{
                "name": "rust-test-archive", "status": "completed",
                "conclusion": "failure",
            }]})
        return Response(payload())

    with pytest.raises(RuntimeError, match="did not succeed"):
        waiter.wait_for_artifact(
            "owner/repo", 42, "archive", "secret",
            producer_job="rust-test-archive", opener=opener,
            sleep=lambda seconds: pytest.fail("failed producer was not detected"),
        )
    assert len(requested) == 2
    assert requested[0].endswith("/artifacts?name=archive&per_page=100")
    assert requested[1].endswith("/jobs?per_page=100")


def test_running_producer_can_outlast_previous_ten_minute_bound():
    waiter = load_waiter()
    elapsed = 0
    checks = 0

    def opener(request, *, timeout):
        nonlocal checks
        if "/jobs?" in request.full_url:
            return Response({"jobs": [{
                "name": "rust-test-archive", "status": "in_progress",
                "conclusion": None,
            }]})
        checks += 1
        return Response(payload(artifact()) if elapsed >= 660 else payload())

    def sleep(seconds):
        nonlocal elapsed
        elapsed += seconds

    waiter.wait_for_artifact(
        "owner/repo", 42, "archive", "secret",
        producer_job="rust-test-archive", opener=opener,
        clock=lambda: elapsed, sleep=sleep,
    )
    assert elapsed == 660
    assert checks == 133
