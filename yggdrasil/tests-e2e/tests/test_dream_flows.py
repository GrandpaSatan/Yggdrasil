"""Dream flows — consolidation, exploration, speculation, self-improvement.

Each dream flow is dispatched via the chat endpoint with ``flow`` set. These
tests are marked ``slow`` because cold LLM loads can push individual responses
past 20s — not a fit for sprint-end.
"""

from __future__ import annotations

import pytest

from helpers import OdinClient


DREAM_FLOWS = [
    "dream_consolidation",
    "dream_exploration",
    "dream_speculation",
    "dream_self_improvement",
]


@pytest.mark.slow
@pytest.mark.parametrize("flow", DREAM_FLOWS)
@pytest.mark.required_services("odin", "mimir")
def test_dream_flow_returns_non_empty_content(odin_client: OdinClient, flow: str) -> None:
    content = odin_client.chat_content(
        f"run dream flow {flow} for sprint 066",
        flow=flow,
    )
    assert content.strip(), f"{flow} must return non-empty content"
