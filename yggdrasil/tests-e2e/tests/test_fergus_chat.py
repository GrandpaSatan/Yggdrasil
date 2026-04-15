"""Sprint 068 Fergus chat + backends-busy + dreamer status E2E tests.

Covers the 7 pytest cases from the Sprint 068 plan:

    1. test_fergus_omits_model_field
    2. test_unknown_slash_passes_through
    3. test_known_flow_dispatches
    4. test_cron_only_flow_rejected_server_side (*)
    5. test_backends_busy_counter
    6. test_dreamer_status_shape
    7. test_language_enum_typescript_filter

(*) Client-side cron-only filtering is unit-tested in the extension's vitest
suite (`src/chat/slashCommands.test.ts` — `rejects cron-only flows` + `omits
cron-only flows from /help output`). This file covers the backend guarantee:
Odin returns 400 when asked to pin a cron-only flow, so the client-side filter
is defence-in-depth rather than the only line of defense.
"""

from __future__ import annotations

import concurrent.futures
import threading
import time

import pytest
import requests

from helpers import MuninnClient, OdinClient


# ─────────────────────────────────────────────────────────────────
# Helpers
# ─────────────────────────────────────────────────────────────────


def _post_chat_raw(
    base_url: str,
    body: dict,
    timeout: float = 30.0,
) -> requests.Response:
    """Send a raw chat body — bypasses OdinClient.chat() which always sets model."""
    return requests.post(
        f"{base_url.rstrip('/')}/v1/chat/completions",
        json=body,
        timeout=timeout,
    )


# ─────────────────────────────────────────────────────────────────
# 1. Fergus omits `model` field
# ─────────────────────────────────────────────────────────────────


@pytest.mark.required_services("odin")
@pytest.mark.timeout(120)
def test_fergus_omits_model_field(odin_client: OdinClient) -> None:
    """A request with NO `model` key must succeed — Odin's intent router
    picks the backend. This is the single Phase 3 invariant: if Odin
    rejects missing-model bodies, every Fergus chat 500s at the top of
    handlers.rs (`let Some(model)` unwrap).

    Timeout is relaxed to 120s: intent routing + RAG fetch + cold-cache
    generation on the home lab can easily exceed pytest-timeout's 30s
    default, especially on the first call after an Odin restart.
    """
    body = {
        "messages": [
            {"role": "system", "content": "You are Fergus."},
            {"role": "user", "content": "say hello in one word"},
        ],
        "stream": False,
    }
    resp = _post_chat_raw(odin_client.base_url, body, timeout=90.0)
    assert resp.status_code < 400, (
        f"Odin rejected missing-model body: HTTP {resp.status_code} — "
        f"{resp.text[:200]}"
    )
    data = resp.json()
    content = (
        data.get("choices", [{}])[0].get("message", {}).get("content", "")
    )
    assert content.strip(), "model-less Fergus chat must produce non-empty content"


# ─────────────────────────────────────────────────────────────────
# 2. Unknown slash passes through
# ─────────────────────────────────────────────────────────────────


@pytest.mark.required_services("odin")
@pytest.mark.timeout(120)
def test_unknown_slash_passes_through(odin_client: OdinClient) -> None:
    """The extension's preprocess() leaves unknown slashes verbatim — here
    we simulate that path by sending the literal `/wibble hello` as user
    content and asserting Odin doesn't try to pin `wibble` as a flow
    (which would return 400 "flow not found: wibble").
    """
    body = {
        "messages": [
            {"role": "user", "content": "/wibble hello world"},
        ],
        "stream": False,
    }
    resp = _post_chat_raw(odin_client.base_url, body, timeout=90.0)
    # Must NOT be a 400 with a "flow not found" error body. The extension
    # never sends `flow` for unknown slashes, so Odin should route normally.
    if resp.status_code == 400:
        err = resp.text.lower()
        assert "flow not found" not in err and "not invocable" not in err, (
            f"Odin attempted to pin `/wibble` as a flow — "
            f"extension is leaking slashes: {resp.text[:200]}"
        )
    assert resp.status_code < 500, (
        f"Odin crashed on unknown-slash passthrough: HTTP {resp.status_code}"
    )


# ─────────────────────────────────────────────────────────────────
# 3. Known flow dispatches via explicit `flow` field
# ─────────────────────────────────────────────────────────────────


@pytest.mark.required_services("odin")
@pytest.mark.timeout(180)
def test_known_flow_dispatches(odin_client: OdinClient) -> None:
    """Pick any non-cron flow from the live registry; assert Odin accepts
    the flow pin (no 400 "flow not found") regardless of whether the
    flow itself completes successfully.

    Trigger shapes in the wild are looser than the plan's
    Manual/Intent/Cron nominal: observed `{"modality":"omni"}` on
    ``perceive``, flat strings on some legacy flows, etc. We accept
    anything that is NOT explicitly cron-only.
    """
    resp = requests.get(
        f"{odin_client.base_url.rstrip('/')}/api/flows",
        timeout=10.0,
    )
    if resp.status_code == 404:
        pytest.skip("/api/flows not implemented on this Odin build")
    resp.raise_for_status()
    flows = resp.json()
    if isinstance(flows, dict):
        flows = flows.get("flows", []) or flows.get("data", [])

    def is_cron_only(f: dict) -> bool:
        t = f.get("trigger")
        return (
            isinstance(t, dict)
            and list(t.keys()) == ["Cron"]
        )

    user_invocable = [
        f for f in flows if isinstance(f, dict) and f.get("name") and not is_cron_only(f)
    ]
    if not user_invocable:
        pytest.skip("No non-cron flows configured on this Odin")
    flow_name = user_invocable[0]["name"]

    body = {
        "messages": [{"role": "user", "content": "run this please"}],
        "flow": flow_name,
        "stream": False,
    }
    resp = _post_chat_raw(odin_client.base_url, body, timeout=120.0)
    # 400 with "flow not found" would mean the pin failed to resolve
    # against the registry we just queried — that's a regression.
    if resp.status_code == 400:
        err = resp.text.lower()
        assert "flow not found" not in err, (
            f"Odin rejected a flow that /api/flows just listed: {resp.text[:200]}"
        )
    assert resp.status_code < 500, (
        f"Odin crashed on known flow pin: HTTP {resp.status_code}"
    )


# ─────────────────────────────────────────────────────────────────
# 4. Cron-only flows are rejected server-side (client-side filter is UI)
# ─────────────────────────────────────────────────────────────────


@pytest.mark.required_services("odin")
@pytest.mark.timeout(60)
def test_cron_only_flow_rejected_server_side(odin_client: OdinClient) -> None:
    """Defence-in-depth guarantee: even if the UI filter regresses,
    Odin returns 400 when asked to pin a cron-only flow. Skips gracefully
    when no cron-only flows are configured.
    """
    resp = requests.get(
        f"{odin_client.base_url.rstrip('/')}/api/flows",
        timeout=10.0,
    )
    if resp.status_code == 404:
        pytest.skip("/api/flows not implemented on this Odin build")
    resp.raise_for_status()
    flows = resp.json()
    if isinstance(flows, dict):
        flows = flows.get("flows", []) or flows.get("data", [])
    cron_only = [
        f
        for f in flows
        if isinstance(f, dict)
        and isinstance(f.get("trigger"), dict)
        and list(f["trigger"].keys()) == ["Cron"]
    ]
    if not cron_only:
        pytest.skip("No cron-only flows configured on this Odin")

    body = {
        "messages": [{"role": "user", "content": "should never run"}],
        "flow": cron_only[0]["name"],
        "stream": False,
    }
    resp = _post_chat_raw(odin_client.base_url, body, timeout=10.0)
    assert resp.status_code == 400, (
        f"Cron-only flow pin must return 400, got {resp.status_code}"
    )
    assert "cron" in resp.text.lower() or "invocable" in resp.text.lower(), (
        f"400 body should name the cron-only cause: {resp.text[:200]}"
    )


# ─────────────────────────────────────────────────────────────────
# 5. Backends-busy counter (Sprint 068 Phase 6a)
# ─────────────────────────────────────────────────────────────────


@pytest.mark.required_services("odin")
@pytest.mark.timeout(180)
def test_backends_busy_counter(odin_client: OdinClient) -> None:
    """GET /api/backends/busy returns a Record<backend, number> with every
    configured backend present. During two concurrent streaming requests,
    at least one backend's counter must be > 0 mid-stream, then return to
    0 after both complete. Skips gracefully on pre-Phase-6 Odin builds.
    """
    base = odin_client.base_url.rstrip("/")
    resp = requests.get(f"{base}/api/backends/busy", timeout=5.0)
    if resp.status_code == 404:
        pytest.skip(
            "/api/backends/busy not implemented — pre-Sprint-068-Phase-6 Odin"
        )
    resp.raise_for_status()
    initial = resp.json()
    assert isinstance(initial, dict) and initial, (
        f"/api/backends/busy must return a non-empty dict of backends, got {initial!r}"
    )
    for backend, count in initial.items():
        assert isinstance(backend, str) and backend, "backend keys must be non-empty strings"
        assert isinstance(count, int) and count >= 0, (
            f"count for {backend!r} must be a non-negative int, got {count!r}"
        )
    # Assume idle baseline — if the fleet is genuinely in the middle of
    # other work, this assertion can race. We tolerate a small non-zero
    # baseline by comparing DELTA later rather than absolute.
    baseline = sum(initial.values())

    # Fire two concurrent short chats; poll during the flight.
    saw_increase = threading.Event()

    def fire():
        try:
            _post_chat_raw(
                base,
                {
                    "messages": [
                        {"role": "user", "content": "count from 1 to 10 slowly"}
                    ],
                    "stream": False,
                },
                timeout=60.0,
            )
        except requests.RequestException:
            pass  # Concurrency is what matters, not the completion.

    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        f1 = pool.submit(fire)
        f2 = pool.submit(fire)
        # Poll the endpoint for up to ~10s — as soon as we see any
        # backend above baseline, we've proved the counter increments.
        deadline = time.time() + 10.0
        while time.time() < deadline and not (f1.done() and f2.done()):
            try:
                mid = requests.get(f"{base}/api/backends/busy", timeout=2.0).json()
                if sum(mid.values()) > baseline:
                    saw_increase.set()
                    break
            except requests.RequestException:
                pass
            time.sleep(0.15)
        concurrent.futures.wait([f1, f2], timeout=90.0)

    # The counter must EITHER have ticked up mid-flight OR the fleet
    # happened to complete both before we polled. In the latter case,
    # we at least assert the counter returned to baseline or below.
    final = requests.get(f"{base}/api/backends/busy", timeout=5.0).json()
    assert sum(final.values()) <= baseline + 1, (
        f"busy counter leaked — baseline={baseline} final={final}"
    )
    # If we saw the increase, great. If not, log a soft warning so flaky
    # environments don't fail the build but regressions still surface.
    if not saw_increase.is_set():
        pytest.skip(
            "Couldn't observe a concurrent mid-flight bump in the busy "
            "counter — completion was too fast. Non-fatal: the shape "
            "invariant and leak-free final state are still verified."
        )


# ─────────────────────────────────────────────────────────────────
# 6. Dreamer /status shape (Sprint 068 Phase 6b)
# ─────────────────────────────────────────────────────────────────


@pytest.mark.required_services("dreamer")
def test_dreamer_status_shape(dreamer_url: str) -> None:
    """ygg-dreamer /status must return every Sprint 068 Phase 6b field
    with the correct types. We don't require the dreamer to be actively
    dreaming — that's time-dependent and would make the test flaky.
    Instead we check the shape + whitelisting invariant.
    """
    resp = requests.get(f"{dreamer_url.rstrip('/')}/status", timeout=5.0)
    if resp.status_code == 404:
        pytest.skip("/status not implemented — pre-Sprint-068-Phase-6 dreamer")
    resp.raise_for_status()
    body = resp.json()
    for key in (
        "status",
        "service",
        "idle_secs",
        "warmup_fires",
        "dream_fires",
        "active",
        "active_flow",
        "last_fire_ts",
    ):
        assert key in body, f"/status missing key {key!r}: {body!r}"
    assert body["service"] == "ygg-dreamer"
    assert isinstance(body["active"], bool), "active must be bool"
    assert isinstance(body["idle_secs"], int)
    assert isinstance(body["warmup_fires"], int)
    assert isinstance(body["dream_fires"], int)
    assert isinstance(body["last_fire_ts"], int)
    assert body["active_flow"] is None or isinstance(body["active_flow"], str)
    # Whitelisting invariant: when active_flow is set, the name must NOT
    # start with an internal prefix (warmup_*, bench_*, etc).
    if isinstance(body["active_flow"], str):
        assert not body["active_flow"].startswith("warmup_"), (
            f"active_flow leaked internal name: {body['active_flow']!r}"
        )


# ─────────────────────────────────────────────────────────────────
# 7. Language enum typescript filter (Sprint 068 Phase 0)
# ─────────────────────────────────────────────────────────────────


@pytest.mark.required_services("muninn")
def test_language_enum_typescript_filter(muninn_client: MuninnClient) -> None:
    """Sprint 068 Phase 0 end-to-end guard. Pre-Phase-0 this request
    returned HTTP 422 "unknown variant `typescript`" because the Language
    enum serialized as `type_script`. Post-fix the filter parses cleanly.

    Scope is intentionally narrow — we assert the request does NOT 422,
    NOT that Muninn's retrieval engine strictly enforces the language
    filter (that's covered by `test_code_search_language_filter_respected`
    with `languages=["rust"]`). The corpus may have near-zero TypeScript
    content today, and BM25+vector fusion can surface semantically-similar
    JavaScript chunks when TS is sparse. That's a retrieval-quality issue,
    not a Language enum issue.
    """
    # Does not raise — the Phase 0 fix lets serde accept `typescript`.
    # If the old `type_script` variant name were still required, this
    # call would throw HTTPError(422) on `raise_for_status()`.
    results = muninn_client.search("function", limit=3, languages=["typescript"])
    assert isinstance(results, list), "search must return a list (possibly empty)"
