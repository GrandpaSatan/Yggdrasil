"""Gaming orchestrator — list (read-only) + launch (destructive, gated)."""

from __future__ import annotations

import pytest
import requests

from helpers.services import service_urls


@pytest.mark.required_services("odin")
def test_gaming_status_list_readonly() -> None:
    """GET gaming status — purely read-only, reachable even without E2E_DESTRUCTIVE."""
    url = service_urls()["odin"].rstrip("/")
    resp = requests.get(f"{url}/api/v1/gaming/status", timeout=10)
    if resp.status_code == 404:
        pytest.skip("/api/v1/gaming/status not exposed")
    assert resp.status_code == 200
    body = resp.json()
    assert isinstance(body, dict) or isinstance(body, list), "gaming status must be structured JSON"


@pytest.mark.destructive
@pytest.mark.required_services("odin")
def test_gaming_launch_returns_vm_running(require_destructive) -> None:
    """REAL VM launch via Proxmox — only when all three gates open.

    This spends real energy and ties up a GPU. Skip path is via ``require_destructive``.
    """
    url = service_urls()["odin"].rstrip("/")
    payload = {"role": "gaming", "host": "thor", "dry_run": False}
    resp = requests.post(f"{url}/api/v1/gaming", json=payload, timeout=60)
    if resp.status_code == 404:
        pytest.skip("/api/v1/gaming not exposed")
    assert resp.status_code in (200, 202)
    body = resp.json()
    assert body.get("vm_id") or body.get("running") or body.get("status"), (
        f"launch response missing VM state; got {body}"
    )
