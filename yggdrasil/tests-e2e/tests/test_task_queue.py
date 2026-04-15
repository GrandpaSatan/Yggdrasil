"""Mimir persistent task queue — push → pop → complete lifecycle.

Backend schemas (crates/mimir/src/handlers.rs::1465+):
  - POST /api/v1/tasks/push    → TaskPushResponse { id: uuid-string }
  - POST /api/v1/tasks/pop     → TaskPopResponse { task: Option<TaskResponse> }
  - POST /api/v1/tasks/complete → TaskCompleteResponse { updated: bool }

TaskResponse carries id/title/description/status/priority/tags/... — the ID
field is ``id`` everywhere (not ``task_id``; only the *request* body for
complete uses ``task_id`` as the field name).
"""

from __future__ import annotations

import pytest
import requests

from helpers.services import service_urls


_INTERNAL_HEADERS = {"X-Yggdrasil-Internal": "true"}


@pytest.mark.required_services("mimir")
def test_task_push_pop_complete_roundtrip(run_scope) -> None:
    """Push a uniquely-titled task, pop until we find it, complete it.

    Pop is FIFO (oldest first), so on a busy fleet the first pop can return a
    pre-existing task. We drain up to ``MAX_POPS`` non-matching tasks (each
    marked complete) to reach ours — this bounds the cleanup side-effect and
    turns "our task was never popped" into a real failure instead of a retry.

    The ``@xfail(strict=True)`` decorator tracks the known SQL regression: the
    assertion about pop.status_code == 200 fails today with 500, the xfail
    catches it, and once the bug is fixed the test flips to XPASS (which is a
    strict failure forcing removal of the marker).
    """
    url = service_urls()["mimir"].rstrip("/")
    # Per-run unique project isolates this test from real in-flight graph_link
    # work sitting in the shared queue. Pop filters by project, so our task is
    # always the first (and only) match.
    iso_project = f"e2e_task_queue_{run_scope.run_id}"
    marker = f"e2e task {run_scope.run_id}"
    push = requests.post(
        f"{url}/api/v1/tasks/push",
        json={
            "title": marker,
            "description": "round-trip test",
            "priority": 0,
            "project": iso_project,
        },
        timeout=10,
        headers=_INTERNAL_HEADERS,
    )
    assert push.status_code in (200, 201), (
        f"push must return 200/201; got {push.status_code}: {push.text[:200]}"
    )
    push_body = push.json()
    assert isinstance(push_body, dict), f"push response must be an object; got {push_body!r}"
    task_id = push_body.get("id")
    assert isinstance(task_id, str) and task_id, (
        f"TaskPushResponse.id must be a non-empty string; got {push_body!r}"
    )

    # Drain up to MAX_POPS non-matching tasks to reach ours. Non-matches are
    # marked complete to empty the queue of stale test residue. On a clean
    # queue the very first pop returns our task.
    MAX_POPS = 10
    popped_others: list[str] = []
    found: dict | None = None
    for attempt in range(MAX_POPS):
        pop = requests.post(
            f"{url}/api/v1/tasks/pop",
            json={
                "agent": f"e2e-{run_scope.run_id[:8]}",
                "project": iso_project,
            },
            timeout=10,
            headers=_INTERNAL_HEADERS,
        )
        assert pop.status_code == 200, (
            f"pop must return 200 (attempt {attempt}); got {pop.status_code}: {pop.text[:200]}"
        )
        pop_body = pop.json()
        assert isinstance(pop_body, dict) and "task" in pop_body, (
            f"TaskPopResponse must be {{'task': ...}}; got {pop_body!r}"
        )
        task = pop_body["task"]
        if task is None:
            break  # queue drained before we found ours
        assert isinstance(task, dict), f"popped task must be a dict; got {type(task).__name__}"
        if task.get("id") == task_id:
            found = task
            break
        popped_others.append(f"{task.get('id', '?')[:8]}:{task.get('title', '?')[:40]}")
        # Mark non-matching pop complete so the queue doesn't hold it in-progress.
        requests.post(
            f"{url}/api/v1/tasks/complete",
            json={"task_id": task.get("id"), "success": True},
            timeout=10,
            headers=_INTERNAL_HEADERS,
        )
    assert found is not None, (
        f"our task {task_id} never popped after draining {len(popped_others)} stale entries "
        f"(MAX_POPS={MAX_POPS}). Queue residue: {popped_others!r}"
    )
    assert found.get("title") == marker, (
        f"popped task title must match the pushed marker; got {found!r}"
    )

    complete = requests.post(
        f"{url}/api/v1/tasks/complete",
        json={"task_id": task_id, "success": True},
        timeout=10,
        headers=_INTERNAL_HEADERS,
    )
    assert complete.status_code == 200, (
        f"complete must return 200; got {complete.status_code}: {complete.text[:200]}"
    )
    complete_body = complete.json()
    assert isinstance(complete_body, dict), f"complete response must be an object; got {complete_body!r}"
    assert complete_body.get("updated") is True, (
        f"TaskCompleteResponse.updated must be True after completing an existing task; "
        f"got {complete_body!r}"
    )
