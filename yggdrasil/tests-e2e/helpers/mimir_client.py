"""Thin client for Mimir HTTP endpoints used by E2E tests."""

from __future__ import annotations

from typing import Any

import requests

from .services import check_response, retry_policy


class MimirClient:
    def __init__(self, base_url: str, vault_token: str | None = None, timeout: float = 20.0):
        self.base_url = base_url.rstrip("/")
        self.vault_token = vault_token
        self.timeout = timeout

    def _url(self, path: str) -> str:
        return f"{self.base_url}{path}"

    def health(self) -> requests.Response:
        return requests.get(self._url("/health"), timeout=5.0)

    @retry_policy()
    def store(
        self,
        cause: str,
        effect: str,
        *,
        tags: list[str] | None = None,
        project: str | None = "yggdrasil",
        force: bool = True,
    ) -> str:
        """POST /api/v1/store with the actual NewEngram schema.

        ``force=True`` bypasses the novelty gate so test runs don't 409 on
        re-runs with similar content. Tests that specifically exercise the
        novelty gate should pass ``force=False``.
        """
        body: dict[str, Any] = {
            "cause": cause,
            "effect": effect,
            "tags": tags or [],
            "force": force,
        }
        if project:
            body["project"] = project
        resp = check_response(
            requests.post(self._url("/api/v1/store"), json=body, timeout=self.timeout)
        )
        resp.raise_for_status()
        payload = resp.json()
        return payload.get("id") or payload.get("engram_id") or ""

    @retry_policy()
    def recall(
        self,
        query: str,
        *,
        limit: int = 5,
        project: str | None = "yggdrasil",
        include_global: bool = True,
    ) -> list[dict[str, Any]]:
        """POST /api/v1/recall with the actual EngramQuery schema (field is `text`, not `query`)."""
        body: dict[str, Any] = {
            "text": query,
            "limit": limit,
            "include_global": include_global,
        }
        if project:
            body["project"] = project
        resp = check_response(
            requests.post(self._url("/api/v1/recall"), json=body, timeout=self.timeout)
        )
        resp.raise_for_status()
        payload = resp.json()
        return (
            payload.get("events")
            or payload.get("results")
            or payload.get("engrams")
            or []
        )

    def get_engram(self, engram_id: str) -> dict[str, Any] | None:
        resp = requests.get(self._url(f"/api/v1/engrams/{engram_id}"), timeout=10.0)
        if resp.status_code == 404:
            return None
        resp.raise_for_status()
        return resp.json()

    def delete_engram(self, engram_id: str) -> bool:
        resp = requests.delete(self._url(f"/api/v1/engrams/{engram_id}"), timeout=10.0)
        return resp.status_code in (200, 204, 404)

    def delete_supported(self) -> bool:
        """Probe whether DELETE /api/v1/engrams/{id} is implemented (some builds 405)."""
        # Use a syntactically-valid but non-existent UUID; we only care about 405 vs 404.
        resp = requests.delete(
            self._url("/api/v1/engrams/00000000-0000-0000-0000-000000000000"),
            timeout=5.0,
        )
        return resp.status_code != 405

    def delete_by_tag(self, tag: str, project: str = "yggdrasil") -> int:
        """Best-effort cleanup helper. Returns count of deletions attempted.

        Silently no-ops if DELETE is not supported on this Mimir build (405).
        """
        if not self.delete_supported():
            return 0
        body = {"text": tag, "limit": 100, "project": project, "include_global": True}
        try:
            resp = requests.post(self._url("/api/v1/recall"), json=body, timeout=10.0)
            resp.raise_for_status()
            payload = resp.json()
            engrams = (
                payload.get("events")
                or payload.get("results")
                or payload.get("engrams")
                or []
            )
        except requests.RequestException:
            return 0

        count = 0
        for e in engrams:
            eid = e.get("id") or e.get("engram_id")
            if eid and self.delete_engram(eid):
                count += 1
        return count

    def timeline(
        self,
        *,
        text: str | None = None,
        after: str | None = None,
        before: str | None = None,
        limit: int = 20,
    ) -> list[dict[str, Any]]:
        """POST /api/v1/timeline with the actual TimelineRequest schema."""
        body: dict[str, Any] = {"limit": limit}
        if text:
            body["text"] = text
        if after:
            body["after"] = after
        if before:
            body["before"] = before
        resp = requests.post(self._url("/api/v1/timeline"), json=body, timeout=self.timeout)
        resp.raise_for_status()
        payload = resp.json()
        return (
            payload.get("events")
            or payload.get("results")
            or payload.get("engrams")
            or []
        )

    def stats(self) -> dict[str, Any]:
        resp = requests.get(self._url("/api/v1/stats"), timeout=5.0)
        resp.raise_for_status()
        return resp.json()

    # ── Vault ────────────────────────────────────────────────────────────
    def _vault_headers(self, token: str | None = None) -> dict[str, str]:
        effective = token if token is not None else self.vault_token
        if not effective:
            return {}
        return {"Authorization": f"Bearer {effective}"}

    def vault_get(self, key: str, *, token: str | None = None) -> requests.Response:
        return requests.post(
            self._url("/api/v1/vault"),
            json={"op": "get", "key": key},
            headers=self._vault_headers(token),
            timeout=10.0,
        )

    def vault_set(self, key: str, value: str, *, token: str | None = None) -> requests.Response:
        return requests.post(
            self._url("/api/v1/vault"),
            json={"op": "set", "key": key, "value": value},
            headers=self._vault_headers(token),
            timeout=10.0,
        )

    def vault_delete(self, key: str, *, token: str | None = None) -> requests.Response:
        return requests.post(
            self._url("/api/v1/vault"),
            json={"op": "delete", "key": key},
            headers=self._vault_headers(token),
            timeout=10.0,
        )
