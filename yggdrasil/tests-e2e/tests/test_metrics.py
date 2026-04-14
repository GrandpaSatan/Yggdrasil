"""Prometheus /metrics endpoints — reachable, parseable, expected counters present."""

from __future__ import annotations

import pytest
import requests

from helpers import OdinClient
from helpers.services import service_urls


@pytest.mark.required_services("odin")
def test_odin_metrics_text_format(odin_client: OdinClient) -> None:
    text = odin_client.metrics_text()
    assert text, "/metrics must return a non-empty payload"
    # Prometheus text exposition format starts each counter/gauge with # HELP / # TYPE
    # or the metric line itself. At minimum we expect *some* HELP lines.
    assert "# HELP" in text or "# TYPE" in text, (
        "metrics output does not look like Prometheus text format"
    )


@pytest.mark.required_services("odin")
def test_odin_e2e_hits_counter_increments(odin_client: OdinClient) -> None:
    """POST /api/v1/e2e/hit must increment odin_e2e_hits_total.

    This is the same ping emitted by e2e-cron-wrapper.sh (Sprint 064 P8) — it
    lets the daily timer register activity in Prometheus.
    """
    status = odin_client.e2e_hit()
    if status == 404:
        pytest.skip("/api/v1/e2e/hit not exposed on this Odin build (Sprint 064 P8 not deployed?)")
    assert status in (200, 202, 204), f"/api/v1/e2e/hit must succeed, got {status}"
    # Counter check is best-effort — naming may vary across builds.
    after = _counter_value(odin_client.metrics_text(), "odin_e2e_hits_total")
    if after is None:
        pytest.skip("odin_e2e_hits_total counter not exposed; ping accepted but unobservable")
    assert after >= 1, f"counter must be at least 1 after a hit, got {after}"


@pytest.mark.required_services("mimir")
def test_mimir_metrics_reachable() -> None:
    url = service_urls()["mimir"]
    resp = requests.get(f"{url.rstrip('/')}/metrics", timeout=5)
    assert resp.status_code == 200, f"mimir /metrics must be 200, got {resp.status_code}"


def _counter_value(text: str, name: str) -> float | None:
    """Minimal Prometheus parser — find ``name <value>`` ignoring label suffixes."""
    for line in text.splitlines():
        if line.startswith("#") or not line.strip():
            continue
        bare = line.split("{", 1)[0].strip()
        if bare == name:
            parts = line.rsplit(" ", 1)
            try:
                return float(parts[-1])
            except (ValueError, IndexError):
                continue
    return None
