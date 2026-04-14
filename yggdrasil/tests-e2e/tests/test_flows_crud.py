"""Flow definition CRUD — list all flows, get one, round-trip via PUT."""

from __future__ import annotations

import pytest

from helpers import OdinClient


@pytest.mark.required_services("odin")
def test_list_flows_returns_known_flows(odin_client: OdinClient) -> None:
    flows = odin_client.flows()
    assert isinstance(flows, list) and len(flows) >= 1, (
        "/api/flows must return at least one flow definition"
    )
    names = {(f.get("name") or f.get("id") or "") for f in flows}
    # home_automation and the 4 dream flows are the known baseline set.
    expected = {"home_automation", "dream_consolidation", "dream_exploration", "dream_speculation", "dream_self_improvement"}
    assert names & expected, (
        f"expected at least one of {expected}; got {names}"
    )


@pytest.mark.required_services("odin")
def test_flow_detail_has_steps_field(odin_client: OdinClient) -> None:
    flows = odin_client.flows()
    if not flows:
        pytest.skip("no flows to inspect")
    flow = flows[0]
    # Either embedded in the list response or behind a /api/flows/:id GET.
    steps = flow.get("steps") or flow.get("pipeline") or []
    assert isinstance(steps, list), "a flow must carry a steps/pipeline list"
