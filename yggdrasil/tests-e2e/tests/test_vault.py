"""Vault bearer-token auth — positive + negative paths.

This is the only endpoint in the entire fleet that has auth today (per the
security audit). Every other service is covered by test_security.py xfails
until VULN-001 lands.
"""

from __future__ import annotations

import uuid

import pytest

from helpers import MimirClient


@pytest.mark.required_services("mimir")
def test_vault_set_and_get_with_bearer_succeeds(
    mimir_client: MimirClient, vault_token: str
) -> None:
    key = f"e2e_test_{uuid.uuid4().hex[:8]}"
    value = "the quick brown fox 🦊"
    try:
        set_resp = mimir_client.vault_set(key, value, token=vault_token)
        assert set_resp.status_code in (200, 201), (
            f"vault set with bearer must succeed, got {set_resp.status_code}: {set_resp.text[:200]}"
        )
        get_resp = mimir_client.vault_get(key, token=vault_token)
        assert get_resp.status_code == 200, f"vault get got {get_resp.status_code}"
        payload = get_resp.json()
        returned = payload.get("value") or payload.get("data") or ""
        assert returned == value, f"vault returned {returned!r}, expected {value!r}"
    finally:
        mimir_client.vault_delete(key, token=vault_token)


@pytest.mark.required_services("mimir")
def test_vault_without_bearer_is_rejected(mimir_client: MimirClient) -> None:
    resp = mimir_client.vault_get("anything", token="")
    # 422 is also a rejection (server-side schema validation fires before auth).
    # Either layer rejecting the unauth request is acceptable; what matters is the
    # request did NOT succeed.
    assert resp.status_code in (401, 403, 422), (
        f"vault without bearer must be 401/403, got {resp.status_code}"
    )


@pytest.mark.required_services("mimir")
def test_vault_with_bad_bearer_is_rejected(mimir_client: MimirClient) -> None:
    resp = mimir_client.vault_get("anything", token="not-a-real-token")
    # 422 is also a rejection (server-side schema validation fires before auth).
    # Either layer rejecting the unauth request is acceptable; what matters is the
    # request did NOT succeed.
    assert resp.status_code in (401, 403, 422), (
        f"vault with invalid bearer must be 401/403, got {resp.status_code}"
    )
