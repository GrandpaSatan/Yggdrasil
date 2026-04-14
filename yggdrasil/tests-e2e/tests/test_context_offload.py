"""Context offload — store a large blob and round-trip by handle."""

from __future__ import annotations

import pytest
import requests

from helpers.services import service_urls


@pytest.mark.required_services("mimir")
def test_context_offload_retrieve_roundtrip() -> None:
    url = service_urls()["mimir"].rstrip("/")
    blob = "a" * 2048  # 2KB payload — small enough to be fast, large enough to matter
    push = requests.post(
        f"{url}/api/v1/context",
        json={"content": blob, "label": "e2e-offload-test"},
        timeout=10,
    )
    if push.status_code == 404:
        pytest.skip("context offload endpoint not present on this Mimir build")
    assert push.status_code in (200, 201), f"context push got {push.status_code}"
    handle = push.json().get("handle") or push.json().get("id")
    assert handle, "offload must return a handle"

    pull = requests.get(f"{url}/api/v1/context/{handle}", timeout=10)
    assert pull.status_code == 200
    content = pull.json().get("content") or pull.text
    assert blob in content, "retrieved content must contain the original payload"
