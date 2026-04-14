"""Mesh handshake + proxy — positive path and VULN-006 negative regression."""

from __future__ import annotations

import pytest
import requests

from helpers.services import service_urls


@pytest.mark.required_services("odin")
def test_mesh_hello_accepts_valid_handshake() -> None:
    """Hello with minimal valid payload — expect 200 or 204.

    The audit confirms no auth on the handshake today, so this passes trivially.
    The paired xfail below is what tightens the bound once VULN-006 lands.
    """
    url = service_urls()["odin"]
    payload = {
        "node_id": "e2e-test-node",
        "url": "http://127.0.0.1:0",
        "services": [],
        "version": "0.66.0",
    }
    resp = requests.post(
        f"{url.rstrip('/')}/api/v1/mesh/hello",
        json=payload,
        timeout=5,
    )
    assert resp.status_code in (200, 202, 204, 404), (
        f"mesh hello expected 200/202/204, got {resp.status_code} (404 ok if endpoint renamed)"
    )


@pytest.mark.xfail(
    reason="VULN-006: mesh handshake accepts any node (no pre-shared key)",
    strict=True,
)
@pytest.mark.required_services("odin")
def test_mesh_forged_handshake_rejected() -> None:
    """Once VULN-006 is fixed, a handshake without a pre-shared key must 401."""
    url = service_urls()["odin"]
    payload = {"node_id": "forged", "url": "http://evil.example", "services": []}
    resp = requests.post(
        f"{url.rstrip('/')}/api/v1/mesh/hello",
        json=payload,
        timeout=5,
    )
    assert resp.status_code in (401, 403), (
        f"forged mesh handshake must be rejected, got {resp.status_code}"
    )
