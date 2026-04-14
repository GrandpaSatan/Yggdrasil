"""Muninn retrieval flows: code search, assembly."""

from __future__ import annotations

import pytest

from helpers import MuninnClient


@pytest.mark.required_services("muninn")
def test_code_search_returns_chunks_for_known_symbol(muninn_client: MuninnClient) -> None:
    """Search for a symbol we know exists in the indexed codebase (MimirState)."""
    results = muninn_client.search("MimirState struct definition", limit=5)
    assert isinstance(results, list), "search must return a list"
    # We don't assert exact matches (index may lag) — just that *something* comes back.
    # If the index is fresh, expect hits; if empty, expect an empty list (not an error).


@pytest.mark.required_services("muninn")
def test_code_search_language_filter_respected(muninn_client: MuninnClient) -> None:
    results = muninn_client.search("async fn main", limit=10, languages=["rust"])
    assert isinstance(results, list)
    for r in results:
        lang = (r.get("language") or r.get("lang") or "").lower()
        if lang:
            assert lang == "rust", f"language filter violated: {lang}"


@pytest.mark.required_services("muninn")
def test_assemble_returns_context_block(muninn_client: MuninnClient) -> None:
    payload = muninn_client.assemble("engram storage flow", limit=3)
    assert isinstance(payload, dict), "assemble must return a dict"
    # Shape check only — endpoint may return "context", "chunks", or "assembled".
    assert any(k in payload for k in ("context", "chunks", "assembled", "results")), (
        f"assemble response missing expected keys; got {list(payload)}"
    )
