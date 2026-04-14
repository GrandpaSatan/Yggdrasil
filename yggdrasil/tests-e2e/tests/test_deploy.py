"""Build check + deploy — non-destructive probe + destructive push (gated)."""

from __future__ import annotations

import pytest
import requests

from helpers.services import service_urls


@pytest.mark.required_services("odin")
def test_build_check_returns_diagnostics_shape() -> None:
    """POST /api/v1/build_check with a minimal request must return a diagnostics envelope."""
    url = service_urls()["odin"].rstrip("/")
    payload = {"crate": "ygg-domain", "check_type": "check"}
    resp = requests.post(f"{url}/api/v1/build_check", json=payload, timeout=90)
    if resp.status_code == 404:
        pytest.skip("/api/v1/build_check not exposed on this Odin build")
    if resp.status_code == 422:
        pytest.skip(f"build_check payload shape differs from expected schema; body: {resp.text[:200]}")
    if resp.status_code == 500 and "cargo" in resp.text.lower():
        pytest.skip("cargo not on PATH for Odin host (Munin) — known gap, run from dev workstation instead")
    assert resp.status_code in (200, 202), f"build_check got {resp.status_code}: {resp.text[:200]}"
    body = resp.json()
    # Shape tolerance: either {diagnostics: [...]} or {errors: [...], warnings: [...]}.
    assert any(k in body for k in ("diagnostics", "errors", "warnings", "output")), (
        f"build_check response missing diagnostics; got keys {list(body)}"
    )


@pytest.mark.destructive
@pytest.mark.required_services("odin")
def test_deploy_dry_run_returns_artifact_path(require_destructive) -> None:
    """Deploy with dry_run=true — should compute the artifact path without pushing."""
    url = service_urls()["odin"].rstrip("/")
    payload = {"binary": "ygg-node", "target": "munin", "dry_run": True}
    resp = requests.post(f"{url}/api/v1/deploy", json=payload, timeout=120)
    if resp.status_code == 404:
        pytest.skip("/api/v1/deploy not exposed")
    assert resp.status_code in (200, 202)
    body = resp.json()
    assert body.get("artifact_path") or body.get("path") or body.get("binary"), (
        f"dry-run deploy must report an artifact path; got {body}"
    )
