#!/usr/bin/env python3
"""Wait for an artifact uploaded by this GitHub Actions workflow run."""

from __future__ import annotations

import json
import os
import re
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import Request, urlopen


RETRYABLE_HTTP_STATUSES = {429, 500, 502, 503, 504}


def artifact_ready(payload: object, name: str, run_id: int) -> bool:
    if not isinstance(payload, dict) or not isinstance(payload.get("artifacts"), list):
        raise ValueError("invalid workflow artifact response")
    for artifact in payload["artifacts"]:
        if not isinstance(artifact, dict):
            raise ValueError("invalid workflow artifact entry")
        workflow_run = artifact.get("workflow_run")
        if (
            artifact.get("name") == name
            and artifact.get("expired") is False
            and isinstance(artifact.get("size_in_bytes"), int)
            and artifact["size_in_bytes"] > 0
            and isinstance(workflow_run, dict)
            and workflow_run.get("id") == run_id
        ):
            return True
    return False


def producer_failed(payload: object, job_name: str) -> bool:
    if not isinstance(payload, dict) or not isinstance(payload.get("jobs"), list):
        raise ValueError("invalid workflow jobs response")
    for job in payload["jobs"]:
        if not isinstance(job, dict):
            raise ValueError("invalid workflow job entry")
        if job.get("name") == job_name and job.get("status") == "completed":
            return job.get("conclusion") != "success"
    return False


def wait_for_artifact(
    repository: str,
    run_id: int,
    name: str,
    token: str,
    *,
    producer_job: str | None = None,
    api_url: str = "https://api.github.com",
    timeout_seconds: float = 3600,
    poll_seconds: float = 5,
    job_check_seconds: float = 60,
    opener=urlopen,
    clock=time.monotonic,
    sleep=time.sleep,
) -> None:
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("invalid GitHub repository")
    if not isinstance(run_id, int) or run_id <= 0 or not name or not token:
        raise ValueError("missing workflow artifact lookup input")
    if not api_url.startswith("https://"):
        raise ValueError("GitHub API URL must use HTTPS")
    if timeout_seconds <= 0 or poll_seconds <= 0 or job_check_seconds <= 0:
        raise ValueError("invalid artifact wait duration")

    run_url = f"{api_url.rstrip('/')}/repos/{repository}/actions/runs/{run_id}"
    artifact_url = (
        f"{run_url}/artifacts?"
        + urlencode({"name": name, "per_page": 100})
    )
    headers = {
        "Accept": "application/vnd.github+json",
        "Authorization": f"Bearer {token}",
        "X-GitHub-Api-Version": "2022-11-28",
    }
    artifact_request = Request(artifact_url, headers=headers)
    jobs_request = Request(f"{run_url}/jobs?per_page=100", headers=headers)
    deadline = clock() + timeout_seconds
    next_job_check = clock()
    while True:
        try:
            with opener(artifact_request, timeout=15) as response:
                ready = artifact_ready(json.load(response), name, run_id)
        except HTTPError as exc:
            if exc.code not in RETRYABLE_HTTP_STATUSES:
                raise
            ready = False
        except URLError:
            ready = False
        if ready:
            return
        if producer_job and clock() >= next_job_check:
            try:
                with opener(jobs_request, timeout=15) as response:
                    failed = producer_failed(json.load(response), producer_job)
            except HTTPError as exc:
                if exc.code not in RETRYABLE_HTTP_STATUSES:
                    raise
                failed = False
            except URLError:
                failed = False
            if failed:
                raise RuntimeError(f"producer job {producer_job!r} did not succeed")
            next_job_check = clock() + job_check_seconds
        remaining = deadline - clock()
        if remaining <= 0:
            raise TimeoutError(f"workflow artifact {name!r} did not become available")
        sleep(min(poll_seconds, remaining))


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: wait-for-workflow-artifact.py ARTIFACT_NAME PRODUCER_JOB", file=sys.stderr)
        return 2
    try:
        wait_for_artifact(
            os.environ["GITHUB_REPOSITORY"],
            int(os.environ["GITHUB_RUN_ID"]),
            sys.argv[1],
            os.environ["GH_TOKEN"],
            producer_job=sys.argv[2],
            api_url=os.environ.get("GITHUB_API_URL", "https://api.github.com"),
        )
    except (KeyError, ValueError, TimeoutError, RuntimeError, HTTPError, URLError) as exc:
        print(f"artifact wait failed: {exc}", file=sys.stderr)
        return 1
    print(f"workflow artifact {sys.argv[1]!r} is available")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
