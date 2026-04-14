"""Thin client for Odin HTTP endpoints used by E2E tests."""

from __future__ import annotations

from typing import Any

import requests

from .services import check_response, retry_policy


class OdinClient:
    def __init__(self, base_url: str, timeout: float = 60.0):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    def _url(self, path: str) -> str:
        return f"{self.base_url}{path}"

    def health(self) -> requests.Response:
        return requests.get(self._url("/health"), timeout=5.0)

    def models(self) -> list[dict[str, Any]]:
        resp = requests.get(self._url("/v1/models"), timeout=10.0)
        resp.raise_for_status()
        return resp.json().get("data", [])

    def activity(self) -> dict[str, Any]:
        resp = requests.get(self._url("/internal/activity"), timeout=5.0)
        resp.raise_for_status()
        return resp.json()

    @retry_policy()
    def chat(
        self,
        message: str,
        *,
        flow: str | None = None,
        model: str | None = None,
        stream: bool = False,
    ) -> dict[str, Any]:
        body: dict[str, Any] = {
            "model": model,
            "messages": [{"role": "user", "content": message}],
            "stream": stream,
        }
        if flow:
            body["flow"] = flow
        resp = check_response(
            requests.post(
                self._url("/v1/chat/completions"),
                json=body,
                timeout=self.timeout,
            )
        )
        resp.raise_for_status()
        return resp.json()

    def chat_content(self, message: str, **kwargs: Any) -> str:
        """Convenience: return choices[0].message.content or empty string."""
        result = self.chat(message, **kwargs)
        choices = result.get("choices") or []
        if not choices:
            return ""
        return choices[0].get("message", {}).get("content", "") or ""

    def flows(self) -> list[dict[str, Any]]:
        resp = requests.get(self._url("/api/flows"), timeout=10.0)
        resp.raise_for_status()
        payload = resp.json()
        return payload if isinstance(payload, list) else payload.get("flows", [])

    def metrics_text(self) -> str:
        resp = requests.get(self._url("/metrics"), timeout=10.0)
        resp.raise_for_status()
        return resp.text

    def e2e_hit(self) -> int:
        """Ping /api/v1/e2e/hit so the daily counter increments from this run."""
        resp = requests.post(self._url("/api/v1/e2e/hit"), timeout=5.0)
        return resp.status_code
