"""Chat completion smoke + home_automation router regression guard.

Ported from ``scripts/smoke/e2e-live.sh``:
  - ``turn on the kitchen light`` must route to HA and mention the light.
  - ``turn on kitchen light while I play Fallout`` must still route HA
    (Sprint 062 router regression guard — mixed HA+gaming intent must not
    collapse to gaming).
"""

from __future__ import annotations

import re

import pytest

from helpers import OdinClient


@pytest.mark.required_services("odin")
def test_chat_completion_200_with_non_empty_content(odin_client: OdinClient) -> None:
    content = odin_client.chat_content("say hello in one word")
    assert content.strip(), "chat must return non-empty assistant content"


@pytest.mark.required_services("odin")
def test_ha_flow_mentions_kitchen_light(odin_client: OdinClient) -> None:
    content = odin_client.chat_content("turn on the kitchen light")
    assert content, "chat must return content for HA message"
    assert re.search(r"(light|kitchen|turn|on|lamp)", content, re.IGNORECASE), (
        f"response must reference the HA action; got: {content[:200]!r}"
    )


@pytest.mark.required_services("odin")
def test_mixed_ha_plus_gaming_still_routes_ha(odin_client: OdinClient) -> None:
    """Sprint 062 router regression guard — the HA intent must not be lost."""
    content = odin_client.chat_content(
        "turn on the kitchen light while I play Fallout"
    )
    assert content, "chat must return content for mixed-intent message"
    assert re.search(r"(light|kitchen|turn|lamp)", content, re.IGNORECASE), (
        "mixed HA+gaming message must still reference light control; "
        f"got: {content[:200]!r}"
    )
