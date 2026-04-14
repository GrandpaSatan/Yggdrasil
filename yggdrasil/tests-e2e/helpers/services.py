"""Shared HTTP helpers: retry policy, health probes, service-URL resolution."""

from __future__ import annotations

import os
import time
from dataclasses import dataclass
from typing import Callable, Iterable

import requests
from tenacity import (
    retry,
    retry_if_exception_type,
    stop_after_attempt,
    wait_random_exponential,
)

RETRYABLE_STATUS = {429, 500, 502, 503, 504}


class TransientHttpError(Exception):
    """Raised for responses we want the retry decorator to handle."""


def retry_policy() -> Callable:
    """Decorator: 3 attempts, 50ms→800ms jittered backoff on transient errors.

    Applied to every write-path client method. Read-only calls (GET /health,
    GET /v1/models) do not retry so a broken service fails fast.
    """
    return retry(
        stop=stop_after_attempt(3),
        wait=wait_random_exponential(multiplier=0.05, max=0.8),
        retry=retry_if_exception_type(
            (TransientHttpError, requests.ConnectionError, requests.Timeout)
        ),
        reraise=True,
    )


def check_response(resp: requests.Response) -> requests.Response:
    """Raise TransientHttpError on 429/5xx so retry_policy picks it up."""
    if resp.status_code in RETRYABLE_STATUS:
        raise TransientHttpError(
            f"{resp.request.method} {resp.url} → {resp.status_code}"
        )
    return resp


@dataclass(frozen=True)
class ServiceHealth:
    name: str
    url: str
    ok: bool
    detail: str = ""


def probe(name: str, url: str, path: str = "/health", timeout: float = 5.0) -> ServiceHealth:
    """Hit <url><path> and report reachability. Never raises."""
    try:
        resp = requests.get(f"{url.rstrip('/')}{path}", timeout=timeout)
        if resp.status_code == 200:
            return ServiceHealth(name=name, url=url, ok=True, detail="200 OK")
        return ServiceHealth(
            name=name, url=url, ok=False, detail=f"{path} → {resp.status_code}"
        )
    except (requests.ConnectionError, requests.Timeout) as exc:
        return ServiceHealth(name=name, url=url, ok=False, detail=str(exc))


def wait_for_ready(
    name: str,
    url: str,
    path: str = "/health",
    timeout_secs: float = 15.0,
    interval_secs: float = 0.5,
) -> bool:
    """Block until a service responds 200 or timeout. Used by fixtures only."""
    deadline = time.monotonic() + timeout_secs
    while time.monotonic() < deadline:
        if probe(name, url, path).ok:
            return True
        time.sleep(interval_secs)
    return False


def service_urls() -> dict[str, str]:
    """Resolve fleet URLs from env with sensible defaults."""
    return {
        "odin": os.environ.get("ODIN_URL", "http://10.0.65.8:8080"),
        "mimir": os.environ.get("MIMIR_URL", "http://10.0.65.8:9090"),
        "muninn": os.environ.get("MUNINN_URL", "http://10.0.65.8:9091"),
        "voice": os.environ.get("HUGIN_VOICE_URL", "http://10.0.65.9:9098"),
        "mcp_http": os.environ.get("MCP_HTTP_URL", "http://10.0.65.8:9093"),
        "dreamer": os.environ.get("DREAMER_URL", "http://10.0.65.8:9094"),
    }


def require_env(name: str) -> str:
    """Fetch an env var or raise a clear test-skip error."""
    value = os.environ.get(name)
    if not value:
        import pytest

        pytest.skip(f"env var {name} is required for this test but is unset")
    return value
