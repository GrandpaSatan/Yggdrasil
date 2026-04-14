"""Parallel-load stress — verify the fleet handles burst chat traffic.

Marked ``slow`` so the sprint-end hook never runs it. Run with::

    E2E_PARALLEL_OK=1 pytest tests/test_concurrency.py -n 4

to actually exercise xdist workers. Without xdist it just runs sequentially
and validates that the serial path still works.
"""

from __future__ import annotations

import concurrent.futures

import pytest

from helpers import OdinClient


@pytest.mark.slow
@pytest.mark.required_services("odin")
def test_burst_chat_completions_10_parallel(odin_client: OdinClient) -> None:
    """Fire 10 chat completions in parallel; all must return 200 with non-empty content.

    Catches: Mimir Postgres pool exhaustion, store_gate LFM2.5 queue backpressure
    leaking into 5xx, Ollama rate limits.
    """
    def _one(i: int) -> str:
        return odin_client.chat_content(f"say the number {i}")

    with concurrent.futures.ThreadPoolExecutor(max_workers=10) as pool:
        results = list(pool.map(_one, range(10)))

    empty = [i for i, r in enumerate(results) if not r.strip()]
    assert not empty, f"chat completions {empty} returned empty content under load"
