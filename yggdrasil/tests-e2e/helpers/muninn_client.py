"""Thin client for Muninn retrieval endpoints."""

from __future__ import annotations

from typing import Any

import requests

from .services import check_response, retry_policy


class MuninnClient:
    def __init__(self, base_url: str, timeout: float = 15.0):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    def _url(self, path: str) -> str:
        return f"{self.base_url}{path}"

    def health(self) -> requests.Response:
        return requests.get(self._url("/health"), timeout=5.0)

    @retry_policy()
    def search(
        self,
        query: str,
        *,
        limit: int = 10,
        languages: list[str] | None = None,
    ) -> list[dict[str, Any]]:
        body: dict[str, Any] = {"query": query, "limit": limit}
        if languages:
            body["languages"] = languages
        resp = check_response(
            requests.post(self._url("/api/v1/search"), json=body, timeout=self.timeout)
        )
        resp.raise_for_status()
        payload = resp.json()
        return payload.get("results") or payload.get("chunks") or []

    def search_code(self, query: str, **kwargs: Any) -> list[dict[str, Any]]:
        return self.search(query, **kwargs)

    def assemble(self, query: str, *, limit: int = 5) -> dict[str, Any]:
        resp = requests.post(
            self._url("/api/v1/assemble"),
            json={"query": query, "limit": limit},
            timeout=self.timeout,
        )
        resp.raise_for_status()
        return resp.json()
