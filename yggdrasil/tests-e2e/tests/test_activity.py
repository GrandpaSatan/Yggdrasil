"""Regression guard for the /internal/activity endpoint.

Ported from ``scripts/ops/voice-e2e-test.sh`` — commit 6f4e317 fixed a silent
fallback where voice-e2e-test.sh accepted ``idle_secs=null`` without failing,
which masked the case where Odin had not been rebuilt with Sprint 065.
This test hard-asserts a finite number so that regression cannot recur.
"""

from __future__ import annotations

import numbers

import pytest

from helpers import OdinClient


@pytest.mark.required_services("odin")
def test_internal_activity_returns_finite_idle_secs(odin_client: OdinClient) -> None:
    payload = odin_client.activity()
    assert "idle_secs" in payload, "/internal/activity must include idle_secs"
    idle = payload["idle_secs"]
    assert isinstance(idle, numbers.Real) and not isinstance(idle, bool), (
        f"idle_secs must be a finite number, got {idle!r} (type={type(idle).__name__}). "
        "Was Odin rebuilt with Sprint 065+?"
    )
    assert idle >= 0, f"idle_secs must be non-negative, got {idle}"


@pytest.mark.required_services("odin")
def test_models_endpoint_reachable(odin_client: OdinClient) -> None:
    data = odin_client.models()
    assert isinstance(data, list) and len(data) >= 1, (
        "/v1/models must return at least one model entry"
    )
    # Each entry should have an id field.
    assert all("id" in m for m in data), "every model entry must have an 'id'"
