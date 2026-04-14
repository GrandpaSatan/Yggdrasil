"""Sprint lifecycle — start/end doc sync via the MCP local server.

These tests are gated behind E2E_SPRINT_LIFECYCLE=1 because they mutate the
sprints/ directory and Mimir archive. Running them on every sprint-end would
recurse forever.
"""

from __future__ import annotations

import os

import pytest

from helpers import MimirClient


@pytest.mark.required_services("mimir")
@pytest.mark.skipif(
    os.environ.get("E2E_SPRINT_LIFECYCLE") != "1",
    reason="set E2E_SPRINT_LIFECYCLE=1 to run sprint_start/sprint_end mutating tests",
)
def test_sprint_archive_engram_is_queryable(mimir_client: MimirClient, run_scope) -> None:
    """After a sprint_end runs, an engram tagged sprint:NNN must be queryable.

    We don't *trigger* sprint_end here (that's a destructive doc sync). We assert
    that the currently-detected sprint already has an archive engram reachable.
    """
    if not run_scope.sprint_id:
        pytest.skip("no active sprint detected")
    results = mimir_client.recall(
        f"sprint {run_scope.sprint_id} archive", limit=5
    )
    assert any(
        f"sprint:{run_scope.sprint_id}" in (r.get("tags") or [])
        or str(run_scope.sprint_id) in (r.get("cause") or "")
        for r in results
    ), (
        f"no archive engram found for sprint {run_scope.sprint_id}; has sprint_end ever run?"
    )
